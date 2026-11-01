# Agon

Unofficial, non-commercial race timing, historical replay, approximate telemetry and explainable strategy estimates. Implementation follows [PRD.md](PRD.md) and the [approved design](docs/superpowers/specs/2026-11-01-agon-design.md). Production acceptance is **not complete**: see [the acceptance matrix](docs/acceptance.md).

## Run locally

Requires Rust (stable), Node 22.18+ and npm. From the repository root:

```powershell
npm ci
npm run dev
```

Open <http://127.0.0.1:5173>. The gateway listens on `127.0.0.1:3001`. The initial Strategy lab is explicitly synthetic. Select a season/event/session for real OpenF1 history from 2023 onward. First import follows the 30 requests/minute source quota and can take a minute; completed archives are cached privately in `data/`. Source failures retain the current view and expose an error/retry state.

Tower columns can be hidden/reordered and are saved locally. Click a driver for telemetry; Expert reveals corners/history. Charts selects 2–6 drivers, exports PNG, shares driver/chart/lap-range URLs, pins a chart, and shows forecast accuracy. Replay controls support pause, 1x–20x and seeking. Each connection has an independent replay clock; seek clears evaluation and never uses future lap results.

## Native trained-model inference

The model files and measured report are included. Install the platform's ONNX Runtime native shared library (API 22 or newer), and set:

```powershell
$env:ONNXRUNTIME_DYLIB_PATH='C:/absolute/path/to/onnxruntime.dll'
npm run dev
```

For this workspace, the offline Python environment's library can also be used; it does not launch Python on race day:

```powershell
python -m venv .venv
./.venv/Scripts/python.exe -m pip install -r training/requirements.txt
$env:ONNXRUNTIME_DYLIB_PATH=(Resolve-Path '.venv/Lib/site-packages/onnxruntime/capi/onnxruntime.dll').Path
npm run dev
```

Linux uses `libonnxruntime.so.*`, macOS `libonnxruntime.dylib`. `AGON_MODELS` optionally selects an artifact directory. Restart the server after changing artifacts/runtime configuration. A missing/failed model is visibly labelled **basic estimate**, with uncertainty; the synthetic lab always uses the basic model to avoid presenting trained historic data as validation of invented data.

## PostgreSQL, TimescaleDB and Redis

File persistence works without services. For the deployed store and Redis snapshot channel:

```powershell
docker compose up -d
$env:DATABASE_URL='postgres://agon:agon-local@127.0.0.1:54329/agon'
$env:REDIS_URL='redis://127.0.0.1:63799'
npm run dev
```

These credentials are local-development defaults; the database and Redis ports bind loopback. Set deployment credentials outside Git. Additive schema creation runs before listening; failed persistence is reported. Volumes are never reset automatically. Redis stores expiring snapshots and publishes a bounded latest-state channel. Run one primary with `AGON_LIVE_ROLE=primary`; additional gateways use `AGON_LIVE_ROLE=subscriber`, `REDIS_URL` and the same `LIVE_SESSION_KEY`. The primary requires `DATABASE_URL` and records original MQTT revisions, REST repair receipts and restart watermarks privately before applying them. Subscribers receive timing and bounded accuracy evaluation without opening their own upstream feed.

Container packaging:

```powershell
docker compose --profile app up --build -d
```

Then open <http://127.0.0.1:3001>. The runtime image has a native ONNX library, Rust executable and static frontend; Python is used only in a build stage. Production TLS/reverse proxy and commercial licensing are outside this local delivery. The app reads actual socket IPs for rate limiting; do not expose it behind a shared proxy without trusted-IP handling at that proxy.

## OpenF1 live access

An OpenF1 sponsor account is required. Configure on the server only:

```powershell
$env:OPENF1_USERNAME='issued-account-username'
$env:OPENF1_PASSWORD='your-password'
$env:LIVE_SESSION_KEY='actual-active-session-key'
$env:DATABASE_URL='postgres://agon:agon-local@127.0.0.1:54329/agon'
npm run dev
```

Use the Live button. MQTT TLS connects to the documented broker, refreshes tokens, and reconciles against REST. Do not place credentials in frontend variables, URLs or local storage. `OPENF1_TOKEN` can supply an externally managed short-lived token, but username/password are needed for automatic renewal. Live forecast logs survive restarts. Live status becomes delayed if the source stops; the last state remains. The feed's external ~3-second delay is distinct from gateway latency. Authenticated live transport has not been tested without sponsor credentials.

## Offline model training

```powershell
./.venv/Scripts/python.exe -m unittest discover -s training -v
./.venv/Scripts/python.exe training/pipeline.py acquire
./.venv/Scripts/python.exe training/pipeline.py train
```

Acquisition imports 70 Race sessions in 2023–2025, caches responses and resumes completed race files. It respects the free quota; run it separately from live ingest on the same quota. Private JSON/Parquet remain under ignored `data/training`. Models use only completed past laps, tyre/stint state, available weather and traffic. Train/calibration/holdout races are disjoint; grouped cross-validation never splits one race across train/test. Reports include 1–5 lap error, calibration coverage, pit prediction accuracy exactly ten laps before a stop, and forecast coverage. Races with missing pit data are not treated as zero-stop races.

Scheduled race distances and complete licensed circuit metadata are not yet available across all supported configurations. Position accuracy at half distance is therefore unscored for those sessions. No final lap count or classification is fed back into earlier predictions. Targets not achieved by the measured models remain open launch gates.

## Verify

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm test
npm run build
npm run test:e2e
```

On Windows, add `%USERPROFILE%/.cargo/bin` to PATH or invoke its `cargo.exe`. With the services/model library configured, also run:

```powershell
cargo test -p pitwall-store -- --include-ignored --test-threads=1
cargo test -p pitwall-api --test live -- --include-ignored --test-threads=1
$env:AGON_TEST_MODELS=(Resolve-Path models).Path
cargo test -p pitwall-predict --test model -- --include-ignored
```

`/health`, Prometheus `/metrics`, `/api/accuracy` and structured tracing support inspection. Set `OTEL_EXPORTER_OTLP_ENDPOINT` to enable OTLP traces. Private archives (including locally captured arrival journals when available) can be exported with `cargo run -p pitwall-store --bin export -- <session-key>`; output stays under `AGON_DATA`/`data`, with timestamp and lossless event JSON columns. Default browser replay loads complete historical REST archives; captured segments can have gaps and are not substituted for a complete race. CI runs Rust checks, database/Redis and native inference tests, frontend tests/build, generated-type drift and browser acceptance. CI itself and production availability require an actual hosted run; local passing tests do not prove 10,000-client capacity, WCAG compliance or multi-browser/4G targets.

## Data and precision

No official F1, FIA or team logos are included. OpenF1 attribution and CC BY-NC-SA 4.0 apply to source data; this remains a non-commercial fan/educational project, not affiliated with those organizations. No general raw-feed proxy/download endpoint exists. Telemetry and derived distance/corner measurements are approximate (~3.7 Hz). Missing values stay unavailable. See [verified source contracts and metadata gaps](docs/data-sources.md).
