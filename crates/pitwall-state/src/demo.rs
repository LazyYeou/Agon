//! Explicitly synthetic test circuit and race. Never represented as a historical Grand Prix.
use pitwall_core::*;
use std::f64::consts::TAU;

fn point(progress: f64) -> (f64, f64) {
    let t = progress * TAU;
    (
        500.0 + 360.0 * t.cos() + 70.0 * (3.0 * t).sin(),
        300.0 + 190.0 * t.sin() + 50.0 * (2.0 * t).cos(),
    )
}
pub fn demo_archive() -> SessionArchive {
    let start = 1_730_000_000_000i64;
    let session = Session {
        key: 0,
        year: 2026,
        event: "Strategy lab".into(),
        name: "Race · synthetic demo".into(),
        circuit: "Test circuit".into(),
        country: "Simulated".into(),
        start_ms: start,
        end_ms: start + 1_080_000,
        total_laps: Some(12),
        synthetic: true,
    };
    let codes = [
        "VER", "NOR", "LEC", "PIA", "RUS", "HAM", "ALO", "SAI", "GAS", "ALB", "OCO", "STR", "TSU",
        "HUL", "BEA", "ANT", "LAW", "HAD", "BOR", "COL",
    ];
    let numbers = [
        1, 4, 16, 81, 63, 44, 14, 55, 10, 23, 31, 18, 22, 27, 87, 12, 30, 6, 5, 43,
    ];
    let colors = [
        "5d9cff", "60e1d8", "ff737f", "60e1d8", "c6a2ff", "ff737f", "6aca9c", "68c7ee", "eda0f5",
        "68c7ee", "f0c875", "6aca9c", "5d9cff", "d2e0eb", "f0c875", "c6a2ff", "5d9cff", "5d9cff",
        "d2e0eb", "eda0f5",
    ];
    let drivers: Vec<_> = (0..20)
        .map(|i| Driver {
            number: numbers[i],
            code: codes[i].into(),
            name: format!("{} · simulated", codes[i]),
            team: format!("Garage {}", i / 2 + 1),
            color: colors[i].into(),
        })
        .collect();
    let circuit = Circuit {
        outline: (0..=200).map(|i| point(i as f64 / 200.0)).collect(),
        corners: (1..=12)
            .map(|i| {
                let (x, y) = point(i as f64 / 12.0);
                (format!("T{i}"), x, y)
            })
            .collect(),
        sectors: vec![0.0, 0.32, 0.68],
        drs_zones: vec![(0.12, 0.23), (0.73, 0.85)],
        pit_lane: vec![point(0.94), (880.0, 330.0), (880.0, 200.0), point(0.06)],
        pit_loss: Some(Band {
            value: 22.0,
            low: 20.0,
            high: 24.0,
        }),
        source: "Synthetic circuit / deterministic fixture; not a real venue".into(),
        approximate: true,
    };
    let mut events = vec![
        RaceEvent {
            time_ms: start,
            driver: None,
            payload: EventData::Status("GREEN".into()),
        },
        RaceEvent {
            time_ms: start,
            driver: None,
            payload: EventData::Weather(Weather {
                air: Some(24.0),
                track: Some(37.0),
                humidity: Some(58.0),
                wind_speed: Some(3.2),
                wind_direction: Some(125.0),
                rain: Some(false),
            }),
        },
    ];
    for (i, driver) in drivers.iter().enumerate() {
        let n = Some(driver.number);
        events.push(RaceEvent {
            time_ms: start,
            driver: n,
            payload: EventData::Position(i as u32 + 1),
        });
        events.push(RaceEvent {
            time_ms: start,
            driver: n,
            payload: EventData::Stint(Stint {
                number: 1,
                compound: if i % 3 == 0 { "SOFT" } else { "MEDIUM" }.into(),
                start_lap: 1,
                is_new: Some(true),
                ..Default::default()
            }),
        });
        let mut t = start + i as i64 * 1800;
        for lap in 1..=12 {
            let duration = 80.0
                + i as f64 * 0.10
                + lap as f64 * (0.045 + (i % 4) as f64 * 0.018)
                + ((lap + i as u32) % 3) as f64 * 0.04;
            for sample in 0..(duration * 2.0) as i64 {
                let progress = sample as f64 / (duration * 2.0);
                let wave = (progress * TAU * 5.0).sin();
                let speed = 220.0 + 100.0 * wave;
                let (x, y) = point(progress);
                let time = t + sample * 500;
                events.push(RaceEvent {
                    time_ms: time,
                    driver: n,
                    payload: EventData::Telemetry(TelemetrySample {
                        time_ms: time,
                        speed: Some(speed),
                        rpm: Some(9000.0 + speed * 10.0),
                        gear: Some((speed / 45.0).clamp(2.0, 8.0) as u8),
                        throttle: Some(if wave < -0.4 { 35.0 } else { 100.0 }),
                        brake: Some(wave < -0.7),
                        drs: Some((0.12..0.23).contains(&progress)),
                        x: Some(x),
                        y: Some(y),
                    }),
                });
                events.push(RaceEvent {
                    time_ms: time,
                    driver: n,
                    payload: EventData::Location(x, y),
                });
            }
            t += (duration * 1000.0) as i64;
            events.push(RaceEvent {
                time_ms: t,
                driver: n,
                payload: EventData::Interval {
                    gap: Some(i as f64 * 2.1 + lap as f64 * i as f64 * 0.12),
                    interval: Some(if i == 0 { 0.0 } else { 2.1 + lap as f64 * 0.12 }),
                },
            });
            events.push(RaceEvent {
                time_ms: t,
                driver: n,
                payload: EventData::Lap(Lap {
                    number: lap,
                    time_ms: t,
                    duration: Some(duration),
                    sectors: vec![
                        Some(duration * 0.31),
                        Some(duration * 0.36),
                        Some(duration * 0.33),
                    ],
                    segments: vec![2048, 2049, 2051, 2048, 2049, 2048, 2051, 2048, 2049],
                    ..Default::default()
                }),
            });
            if lap == 7 {
                events.push(RaceEvent {
                    time_ms: t + 1,
                    driver: n,
                    payload: EventData::Pit(Pit {
                        time_ms: t + 1,
                        lap,
                        lane_duration: Some(23.0),
                        stop_duration: Some(2.5),
                    }),
                });
                events.push(RaceEvent {
                    time_ms: t + 23_000,
                    driver: n,
                    payload: EventData::Stint(Stint {
                        number: 2,
                        compound: "HARD".into(),
                        start_lap: 8,
                        is_new: Some(true),
                        ..Default::default()
                    }),
                });
            }
        }
    }
    for (offset, flag, message) in [
        (0, "GREEN", "Track clear — synthetic test race"),
        (
            520_000,
            "YELLOW",
            "Yellow flag at turn 8 — simulated incident",
        ),
        (535_000, "GREEN", "Track clear"),
        (700_000, "SC", "Safety car deployed — simulated scenario"),
        (740_000, "GREEN", "Safety car in this lap — track clear"),
    ] {
        events.push(RaceEvent {
            time_ms: start + offset,
            driver: None,
            payload: EventData::Status(flag.into()),
        });
        events.push(RaceEvent {
            time_ms: start + offset,
            driver: None,
            payload: EventData::Control(ControlMessage {
                time_ms: start + offset,
                driver: None,
                message: message.into(),
                category: "Flag".into(),
                flag: Some(flag.into()),
                sector: None,
            }),
        });
    }
    events.sort_by_key(|e| e.time_ms);
    SessionArchive {
        session,
        drivers,
        events,
        circuit,
    }
}
