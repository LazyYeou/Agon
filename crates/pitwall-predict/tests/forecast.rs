use pitwall_predict::*;
use pitwall_state::*;
use std::sync::Arc;
fn race() -> pitwall_core::Snapshot {
    let a = Arc::new(demo_archive());
    let start = a.session.start_ms;
    let mut r = Replay::new(a);
    r.seek(start + 480_000);
    r.state().snapshot()
}
#[test]
fn forecasts_have_ordered_widening_bands_and_finish_probabilities() {
    let s = race();
    let p = predict(&s);
    assert_eq!(p.len(), 20);
    let p = &p[0];
    assert_eq!(p.pace.len(), 5);
    for band in &p.pace {
        assert!(band.validate().is_ok());
    }
    assert!(p.pace[4].high - p.pace[4].low > p.pace[0].high - p.pace[0].low);
    assert!((p.finish.iter().map(|(_, v)| v).sum::<f64>() - 1.0).abs() < 0.001);
    assert!(p.model.contains("basic"));
    assert_eq!(p.factors.len(), 3);
}
#[test]
fn red_flags_pause_and_sc_lowers_confidence() {
    let mut s = race();
    let normal = predict(&s)[0].confidence;
    s.track_status = "SC".into();
    assert!(predict(&s)[0].confidence < normal);
    s.track_status = "RED".into();
    let p = predict(&s);
    assert_eq!(p[0].status, "paused");
    assert!(p[0].pace.is_empty());
}
#[test]
fn forecast_log_scores_only_completed_actuals_and_deduplicates() {
    let mut log = PredictionLog::default();
    let mut s = race();
    log.record(&s, &predict(&s));
    let count = log.records.len();
    log.record(&s, &predict(&s));
    assert_eq!(log.records.len(), count);
    assert_eq!(log.report().pace_samples, 0);
    let a = Arc::new(demo_archive());
    let start = a.session.start_ms;
    let mut r = Replay::new(a);
    r.seek(start + 560_000);
    s = r.state().snapshot();
    log.observe(&s);
    assert!(log.report().pace_samples > 0);
}
#[test]
fn qualifying_cutoff_has_uncertainty_and_is_not_a_race_forecast() {
    let mut s = race();
    s.session.name = "Qualifying Q1".into();
    let predictions = predict(&s);
    assert!(predictions.iter().all(|p| p.qualifying_need.is_some()));
    for p in predictions {
        let band = p.qualifying_need.unwrap();
        assert!(band.validate().is_ok());
    }
    s.session.name = "Race".into();
    assert!(predict(&s).iter().all(|p| p.qualifying_need.is_none()));
}
#[test]
fn strategy_success_uses_gap_and_refuses_missing_pit_loss() {
    let mut s = race();
    s.drivers.get_mut(&4).unwrap().gap = Some(1.0);
    let near = undercut(&s, 4, 1);
    s.drivers.get_mut(&4).unwrap().gap = Some(100.0);
    let far = undercut(&s, 4, 1);
    assert!(near.undercut_success.unwrap() > far.undercut_success.unwrap());
    s.circuit.pit_loss = None;
    assert!(undercut(&s, 4, 1).undercut.is_none());
}
#[test]
fn pit_exit_does_not_forecast_using_the_previous_tyres_fit() {
    let archive = std::sync::Arc::new(demo_archive());
    let start = archive.session.start_ms;
    let mut replay = Replay::new(archive);
    replay.seek(start + 600000);
    let p = predict(replay.state().view())
        .into_iter()
        .find(|p| p.driver == 1)
        .unwrap();
    assert!(p.pace.is_empty());
    assert!(p.factors[0].contains("0 completed"));
}

