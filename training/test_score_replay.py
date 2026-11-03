import json
from pathlib import Path
import tempfile
import unittest
from pipeline import stamp
from test_risk import archive
from score_replay import RaceLabels, score_report


def forecast(kind='overtake', minute=2, lap=1, **changes):
    return dict(kind=kind, lap=lap, time_ms=int(stamp(f'2023-01-01T00:{minute:02}:00Z') * 1000), probability=.25, driver=2, target=1) | changes


class ReplayRiskScoreTest(unittest.TestCase):
    def test_pass_before_issue_is_not_future_success(self):
        a = archive()
        a['position'] += [dict(driver_number=d, position=d, date='2023-01-01T00:09:30Z') for d in (1, 2)]
        labels = RaceLabels(a)
        self.assertEqual(labels.outcome(forecast())[0], 1)
        self.assertEqual(labels.outcome(forecast(minute=10, lap=5))[0], 0)

    def test_original_target_is_fixed_and_must_be_adjacent_at_issue(self):
        a = archive()
        a['laps'] += [r | {'driver_number': 3} for r in a['laps'] if r['driver_number'] == 1]
        a['position'] = [dict(driver_number=d, position=d, date='2023-01-01T00:00:00Z') for d in (1, 2, 3)]
        a['position'] += [dict(driver_number=3, position=1, date='2023-01-01T00:09:00Z'), dict(driver_number=1, position=3, date='2023-01-01T00:09:00Z')]
        labels = RaceLabels(a)
        self.assertEqual(labels.outcome(forecast())[0], 1)
        self.assertEqual(labels.outcome(forecast(target=3))[1], 'target_not_adjacent')

    def test_incomplete_pit_retirement_and_neutralisation_are_censored(self):
        for cause in ('horizon', 'pit', 'retirement', 'sc'):
            a = archive()
            if cause == 'horizon': a['laps'] = [r for r in a['laps'] if r['lap_number'] < 6]
            if cause == 'pit': a['pit'] = [dict(driver_number=1, lap_number=4, date='2023-01-01T00:07:00Z')]
            if cause == 'retirement': a['laps'] = [r for r in a['laps'] if r['driver_number'] != 1 or r['lap_number'] < 5]
            if cause == 'sc': a['race_control'].append(dict(date='2023-01-01T00:09:00Z', message='SAFETY CAR DEPLOYED'))
            self.assertIsNone(RaceLabels(a).outcome(forecast())[0], cause)

    def test_issue_clocks_and_nonfuture_lap_are_validated(self):
        labels = RaceLabels(archive())
        for issue in (forecast(time_ms=float('nan')), forecast(time_ms=-1), forecast(minute=2, lap=5), forecast(probability=1.1), forecast(minute=22, lap=11), forecast(driver=[]), forecast(target=None)):
            self.assertIsNone(labels.outcome(issue)[0])

    def test_sc_exact_issue_time_and_five_lap_horizon(self):
        a = archive()
        a['race_control'] += [dict(date='2023-01-01T00:09:00Z', message='VIRTUAL SAFETY CAR DEPLOYED'), dict(date='2023-01-01T00:09:30Z', message='VIRTUAL SAFETY CAR ENDED')]
        labels = RaceLabels(a)
        self.assertEqual(labels.outcome(forecast('sc'))[0], 1)
        self.assertEqual(labels.outcome(forecast('sc', minute=10, lap=5))[0], 0)
        self.assertIsNone(labels.outcome(forecast('sc', minute=12, lap=6))[0])

    def test_unissued_risk_is_unmeasured_not_zero_error(self):
        result = score_report({'races': []}, Path('.'), {'model': 'risk-v1', 'holdout_races': []})
        for kind in ('sc', 'overtake'):
            score = result['held_out'][kind]
            self.assertEqual((score['candidates'], score['eligible']), (0, 0))
            self.assertIsNone(score['brier'])
            self.assertIsNone(score['coverage'])

    def test_risk_holdout_manifest_and_model_identity_override_pace_split(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder); (root / 'races').mkdir()
            for session in (1, 2):
                a = archive(); a['session']['session_key'] = session
                (root / f'races/{session}.json').write_text(json.dumps(a))
            report = {'races': [dict(session=1, held_out=True, risk_forecasts=[forecast(model='risk-v1')]), dict(session=2, held_out=False, risk_forecasts=[forecast(probability=.75, model='risk-v1')])]}
            manifest = dict(model='risk-v1', holdout_races=[2])
            result = score_report(report, root, manifest)
            self.assertEqual(result['held_out']['overtake']['brier'], .25 ** 2)
            self.assertTrue(result['races'][0]['held_out'])
            self.assertFalse(result['races'][0]['risk_held_out'])
            self.assertTrue(result['races'][1]['risk_held_out'])
            report['races'][0]['risk_forecasts'][0]['model'] = 'different-model'
            with self.assertRaises(ValueError): score_report(report, root, manifest)

    def test_report_scores_original_probabilities_and_holdout_only(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder); (root / 'races').mkdir()
            for session in (1, 2):
                a = archive(); a['session']['session_key'] = session
                (root / f'races/{session}.json').write_text(json.dumps(a))
            report = {'races': [dict(session=1, held_out=True, risk_forecasts=[forecast(), forecast(minute=12, lap=6)]), dict(session=2, held_out=False, risk_forecasts=[forecast(probability=.75)])]}
            result = score_report(report, root)
            score = result['held_out']['overtake']
            self.assertEqual((score['candidates'], score['eligible'], score['positives']), (2, 1, 1))
            self.assertEqual(score['coverage'], .5)
            self.assertEqual(score['brier'], .75 ** 2)
            self.assertEqual(result['all']['overtake']['eligible'], 2)


if __name__ == '__main__': unittest.main()
