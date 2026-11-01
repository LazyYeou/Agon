use pitwall_core::*;
use pitwall_state::*;
use std::sync::Arc;

#[test]
fn duplicate_laps_do_not_grow_history_and_old_positions_do_not_win() {
    let archive = demo_archive();
    let mut state = RaceState::new(archive.session, archive.drivers, archive.circuit);
    let lap = RaceEvent {
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
        time_ms: 200,
        driver: Some(1),
        payload: EventData::Position(2),
    });
    state.apply(RaceEvent {
        time_ms: 150,
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
