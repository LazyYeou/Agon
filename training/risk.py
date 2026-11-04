"""Offline, race-disjoint five-lap SC/VSC and position-observed pass models.

P8 is an observed relative-position reversal, with pit/retirement/neutralisation
horizons censored. Timing-feed resolution cannot prove an exact physical pass.
Run: python training/risk.py [--acquire-position] [--root data/training] [--output models]
"""
import argparse
from bisect import bisect_right
from collections import defaultdict
import hashlib
import json
import math
from pathlib import Path
from pipeline import COMPOUNDS, Source, stamp, track_status

FEATURES = {
    'safety_car': ['lap', 'rain', 'circuit_prior'],
    'overtake': ['lap', 'rain', 'circuit_prior', 'interval', 'tyre_age_delta', 'compound_delta'],
}
HORIZON = 5


def timeline(rows):
    values = sorted((stamp(r.get('date')), i, r) for i, r in enumerate(rows) if stamp(r.get('date')) is not None)
    return [t for t, _, _ in values], [r for _, _, r in values]


def latest(series, time):
    i = bisect_right(series[0], time) - 1
    return series[1][i] if i >= 0 else None


def split_races(races):
    races = sorted(set(races))
    if len(races) < 6: raise ValueError('At least six races required')
    first = int(len(races) * .6); second = int(len(races) * .8)
    return races[:first], races[first:second], races[second:]


def circuit_priors(rows, exclude_race=None):
    eligible = [r for r in rows if r['race'] != exclude_race]
    if not eligible: raise ValueError('No races available for causal circuit prior')
    pooled = sum(r['label'] for r in eligible) / len(eligible)
    grouped = defaultdict(list)
    for r in eligible: grouped[r['circuit']].append(r['label'])
    return {c: (sum(labels) + 20 * pooled) / (len(labels) + 20) for c, labels in grouped.items()}


def feature_matrix(rows, priors):
    import numpy as np
    return np.asarray([[r[f] if f != 'circuit_prior' else priors[r['circuit']] for f in FEATURES[r['kind']]] for r in rows], dtype=np.float32)


def build_samples(a):
    rows = {'safety_car': [], 'overtake': []}; counts = {'safety_car': 0, 'overtake': 0}
    if a['session'].get('session_name', 'Race') not in ('Race', 'Sprint'): return rows, counts
    race = a['session']['session_key']; circuit = a['session'].get('circuit_short_name', '')
    controls = timeline(a['race_control']); weather = timeline(a['weather'])
    statuses = []; status = 'UNKNOWN'; deployments = []
    for date, control in zip(*controls):
        status = track_status(control, status); statuses.append({'status': status})
        if 'SAFETY CAR DEPLOYED' in control.get('message', '').upper(): deployments.append(date)
    statuses = controls[0], statuses
    def current_status(time):
        value = latest(statuses, time)
        return value['status'] if value else 'UNKNOWN'
    laps = defaultdict(dict); completed = defaultdict(list); leader = {}
    for lap in a['laps']:
        start = stamp(lap.get('date_start')); duration = lap.get('lap_duration')
        if start is None or not isinstance(duration, (int, float)) or not math.isfinite(duration) or duration <= 0: continue
        end = start + duration; d = lap['driver_number']; n = lap['lap_number']
        laps[d][n] = (end, lap); completed[d].append(end)
        leader[n] = min(leader.get(n, math.inf), end)
    for values in completed.values(): values.sort()
    positions = {d: timeline([r for r in a.get('position', []) if r['driver_number'] == d]) for d in laps}
    intervals = {d: timeline([r for r in a.get('intervals', []) if r['driver_number'] == d]) for d in laps}
    stints = {d: sorted([r for r in a['stints'] if r['driver_number'] == d], key=lambda r: r['lap_start']) for d in laps}
    def tyre(d, n):
        st = next((s for s in reversed(stints[d]) if s['lap_start'] <= n), None)
        if st is None or st.get('compound') not in COMPOUNDS: return None
        return st.get('tyre_age_at_start', 0) + n - st['lap_start'] + 1, COMPOUNDS[st['compound']]
    def base(kind, n, time):
        w = latest(weather, time); rain = w.get('rainfall') if w else None
        if rain is None or current_status(time) != 'GREEN' or not circuit: return None
        return dict(kind=kind, race=race, circuit=circuit, lap=n, issued_at=time, rain=int(bool(rain)))
    for n, time in sorted(leader.items()):
        counts['safety_car'] += 1
        if n + HORIZON not in leader: continue
        row = base('safety_car', n, time)
        if row is None: continue
        row['label'] = int(any(time < date <= leader[n + HORIZON] for date in deployments))
        rows['safety_car'].append(row)
    for d, history in laps.items():
        for n, (time, lap) in sorted(history.items()):
            counts['overtake'] += 1
            row = base('overtake', n, time)
            if row is None or lap.get('is_pit_out_lap') or not all(k in history for k in range(n + 1, n + HORIZON + 1)): continue
            end = history[n + HORIZON][0]
            own_position = latest(positions[d], time)
            if own_position is None or own_position['position'] <= 1: continue
            candidates = [other for other in laps if other != d and (p := latest(positions[other], time)) and p['position'] == own_position['position'] - 1]
            if len(candidates) != 1: continue
            target = candidates[0]
            # Same-lap adjacent opponents only; final classified positions cannot identify passes.
            current_laps = [k for k, (t, _) in laps[target].items() if t <= time]
            if not current_laps or max(current_laps) != n or max(completed[target], default=0) < end: continue
            target_last = min((k for k, (t, _) in laps[target].items() if t >= end), default=None)
            if target_last is None or not all(k in laps[target] for k in range(n + 1, target_last + 1)): continue
            if any(current_status(t) != 'GREEN' for t in controls[0] if time < t <= end): continue
            if any(p['driver_number'] in (d, target) and ((date := stamp(p.get('date'))) is None or time - 60 <= date <= end + 60) for p in a['pit']): continue
            if any(p['driver_number'] in (d, target) and n <= p.get('lap_number', -1) <= max(n + HORIZON, target_last) for p in a['pit']): continue
            if any(l.get('is_pit_out_lap') for driver in (d, target) for t, l in laps[driver].values() if time < t <= end): continue
            interval = latest(intervals[d], time)
            if interval is None or time - stamp(interval['date']) > 30: continue
            gap = interval.get('interval'); ta = tyre(d, n); tb = tyre(target, n)
            if not isinstance(gap, (int, float)) or not math.isfinite(gap) or not 0 < gap <= 10 or ta is None or tb is None: continue
            changes = sorted(set([end] + [t for driver in (d, target) for t in positions[driver][0] if time < t <= end]))
            passed = False
            for t in changes:
                pa = latest(positions[d], t); pb = latest(positions[target], t)
                if pa and pb and pa['position'] < pb['position']: passed = True
            row.update(driver=d, target=target, interval=float(gap), tyre_age_delta=ta[0] - tb[0], compound_delta=ta[1] - tb[1], label=int(passed))
            rows['overtake'].append(row)
    return rows, counts


