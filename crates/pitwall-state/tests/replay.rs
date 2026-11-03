use pitwall_core::*;
use pitwall_state::*;
use std::sync::Arc;

#[test]
fn driver_freshness_tracks_source_time_and_rejects_older_updates() {
    let a = demo_archive();
    let mut state = RaceState::new(a.session, a.drivers, a.circuit);
    for (source, received, position) in [(1000, 2000, 2), (900, 3000, 3)] {
        state.apply(RaceEvent {
            source_time_ms: Some(source),
            time_ms: received,
            driver: Some(1),
            payload: EventData::Position(position),
        });
        state.apply(RaceEvent {
            source_time_ms: Some(source),
            time_ms: received,
            driver: Some(1),
            payload: EventData::Interval {
                gap: Some(2.0),
                interval: Some(1.0),
            },
        });
    }
    let driver = &state.view().drivers[&1];
    assert_eq!(driver.position, Some(2));
    assert_eq!(driver.position_time_ms, Some(1000));
    assert_eq!(driver.interval_time_ms, Some(1000));
}

#[test]
fn qualifying_phase_uses_distinct_source_session_starts() {
    let mut a = demo_archive();
    a.session.name = "Qualifying".into();
    let mut state = RaceState::new(a.session, a.drivers, a.circuit);
    for (time, expected) in [(1000, 1), (1000, 1), (2000, 2), (3000, 3)] {
        state.apply(RaceEvent {
            source_time_ms: Some(time),
            time_ms: time,
            driver: None,
            payload: EventData::Control(ControlMessage {
                time_ms: time,
                driver: None,
                category: "SessionStatus".into(),
                message: "SESSION STARTED".into(),
                flag: None,
                sector: None,
                qualifying_phase: None,
            }),
        });
        assert_eq!(state.view().qualifying_phase, Some(expected));
    }
    assert_eq!(state.view().qualifying_start_ms, Some(3000));
}

#[test]
fn restored_snapshot_rejects_pre_checkpoint_events_but_accepts_new_corrections() {
    let a = demo_archive();
    let start = a.session.start_ms;
    let mut state = RaceState::new(a.session, a.drivers, a.circuit);
    state.apply(RaceEvent {
        source_time_ms: None,
        time_ms: start + 1000,
        driver: Some(1),
        payload: EventData::Position(2),
    });
    let mut restored = RaceState::restore(state.snapshot());
    assert!(!restored.apply(RaceEvent {
        source_time_ms: None,
        time_ms: start + 900,
        driver: Some(1),
        payload: EventData::Position(7)
    }));
    assert!(restored.apply(RaceEvent {
        source_time_ms: None,
        time_ms: start + 1100,
        driver: Some(1),
        payload: EventData::Position(3)
    }));
    assert_eq!(restored.snapshot().drivers[&1].position, Some(3));
}

#[test]
fn pit_entry_marks_the_matching_lap_in_either_arrival_order() {
    for pit_first in [false, true] {
        let a = demo_archive();
        let mut state = RaceState::new(a.session, a.drivers, a.circuit);
        let mut events = vec![
            RaceEvent {
                source_time_ms: None,
                time_ms: 1000,
                driver: Some(1),
                payload: EventData::Lap(Lap {
                    number: 1,
                    time_ms: 1000,
                    duration: Some(90.0),
                    ..Default::default()
                }),
            },
            RaceEvent {
                source_time_ms: None,
                time_ms: 1100,
                driver: Some(1),
                payload: EventData::Pit(Pit {
                    lap: 1,
                    time_ms: 950,
                    lane_duration: Some(25.0),
                    stop_duration: Some(2.0),
                }),
            },
        ];
        if pit_first {
            events.reverse();
        }
        for event in events {
            state.apply(event);
        }
        assert!(state.snapshot().drivers[&1].laps[0].pit_in);
    }
}

