# Agon

An unofficial, non-commercial F1 timing application with historical replay, driver telemetry, comparison charts and explainable strategy forecasts.

Agon combines a Rust backend with a React/TypeScript interface. Models train offline in Python and run through native ONNX Runtime in Rust. The implementation follows [the PRD](PRD.md); [acceptance evidence](docs/acceptance.md) records what is implemented, measured and still incomplete.

**Status: experimental.** Local workflows are implemented, but prediction accuracy misses the PRD launch targets. Authenticated live access, production capacity and complete circuit metadata remain open acceptance gates.

## Capabilities

- Live timing tower with configurable columns, sector times, tyres, pit history, weather and race-control messages.
- Historical OpenF1 session replay from 2023 onward, with independent playback clocks, pause, seek and 1×–20× speed.
- Interpolated track map and driver telemetry, including distance traces, lap overlays and beginner/expert views.
- Nine comparison charts with driver selection, pinning, PNG export and shareable views.
- Pace and pit-window forecasts, Bayesian tyre degradation/changepoints, qualifying estimates and anomaly alerts.
- Experimental five-lap SC/VSC and overtake probabilities, with model provenance and contributing factors.
- A 1,000-run finishing simulation with sequential pit stops and tyre-age resets, where scheduled distance and pit-loss metadata are available.
- Durable live arrival journals, restart recovery, PostgreSQL persistence, Redis fanout and private Parquet export.

Missing inputs remain unavailable. Synthetic demonstrations are labelled, and approximate telemetry never implies official engineering precision.

## Quick start

### Docker

Requires Docker with Compose. From the repository root:

```powershell
docker compose --profile app up --build -d
```

Open <http://127.0.0.1:3001>. The image includes the frontend, Rust gateway and native ONNX Runtime. PostgreSQL and Redis use persistent volumes; their published ports bind to localhost.

Stop the application while retaining the database and Redis services:

```powershell
docker compose stop app
```

Stop the complete stack without removing stored data:

```powershell
docker compose down
```

### Native development

Requires stable Rust, Node.js 22.18+ and npm. Cargo must be on `PATH`.

```powershell
npm ci
npm run dev
```

Open <http://127.0.0.1:5173>; the gateway runs at `127.0.0.1:3001`. Stop the development command with `Ctrl+C`.

The initial Strategy lab is a synthetic demonstration. Select a real season, event and session to import OpenF1 history. The initial import follows the source quota and can take a minute or more. Completed archives are cached privately under `data/`.

Without a native inference library, pace/pit forecasts use a labelled **basic estimate** and trained SC/overtake probabilities remain unavailable. To enable the bundled models on Windows:

```powershell
python -m venv .venv
./.venv/Scripts/python.exe -m pip install -r training/requirements.txt
$env:ONNXRUNTIME_DYLIB_PATH=(Resolve-Path '.venv/Lib/site-packages/onnxruntime/capi/onnxruntime.dll').Path
npm run dev
```

Linux uses `libonnxruntime.so.*`; macOS uses `libonnxruntime.dylib`. Use ONNX Runtime API 22 or newer. The Python environment supplies the native library; the running gateway does not start Python. Restart the gateway after changing models or runtime configuration.

## Configuration

Set environment variables on the gateway process. Keep credentials out of frontend variables, share URLs and Git.

| Variable | Purpose | Default |
| --- | --- | --- |
| `AGON_BIND` | Gateway listen address | `127.0.0.1:3001` |
| `AGON_DATA` | Private archive and checkpoint directory | `data` |
| `AGON_MODELS` | ONNX artifacts and manifests | `models` |
| `ONNXRUNTIME_DYLIB_PATH` | Absolute native ONNX library path | Basic fallback when absent |
| `AGON_METADATA_FILE` | Validated circuit/session metadata manifest | Bundled partial coverage |
| `DATABASE_URL` | PostgreSQL persistence; required for primary live ingest | File persistence for replay |
| `REDIS_URL` | Live snapshot/evaluation fanout | Disabled |
| `AGON_LIVE_ROLE` | `primary` ingests; `subscriber` consumes Redis | `primary` |
| `LIVE_SESSION_KEY` | Active OpenF1 session to ingest | No live ingest |
| `OPENF1_USERNAME`, `OPENF1_PASSWORD` | Sponsor account and automatic token renewal | Unset |
| `OPENF1_TOKEN` | Externally managed short-lived access token | Unset |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | Enable OTLP trace export | Disabled |
| `RUST_LOG` | Structured log filter | Gateway informational logs |

