# Prediction completion implementation plan

> **For agentic workers:** Use Superpowers execution and verification for each independently testable deliverable.

**Goal:** Implement the remaining prediction features and the supporting metadata/backtest workflows requested after the PRD gap review.

**Architecture:** Extend the existing Python offline pipeline, Rust prediction/state engine, generated TypeScript contracts and React UI. Reuse private historical archives. No fabricated source metadata or model scores; unavailable inputs withhold affected predictions.

**Tech stack:** Rust, ONNX Runtime, Python/scikit-learn, React/TypeScript, existing OpenF1 data.

**Spec:** `PRD.md` sections 4–7 and 10; `docs/superpowers/specs/2026-11-01-agon-design.md` remains applicable except P7/P8 are now requested.

## Constraints and decisions

- Atomic commits use `feat : message`, `fix : message`, or `chore : message`; no coauthor attribution.
- Work on `feat/prediction-completion` in the existing checkout to reuse its private cache and toolchains.
- Keep checks focused on new behavior and contract compatibility; no repeated full application testing.
- Train/calibration/holdout races remain disjoint. Features use only observations available at issue time.
- P7 predicts a new SC/VSC deployment in the next five laps. P8 predicts an on-track pass of the current car ahead in the next five laps; pit/retirement position changes must not become pass labels.
- Circuit inputs must identify their source, licence and applicable session/configuration; final race distance must never masquerade as a pre-known schedule.
- Missing live sponsor credentials, historical arrivals before capture, public hosting, and production load/SLA evidence cannot be replaced by code or synthetic results.

## Review focus

- Red flags and non-race sessions must not show active race probabilities.
- Missing, corrupt, incompatible models and unknown circuits must remain explicit unavailable states.
- Training labels must exclude truncated horizons, pit passes and future feature leakage.
- Future simulation stops must reset tyre age and must not sample the same first-stop distribution repeatedly.
- Replay scoring must record predictions before observing their actuals and report coverage.

## Tasks

- [ ] **1. SC/overtake models and contracts.** Add optional risk forecast fields to `Prediction`, generated types, offline causal labelled data, calibrated models with held-out metrics/checksums, native Rust inference and focused feature/parity checks. Commit `feat : add calibrated safety car and overtake forecasts`.
- [ ] **2. Multi-stop simulation.** Extend `crates/pitwall-predict/src/simulation.rs` with sequential stint/stop policies and shared sampled neutralisation scenarios driven by the SC forecast, retaining uncertainty and documented assumptions. Check deterministic distributions, multiple-stop age resets and unavailable metadata. Commit `feat : simulate multiple pit stops and safety car scenarios`.
- [ ] **3. Circuit metadata workflow.** Implement sourced, validated session/configuration metadata loading for historical/live paths and private offline replay. Reuse existing approximate outlines; never invent licensed geometry. Check identity, malformed geometry and causal schedule/pit-loss provenance. Commit `feat : load verified circuit configuration metadata`.
- [ ] **4. Full Rust replay backtest.** Add a private CLI to convert acquired OpenF1 documents using existing normalisation, replay all requested races through the state/prediction engine and aggregate held-out scores/coverage. Check issue-before-actual ordering and truncated sessions. Execute against available private archives. Commit `feat : backtest forecasts through the Rust replay engine`.
- [ ] **5. UI and documentation.** Surface P7/P8 probabilities, horizon, target, contributing factors and unavailable states; update README and acceptance with measured results and remaining external gates. Check focused UI rendering, TypeScript/build and diff formatting. Commit `feat : expose race risk forecasts and document validation`.
