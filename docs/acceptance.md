# Agon acceptance evidence

The local timing, replay, telemetry, comparison and prediction workflows follow [PRD.md](../PRD.md). **Full launch acceptance remains open.** Implementation, data coverage and measured quality are separate requirements.

## Implemented workflows

| Area | Implementation |
| --- | --- |
| Contracts | Six Rust crates, generated TypeScript contracts, validated binary MessagePack snapshots/deltas. Source measurement and receipt timestamps are separate; older revisions cannot rewind current fields. |
| Replay | Historical OpenF1 sessions from 2023 onward, quota-limited private cache, per-client playback, pause/1×–20×/seek and causal prefetch. Seeking discards future traces and evaluations. |
| Timing/header | Configurable saved columns, position/change, gap/interval, laps, partial sectors/minisectors, tyres/stints/newness, pit revisions, prediction bands, weather/status and filtered race control. |
| Track map | Session-location outline, interpolated readable driver markers and selection. Metadata-driven sectors/DRS/corners/pit lane/rejoin render only when their required geometry or pit-loss inputs exist. |
| Telemetry | On-demand bounded subscriptions/history, five distance traces and delta, lap/driver overlays, hover map, approximate corner/mini-sector comparison and beginner/expert race history. |
| Charts | Nine comparisons, rolling/fuel options, pit/SC outliers, per-stint fits, forecast continuation, shared crosshair/zoom, teammate line styles, pin/export/share and original forecast-versus-actual data. |
| Pace/tyres/pits | Bayesian regression and sustained changepoints, rolling anomaly z-scores, native ONNX pace/hazard inference, labelled basic fallback, undercut/overcut and qualifying evolution. |
| SC/overtake | Offline logistic models with separate Platt calibration races; schema/hash/version validation and native ONNX probabilities. Five-lap outputs require a green supported race, sourced scheduled distance and required observations. Overtake also requires a fresh interval and same-lap adjacent target. Missing/corrupt models or inputs withhold probabilities. |
| Finishing simulation | 1,000 deterministic-seeded runs with first-stop hazard, sequential future stint policies, tyre-age resets and pace/pit-loss uncertainty. A sampled SC/VSC deployment is shared across drivers; duration/slowdown use completed observed episodes. An inactive/delayed or ambiguous participant withholds the complete classification. |
| Prediction scheduling | Live and replay share an input signature covering corrections, positions, gap/interval observations, pit state and expiry boundaries. Clock ticks within an unchanged eligibility state do not trigger work. Timing publication remains independent of prediction work. |
| Circuit metadata | Validated exact session/configuration identity, separate sourced schedule/geometry/pit loss, coordinate registration and pre-session provenance/calibration cutoffs. Historical imports, cached archives and recovered primary live checkpoints use the same loader. Private import tooling validates before enriching an archive. |
| Live durability | Bounded MQTT processing, immutable PostgreSQL revision/REST receipt journal, transactional checkpoints/watermarks and restart recovery. REST repair cannot fabricate upstream revision IDs or reset MQTT health. |
| Fanout/storage | Redis latest-state/evaluation fanout; primary/subscriber roles; private PostgreSQL/file archives and forecast logs; private Parquet export. TimescaleDB currently hosts PostgreSQL tables. |
| Offline evaluation | Seventy acquired 2023–2025 race archives; disjoint race training/calibration/holdout groups; checksummed ONNX artifacts and Python/native parity. Rust backtest CLI runs normalised events through Replay/RaceState/predict/PredictionLog; a separate scorer labels original Rust-issued risk probabilities at exact issue timestamps and preserves original opponents. |
| Operations | Docker image with static frontend and native inference; Compose PostgreSQL/Redis; CI configuration; health, Prometheus counters, structured tracing and optional OTLP export. |

## Measured models

Pace/pit source: [manifest](../models/manifest.json), model `openf1-lgbm-v2-24c99efbc5`. These are the existing offline causal-feature scores, not a substitute for Rust replay scores.

| Metric | Measured | PRD target |
| --- | --- | --- |
| Next-lap MAE | 1.556 s; 6,265 samples | ≤0.35 s for next 1–5 laps |
| Next 1–5 lap MAE | 1.869 s; 24,951 horizon samples | ≤0.35 s |
| Approximate 80% band coverage | 73.58% | Validated visible uncertainty |
| Pit ±2, issued ten laps before stop | 26.03%; 219 scored stops | ≥60% |
| Pit forecast coverage | 219/461 eligible stops; 47.51% | Report with accuracy |
| Finish ±1 at half distance | Unscored without required sourced metadata | ≥70% |

**Pace and pit accuracy targets fail.** Current causal features omit unavailable pit-loss/team-history inputs. Missing pit records are excluded from hazard labels, not interpreted as no stops.

Risk source: [risk manifest](../models/risk_manifest.json), content-versioned model `openf1-calibrated-logistic-risk-v1-fe39d3c3754071eb`. Training/calibration/holdout use 42/14/14 disjoint races. Circuit priors use training races only, with each training row's own race excluded.

| Held-out metric | SC/VSC | Overtake |
| --- | --- | --- |
| Scored/candidate windows | 667/834 | 6,450/15,417 |
| Eligibility coverage | 79.98% | 41.84% |
| Positive/negative labels | 60/607 | 695/5,755 |
| Brier score; lower is better | 0.083096 | 0.076957 |
| Constant training-rate baseline | 0.082696 | 0.096775 |
| Maximum ONNX probability parity error | 9.31e-8 | 1.79e-7 |

