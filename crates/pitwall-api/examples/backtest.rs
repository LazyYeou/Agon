//! Private offline evaluation: no server, network requests or raw-data publication.
use anyhow::{ensure, Context, Result};
use pitwall_core::*;
use pitwall_predict::{predict, PredictionLog};
use pitwall_state::Replay;
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

fn document_archive(document: &Value) -> Result<SessionArchive> {
    let mut session = pitwall_ingest::session_from_openf1(&document["session"])
        .context("Invalid OpenF1 session")?;
    let laps = document["laps"]
        .as_array()
        .context("Missing lap documents")?;
    let numbers: BTreeSet<_> = laps
        .iter()
        .filter_map(|r| r["driver_number"].as_u64())
        .filter(|n| (1..=999).contains(n))
        .collect();
    // Identities are not model features; missing names remain numeric rather than invented.
    let drivers = numbers
        .iter()
        .map(|&number| Driver {
            number: number as u32,
            code: format!("D{number}"),
            name: format!("Driver {number}"),
            team: String::new(),
            color: "a5b5c9".into(),
        })
        .collect();
    let mut events: Vec<_> = laps
        .iter()
        .filter_map(|row| pitwall_ingest::normalize("laps", row, None))
        .collect();
    for topic in ["position", "intervals", "pit", "race_control", "weather"] {
        let rows = document[topic]
            .as_array()
            .with_context(|| format!("Missing {topic} documents; reacquire history"))?;
        for row in rows {
            if let Some(event) = pitwall_ingest::normalize(topic, row, None) {
                if let EventData::Control(message) = &event.payload {
                    if let Some(status) = pitwall_ingest::status_from_control(message) {
                        events.push(RaceEvent {
                            time_ms: event.time_ms,
                            source_time_ms: event.source_time_ms,
                            driver: None,
                            payload: EventData::Status(status),
                        });
                    }
                }
                events.push(event);
            }
        }
    }
    for stint in document["stints"]
        .as_array()
        .context("Missing stint documents")?
    {
        let start = laps
            .iter()
            .find(|lap| {
                lap["driver_number"] == stint["driver_number"]
                    && lap["lap_number"] == stint["lap_start"]
            })
            .and_then(|lap| pitwall_ingest::timestamp(&lap["date_start"]));
        if let Some(event) =
            start.and_then(|time| pitwall_ingest::normalize("stints", stint, Some(time)))
        {
            events.push(event);
        }
    }
    events.sort_by_key(|e| e.time_ms);
    let mut circuit = Circuit {
        source: "OpenF1 private offline documents; no location geometry imported".into(),
        approximate: true,
        ..Default::default()
    };
    pitwall_ingest::metadata::apply_metadata(&mut session, &mut circuit)?;
    Ok(SessionArchive {
        session,
        drivers,
        events,
        circuit,
        captured_at_ms: None,
    })
}