For native PostgreSQL/Redis development:

```powershell
docker compose up -d postgres redis
$env:DATABASE_URL='postgres://agon:agon-local@127.0.0.1:54329/agon'
$env:REDIS_URL='redis://127.0.0.1:63799'
```

These credentials are local defaults. Configure deployment credentials outside Git. Schema creation is additive; volumes are never reset automatically. TimescaleDB currently hosts PostgreSQL tables; automatic retention deletion is not configured.

## Live timing

Live OpenF1 access requires a sponsor account and an active session:

```powershell
$env:OPENF1_USERNAME='your-sponsor-username'
$env:OPENF1_PASSWORD='your-password'
$env:LIVE_SESSION_KEY='active-session-key'
$env:DATABASE_URL='postgres://agon:agon-local@127.0.0.1:54329/agon'
npm run dev
```

Select **Live** in the application. The gateway connects through MQTT TLS, renews account tokens and reconciles REST history. Source outages retain the last state and display delayed data. Source delay is separate from gateway latency.

Run one primary per upstream session. Additional gateways use `AGON_LIVE_ROLE=subscriber`, the same `LIVE_SESSION_KEY` and `REDIS_URL`. They consume bounded timing/evaluation fanout without opening another upstream feed. The primary persists captured arrivals before applying them and recovers forecast logs/checkpoints after restart.

Sponsor transport has not been verified with real credentials in this workspace. Public hosting, TLS and trusted proxy/IP configuration require deployment setup.

## Models and evaluation

Bundled artifacts use real OpenF1 race data from 2023–2025. Race groups are disjoint between training, calibration and holdout. Model manifests contain feature order, artifact hashes, parity cases, sample counts and measured limitations.

| Evaluation | Measured result | PRD target |
| --- | --- | --- |
| Pace, next 1–5 laps | MAE 1.869 s | ≤0.35 s |
| Pit lap, forecast ten laps before stop | 26.03% within ±2 laps; 47.51% coverage | ≥60% |
| SC/VSC, next five laps | Brier 0.0831; constant baseline 0.0827 | No numeric target |
| Overtake, next five laps | Brier 0.0770; constant baseline 0.0968 | No numeric target |
| Finish at half distance | Unscored without required sourced metadata | ≥70% within ±1 position |

Lower Brier is better. The SC model currently performs slightly worse than its constant baseline. Risk scores apply only to eligible held-out windows: approximately 80.0% coverage for SC and 41.8% for overtakes. Overtake labels use filtered position reversals and are conditional on five green laps without pits/retirement; sparse timing cannot establish exact physical passes. DRS and pace-delta inputs are not included in the current risk model.

Runtime risk probabilities additionally require a sourced scheduled distance with at least five laps remaining. Current bundled schedule coverage therefore limits these outputs to Singapore 2023 unless a private metadata manifest supplies other sessions. Offline risk scores do not establish runtime issuance coverage across the calendar.

The finishing simulator uses an explicit strategy policy rather than a calibrated full-race strategy optimiser. Future SC effects require a calibrated deployment forecast plus an already completed observed episode for duration/slowdown; onset, tyre inventory and warm-up assumptions remain visible in prediction factors.

See [pace/pit metrics](models/manifest.json), [risk metrics](models/risk_manifest.json), [model documentation](models/README.md) and [acceptance evidence](docs/acceptance.md). Offline feature scores and Rust replay results are separate evaluations.