#[test]
fn duplicate_laps_do_not_grow_history_and_old_positions_do_not_win() {
    let archive = demo_archive();
    let mut state = RaceState::new(archive.session, archive.drivers, archive.circuit);
    let lap = RaceEvent {
        source_time_ms: None,
        time_ms: 100,
        driver: Some(1),
        payload: EventData::Lap(Lap {
            number: 1,
            time_ms: 100,
            duration: Some(80.0),
            ..Default::default()
        }),
    };
    assert!(state.apply(lap.clone()));
    assert!(!state.apply(lap));
    state.apply(RaceEvent {
        source_time_ms: None,
        time_ms: 200,
        driver: Some(1),
        payload: EventData::Position(2),
    });
    state.apply(RaceEvent {
        source_time_ms: Some(150),
        time_ms: 250,
        driver: Some(1),
        payload: EventData::Position(7),
    });
    assert_eq!(state.snapshot().drivers[&1].laps.len(), 1);
    assert_eq!(state.snapshot().drivers[&1].position, Some(2));
}

#[test]
fn seek_is_deterministic_causal_and_clients_are_isolated() {
    let archive = Arc::new(demo_archive());
    let start = archive.session.start_ms;
    let mut a = Replay::new(archive.clone());
    let mut b = Replay::new(archive);
    a.seek(start + 400_000);
    let expected = a.state().snapshot();
    a.seek(start + 100_000);
    assert!(a
        .state()
        .snapshot()
        .drivers
        .values()
        .all(|d| d.laps.iter().all(|l| l.time_ms <= start + 100_000)));
    assert!(b
        .state()
        .snapshot()
        .drivers
        .values()
        .all(|d| d.laps.is_empty()));
    a.seek(start + 400_000);
    assert_eq!(a.state().snapshot(), expected);
    b.advance(start + 80_000);
    assert!(b.state().snapshot().lap < expected.lap);
}

