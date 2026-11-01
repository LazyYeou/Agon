# Agon acceptance evidence

The application implements the local end-to-end timing, replay, telemetry, comparison and prediction workflows from [PRD.md](../PRD.md). **Full PRD launch acceptance remains open.** Implemented behavior and measured limits are separate below; PRD targets are unchanged.

## Implemented behavior

| Area | Implementation and evidence |
| --- | --- |
| Contracts and backend | Six Rust crates, generated TypeScript, validated binary MessagePack snapshot/delta protocol. Source measurement time is separate from receipt time; stale samples cannot rewind current values. |
| Historical replay | OpenF1 sessions from 2023 onward, rate-limited private cache, per-client clocks, pause/1x?20x/seek and causal prefetch. In-progress captures expire instead of becoming permanently partial historical archives. |
| Tower and header | Position/change, gap/interval, last/best laps, live partial sectors, minisectors, tyre age/newness/stints, pit revisions, prediction bands; locally saved column visibility/order; weather/status/control filters and source states. |
| Map | Real session location outline and selected-driver hover, interpolated markers. Verified metadata drives zones, pit lane, corners and rejoin when available; synthetic geometry is explicitly labelled. |
| Telemetry | Immediate dialog shell, subscriptions, complete long-lap requests in bounded chunks, five distance traces plus delta, driver/lap overlay, aligned approximate xy, hover map, approximate corner/mini-sector comparison, beginner/expert and history. Seeking removes future traces; older live laps use bounded history fallback. |
| Comparison charts | Nine chart datasets, rolling/fuel options, pit/SC outliers, per-stint fits/continuation, shaded strategy bands, crosshair/zoom, team colour/teammate line style, pin/export/share and accumulated original forecast-vs-actual data. |
| Online prediction | Bayesian per-stint regression with sustained changepoint detection, rolling sector/speed z-score alerts, uncertainty/factors, paused red flags and low-confidence SC/rain, ONNX pace/hazard with honest basic fallback, undercut/overcut and 1,000-run finishing distribution when required metadata exists. |
| Qualifying | Phase-aware track-evolution estimate. Distinct source `SessionStatus / SESSION STARTED` messages establish phases, verified against actual 2023 Singapore qualifying control messages; missing boundaries withhold phase-dependent outputs. |
| Live durability | Bounded MQTT queue/batches, immutable PostgreSQL revision/REST receipt journal, stale source revision guards, transactional checkpoints/watermarks and committed-batch recovery. REST repairs are durable and do not fabricate source revision IDs or reset MQTT health. Uncaptured broker packets cannot be recovered from a nonexistent upstream historical revision stream. |
| Live fanout/evaluation | Redis RESP3 latest-only subscription, primary/subscriber gateway roles, delayed-state retention, bounded forecast/report fanout. Forecast logs survive primary restarts; switching from replay to live clears the previous evaluation. |
| Storage/export | Atomic file cache; PostgreSQL archives, checkpoints and prediction logs; TimescaleDB container using PostgreSQL tables; private Parquet archive export. Captured arrival replay is available to offline processing; default browser replay uses complete historical REST rather than presenting interrupted captures as whole sessions. |
| Offline models | Real acquisition of 70 Race sessions in 2023?2025, Parquet features, 45/11/14 disjoint race train/calibration/holdout splits, grouped cross-validation, versioned ONNX artifacts with schema/hash checks and native Python/Rust inference parity. |
| Operations | Docker multi-stage image with Rust, static frontend and native ONNX runtime; Compose PostgreSQL/Redis; CI configuration; health, Prometheus counters, structured tracing and optional OTLP export. |

## Measured models

Source of truth: [manifest](../models/manifest.json), model `openf1-lgbm-v2-24c99efbc5`.

| Metric | Measured | PRD target |
| --- | --- | --- |
| Next-lap MAE | 1.556 s; 6265 samples | ?0.35 s for next 1?5 laps |
| Next 1?5 lap MAE | 1.869 s; 24951 horizon samples | ?0.35 s |
| Held-out band coverage | 73.58% for approximate 80% bands | Validated visible uncertainty |
| Pit ?2, issued 10 laps before stop | 26.03%; 219 scored stops | ?60% |
| Pit forecast coverage | 219 / 461 eligible stops (47.51%) | Report alongside accuracy |
| Finish ?1 at half distance | Unscored without pre-known race distance | ?70% |

**Accuracy targets fail.** Do not describe the model as launch-validated. The seven causal features omit unavailable pit-loss/team-history inputs; races missing pit records are excluded from hazard labels. The offline report is a causal feature backtest, not a complete all-race Rust replay validation claim.

## Remaining acceptance gates

| Gate | Current limit |
| --- | --- |
| Authenticated live E2E | Sponsor credentials were not supplied. MQTT/auth/token renewal/reconciliation are implemented; actual sponsor transport remains unverified. |
| Circuit/configuration coverage | Complete licensed corner, DRS, timing-sector, pit-lane, pit-loss and scheduled-distance metadata is unavailable for all years/configurations. Missing information stays unavailable; synthetic data does not establish coverage. |
| Finishing simulation | Uses forecast uncertainty and a sampled future stop; a calibrated SC model and full multi-stop race policy are not provided. Valid half-distance accuracy needs pre-known scheduled distance. |
| Original feed history | REST gives final documents, so normal historical replay approximates physical completion. Captured journal preserves original receipts from capture onward; capture gaps and pre-capture history cannot be reconstructed exactly. Snapshot recovery uses a conservative ordering floor where per-field receipt history is absent. |
| Performance and compatibility | Local smoke/tests do not establish 10,000 viewers, p95 <1 s feed-to-screen, <500 ms inference under load, <300 ms panel on production hardware, 4G <2.5 s, 60 fps, all target browsers, WCAG 2.1 AA or 99.5% availability. |
| Deployment/observability | Docker/Compose/CI are supplied; hosted CI/public deployment and an actual OTLP collector export are not claimed. No automatic time-series retention deletes are installed. |

## Local verification

Final local evidence: 69 enabled Rust tests passed; strict Clippy and formatting passed; 22 frontend tests, TypeScript checking, production build and six Python pipeline tests passed. Five Chromium flows passed (four in the consolidated run, desktop rerun after fixing the new test selector). PostgreSQL/Redis integration and native model parity were also exercised explicitly. Docker image built and the Compose app is healthy. A packaged Chromium smoke imported actual Singapore 2023 Race session 9165, sought to lap 32 and displayed native `Trained model ? below target` forecasts with no JavaScript errors. A 12-client, 3-second container smoke measured control acknowledgement p95 237.4 ms and mean 7,051 bytes/s/client; this measures neither feed-to-screen latency nor production capacity. Details are recorded in the implementation ledger. Browser coverage exercises the real Rust gateway with a labelled synthetic race, desktop/mobile workflows, live-control boundaries and causal seek. Real OpenF1 source checks remain distinct from synthetic E2E. Packaging and small-client load checks establish local behavior only.

Work remains on `feat/agon-v1`. New commits use `feat : message`, `fix : message` or `chore : message`, without Codex attribution. No public deployment or merge is implied.