SC is slightly worse than its constant baseline; forecast factors disclose this. Overtake labels are position-observed reversals conditional on five green laps without pits, retirement or truncated horizons. Sparse position data and administrative corrections cannot establish exact physical passes. DRS/pace-delta features and sprint calibration are absent.

Runtime probabilities additionally require a pre-known schedule with five remaining laps. The bundled schedule currently covers only Singapore 2023. Offline risk eligibility coverage does not establish runtime issuance coverage across the calendar. Risk replay scoring uses the risk manifest's split independently of the pace split; an unissued/unscored forecast has a null score.

## Full Rust replay evaluation

On 2026-11-04, the release backtest completed all 70 private archives through the Rust ingest normalisers, Replay, RaceState, prediction engine and PredictionLog. It replayed every driver lap, pit, status and weather event, batching simultaneous receipts. The pace manifest selects 14 holdout races; the separate risk manifest also selects 14 holdout races. Final REST documents approximate event completion and cannot reproduce original upstream receipt/revision history.

| Rust replay holdout metric | Measured | PRD target |
| --- | --- | --- |
| Next 1–5 lap MAE | 2.703456 s; 38,907 scored horizons | ≤0.35 s |
| Scored/issued pace horizons | 38,907/44,735; 86.97% | Report coverage |
| Approximate 80% band coverage | 73.68% | Validated visible uncertainty |
| Pit ±2, issued ten laps before stop | 20.93%; 301 scored stops | ≥60% |
| Pit forecast coverage | 301/461 candidate stops; 65.29% | Report with accuracy |
| Finish ±1 at half distance | Null; zero scored drivers | ≥70% |

These are engine-level results, including its labelled `basic-bayesian-v2` fallback and `openf1-lgbm-v2-24c99efbc5` inference where eligible. They use a different issue cadence and population from the offline feature table above. The full 70-race run scored 205,913 pace horizons, but its all-selected aggregate includes training/calibration races and is not held-out evidence. Both measured pace/pit evaluations miss their PRD targets.

Original Rust risk forecasts were issued only for Singapore 2023 session 9165, which is outside the risk holdout. The scorer retained the issue timestamp, original opponent and model identity:

| Original runtime risk metric, Singapore 2023 only | SC/VSC | Overtake |
| --- | --- | --- |
| Scored/issued forecasts | 56/56 | 595/812 |
| Outcome eligibility coverage | 100% | 73.28% |
| Brier | 0.162488 | 0.088351 |
| Risk holdout issued/scored | 0/0 | 0/0 |
| Risk holdout Brier | Null; unmeasured | Null; unmeasured |

Overtake censoring excluded 39 pit windows, 148 neutralised windows, 20 incomplete target horizons and 10 incomplete driver horizons. These Singapore scores verify original-forecast scoring, not generalisation. No holdout risk probabilities were issued because sourced schedules are unavailable for those races; outcome eligibility is not calendar issuance coverage.

Reproduce with the README's `backtest -- --all-races` and `training/score_replay.py` commands. Reports remain private at `data/training/rust-backtest.json` and `data/training/rust-risk-score.json`; no raw race data is committed.

## Remaining acceptance gates

| Gate | Current limit |
| --- | --- |
| Authenticated live | MQTT/auth/renewal/reconciliation are implemented. Sponsor credentials were not supplied; actual sponsor E2E remains unverified. |
| Circuit coverage | [Coverage manifest](../metadata/coverage.json) includes one verified pre-race schedule: Singapore 2023 session 9165, 62 laps. Other schedules and licensed corner/DRS/sector/pit-lane geometry remain unavailable. Prior-session pit-loss calibration is not bundled. |
| Finishing quality | Sequential replacement policy, fresh same-compound inventory, warm-up and uniform SC onset remain assumptions. SC scenarios omit later deployments, bunching and pit-loss discounts. Ambiguous inactive participants withhold classification; no retirement is inferred. Half-distance accuracy remains unscored without required metadata. |
| Original feed history | REST gives final documents, not original arrivals/revisions. Captured journals preserve receipts only from capture onward. Capture gaps and pre-capture history cannot be reconstructed exactly. |
| Production performance | Local checks do not establish 10,000 clients, p95 feed-to-screen <1 s, inference <500 ms under load, panel <300 ms on production hardware, 4G <2.5 s, 60 fps, all target browsers, WCAG 2.1 AA or 99.5% availability. |
| Deployment/observability | Hosted CI, public deployment and an actual OTLP collector export remain unverified. Automatic time-series retention deletion is not installed. |

## Verification scope

The prediction completion change includes focused regression checks for model schemas/checksums/native parity, feature freshness and horizon guards, multi-stop age resets, shared SC scenarios, stale-field suppression, scheduling expiry boundaries, metadata provenance and replay causality. UI checks cover unavailable probabilities, horizons, targets, model limitations and existing live/model labels; TypeScript and the production frontend build are checked. Python scoring checks use original probabilities, issue times, risk-specific holdout groups and censoring rules.

Database volumes were not reset, sponsor access was not invented, and the stopped application was not restarted for this work. Browser/production load and full accessibility certification are separate external acceptance gates. Work is on `feat/prediction-completion`, using atomic commits without coauthor attribution; no push or merge is implied.
