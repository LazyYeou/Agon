import unittest
import json
import tempfile
from pathlib import Path
from pipeline import clean_lap, grouped_split, score_forecasts, decode_response, prepare, stint_baseline

class CausalityTest(unittest.TestCase):
    def test_training_baseline_restarts_after_sustained_changepoint(self):
        samples=[(i+1,y+(i+1)*0.035) for i,y in enumerate([90,90.04,90.08,90.12,90.16,90.20,93,93.1,93.2])]
        next_lap,deg=stint_baseline(samples,10)
        self.assertAlmostEqual(next_lap,93.165)
        self.assertAlmostEqual(deg,0.0675)
    def test_features_use_only_completed_past_laps_and_weather(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);(root/"races").mkdir()
            archive={"session":{"session_key":100,"year":2023},"stints":[{"driver_number":1,"stint_number":1,"lap_start":1,"lap_end":100,"tyre_age_at_start":0,"compound":"MEDIUM"}],"pit":[],"intervals":[],
                "race_control":[{"date":"2023-01-01T00:00:00Z","message":"GREEN LIGHT"}],
                "weather":[{"date":"2023-01-01T00:00:00Z","track_temperature":30},{"date":"2023-01-02T00:00:00Z","track_temperature":99}],
                "laps":[{"driver_number":1,"lap_number":i+1,"date_start":f"2023-01-01T00:{i*2:02}:00Z","lap_duration":90+i,"is_pit_out_lap":False} for i in range(6)]}
            path=root/"races/100.json";path.write_text(json.dumps(archive))
            first=prepare(root).iloc[0]
            self.assertEqual(first.issued_lap,3);self.assertEqual(first.tyre_age,3);self.assertEqual(first.track_temperature,30)
            self.assertAlmostEqual(first.baseline,91.965)
            archive["laps"][-1]["lap_duration"]=300;path.write_text(json.dumps(archive))
            self.assertAlmostEqual(prepare(root).iloc[0].baseline,91.965)
            archive["intervals"]=[{"driver_number":1,"date":"2023-01-01T00:05:00Z","interval":2},{"driver_number":1,"date":"2023-01-01T00:07:00Z","interval":0.8}]
            path.write_text(json.dumps(archive));row=prepare(root).iloc[0]
            self.assertAlmostEqual(row.baseline,91.965)
            self.assertTrue(row.residual is None or __import__('math').isnan(row.residual))
    def test_source_empty_result_is_distinct_from_broken_route(self):
        self.assertEqual(decode_response(404,b'{"detail":"No results found."}'),[])
        with self.assertRaises(ValueError):decode_response(404,b'{"detail":"Not Found"}')
    def test_cleaning_excludes_pit_safety_car_and_traffic(self):
        base={"lap_duration":90,"is_pit_out_lap":False,"status":"GREEN","interval":2.0}
        self.assertTrue(clean_lap(base))
        for change in ({"is_pit_out_lap":True},{"status":"SC"},{"interval":0.9},{"lap_duration":None}):
            self.assertFalse(clean_lap(base|change))

    def test_race_group_split_never_splits_a_race(self):
        train,test=grouped_split([10,10,20,20,30,30,40,40])
        self.assertTrue(set(train).isdisjoint(test))
        self.assertEqual(set(train)|set(test),{10,20,30,40})

    def test_scoring_counts_only_actuals_and_uses_literal_error(self):
        result=score_forecasts([{"prediction":90,"actual":91},{"prediction":92,"actual":None}])
        self.assertEqual(result,{"samples":1,"mae":1.0})

if __name__=="__main__":unittest.main()
