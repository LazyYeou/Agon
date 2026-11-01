use pitwall_core::*;

#[test]
fn boundary_rejects_invalid_samples_and_identity() {
    assert!(TelemetrySample {
        time_ms: 1,
        speed: Some(f64::NAN),
        ..Default::default()
    }
    .validate()
    .is_err());
    assert!(Driver {
        number: 0,
        code: "BAD".into(),
        ..Default::default()
    }
    .validate()
    .is_err());
    assert!(Band {
        value: 90.0,
        low: 91.0,
        high: 89.0
    }
    .validate()
    .is_err());
}

#[test]
fn binary_protocol_roundtrips_named_fields() {
    let snapshot = Snapshot::empty(Session {
        key: 1,
        name: "Race".into(),
        ..Default::default()
    });
    let frame = ServerFrame::Snapshot {
        sequence: 1,
        generation: 0,
        state: Box::new(snapshot),
    };
    let bytes = rmp_serde::to_vec_named(&frame).unwrap();
    let decoded: ServerFrame = rmp_serde::from_slice(&bytes).unwrap();
    assert!(matches!(decoded, ServerFrame::Snapshot { sequence: 1, .. }));
}