#[test]
fn telemetry_buffers_are_bounded() {
    let a = demo_archive();
    let mut s = RaceState::new(a.session, a.drivers, a.circuit);
    for i in 0..2_000 {
        s.apply(RaceEvent {
            source_time_ms: None,
            time_ms: i,
            driver: Some(1),
            payload: EventData::Telemetry(TelemetrySample {
                time_ms: i,
                speed: Some(100.0),
                ..Default::default()
            }),
        });
    }
    assert_eq!(s.telemetry(1).len(), 1_200);
}
#[test]
fn partial_sectors_never_count_as_a_completed_lap() {
    let a = demo_archive();
    let mut state = RaceState::new(a.session, a.drivers, a.circuit);
    state.apply(RaceEvent {
        source_time_ms: None,
        time_ms: 100,
        driver: Some(1),
        payload: EventData::Sectors(Lap {
            number: 1,
            time_ms: 100,
            sectors: vec![Some(27.111), None, None],
            ..Default::default()
        }),
    });
    assert_eq!(state.view().drivers[&1].lap, 0);
    assert!(state.view().drivers[&1].laps.is_empty());
    assert_eq!(
        state.view().drivers[&1]
            .partial_lap
            .as_ref()
            .unwrap()
            .sectors[0],
        Some(27.111)
    );
    state.apply(RaceEvent {
        source_time_ms: None,
        time_ms: 200,
        driver: Some(1),
        payload: EventData::Lap(Lap {
            number: 1,
            time_ms: 200,
            duration: Some(90.0),
            ..Default::default()
        }),
    });
    assert_eq!(state.view().drivers[&1].lap, 1);
    assert!(state.view().drivers[&1].partial_lap.is_none());
}
#[test]
fn revisions_preserve_closed_stints_and_update_one_pit_stop() {
    let a = demo_archive();
    let mut s = RaceState::new(a.session, a.drivers, a.circuit);
    let stint = Stint {
        number: 1,
        start_lap: 1,
        compound: "MEDIUM".into(),
        ..Default::default()
    };
    s.apply(RaceEvent {
        source_time_ms: None,
        time_ms: 1,
        driver: Some(1),
        payload: EventData::Stint(stint.clone()),
    });
    s.apply(RaceEvent {
        source_time_ms: None,
        time_ms: 2,
        driver: Some(1),
        payload: EventData::Stint(Stint {
            number: 2,
            start_lap: 8,
            compound: "HARD".into(),
            ..Default::default()
        }),
    });
    s.apply(RaceEvent {
        source_time_ms: None,
        time_ms: 3,
        driver: Some(1),
        payload: EventData::Stint(stint),
    });
    assert_eq!(s.view().drivers[&1].stints[0].end_lap, Some(7));
    let pit = Pit {
        time_ms: 10,
        lap: 7,
        ..Default::default()
    };
    s.apply(RaceEvent {
        source_time_ms: None,
        time_ms: 10,
        driver: Some(1),
        payload: EventData::Pit(pit.clone()),
    });
    s.apply(RaceEvent {
        source_time_ms: None,
        time_ms: 20,
        driver: Some(1),
        payload: EventData::Pit(Pit {
            lane_duration: Some(23.0),
            stop_duration: Some(2.5),
            ..pit
        }),
    });
    assert_eq!(s.view().drivers[&1].pits.len(), 1);
    assert_eq!(s.view().drivers[&1].pits[0].lane_duration, Some(23.0));
}
#[test]
fn leader_zero_gap_is_derived_only_from_known_leading_position() {
    let a = demo_archive();
    let mut s = RaceState::new(a.session, a.drivers, a.circuit);
    s.apply(RaceEvent {
        source_time_ms: None,
        time_ms: 1,
        driver: Some(1),
        payload: EventData::Position(1),
    });
    s.apply(RaceEvent {
        source_time_ms: None,
        time_ms: 2,
        driver: Some(1),
        payload: EventData::Interval {
            gap: None,
            interval: None,
        },
    });
    assert_eq!(s.view().drivers[&1].gap, Some(0.0));
    s.apply(RaceEvent {
        source_time_ms: None,
        time_ms: 3,
        driver: Some(1),
        payload: EventData::Position(2),
    });
    assert_eq!(s.view().drivers[&1].gap, None);
}
#[test]
fn new_stint_reports_fresh_age_before_its_first_completed_lap() {
    let archive = std::sync::Arc::new(demo_archive());
    let start = archive.session.start_ms;
    let mut replay = Replay::new(archive);
    replay.seek(start + 600000);
    let driver = &replay.state().view().drivers[&1];
    assert_eq!(driver.stints.last().unwrap().number, 2);
    assert_eq!(driver.lap, 7);
    assert_eq!(driver.tyre_age, Some(0));
}
#[test]
fn lap_corrections_use_receipt_order_and_keep_original_position() {
    let a = demo_archive();
    let mut state = RaceState::new(a.session, a.drivers, a.circuit);
    state.apply(RaceEvent {
        source_time_ms: None,
        time_ms: 1,
        driver: Some(1),
        payload: EventData::Position(1),
    });
    state.apply(RaceEvent {
        source_time_ms: None,
        time_ms: 1000,
        driver: Some(1),
        payload: EventData::Lap(Lap {
            number: 1,
            time_ms: 900,
            duration: Some(90.0),
            ..Default::default()
        }),
    });
    state.apply(RaceEvent {
        source_time_ms: None,
        time_ms: 1100,
        driver: Some(1),
        payload: EventData::Position(2),
    });
    state.apply(RaceEvent {
        source_time_ms: None,
        time_ms: 1200,
        driver: Some(1),
        payload: EventData::Lap(Lap {
            number: 1,
            time_ms: 850,
            duration: Some(85.0),
            ..Default::default()
        }),
    });
    state.apply(RaceEvent {
        source_time_ms: None,
        time_ms: 1150,
        driver: Some(1),
        payload: EventData::Lap(Lap {
            number: 1,
            time_ms: 880,
            duration: Some(88.0),
            ..Default::default()
        }),
    });
    let lap = &state.view().drivers[&1].laps[0];
    assert_eq!(lap.duration, Some(85.0));
    assert_eq!(lap.position, Some(1));
}