fn evaluate(archive: SessionArchive) -> Value {
    let key = archive.session.key;
    let scheduled = archive.session.total_laps;
    let pit_loss = archive.circuit.pit_loss.is_some();
    let candidates: BTreeSet<_> = archive
        .events
        .iter()
        .filter_map(|e| match &e.payload {
            EventData::Pit(p) if p.lap > 10 => e.driver.map(|d| (d, p.lap)),
            _ => None,
        })
        .collect();
    let issues: BTreeSet<_> = archive
        .events
        .iter()
        .filter(|e| {
            matches!(
                e.payload,
                EventData::Lap(_)
                    | EventData::Pit(_)
                    | EventData::Status(_)
                    | EventData::Weather(_)
            )
        })
        .map(|e| e.time_ms)
        .filter(|&t| t >= archive.session.start_ms && t <= archive.session.end_ms)
        .collect();
    let end = archive.session.end_ms;
    let mut replay = Replay::new(Arc::new(archive));
    let mut log = PredictionLog::default();
    let mut models = BTreeSet::new();
    let mut risk_models = BTreeSet::new();
    let mut issued = BTreeSet::new();
    let mut sc_issued = BTreeSet::new();
    let mut pass_issued = BTreeSet::new();
    let mut risk_scored = Vec::new();
    for time in issues {
        replay.advance(time);
        let snapshot = replay.state().view();
        log.observe(snapshot);
        let predictions = predict(snapshot);
        for p in &predictions {
            models.insert(p.model.clone());
            if !p.pace.is_empty() {
                issued.insert((p.driver, p.lap));
            }
            if let Some(risk) = &p.safety_car {
                risk_models.insert(risk.model.clone());
                if sc_issued.insert(snapshot.lap) {
                    risk_scored.push(json!({"kind":"sc","lap":snapshot.lap,"time_ms":time,"probability":risk.probability,"model":risk.model}));
                }
            }
            if let Some(pass) = &p.overtake {
                risk_models.insert(pass.forecast.model.clone());
                if pass_issued.insert((p.driver, p.lap)) {
                    risk_scored.push(
                        json!({"kind":"overtake","driver":p.driver,"target":pass.target,"lap":p.lap,
                        "time_ms":time,"probability":pass.forecast.probability,"model":pass.forecast.model}),
                    );
                }
            }
        }
        log.record(snapshot, &predictions);
    }
    replay.advance(end);
    log.observe(replay.state().view());
    let report = log.report();
    let pace_eligible = log.records.len();
    json!({"session":key, "report":report, "models":models, "risk_models":risk_models,
        "scheduled_laps":scheduled,"pit_loss_available":pit_loss,
        "issued_driver_laps":issued.len(), "pace_issued_horizons":pace_eligible,
        "pace_scored_fraction": if pace_eligible==0 {None} else {Some(report.pace_samples as f64/pace_eligible as f64)},
        "pit_candidates":candidates.len(), "pit_forecast_coverage":if candidates.is_empty(){None}else{Some(report.pit_samples as f64/candidates.len() as f64)},
        "sc_forecasts":sc_issued.len(),"overtake_forecasts":pass_issued.len(),
        "risk_forecasts":risk_scored})
}

fn aggregate(races: &[Value]) -> Value {
    let metrics = [
        ("pace_samples", "pace_mae"),
        ("pace_samples", "band_coverage"),
        ("pit_samples", "pit_accuracy"),
        ("position_samples", "position_accuracy"),
    ];
    let mut report = serde_json::Map::new();
    for (count, metric) in metrics {
        let n: u64 = races
            .iter()
            .map(|r| r["report"][count].as_u64().unwrap_or(0))
            .sum();
        let sum: f64 = races
            .iter()
            .map(|r| {
                r["report"][metric].as_f64().unwrap_or(0.0)
                    * r["report"][count].as_u64().unwrap_or(0) as f64
            })
            .sum();
        report.insert(count.into(), json!(n));
        report.insert(
            metric.into(),
            if n == 0 {
                Value::Null
            } else {
                json!(sum / n as f64)
            },
        );
    }
    let candidates: u64 = races
        .iter()
        .map(|r| r["pit_candidates"].as_u64().unwrap_or(0))
        .sum();
    report.insert("pit_candidates".into(), json!(candidates));
    report.insert(
        "pit_forecast_coverage".into(),
        if candidates == 0 {
            Value::Null
        } else {
            json!(report["pit_samples"].as_u64().unwrap_or(0) as f64 / candidates as f64)
        },
    );
    Value::Object(report)
}

