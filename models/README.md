# Model artifacts

`pace.onnx` and `hazard.onnx` are LightGBM models trained offline on real OpenF1 race data from 2023–2025. `manifest.json` records the exact feature order, race groups, independent calibration and held-out metrics, parity case, version, hashes, coverage and limitations. No Python process is used by the timing server.

The measured accuracy is below the PRD launch targets. These are experimental educational estimates, not validated strategy recommendations. Missing/corrupt/incompatible artifacts or a missing native ONNX Runtime library produce the labelled basic fallback.

`risk_sc.onnx` and `risk_overtake.onnx` are logistic models with Platt calibration on separate race groups. `risk_manifest.json` records their content-derived version, hashes, feature order, training-only circuit priors, label assumptions, calibration/holdout groups, reliability bins, Brier scores and Python/native parity cases. Regenerate with `python training/risk.py --acquire-position` after acquiring the historical corpus. No Python service runs in production.

SC predicts a new SC/VSC deployment within five race laps; its held-out Brier is 0.0831 versus 0.0827 for a constant baseline. The pass model scores 0.0770 versus 0.0968 and is conditional on five green laps without either car pitting/retiring. Position-feed reversals approximate physical passes. DRS and pace-delta features and sprint calibration are absent. A sourced schedule with five remaining laps, known circuit and required observations are mandatory at runtime; missing risk inputs/artifacts withhold the probability.

`training/score_replay.py` evaluates probabilities recorded by the Rust replay CLI at their original issue times and against the original opponents. Its held-out aggregate uses the risk manifest's split and never substitutes offline model scores for unissued forecasts.

The completed 70-race Rust replay issued risk forecasts only for Singapore 2023 because of schedule coverage. That session is outside the risk holdout, so runtime held-out Brier scores remain null. Engine-level pace/pit results and original-risk scoring counts are recorded separately in [acceptance evidence](../docs/acceptance.md#full-rust-replay-evaluation).

Source: <https://openf1.org>, source data license CC BY-NC-SA 4.0. These derived artifacts are supplied for the same non-commercial educational scope, with attribution; raw source records are not included. Regeneration: see `training/pipeline.py` and the project README. The `smoke/` directory is ignored development output.
