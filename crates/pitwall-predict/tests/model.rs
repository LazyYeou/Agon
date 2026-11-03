use pitwall_predict::ModelBundle;
#[test]
fn corrupt_model_artifact_is_rejected_before_native_loading() {
    let path = std::env::temp_dir().join(format!("agon-checksum-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    let manifest = serde_json::json!({"features":["baseline","tyre_age","compound","track_temperature","interval","lap","stint"],"baseline_model":"bayesian-changepoint-v2","model":"test","pace_p10_p90_half_width":1,"pace_mae":1,"artifact_sha256":{"pace.onnx":"0000","hazard.onnx":"0000"}});
    std::fs::write(path.join("manifest.json"), manifest.to_string()).unwrap();
    std::fs::write(path.join("pace.onnx"), b"corrupt").unwrap();
    std::fs::write(path.join("hazard.onnx"), b"corrupt").unwrap();
    let error = ModelBundle::load(&path)
        .err()
        .expect("corrupt artifact must fail");
    assert!(error.to_string().contains("checksum"), "{error}");
    for file in ["manifest.json", "pace.onnx", "hazard.onnx"] {
        std::fs::remove_file(path.join(file)).unwrap();
    }
    std::fs::remove_dir(path).unwrap();
}
#[test]
fn incompatible_model_schema_is_rejected_before_inference() {
    let path = std::env::temp_dir().join(format!("agon-model-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        path.join("manifest.json"),
        r#"{"features":["future_actual_lap"],"model":"bad"}"#,
    )
    .unwrap();
    assert!(ModelBundle::load(&path).is_err());
    std::fs::remove_file(path.join("manifest.json")).unwrap();
    std::fs::remove_dir(path).unwrap();
}
#[test]
#[ignore = "requires trained artifacts and ONNXRUNTIME_DYLIB_PATH"]
fn rust_onnx_matches_python_exported_reference() {
    let path = std::path::PathBuf::from(std::env::var("AGON_TEST_MODELS").unwrap());
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.join("manifest.json")).unwrap()).unwrap();
    let features = value["parity_case"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().map_or(f32::NAN, |v| v as f32))
        .collect::<Vec<_>>();
    let mut model = ModelBundle::load(&path).unwrap();
    assert!(
        (model.residual(&features).unwrap() - value["parity_case"]["residual"].as_f64().unwrap())
            .abs()
            < 0.001
    );
    let p = model.hazard(&features).unwrap();
    assert!((0.0..=1.0).contains(&p));
    let mut batch = Vec::new();
    let mut expected = Vec::new();
    for h in 0..30 {
        let mut row = features.clone();
        row[1] += h as f32;
        row[5] += h as f32;
        expected.push(model.hazard(&row).unwrap());
        batch.extend(row);
    }
    let actual = model.hazards(&batch).unwrap();
    assert_eq!(actual.len(), expected.len());
    assert!(actual
        .iter()
        .zip(expected)
        .all(|(a, b)| (a - b).abs() < 1e-6));
    assert!(model.hazards(&[]).is_err());
    assert!(model.hazards(&[1.0, 2.0]).is_err());
}
