# Agon — Product Requirements Document

Nov 1, 2026 · @LazyYeou

## 1. Overview

Agon is a real-time F1 timing web app that tells fans what is about to happen, not only what just happened. It keeps everything a classic live-timing tower shows (gaps, sectors, tyres, track map) and adds a live prediction layer: pit-stop timing, pace forecasts for the next laps, tyre degradation curves, undercut/overcut chances and projected finishing order.

**Problem.** Today's live-timing tools (like the reference screen) are excellent mirrors of the data feed, but the fan must do the strategy maths in their head. Questions such as "when will he pit?", "is that tyre dropping off?" or "can he undercut the car ahead?" go unanswered until the commentators guess.

**What makes it different**

- **Predictive, not just descriptive:** every driver row carries a forecast (next pit lap, predicted pace, cliff risk) with a confidence band.
- **Telemetry made readable:** one click on a driver opens a full telemetry panel that explains itself (corner-by-corner, plain-language insights) instead of raw traces only.
- **Live comparison charts:** race pace, tyre deg, gaps and sector deltas between any drivers, updating as laps complete.
- **Rust end to end:** Rust ingestion, prediction serving and (optionally) a Rust/WASM frontend for low latency and low hosting cost.

## 2. Goals, non-goals and success metrics

**Goals**

1. Show complete live timing with end-to-end latency under 1 s from data arrival to screen.
2. Deliver live AI predictions (pit lap, pace, tyre deg, positions) refreshed every lap, with visible confidence.
3. Let any user open a driver's full telemetry in one click and understand it without F1-engineering knowledge.
4. Provide real-time multi-driver comparison charts.
5. Replay any past session (2023 onward) with the same predictions, for testing and for fans who missed the race.

**Non-goals (v1)**

- Video streaming, team radio audio playback, betting or odds.
- Official F1 branding, logos or broadcast footage.
- Native mobile apps (responsive web only).
- Fantasy-league management.

**Success metrics**

| Metric | Target |
| --- | --- |
| Feed-to-screen latency (p95) | < 1 s after data reaches our ingest |
| Pit-lap prediction accuracy | ≥ 60% within ±2 laps, measured 10 laps before the stop |
| Next-5-laps pace forecast error (MAE) | ≤ 0.35 s per lap on green-flag laps |
| Finishing-position forecast at 50% race distance | ≥ 70% of drivers within ±1 position |
| Concurrent viewers per server node | ≥ 10,000 WebSocket clients |
| Telemetry panel open time | < 300 ms after click |
| Weekly returning users during season | Track from launch; target set after 3 races |

Accuracy targets are initial guesses to be validated on 2023–2025 backtests before launch.

## 3. Target users

| Persona | Who | What they need most |
| --- | --- | --- |
| Strategy nerd | Watches every session with timing open on a second screen | Pit predictions, undercut windows, deg curves, comparison charts |
| Casual fan | Watches the race on TV, checks the app now and then | A simple "what's going to happen" summary and plain-language insights |
| Data enthusiast / student | Learns data science or motorsport engineering | Deep telemetry, exportable data, model explanations |
| Content creator | Posts live analysis on social media | Shareable chart snapshots and clean visuals |

## 4. Core live timing features

The base screen matches the reference layout (tower on the left, track map on the right) so experienced users feel at home, then adds prediction columns.

**4.1 Timing tower (one row per driver)**

| Column | Content | Notes |
| --- | --- | --- |
| Position + change | Current position, arrow for places gained/lost | Animated on change |
| Driver | Code, team colour, car number | Click opens telemetry panel (section 6) |
| Gap / Interval | Gap to leader and to car ahead; toggle | Coloured when within 1.0 s (DRS range) |
| Last lap / Best lap | Lap times with purple/green/yellow colouring | Standard F1 colour code |
| Sectors + mini-sectors | Last and best sector times, mini-sector bar | Same as reference |
| Tyre | Compound icon, tyre age (laps), stint number, new/used |  |
| Pit | In pit / pit count / last pit duration |  |
| **Pred. pace** (new) | Forecast lap time for next lap ± band | AI, section 5 |
| **Pit window** (new) | Predicted pit lap range, e.g. "L34–37" + probability | AI, section 5 |
| **Tyre health** (new) | Bar showing estimated remaining tyre life; cliff warning icon | AI, section 5 |
| **Proj. finish** (new) | Predicted finishing position | AI, section 5 |

