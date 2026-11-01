#[tokio::test]
#[ignore = "requires OpenF1 public network and source quota"]
async fn actual_2023_session_normalizes_completed_laps_and_outline() {
    let source = pitwall_ingest::OpenF1::new().unwrap();
    let archive = source.load_archive(9161).await.unwrap();
    assert_eq!(archive.session.year, 2023);
    assert!(!archive.session.synthetic);
    assert!(archive.drivers.len() >= 20);
    assert!(archive.events.iter().any(|event|event.driver==Some(63)&&matches!(&event.payload,pitwall_core::EventData::Lap(lap) if lap.number==8&&lap.duration==Some(91.743))));
    assert!(archive.circuit.outline.len() > 10);
}
