import unittest
from risk import build_samples, split_races, circuit_priors, feature_matrix


def archive():
    return {
        'session': {'session_key': 1, 'circuit_short_name': 'Test', 'session_name': 'Race'},
        'laps': [{'driver_number': d, 'lap_number': n, 'date_start': f'2023-01-01T00:{(n-1)*2:02}:00Z', 'lap_duration': 120, 'is_pit_out_lap': False} for d in [1, 2] for n in range(1, 11)],
        'race_control': [{'date': '2023-01-01T00:00:00Z', 'message': 'GREEN LIGHT'}],
        'weather': [{'date': '2023-01-01T00:00:00Z', 'rainfall': 0}],
        'stints': [{'driver_number': d, 'lap_start': 1, 'stint_number': 1, 'compound': 'MEDIUM', 'tyre_age_at_start': 0} for d in [1, 2]],
        'pit': [],
        'position': [{'driver_number': 1, 'position': 1, 'date': '2023-01-01T00:00:00Z'}, {'driver_number': 2, 'position': 2, 'date': '2023-01-01T00:00:00Z'}, {'driver_number': 1, 'position': 2, 'date': '2023-01-01T00:09:00Z'}, {'driver_number': 2, 'position': 1, 'date': '2023-01-01T00:09:00Z'}],
        'intervals': [{'driver_number': d, 'interval': 0.8, 'date': f'2023-01-01T00:{n*2:02}:00Z'} for d in [1, 2] for n in range(1, 11)],
    }


class RiskTest(unittest.TestCase):
    def test_position_pass_and_fully_observed_horizon(self):
        rows, counts = build_samples(archive())
        self.assertEqual(len(rows['safety_car']), 5)
        case = next(r for r in rows['overtake'] if r['lap'] == 1 and r['driver'] == 2)
        self.assertEqual(case['target'], 1)
        self.assertEqual(case['label'], 1)
        self.assertTrue(all(r['lap'] <= 5 for r in rows['overtake']))

    def test_pit_retirement_and_neutralisation_are_not_passes(self):
        for change in ['pit', 'retirement', 'sc']:
            a = archive()
            if change == 'pit': a['pit'] = [{'driver_number': 1, 'lap_number': 4, 'date': '2023-01-01T00:07:00Z'}]
            if change == 'retirement': a['laps'] = [r for r in a['laps'] if r['driver_number'] != 1 or r['lap_number'] < 5]
            if change == 'sc': a['race_control'].append({'date': '2023-01-01T00:09:00Z', 'message': 'SAFETY CAR DEPLOYED'})
            rows, _ = build_samples(a)
            self.assertFalse(any(r['lap'] == 1 and r['driver'] == 2 for r in rows['overtake']), change)

    def test_new_deployment_and_future_weather_dont_leak(self):
        a = archive()
        a['race_control'].append({'date': '2023-01-01T00:09:00Z', 'message': 'VIRTUAL SAFETY CAR DEPLOYED'})
        a['weather'].append({'date': '2023-01-01T00:09:00Z', 'rainfall': 1})
        rows, _ = build_samples(a)
        self.assertEqual(rows['safety_car'][0]['label'], 1)
        self.assertEqual(rows['safety_car'][0]['rain'], 0)
        self.assertTrue(all(r['lap'] < 5 for r in rows['safety_car']))

    def test_interval_freshness_and_missing_weather_withhold(self):
        a = archive()
        a['intervals'] = [r for r in a['intervals'] if r['date'] == '2023-01-01T00:02:00Z']
        rows, _ = build_samples(a)
        self.assertFalse(any(r['lap'] == 2 for r in rows['overtake']))
        a['weather'] = []
        rows, _ = build_samples(a)
        self.assertEqual(rows, {'safety_car': [], 'overtake': []})

    def test_missing_positions_never_infer_pass_from_intervals(self):
        a = archive(); a['position'] = []
        rows, _ = build_samples(a)
        self.assertEqual(rows['overtake'], [])

    def test_disjoint_splits_and_prior_excludes_own_race(self):
        train, calibration, holdout = split_races(list(range(10)))
        self.assertFalse(set(train) & set(calibration) | set(train) & set(holdout) | set(calibration) & set(holdout))
        rows = [{'race': 1, 'circuit': 'Test', 'label': 1}, {'race': 2, 'circuit': 'Test', 'label': 0}]
        self.assertLess(circuit_priors(rows, exclude_race=1)['Test'], circuit_priors(rows, exclude_race=2)['Test'])


if __name__ == '__main__': unittest.main()