Users can hide/reorder columns and save layouts locally.

**4.2 Session header** – event, session, lap counter / clock, track status (green, yellow, SC, VSC, red), weather (track, air, humidity, wind, rain).

**4.3 Live track map** – circuit outline drawn from position data, a dot per car in team colour, sector colouring, flag zones, DRS zones, pit lane. Hover shows driver and gap; clicking selects the driver. Positions are interpolated between samples so cars move smoothly.

**4.4 Race control feed** – flags, penalties, investigations, track limits, safety car messages, filterable by driver.

**4.5 Session modes** – Practice, Qualifying (knockout line, "needs X.XXX to get through" estimate) and Race/Sprint each get a tuned tower.

**4.6 Replay mode** – any session from 2023 onward can be replayed at 1×–20× speed with a timeline scrubber; predictions are recomputed as if live.

## 5. AI and real-time prediction features

Every prediction updates at least once per completed lap (and instantly on events like SC, VSC, rain, pit entry), always shows a confidence band, and can be expanded to see *why* (top contributing factors).

| # | Feature | What the user sees | Inputs | Approach (v1) | Priority |
| --- | --- | --- | --- | --- | --- |
| P1 | Pace forecast | Predicted lap times for next 1–5 laps per driver, with band | Recent laps, tyre compound + age, fuel load estimate, track temp, traffic gap | Per-stint degradation model + gradient-boosted residual model | Must |
| P2 | Tyre degradation & cliff | Deg rate (s/lap), estimated laps left before cliff, warning icon | Fuel-corrected lap times, compound, track temp, sector-level drop | Piecewise linear / Bayesian regression per stint, cliff = changepoint detection | Must |
| P3 | Pit-stop prediction | "Likely pit lap 34–37 (68%)" + probability curve per lap | Tyre age vs typical stint length, deg, gap to car behind, pit loss, SC status, team history | Survival / hazard model (gradient boosting on historic stints) | Must |
| P4 | Undercut / overcut calculator | For any pair of drivers: "Undercut works if he pits now: +0.8 s" | Pit loss for this track, out-lap estimate, pace forecasts of both cars, gap | Simulation using P1 + P2 outputs | Must |
| P5 | Projected finishing order | Predicted final classification + probability of each position | All of above, remaining laps, SC probability | Monte Carlo race simulation (1,000+ runs per lap) | Should |
| P6 | Pit rejoin position | Ghost marker on map: where the car would rejoin if it pits now | Pit loss, live gaps | Deterministic projection | Should |
| P7 | Safety car probability | % chance of SC/VSC in next 5 laps | Track history, incidents so far, weather, laps remaining | Calibrated classifier on historic races | Could |
| P8 | Overtake probability | Chance driver A passes B within N laps | Gap trend, pace delta, DRS, tyre offset, track overtaking difficulty | Logistic model | Could |
| P9 | Qualifying cut-off | Predicted Q1/Q2 cut time and "needs X.XXX" per driver | Track evolution, current laps | Track-evolution regression | Should |
| P10 | Anomaly alerts | "VER sector 2 down 0.4 s in last 3 laps – possible damage/issue" | Mini-sector deltas, speed traps, telemetry | Rolling z-score / isolation forest | Should |
| P11 | AI race narrator | One-line plain-language summaries: "NOR is 2.1 s faster than PIA on fresher tyres; catches him in \~4 laps" | Outputs of P1–P10 | Rule templates in v1; optional LLM summarisation in v2 | Could |

**Rules for every prediction**

- Show a confidence band or probability, never a single bare number.
- Mark predictions as *paused* during red flags and *low confidence* during SC/VSC and changing weather.
- Keep a hidden "prediction log" of every forecast vs actual so accuracy can be scored after each race and shown publicly ("our pit predictions were 72% accurate this weekend").
- Explain on click: top 3 factors driving the forecast (e.g. "tyre age 24 laps, deg +0.09 s/lap, gap behind 22 s").