def export_logistic(path, coefficient, intercept):
    import numpy as np
    import onnx
    from onnx import TensorProto, helper, numpy_helper
    coefficient = np.asarray(coefficient, dtype=np.float32).reshape(-1, 1)
    graph = helper.make_graph([
        helper.make_node('MatMul', ['input', 'coef'], ['dot']),
        helper.make_node('Add', ['dot', 'intercept'], ['logit']),
        helper.make_node('Sigmoid', ['logit'], ['probability'])], 'calibrated-risk',
        [helper.make_tensor_value_info('input', TensorProto.FLOAT, [None, len(coefficient)])],
        [helper.make_tensor_value_info('probability', TensorProto.FLOAT, [None, 1])],
        [numpy_helper.from_array(coefficient, 'coef'), numpy_helper.from_array(np.asarray([intercept], dtype=np.float32), 'intercept')])
    model = helper.make_model(graph, opset_imports=[helper.make_opsetid('', 15)], ir_version=8)
    onnx.checker.check_model(model)
    temporary = path.with_suffix('.onnx.tmp'); temporary.write_bytes(model.SerializeToString()); temporary.replace(path)


def train(root, output, acquire_position=False):
    import numpy as np
    import onnxruntime as ort
    from sklearn.linear_model import LogisticRegression
    from sklearn.preprocessing import StandardScaler
    from sklearn.metrics import brier_score_loss
    all_rows = {k: [] for k in FEATURES}; candidates = defaultdict(lambda: defaultdict(int)); archives = []
    source = Source(root / 'raw')
    for path in sorted((root / 'races').glob('*.json')):
        a = json.loads(path.read_text(encoding='utf-8'))
        if acquire_position and 'position' not in a:
            a['position'] = source.get('position', session_key=a['session']['session_key'])
            temporary = path.with_suffix('.tmp'); temporary.write_text(json.dumps(a)); temporary.replace(path)
        rows, counts = build_samples(a); race = a['session']['session_key']; archives.append(race)
        for kind in FEATURES:
            all_rows[kind].extend(rows[kind]); candidates[kind][race] += counts[kind]
    train_races, cal_races, holdout_races = split_races(archives)
    manifest = dict(schema='agon-risk-v1', model='openf1-calibrated-logistic-risk-v1', horizon_laps=HORIZON,
        source='OpenF1 historical timing/race control; non-commercial educational use',
        train_races=train_races, calibration_races=cal_races, holdout_races=holdout_races,
        feature_assumptions={
            'circuit_prior': 'Training races only; each training row uses a leave-one-race-out, 20-observation pooled-rate shrinkage prior. Calibration/holdout/runtime use all training races only. Unknown circuits withhold.',
            'freshness': 'Latest weather at issue; position is event-driven and persists. Both position timestamps must exist and be nonfuture; current numeric interval requires age <=30 seconds. Same completed lap, gap >0 and <=10 seconds.',
            'omissions': 'No DRS, relative pace trend, telemetry, incident severity or race-distance feature. Archives are Grand Prix races; sprint probability calibration has not been measured.',
            'coverage': 'Holdout scored windows / all holdout leader laps (SC) or driver completed laps (pass), including censored/incomplete/missing-input/unknown-circuit windows in denominator.',
        },
        label_assumptions={
            'safety_car': 'New SC or VSC deployment within next five leader-completed laps; green issue only; observed complete horizon.',
            'overtake': 'Observed relative-position reversal of current same-lap adjacent opponent within next five driver laps. Both cars must finish horizon; censor pits, pit-out laps, retirements, missing laps and neutralisations. Position-feed resolution and administrative corrections limit physical-pass identification; not an exact overtake ground truth.'},
        models={})
    output.mkdir(parents=True, exist_ok=True)
    for kind, rows in all_rows.items():
        fit = [r for r in rows if r['race'] in train_races]
        priors = circuit_priors(fit)
        calibration = [r for r in rows if r['race'] in cal_races and r['circuit'] in priors]
        heldout = [r for r in rows if r['race'] in holdout_races and r['circuit'] in priors]
        race_priors = {race: circuit_priors(fit, race) for race in train_races}
        for race in train_races:
            other = [r for r in fit if r['race'] != race]
            pooled = sum(r['label'] for r in other) / len(other)
            for circuit in priors: race_priors[race].setdefault(circuit, pooled)
        x = np.concatenate([feature_matrix([r], race_priors[r['race']]) for r in fit])
        y = np.asarray([r['label'] for r in fit])
        cx = feature_matrix(calibration, priors); cy = np.asarray([r['label'] for r in calibration])
        tx = feature_matrix(heldout, priors); ty = np.asarray([r['label'] for r in heldout])
        if any(len(set(values)) < 2 for values in (y, cy, ty)): raise ValueError(f'{kind}: insufficient positive/negative race-disjoint examples')
        scaler = StandardScaler().fit(x)
        logistic = LogisticRegression(C=1, max_iter=2000, random_state=42).fit(scaler.transform(x), y)
        platt = LogisticRegression(C=1, max_iter=2000, random_state=42).fit(logistic.decision_function(scaler.transform(cx)).reshape(-1, 1), cy)
        raw_coef = logistic.coef_[0] / scaler.scale_
        raw_intercept = logistic.intercept_[0] - raw_coef @ scaler.mean_
        coefficient = raw_coef * platt.coef_[0, 0]; intercept = raw_intercept * platt.coef_[0, 0] + platt.intercept_[0]
        predicted = platt.predict_proba(logistic.decision_function(scaler.transform(tx)).reshape(-1, 1))[:, 1]
        filename = 'risk_sc.onnx' if kind == 'safety_car' else 'risk_overtake.onnx'
        path = output / filename; export_logistic(path, coefficient, intercept)
        session = ort.InferenceSession(str(path), providers=['CPUExecutionProvider'])
        actual = session.run(None, {'input': tx})[0].reshape(-1)
        parity = float(np.max(np.abs(actual - predicted)))
        if parity > 1e-5: raise ValueError(f'ONNX parity error: {parity}')
        bins = []
        for low in np.arange(0, 1, .1):
            mask = (predicted >= low) & (predicted < low + .1)
            if mask.any(): bins.append(dict(count=int(mask.sum()), probability=float(predicted[mask].mean()), frequency=float(ty[mask].mean())))
        manifest['models'][kind] = dict(file=filename, sha256=hashlib.sha256(path.read_bytes()).hexdigest(), features=FEATURES[kind], circuit_priors=priors,
            train_samples=len(fit), train_positives=int(y.sum()), calibration_samples=len(calibration), calibration_positives=int(cy.sum()), holdout_samples=len(heldout), holdout_positives=int(ty.sum()), holdout_negatives=int(len(ty) - ty.sum()),
            holdout_candidates=sum(candidates[kind][r] for r in holdout_races), coverage=len(heldout) / sum(candidates[kind][r] for r in holdout_races),
            brier=float(brier_score_loss(ty, predicted)), constant_brier=float(brier_score_loss(ty, np.full(len(ty), float(y.mean())))),
            observed_rate=float(ty.mean()), mean_probability=float(predicted.mean()), reliability_bins=bins, onnx_max_probability_error=parity,
            parity=[dict(features=tx[i].tolist(), probability=float(actual[i])) for i in range(min(8, len(tx)))])
    identity = hashlib.sha256(json.dumps(manifest, sort_keys=True, separators=(',', ':')).encode('utf-8'))
    for kind in sorted(manifest['models']): identity.update((output / manifest['models'][kind]['file']).read_bytes())
    manifest['model'] += '-' + identity.hexdigest()[:16]
    temporary = output / 'risk_manifest.json.tmp'
    temporary.write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8'); temporary.replace(output / 'risk_manifest.json')
    print(json.dumps({kind: {k:v for k,v in model.items() if k in ('brier','constant_brier','coverage','train_samples','calibration_samples','holdout_samples','holdout_positives','onnx_max_probability_error')} for kind, model in manifest['models'].items()}, indent=2))
    return manifest


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path('data/training'))
    parser.add_argument('--output', type=Path, default=Path('models'))
    parser.add_argument('--acquire-position', action='store_true')
    args = parser.parse_args(); train(args.root, args.output, args.acquire_position)
