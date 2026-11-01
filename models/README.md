# Model artifacts

`pace.onnx` and `hazard.onnx` are LightGBM models trained offline on real OpenF1 race data from 2023–2025. `manifest.json` records the exact feature order, race groups, independent calibration and held-out metrics, parity case, version, hashes, coverage and limitations. No Python process is used by the timing server.

The measured accuracy is below the PRD launch targets. These are experimental educational estimates, not validated strategy recommendations. Missing/corrupt/incompatible artifacts or a missing native ONNX Runtime library produce the labelled basic fallback.

Source: <https://openf1.org>, source data license CC BY-NC-SA 4.0. These derived artifacts are supplied for the same non-commercial educational scope, with attribution; raw source records are not included. Regeneration: see `training/pipeline.py` and the project README. The `smoke/` directory is ignored development output.