## 6. Driver telemetry panel

Clicking a driver opens a side panel (desktop) or full-screen sheet (mobile) with everything about that car, organised from simple to deep so it stays easy to read.

**Layer 1 – At a glance (top of panel)**

- Live gauges: speed, gear, RPM, throttle %, brake on/off, DRS state.
- Current stint card: compound, tyre age, deg rate, laps-to-cliff, predicted pit window.
- Position, gap ahead/behind with trend arrows, and a 1-line AI insight ("Losing 0.2 s/lap to HAM, mostly in sector 3").

**Layer 2 – Lap telemetry traces**

- Speed, throttle, brake, gear and RPM traces plotted against **distance around the lap** (not time), stacked and sharing one x-axis.
- Overlay any previous lap of the same driver, their best lap, or another driver's lap; a **delta-time line** shows where time is won or lost.
- Corner markers (T1, T2…) on the x-axis; hovering a point highlights the car's position on a mini-map.

**Layer 3 – Corner-by-corner breakdown**

| Per corner | Shown as |
| --- | --- |
| Braking point | metres before apex, vs reference lap |
| Minimum (apex) speed | km/h, vs reference |
| Exit / traction | time to full throttle |
| Time gained/lost | ± seconds, colour coded |

The track map can be coloured by "who is faster here" (fastest-driver mini-sectors map).

**Layer 4 – Race history**

- Lap-time chart for the whole race with stints shaded by compound and pit stops marked.
- Tyre history table (stints, compounds, laps, average pace, deg rate).
- Speed-trap and sector bests, position-by-lap chart, race-control messages for this driver.

**Ease-of-use requirements**

- Every chart has a one-sentence "what this shows" tooltip.
- Plain-language insights are generated automatically from the data (rule-based in v1).
- A "Beginner / Expert" toggle hides or shows Layers 3–4.
- Telemetry sampling is about 3.7 Hz in the open data source, so traces are smoothed and labelled as approximate; the UI must not imply more precision than the data has.

## 7. Real-time comparison charts

A "Charts" tab lets users pick 2–6 drivers and see live-updating comparisons; each chart can also be pinned beside the timing tower.

| Chart | What it compares | Updates |
| --- | --- | --- |
| Race pace | Lap times per lap (fuel-corrected option), outliers like SC/pit laps greyed out, rolling 3-lap average | Each lap |
| Tyre degradation | Lap time vs tyre age per stint, with fitted deg line and predicted continuation (dashed) | Each lap |
| Gap evolution | Gap between chosen drivers over laps ("is he catching?"), with predicted catch-up lap | Each lap |
| Gap to leader (race trace) | Classic race-history chart, all selected drivers | Each lap |
| Sector deltas | Per-sector average difference between two drivers over last N laps | Each lap |
| Position chart | Positions by lap, pit stops marked | Each lap |
| Stint strategy | Horizontal bars of compounds per driver, actual + predicted remaining stints | On pit / prediction change |
| Speed traps | Top speed comparison (straight-line speed vs downforce trade-off) | Each lap |
| Prediction vs actual | Predicted lap time band vs real lap time, to build trust in the model | Each lap |

**Chart requirements**

- New points animate in without redrawing the whole chart; 60 fps pan/zoom on a mid-range laptop.
- Hover crosshair shared across stacked charts.
- Export as PNG / share link that opens the same view (driver set, chart, lap range).
- Colour = team colour; teammates distinguished by solid vs dashed line.

## 8. Data sources and legal considerations