The full Rust engine replay completed all 70 acquired races. Its 14-race pace holdout measured 2.703 s MAE across 38,907 horizons and 20.93% pit accuracy with 65.29% coverage. Runtime risk holdout scores remain unmeasured: forecasts were issued only for Singapore 2023, outside the risk holdout. [The replay evidence](docs/acceptance.md#full-rust-replay-evaluation) records the counts, censoring and limits.

## Offline workflows

Install `training/requirements.txt` in `.venv` before running the Python commands.

```powershell
./.venv/Scripts/python.exe training/pipeline.py acquire
./.venv/Scripts/python.exe training/pipeline.py train
./.venv/Scripts/python.exe training/risk.py --acquire-position
```

Acquisition is resumable and quota-limited. Avoid running it alongside live ingest on the same source quota. Raw JSON, feature Parquet and replay reports stay in ignored `data/training/`.

Evaluate the acquired holdout races through the Rust replay, state and prediction engine:

```powershell
cargo run --release -p pitwall-api --example backtest
```

This requires `ONNXRUNTIME_DYLIB_PATH`. Add `-- --all-races` to process all acquired races; the output keeps holdout metrics separate from training/calibration races. The private report is written to `data/training/rust-backtest.json` and records coverage and original issued probabilities.

Score those original risk probabilities at their actual issue timestamps:

```powershell
./.venv/Scripts/python.exe training/score_replay.py --report data/training/rust-backtest.json --output data/training/rust-risk-score.json
```

The scorer uses risk-manifest holdout races independently of the pace split, preserves original targets, and reports censored/incomplete horizons. No issued forecasts means an unmeasured score, not zero error.

Validate a sourced metadata manifest:

```powershell
cargo run -p pitwall-ingest --example import_metadata -- metadata/verified.json
```

[Metadata documentation](metadata/README.md) describes private imports, identity/configuration matching, coordinate registration and prior-session pit-loss provenance. The bundled manifest currently supplies only the verified pre-race Singapore 2023 schedule. Complete licensed historical geometry and pit-loss calibration remain unavailable.

Private captured archives can be exported to Parquet with `cargo run -p pitwall-store --bin export -- <session-key>`. Browser replay uses historical REST documents; a captured arrival journal preserves only the period actually captured. Original pre-capture arrivals and capture gaps cannot be reconstructed from final REST documents.

## Development checks

Run the checks relevant to a change:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p pitwall-api --example backtest
npm test
npm run build
./.venv/Scripts/python.exe -m unittest discover -s training -v
```

Browser workflows use `npm run test:e2e`. Database/Redis and native model integration checks require their configured services/libraries; [CI](.github/workflows/ci.yml) documents those commands. Local checks do not establish production latency, 10,000-client capacity, cross-browser coverage, WCAG 2.1 AA or availability.

## Architecture and operations

| Component | Responsibility |
| --- | --- |
| `pitwall-core` | Shared domain contracts and generated TypeScript types |
| `pitwall-ingest` | OpenF1 source boundary, historical/live normalisation and sourced metadata |
| `pitwall-state` | Causal race state and per-client replay |
| `pitwall-predict` | Online analysis, ONNX inference, simulation and forecast scoring |
| `pitwall-api` | REST, binary MessagePack WebSocket deltas and subscription control |
| `pitwall-store` | Private file/PostgreSQL persistence, Redis fanout and Parquet export |
| `pitwall-web` | React interface and ECharts visualisations |
| `training` | Offline acquisition, model fitting, calibration and evaluation |

`/health` exposes gateway health; `/metrics` serves Prometheus counters; `/api/accuracy` exposes model evaluation. Structured tracing is enabled, with optional OTLP export. Timing publication runs independently of prediction work.

## Data, attribution and precision

Agon is an unofficial fan/educational project and is not affiliated with Formula 1, the FIA or any team. No official logos or broadcast footage are included.

OpenF1 source data is attributed under CC BY-NC-SA 4.0. Keep use non-commercial and obtain appropriate permission before monetisation. Raw feeds are not exposed through a general download/proxy endpoint. Separately sourced metadata retains its own provenance and rights notes.

Telemetry is approximately 3.7 Hz; integrated distance, corner comparisons and interpolated locations are estimates. Read [source contracts and limitations](docs/data-sources.md) before relying on derived measurements.
