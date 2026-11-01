use pitwall_core::*;
use pitwall_ingest::*;
use serde_json::json;
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