The recommended primary source is [OpenF1](https://openf1.org): live data during sessions needs the €9.90/month sponsor tier, historical data (2023 onward) is free.

| Source | Use | Key facts | Risk |
| --- | --- | --- | --- |
| [OpenF1](https://openf1.org) – sponsor tier | Live ingest | 18 endpoints; live via REST, MQTT and WebSocket; \~3 s delay after real events; car telemetry and location at \~3.7 Hz; positions/intervals every \~4 s; 6 req/s, up to 10 concurrent streams | Licence is non-commercial (CC BY-NC-SA 4.0) |
| [OpenF1](https://openf1.org) – free tier | Historical data, model training, replay | All sessions since 2023, JSON/CSV, 3 req/s and 30 req/min | Same licence |
| Official F1 live-timing SignalR feed (unofficial use) | Optional lower-latency fallback | Used by community tools; undocumented | Terms-of-use risk; may break without notice |
| FastF1 historical archives | Extra training data (pre-2023) | Python library; data exported offline to Parquet | Same unofficial-data caveats |

**Legal and branding rules**

- Launch as a **non-commercial fan / educational project** to stay within OpenF1's licence; contact OpenF1 and seek an official data licence before any monetisation.
- No F1, FIA or team logos or trademarks; use generic team colours and driver codes; add a clear "unofficial, not affiliated" disclaimer.
- Do not redistribute raw data feeds; serve derived views and predictions.
- Build a `DataSource` trait in Rust so the source can be swapped without touching the rest of the system.

Open question: whether to add a paid tier later, which would require a commercial data licence.

## 9. System architecture (Rust)

Yes, Rust works for the whole live stack: one Cargo workspace covers ingest, state, predictions, API and even the frontend (via WebAssembly). Only model training stays in Python, offline.

&#91;embedded content: system architecture · live path top to bottom\]

The state engine streams raw timing deltas to the gateway immediately; the prediction engine adds forecasts a few hundred milliseconds later, so live timing never waits on the models.

**Cargo workspace layout**

| Crate | Responsibility | Key libraries |
| --- | --- | --- |
| `pitwall-core` | Shared domain types: `Driver`, `Lap`, `Stint`, `TelemetrySample`, `RaceEvent`, `Prediction` | `serde`, `chrono` |
| `pitwall-ingest` | `DataSource` trait; OpenF1 live (MQTT/WebSocket) and history (REST) clients; replay source from files | `tokio`, `rumqttc`, `tokio-tungstenite`, `reqwest` |
| `pitwall-state` | Per-session race model, lap/stint detection, gap and position tracking, telemetry ring buffers | `tokio`, `dashmap` |
| `pitwall-predict` | Online deg models, ONNX inference, Monte Carlo simulator, prediction log | `ort`, `ndarray`, `nalgebra`, `rayon` |
| `pitwall-api` | WebSocket delta stream, REST for history/telemetry, subscriptions per driver/chart | `axum`, `tower`, `rmp-serde` (MessagePack) |
| `pitwall-store` | Persistence to Postgres/TimescaleDB, Parquet export, Redis snapshot | `sqlx`, `arrow`/`parquet`, `redis` |
| `pitwall-web` | Frontend SPA | `leptos`, `web-sys`; charts on canvas (`plotters` or custom) |

**Frontend choice.** Leptos (Rust → WASM) keeps one language and shares `pitwall-core` types with the server. If faster UI iteration matters more, a fallback is SvelteKit/React with uPlot or ECharts for charts while keeping the whole backend in Rust; types are then generated with `ts-rs`.

**Real-time protocol.** Client opens one WebSocket, receives a full snapshot, then binary delta frames (`{topic, driver, fields}`). Telemetry for a driver streams only after the user opens that driver's panel, which keeps bandwidth low.

**Deployment.** Docker images per service (or one binary with feature flags for small scale), Fly.io / a VPS near the users, Cloudflare in front for static assets; GitHub Actions CI with `cargo clippy`, `cargo test` and replay-based regression tests.

## 10. ML model design and serving

Models are trained offline in Python (fastest ecosystem for experimentation), exported to **ONNX**, and served inside the Rust prediction engine – so no Python runs on race day.

**Training pipeline (offline)**

1. Pull 2023–2025 sessions from OpenF1 (plus FastF1 archives for older seasons) into Parquet.
2. Clean: remove in/out laps, SC/VSC laps, red-flag periods, laps in traffic (gap < 1.5 s) for pace models.
3. Feature engineering: fuel-corrected lap time (≈ linear fuel burn over race distance), tyre age, compound, track temp, track id, stint number, gap ahead/behind, pit-loss per track, laps remaining.
4. Validate with **grouped cross-validation by race** (never split laps of one race across train/test) to avoid leakage.
5. Backtest by replaying full races through the live engine and scoring every forecast.

**Models**

| Prediction | Model | Runs where |
| --- | --- | --- |
| Tyre deg / cliff (P2) | Per-stint online Bayesian linear regression + changepoint detection | Pure Rust (`nalgebra`), updated every lap |
| Pace forecast (P1) | Deg model baseline + LightGBM residual model | ONNX in Rust (`ort` crate) |
| Pit-stop hazard (P3) | LightGBM discrete-time survival model | ONNX in Rust |
| SC / overtake probability (P7, P8) | Calibrated logistic regression / LightGBM | ONNX in Rust |
| Finishing order (P5) | Monte Carlo simulator using P1–P3 + SC probability | Pure Rust, parallel with `rayon` (1,000+ sims < 200 ms) |
| Anomaly alerts (P10) | Rolling z-score on mini-sector and speed-trap deltas | Pure Rust |

**Serving rules**

- Inference triggered on events: lap completed, pit entry/exit, track-status change, weather change.
- Each model output carries `value`, `p10/p90` band, `confidence`, `top_factors`.
- Model versions are tagged; every live forecast is logged with its model version for later scoring.
- Fallback: if a model fails or input is missing, show the simple heuristic (e.g. median stint length per compound) labelled "basic estimate".

## 11. Non-functional requirements

| Area | Requirement |
| --- | --- |
| Latency | Ingest → client p95 < 1 s; prediction update < 500 ms after lap completion |
| Scale | 10,000 concurrent WebSocket clients per node; horizontal scale via Redis pub/sub or NATS fan-out |
| Bandwidth | Delta updates only (changed fields), binary MessagePack frames; < 20 KB/s per client in a race |
| Availability | 99.5% during live sessions; auto-reconnect with state resync on client |
| Resilience | Feed outage → show "data delayed" banner, keep last state; ingest restarts resume from buffered history |
| Performance (client) | First load < 2.5 s on 4G; 60 fps map animation; works on Chrome, Firefox, Safari, Edge (last 2 versions) |
| Accessibility | Colour-blind-safe palettes (purple/green not the only signal), keyboard navigation, WCAG 2.1 AA |
| Responsive | Desktop multi-panel; tablet two-column; phone single-column with swipe between Tower / Map / Charts |
| Timezone | Session times shown in the user's local time (e.g. WIB for Indonesia) |
| Security | Rate limiting per IP, no user secrets stored in v1, data-source API keys server-side only |
| Observability | Metrics (Prometheus), tracing (`tracing` + OpenTelemetry), per-model accuracy dashboard |

## 12. Roadmap, risks and open questions

The plan reaches a live public beta with the core predictions by the 2027 season opener; the advanced AI features follow in v2.

&#91;embedded content: roadmap · 5 phases, 4 gates\]

Each gate must pass before the next phase starts; phases are not drawn to scale.

**Risks**

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Data source changes access, pricing or format | Live features stop | `DataSource` trait, replay fallback, monitor OpenF1 changelog |
| Licence limits (non-commercial) | Cannot monetise | Stay non-commercial in v1; seek a data licence before any paid tier |
| \~3 s feed delay and \~3.7 Hz telemetry | Predictions and traces less precise than official tools | Label data as approximate, smooth traces, never claim real-time to the millisecond |
| Model accuracy poor in SC, rain, red flags | Users lose trust | Confidence bands, "low confidence" states, public accuracy scores |
| Rust/WASM frontend slows UI development | Late beta | Fallback to TypeScript frontend with shared generated types |
| Traffic spikes at race start | Outages | Load-test with replays at 2× expected peak; horizontal fan-out via NATS |

**Open questions**

- [ ] Leptos (all-Rust) or a TypeScript frontend for v1?
- [ ] Pay for the OpenF1 sponsor tier only, or also add the official SignalR feed as a lower-latency backup?
- [ ] Should user accounts exist in v1 (saved layouts, favourite drivers) or stay anonymous?
- [ ] Is the LLM race narrator worth its running cost per session?
- [ ] Target audience language: English only, or Bahasa Indonesia too?
