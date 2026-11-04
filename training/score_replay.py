"""Score recorded Rust risk probabilities against private historical outcomes.

No predictions are regenerated. Coverage describes eligible labels among recorded
forecasts; it does not measure how often the runtime issued a forecast.
"""
import argparse
from collections import Counter, defaultdict
from datetime import datetime, timezone
import json
import math
from pathlib import Path
from pipeline import COMPOUNDS, track_status
from risk import HORIZON, latest

KINDS = ('sc', 'overtake')


def number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def source_ms(value):
    try:
        date = datetime.fromisoformat(value.replace('Z', '+00:00'))
        if date.tzinfo is None: return None
        delta = date.astimezone(timezone.utc) - datetime(1970, 1, 1, tzinfo=timezone.utc)
        return (delta.days * 86400 + delta.seconds) * 1000 + delta.microseconds // 1000
    except (AttributeError, TypeError, ValueError, OverflowError):
        return None


def source_timeline(rows):
    values = sorted((date, i, row) for i, row in enumerate(rows)
                    if (date := source_ms(row.get('date'))) is not None)
    return [date for date, _, _ in values], [row for _, _, row in values]


class RaceLabels:
    def __init__(self, archive):
        self.archive = archive
        self.controls = source_timeline(archive['race_control'])
        self.weather = source_timeline(archive['weather'])
        states = []; status = 'UNKNOWN'; self.deployments = []
        for date, row in zip(*self.controls):
            status = track_status(row, status); states.append({'status': status})
            if 'SAFETY CAR DEPLOYED' in row.get('message', '').upper(): self.deployments.append(date)
        self.statuses = self.controls[0], states
        self.laps = defaultdict(dict); self.leader = {}
        for lap in archive['laps']:
            start = source_ms(lap.get('date_start')); duration = lap.get('lap_duration')
            if start is None or not number(duration) or duration <= 0: continue
            # Rust truncates the source timestamp and rounds the positive duration separately.
            milliseconds = duration * 1000
            whole = math.floor(milliseconds)
            end = start + whole + int(milliseconds - whole >= .5)
            d = lap['driver_number']; n = lap['lap_number']
            self.laps[d][n] = (end, lap)
            self.leader[n] = min(self.leader.get(n, math.inf), end)
        self.positions = {d: source_timeline([r for r in archive.get('position', []) if r['driver_number'] == d]) for d in self.laps}
        self.intervals = {d: source_timeline([r for r in archive.get('intervals', []) if r['driver_number'] == d]) for d in self.laps}

    def status(self, time):
        row = latest(self.statuses, time)
        return row['status'] if row else 'UNKNOWN'

    def outcome(self, issue):
        kind = issue.get('kind'); n = issue.get('lap'); clock = issue.get('time_ms'); probability = issue.get('probability')
        if kind not in KINDS: return None, 'invalid_kind'
        if not number(probability) or not 0 <= probability <= 1: return None, 'invalid_probability'
        if not isinstance(n, int) or isinstance(n, bool) or n <= 0: return None, 'invalid_lap'
        if not isinstance(clock, int) or isinstance(clock, bool) or clock <= 0: return None, 'invalid_clock'
        time = clock
        if self.archive['session'].get('session_name', 'Race') not in ('Race', 'Sprint'): return None, 'non_race'
        if not self.leader or time > max(self.leader.values()): return None, 'invalid_clock'
        if self.status(time) != 'GREEN': return None, 'non_green_issue'
        weather = latest(self.weather, time)
        if weather is None or weather.get('rainfall') is None: return None, 'missing_weather'
        if kind == 'sc':
            completed = [lap for lap, end in self.leader.items() if end <= time]
            if not completed or max(completed) != n: return None, 'issue_lap_mismatch'
            if not all(lap in self.leader for lap in range(n + 1, n + HORIZON + 1)): return None, 'incomplete_horizon'
            end = self.leader[n + HORIZON]
            if end <= time: return None, 'nonfuture_horizon'
            return int(any(time < date <= end for date in self.deployments)), None
        driver = issue.get('driver'); target = issue.get('target')
        if any(not isinstance(d, int) or isinstance(d, bool) or d <= 0 for d in (driver, target)): return None, 'invalid_opponents'
        if driver not in self.laps or target not in self.laps or driver == target: return None, 'invalid_opponents'
        history = self.laps[driver]; target_history = self.laps[target]
        completed = [lap for lap, (end, _) in history.items() if end <= time]
        if not completed or max(completed) != n: return None, 'issue_lap_mismatch'
        if not all(lap in history for lap in range(n + 1, n + HORIZON + 1)): return None, 'incomplete_horizon'
        end = history[n + HORIZON][0]
        if end <= time: return None, 'nonfuture_horizon'
        pa = latest(self.positions[driver], time); pb = latest(self.positions[target], time)
        adjacent = [d for d in self.laps if d != driver and (p := latest(self.positions[d], time)) and pa and p['position'] == pa['position'] - 1]
        if pa is None or pb is None or pa['position'] <= 1 or adjacent != [target]: return None, 'target_not_adjacent'
        target_completed = [lap for lap, (finish, _) in target_history.items() if finish <= time]
        if not target_completed or max(target_completed) != n: return None, 'opponents_not_same_lap'
        target_last = min((lap for lap, (finish, _) in target_history.items() if finish >= end), default=None)
        if target_last is None or not all(lap in target_history for lap in range(n + 1, target_last + 1)): return None, 'target_incomplete_horizon'
        if any(self.status(date) != 'GREEN' for date in self.controls[0] if time < date <= end): return None, 'neutralised_horizon'
        for pit in self.archive['pit']:
            if pit['driver_number'] not in (driver, target): continue
            date = source_ms(pit.get('date'))
            if date is None or time - 60_000 <= date <= end + 60_000 or n <= pit.get('lap_number', -1) <= max(n + HORIZON, target_last): return None, 'pit_horizon'
        if history[n][1].get('is_pit_out_lap') or any(lap.get('is_pit_out_lap') for d in (driver, target) for finish, lap in self.laps[d].values() if time < finish <= end): return None, 'pit_out_horizon'
        interval = latest(self.intervals[driver], time)
        gap = interval.get('interval') if interval else None
        if interval is None or time - source_ms(interval['date']) > 30_000 or not number(gap) or not 0 < gap <= 10: return None, 'missing_fresh_interval'
        for d in (driver, target):
            stints = [s for s in self.archive['stints'] if s['driver_number'] == d and s['lap_start'] <= n]
            if not stints or max(stints, key=lambda s: s['lap_start']).get('compound') not in COMPOUNDS: return None, 'missing_compound'
        changes = sorted(set([end] + [date for d in (driver, target) for date in self.positions[d][0] if time < date <= end]))
        passed = any(latest(self.positions[driver], date)['position'] < latest(self.positions[target], date)['position'] for date in changes)
        return int(passed), None