fn measured_race(count: u32) -> pitwall_core::Snapshot {
    use pitwall_core::*;
    let mut s = Snapshot::empty(Session {
        key: 42,
        name: "Race".into(),
        synthetic: true,
        total_laps: Some(30),
        ..Default::default()
    });
    s.clock_ms = count as i64 * 90_000;
    s.lap = count;
    s.track_status = "GREEN".into();
    s.circuit.pit_loss = Some(Band {
        value: 22.0,
        low: 20.0,
        high: 24.0,
    });
    for number in 1..=2 {
        s.drivers.insert(
            number,
            DriverState {
                driver: Driver {
                    number,
                    code: format!("D{number}"),
                    ..Default::default()
                },
                lap: count,
                tyre_age: Some(count),
                gap: Some((number - 1) as f64 * 2.0),
                stints: vec![Stint {
                    number: 1,
                    compound: "MEDIUM".into(),
                    start_lap: 1,
                    ..Default::default()
                }],
                laps: (1..=count)
                    .map(|n| Lap {
                        number: n,
                        time_ms: n as i64 * 90_000,
                        duration: Some(90.0 + 0.065 * n as f64),
                        sectors: vec![Some(30.0), Some(30.0), Some(30.0)],
                        speed_trap: Some(320.0),
                        tyre_age: Some(n),
                        status: "GREEN".into(),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            },
        );
    }
    s
}

#[test]
fn cliff_requires_a_sustained_observed_changepoint() {
    let mut s = measured_race(12);
    assert!(
        predict(&s)[0].laps_to_cliff.is_none(),
        "linear wear is not evidence of a cliff"
    );
    for lap in &mut s.drivers.get_mut(&1).unwrap().laps {
        if lap.number >= 8 {
            *lap.duration.as_mut().unwrap() += 1.2 * (lap.number - 7) as f64;
        }
    }
    let p = predict(&s);
    let cliff = p[0]
        .laps_to_cliff
        .as_ref()
        .expect("sustained accelerated wear detected");
    assert_eq!(cliff.value, 0.0);
    assert!(cliff.validate().is_ok());
    assert!(p[0].insight.contains("changepoint"));
}

#[test]
fn bayesian_stint_fit_ignores_future_and_previous_stint_laps() {
    let mut s = measured_race(12);
    let original = predict(&s)[0].clone();
    let before = original.degradation.clone().unwrap();
    let mut future = s.drivers[&1].laps[0].clone();
    future.number = 13;
    future.time_ms = s.clock_ms + 90_000;
    future.duration = Some(400.0);
    s.drivers.get_mut(&1).unwrap().laps.push(future);
    assert_eq!(predict(&s)[0], original);
    s.drivers.get_mut(&1).unwrap().stints[0].start_lap = 10;
    let short = predict(&s)[0].degradation.clone().unwrap();
    assert!(short.high - short.low > before.high - before.low);
    assert!((before.value - 0.1).abs() < 0.01);
}

#[test]
fn rolling_sector_and_speed_anomalies_are_statistical_insights() {
    let mut s = measured_race(12);
    for lap in &mut s.drivers.get_mut(&1).unwrap().laps[9..] {
        lap.sectors[1] = Some(30.8);
        lap.speed_trap = Some(305.0);
    }
    let p = predict(&s);
    assert!(p[0].insight.contains("sector 2"));
    assert!(p[0].insight.contains("speed trap"));
    assert!(p[0].factors.iter().any(|v| v.contains("z-score")));
    assert!(!p[0].insight.contains("damage"));
    assert!(!p[1].insight.contains("anomaly"));
    s.track_status = "SC".into();
    assert!(!predict(&s)[0].insight.contains("anomaly"));
}

#[test]
fn qualifying_cutoff_projects_observed_evolution_and_refuses_sparse_or_unknown_phase() {
    let mut s = measured_race(3);
    s.session.name = "Qualifying".into();
    s.qualifying_phase = Some(2);
    s.qualifying_start_ms = Some(0);
    for number in 3..=10 {
        let mut d = s.drivers[&2].clone();
        d.driver.number = number;
        s.drivers.insert(number, d);
    }
    for d in s.drivers.values_mut() {
        for l in &mut d.laps {
            l.duration = Some(92.0 + d.driver.number as f64 * 0.1 - l.number as f64 * 0.3);
        }
    }
    let p = predict(&s);
    let need = p
        .iter()
        .find(|p| p.driver == 10)
        .unwrap()
        .qualifying_need
        .as_ref()
        .unwrap();
    assert!(
        need.value > 0.28,
        "the cutoff should improve over the next flying lap"
    );
    assert!(need.validate().is_ok());
    assert!(p[0]
        .factors
        .iter()
        .any(|v| v.contains("evolution regression")));
    s.qualifying_phase = None;
    assert!(predict(&s).iter().all(|p| p.qualifying_need.is_none()));
    s.qualifying_phase = Some(2);
    for d in s.drivers.values_mut() {
        d.laps.truncate(1);
    }
    assert!(predict(&s).iter().all(|p| p.qualifying_need.is_none()));
}

#[test]
fn finish_simulation_accounts_for_future_pit_loss_and_refuses_unknown_inputs() {
    use pitwall_core::Stint;
    let mut s = measured_race(12);
    s.session.total_laps = Some(16);
    // Only driver 1 has an observed same-compound completed-stint prior.
    s.drivers.get_mut(&2).unwrap().stints[0].compound = "HARD".into();
    s.drivers.get_mut(&1).unwrap().stints[0].start_lap = 4;
    s.drivers.get_mut(&1).unwrap().stints[0].tyre_age_at_start = 3;
    s.drivers.get_mut(&1).unwrap().stints.insert(
        0,
        Stint {
            number: 0,
            compound: "MEDIUM".into(),
            start_lap: 1,
            end_lap: Some(3),
            tyre_age_at_start: 10,
            ..Default::default()
        },
    );
    let finish = predict(&s);
    let win = |p: &pitwall_core::Prediction| {
        p.finish
            .iter()
            .find(|(pos, _)| *pos == 1)
            .map_or(0.0, |(_, p)| *p)
    };
    assert!(
        win(&finish[0]) < 0.2,
        "the leader's imminent stop must affect the result"
    );
    assert!(win(&finish[1]) > 0.8);
    s.circuit.pit_loss = None;
    assert!(predict(&s).iter().all(|p| p.finish.is_empty()));
    s.session.total_laps = None;
    assert!(predict(&s).iter().all(|p| p.finish.is_empty()));
}

#[test]
fn log_scores_pit_at_ten_laps_and_finish_at_half_distance_once() {
    use pitwall_core::*;
    let mut s = measured_race(10);
    s.session.total_laps = Some(20);
    let mut p = predict(&s);
    for p in &mut p {
        p.pit_window = Some(PitWindow {
            from_lap: 19,
            to_lap: 21,
            probability: 0.8,
            distribution: vec![(20, 0.8)],
        });
        p.finish = vec![(p.driver, 1.0)];
    }
    let mut log = PredictionLog::default();
    log.record(&s, &p);
    log.record(&s, &p);
    assert_eq!(log.report().pit_samples, 0);
    assert_eq!(log.report().position_samples, 0);
    s.lap = 20;
    for d in s.drivers.values_mut() {
        d.pits.push(Pit {
            lap: 20,
            time_ms: 1_800_000,
            ..Default::default()
        });
        d.laps.push(Lap {
            number: 20,
            time_ms: 1_800_000,
            position: Some(d.driver.number),
            ..Default::default()
        });
    }
    s.clock_ms = 1_800_000;
    log.observe(&s);
    log.observe(&s);
    let report = log.report();
    assert_eq!(report.pit_samples, 2);
    assert_eq!(report.pit_accuracy, Some(1.0));
    assert_eq!(report.position_samples, 2);
    assert_eq!(report.position_accuracy, Some(1.0));
    // A run issued one lap too late cannot count towards the horizon-10 target.
    let mut late = PredictionLog::default();
    for p in &mut p {
        p.lap = 11;
    }
    s.lap = 11;
    late.record(&s, &p);
    late.observe(&s);
    assert_eq!(late.report().pit_samples, 0);
    assert_eq!(late.report().position_samples, 0);
}

#[test]
fn half_distance_score_includes_lapped_drivers_at_the_same_race_instant() {
    use pitwall_core::*;
    let mut s = measured_race(10);
    s.session.total_laps = Some(20);
    s.drivers.get_mut(&2).unwrap().lap = 9;
    let mut predictions = predict(&s);
    for p in &mut predictions {
        p.finish = vec![(p.driver, 1.0)];
    }
    let mut log = PredictionLog::default();
    log.record(&s, &predictions);
    s.lap = 20;
    s.clock_ms = 2_000_000;
    for d in s.drivers.values_mut() {
        d.laps.push(Lap {
            number: 20,
            time_ms: 1_900_000,
            position: Some(d.driver.number),
            ..Default::default()
        });
    }
    log.observe(&s);
    assert_eq!(log.report().position_samples, 2);
}

#[test]
fn q2_cutoff_requires_a_phase_start_to_exclude_q1_laps() {
    let mut s = race();
    s.session.name = "Qualifying".into();
    s.qualifying_phase = Some(2);
    s.qualifying_start_ms = None;
    assert!(predict(&s).iter().all(|p| p.qualifying_need.is_none()));
}

#[test]
fn pit_prior_uses_only_completed_observed_stints() {
    let mut s = measured_race(12);
    s.drivers.get_mut(&1).unwrap().stints.insert(
        0,
        pitwall_core::Stint {
            number: 0,
            compound: "MEDIUM".into(),
            start_lap: 1,
            end_lap: Some(20),
            ..Default::default()
        },
    );
    assert!(predict(&s).iter().all(|p| p.pit_window.is_none()));
}