fn main() -> Result<()> {
    let mut root = PathBuf::from("data/training");
    let mut output = PathBuf::from("data/training/rust-backtest.json");
    let mut all = false;
    let mut limit = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => root = args.next().context("--root requires a path")?.into(),
            "--output" => output = args.next().context("--output requires a path")?.into(),
            "--limit" => {
                limit = Some(
                    args.next()
                        .context("--limit requires a count")?
                        .parse::<usize>()?,
                )
            }
            "--all-races" => all = true,
            _ => anyhow::bail!(
                "Usage: backtest [--root PATH] [--output PATH] [--all-races] [--limit N]"
            ),
        }
    }
    let model_root =
        PathBuf::from(std::env::var("AGON_MODELS").unwrap_or_else(|_| "models".into()));
    pitwall_predict::ModelBundle::load(&model_root)
        .context("Native trained model required for backtest")?;
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(model_root.join("manifest.json"))?)?;
    let holdout: BTreeSet<_> = manifest["held_out_races"]
        .as_array()
        .context("Holdout split missing")?
        .iter()
        .filter_map(Value::as_u64)
        .collect();
    let mut paths: Vec<_> = std::fs::read_dir(root.join("races"))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter(|p| {
            all || p
                .file_stem()
                .and_then(|n| n.to_str())
                .and_then(|n| n.parse().ok())
                .is_some_and(|k| holdout.contains(&k))
        })
        .collect();
    paths.sort();
    if let Some(limit) = limit {
        paths.truncate(limit);
    }
    ensure!(!paths.is_empty(), "No race archives selected");
    let mut races = vec![];
    let started = std::time::Instant::now();
    for (index, path) in paths.iter().enumerate() {
        ensure!(
            std::fs::metadata(path)?.len() <= 64 * 1024 * 1024,
            "Archive too large"
        );
        let document: Value = serde_json::from_slice(&std::fs::read(path)?)?;
        let archive =
            document_archive(&document).with_context(|| format!("Convert {}", path.display()))?;
        let held_out = holdout.contains(&u64::from(archive.session.key));
        let mut result = evaluate(archive);
        result["held_out"] = json!(held_out);
        eprintln!(
            "Race {}/{} session {}: {} scored pace horizons, {:.1}s elapsed",
            index + 1,
            paths.len(),
            result["session"],
            result["report"]["pace_samples"],
            started.elapsed().as_secs_f64()
        );
        races.push(result);
    }
    let held_out: Vec<_> = races
        .iter()
        .filter(|r| r["held_out"] == true)
        .cloned()
        .collect();
    let report = json!({"engine":"Rust RaceState/Replay/predict/PredictionLog", "complete":true,
        "selected_races":races.len(),"held_out_races":held_out.len(),
        "cadence":"Every completed driver lap, pit, track-status and weather event; simultaneous receipts batched",
        "held_out":aggregate(&held_out),"all_selected":aggregate(&races),"races":races,
        "limitations":["REST final documents approximate physical completion, not original upstream revision arrivals.",
            "Only separately sourced, pre-known scheduled distance and prior-session pit loss enable finish scoring.",
            "Risk records are original Rust-issued probabilities; risk holdout scores/label eligibility live in risk_manifest.json.",
            "All-selected aggregate includes training/calibration races when --all-races is used; held-out aggregate is separate."]});
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = output.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(&report)?)?;
    std::fs::rename(&temporary, &output)?;
    println!(
        "{}",
        json!({"report":output,"held_out":report["held_out"],"races":report["selected_races"]})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_replay_hides_future_laps_and_stint_endings() {
        let document = json!({
            "session": {"session_key":100, "year":2023, "session_name":"Race", "circuit_short_name":"Test",
                "date_start":"2023-01-01T00:00:00Z", "date_end":"2023-01-01T01:00:00Z"},
            "laps": [
                {"driver_number":1,"lap_number":1,"date_start":"2023-01-01T00:00:00Z","lap_duration":90},
                {"driver_number":1,"lap_number":2,"date_start":"2023-01-01T00:01:30Z","lap_duration":91}
            ],
            "stints":[{"driver_number":1,"stint_number":1,"lap_start":1,"lap_end":99,"compound":"MEDIUM","tyre_age_at_start":0}],
            "race_control":[{"date":"2023-01-01T00:00:00Z","message":"GREEN LIGHT"}],
            "pit":[], "weather":[], "intervals":[], "position":[]
        });
        let archive = document_archive(&document).unwrap();
        let start = archive.session.start_ms;
        assert!(archive.session.total_laps.is_none());
        let mut replay = Replay::new(Arc::new(archive));
        replay.advance(start + 89_999);
        assert_eq!(replay.state().view().lap, 0);
        replay.advance(start + 90_000);
        let d = &replay.state().view().drivers[&1];
        assert_eq!(d.lap, 1);
        assert_eq!(d.stints[0].end_lap, None);
        let outputs = predict(replay.state().view());
        assert!(outputs.iter().all(|p| p.lap == 1));
    }
}
