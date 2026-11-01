use pitwall_core::*;
use pitwall_ingest::*;
use serde_json::json;

#[test]
fn delayed_point_samples_keep_measurement_order_separate_from_receipt() {
    let row = json!({"driver_number":1,"date":"2023-01-01T00:00:00Z","position":2});
    let event = normalize("position", &row, Some(1_672_531_205_000)).unwrap();
    assert_eq!(event.time_ms, 1_672_531_205_000);
    assert_eq!(event.source_time_ms, Some(1_672_531_200_000));
}
#[test]
fn no_results_404_is_empty_but_unknown_route_is_an_error() {
    assert!(decode_rows(404, br#"{"detail":"No results found."}"#)
        .unwrap()
        .is_empty());
    assert!(decode_rows(404, br#"{"detail":"Not Found"}"#).is_err());
    assert!(decode_rows(200, br#"{"unexpected":"object"}"#).is_err());
}
#[test]
fn telemetry_location_alignment_uses_only_nearby_observed_samples() {
    let mut samples = vec![
        TelemetrySample {
            time_ms: 1100,
            ..Default::default()
        },
        TelemetrySample {
            time_ms: 5000,
            ..Default::default()
        },
    ];
    attach_locations(
        &mut samples,
        vec![RaceEvent {
            source_time_ms: None,
            time_ms: 1200,
            driver: Some(1),
            payload: EventData::Location(5.0, 6.0),
        }],
    );
    assert_eq!((samples[0].x, samples[0].y), (Some(5.0), Some(6.0)));
    assert_eq!((samples[1].x, samples[1].y), (None, None));
}
#[test]
fn comparison_query_does_not_send_a_second_equals_delimiter() {
    let url = openf1_url(
        "location",
        &[
            ("session_key", "9161".into()),
            ("date>=", "2023-09-16T13:00:00Z".into()),
        ],
    )
    .unwrap();
    assert!(url.as_str().contains("date%3E="));
    assert!(!url.as_str().contains("date%3E%3D="));
    assert!(openf1_url("../../token", &[]).is_err());
}
#[test]
fn live_sector_revisions_do_not_require_a_completed_lap() {
    let row = json!({"driver_number":1,"lap_number":4,"lap_duration":null,"duration_sector_1":27.111,"segments_sector_1":[2048,2049]});
    let event = normalize("laps", &row, Some(1100)).unwrap();
    assert!(matches!(
        event.payload,
        EventData::Sectors(Lap {
            number: 4,
            duration: None,
            ..
        })
    ));
    assert!(normalize("laps", &row, None).is_none());
}
#[test]
fn live_lap_keeps_physical_completion_separate_from_receipt() {
    let row = json!({"driver_number":1,"lap_number":4,"date_start":"2023-01-01T00:00:00Z","lap_duration":90});
    let event = normalize("laps", &row, Some(1_672_531_293_000)).unwrap();
    assert_eq!(event.time_ms, 1_672_531_293_000);
    let EventData::Lap(lap) = event.payload else {
        panic!("completed lap expected")
    };
    assert_eq!(lap.time_ms, 1_672_531_290_000);
    assert!(normalize(
        "stints",
        &json!({"driver_number":1,"stint_number":1,"lap_start":1,"compound":"SOFT"}),
        Some(1)
    )
    .is_none());
}
#[test]
fn completed_lap_is_not_observed_at_start_and_nulls_stay_missing() {
    let row = json!({"driver_number":63,"lap_number":8,"date_start":"2023-09-16T13:59:07.606Z","lap_duration":91.743,"duration_sector_1":26.966,"duration_sector_2":38.657,"duration_sector_3":26.12});
    let e = normalize("laps", &row, None).unwrap();
    assert_eq!(e.time_ms, 1_694_872_839_349);
    let EventData::Lap(lap) = e.payload else {
        panic!("lap expected")
    };
    assert_eq!(lap.duration, Some(91.743));
    assert!(normalize(
        "laps",
        &json!({"driver_number":63,"lap_number":9,"lap_duration":null}),
        None
    )
    .is_none());
}
#[test]
fn lap_gap_strings_and_unknown_drs_do_not_become_numbers() {
    let e=normalize("intervals",&json!({"driver_number":1,"date":"2023-09-16T14:00:00Z","gap_to_leader":"+1 LAP","interval":1.2}),None).unwrap();
    assert!(matches!(
        e.payload,
        EventData::Interval {
            gap: None,
            interval: Some(1.2)
        }
    ));
    let e = normalize(
        "car_data",
        &json!({"driver_number":1,"date":"2023-09-16T14:00:00Z","speed":200,"drs":9}),
        None,
    )
    .unwrap();
    assert!(matches!(
        e.payload,
        EventData::Telemetry(TelemetrySample { drs: None, .. })
    ));
}