def empty_score():
    return dict(candidates=0, eligible=0, positives=0, squared_error_sum=0.0, censored_reasons={})


def finish(scores):
    for score in scores.values():
        score['negatives'] = score['eligible'] - score['positives']
        score['coverage'] = score['eligible'] / score['candidates'] if score['candidates'] else None
        score['brier'] = score['squared_error_sum'] / score['eligible'] if score['eligible'] else None
    return scores


def score_report(report, root, manifest=None):
    aggregates = {name: {kind: empty_score() for kind in KINDS} for name in ('all', 'held_out')}; races = []
    for race in report['races']:
        session = race['session']
        if not isinstance(session, int) or isinstance(session, bool) or session <= 0: raise ValueError('Invalid report session key')
        if not isinstance(race['held_out'], bool): raise ValueError('held_out must be boolean')
        archive = json.loads((root / 'races' / f'{session}.json').read_text(encoding='utf-8'))
        if archive['session']['session_key'] != session: raise ValueError('Archive/report session mismatch')
        risk_held_out = session in manifest['holdout_races'] if manifest is not None else race['held_out']
        labels = RaceLabels(archive); scores = {kind: empty_score() for kind in KINDS}
        for issue in race.get('risk_forecasts', []):
            if manifest is not None and issue.get('model') != manifest['model']: raise ValueError(f'Forecast model does not match risk manifest for session {session}')
            kind = issue.get('kind')
            if kind not in KINDS: raise ValueError(f'Unsupported forecast kind: {kind!r}')
            score = scores[kind]; score['candidates'] += 1
            label, reason = labels.outcome(issue)
            if label is None:
                score['censored_reasons'][reason] = score['censored_reasons'].get(reason, 0) + 1
            else:
                score['eligible'] += 1; score['positives'] += label
                score['squared_error_sum'] += (issue['probability'] - label) ** 2
        for name in ('all', 'held_out') if risk_held_out else ('all',):
            for kind, score in scores.items():
                total = aggregates[name][kind]
                for key in ('candidates', 'eligible', 'positives', 'squared_error_sum'): total[key] += score[key]
                total['censored_reasons'] = dict(Counter(total['censored_reasons']) + Counter(score['censored_reasons']))
        races.append(dict(session=session, held_out=race['held_out'], risk_held_out=risk_held_out, scores=finish(scores)))
    return dict(schema='agon-rust-risk-score-v1', horizon_laps=HORIZON, model=manifest['model'] if manifest else None,
        semantics={
            'holdout_source': 'Risk manifest holdout_races; report held_out remains the pace classification.' if manifest is not None else 'Report held_out fallback; no independent risk manifest supplied.',
            'empty_scores': 'No eligible original forecasts means null Brier and null coverage when there are no candidates; unmeasured does not mean zero error.',
            'prediction_source': 'Original Rust report probabilities, exact issue timestamps and original target; no regeneration or retraining.',
            'coverage': 'Eligible outcome labels / recorded forecast candidates, including invalid and censored rows in denominator. Runtime issuance coverage is separate.',
            'sc': 'New SC/VSC deployment strictly after issue through fifth future leader-completed lap, green issue and complete horizon.',
            'overtake': 'Position-observed reversal of original same-lap adjacent target after issue through fifth future driver lap, conditional on neither car pitting/retiring and remaining green. Pit +/-60s guards and full target lap continuity match offline labels; timing feed does not prove an exact physical pass.',
            'clocks': 'Millisecond lap completion clocks match Rust normalisation. Claimed lap must already be completed and current at issue; nonfuture or truncated horizons are censored.',
        }, races=races, **{name: finish(scores) for name, scores in aggregates.items()})


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--root', type=Path, default=Path('data/training'))
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--manifest', type=Path, default=Path('models/risk_manifest.json'))
    args = parser.parse_args()
    result = score_report(json.loads(args.report.read_text(encoding='utf-8')), args.root, json.loads(args.manifest.read_text(encoding='utf-8')))
    result['source_report'] = str(args.report)
    result['source_manifest'] = str(args.manifest)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    temporary = args.output.with_suffix(args.output.suffix + '.tmp')
    temporary.write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8'); temporary.replace(args.output)
    print(json.dumps(result['held_out'], indent=2))
