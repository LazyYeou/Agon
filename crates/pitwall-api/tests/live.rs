use pitwall_core::*;
#[tokio::test]
async fn live_engine_applies_timing_without_waiting_for_predictions_and_keeps_last_state() {
    let archive = std::sync::Arc::new(pitwall_state::demo_archive());
    let engine = pitwall_api::LiveRuntime::new(archive);
    engine
        .apply(RaceEvent {
            source_time_ms: None,
            time_ms: chrono::Utc::now().timestamp_millis(),
            driver: Some(1),
            payload: EventData::Position(3),
        })
        .await;
    assert_eq!(engine.snapshot().await.drivers[&1].position, Some(3));
    engine.mark_delayed().await;
    let view = engine.snapshot().await;
    assert_eq!(view.data_status, "data delayed");
    assert_eq!(view.drivers[&1].position, Some(3));
}

#[tokio::test]
async fn subscriber_rejects_wrong_session_stale_and_reordered_snapshots() {
    let archive = std::sync::Arc::new(pitwall_state::demo_archive());
    let engine = pitwall_api::LiveRuntime::new(archive);
    let mut snapshot = engine.snapshot().await;
    snapshot.revision = 1_000_000;
    snapshot.data_status = "live".into();
    snapshot.drivers.get_mut(&1).unwrap().position = Some(2);
    let now = chrono::Utc::now().timestamp_millis();
    assert!(
        engine
            .receive_snapshot(pitwall_store::PublishedSnapshot {
                evaluation: None,
                published_at_ms: now,
                snapshot: snapshot.clone()
            })
            .await
    );
    snapshot.drivers.get_mut(&1).unwrap().position = Some(8);
    assert!(
        !engine
            .receive_snapshot(pitwall_store::PublishedSnapshot {
                evaluation: None,
                published_at_ms: now - 1,
                snapshot: snapshot.clone()
            })
            .await
    );
    assert!(
        !engine
            .receive_snapshot(pitwall_store::PublishedSnapshot {
                evaluation: None,
                published_at_ms: now - 20_000,
                snapshot: snapshot.clone()
            })
            .await
    );
    snapshot.session.key = 987;
    assert!(
        !engine
            .receive_snapshot(pitwall_store::PublishedSnapshot {
                evaluation: None,
                published_at_ms: now + 1,
                snapshot
            })
            .await
    );
    assert_eq!(engine.snapshot().await.drivers[&1].position, Some(2));
}

#[tokio::test]
#[ignore = "requires PostgreSQL and Redis from docker compose"]
async fn live_runtime_recovers_a_committed_batch_before_advancing_watermark() {
    let key = 7_000_000 + std::process::id();
    let mut archive = pitwall_state::demo_archive();
    archive.session.key = key;
    let runtime = pitwall_api::LiveRuntime::new(std::sync::Arc::new(archive));
    let path = std::env::temp_dir().join(format!("agon-live-runtime-{key}"));
    let local = pitwall_store::Store::local(path.clone()).await.unwrap();
    let arrival = pitwall_ingest::LiveArrival::decode(
        "position",
        &serde_json::json!({
            "session_key":key,"_key":"position","_id":20,"driver_number":1,"position":3
        }),
        key,
        chrono::Utc::now().timestamp_millis(),
    )
    .unwrap();
    let mut watermark = 0;
    let before = runtime.snapshot().await.drivers[&1].position;
    assert!(runtime
        .commit_arrivals(&local, std::slice::from_ref(&arrival), &mut watermark)
        .await
        .is_err());
    assert_eq!(runtime.snapshot().await.drivers[&1].position, before);
    assert_eq!(watermark, 0);
    let store = pitwall_store::Store::connect(path.clone()).await.unwrap();
    // Simulate a commit whose successful response never reached the live process.
    store
        .append_live(std::slice::from_ref(&arrival))
        .await
        .unwrap();
    runtime
        .commit_arrivals(&store, &[arrival], &mut watermark)
        .await
        .unwrap();
    assert_eq!(runtime.snapshot().await.drivers[&1].position, Some(3));
    assert!(watermark > 0);
    tokio::fs::remove_dir(path).await.unwrap();
}

#[tokio::test]
#[ignore = "requires PostgreSQL and Redis from docker compose; run with --test-threads=1"]
async fn secondary_live_node_receives_redis_without_loading_source_archive() {
    let key = 6_000_000 + std::process::id();
    let path = std::env::temp_dir().join(format!("agon-secondary-{key}"));
    let app = pitwall_api::App::configured(path.clone()).await.unwrap();
    let archive = pitwall_state::demo_archive();
    let mut snapshot = pitwall_api::LiveRuntime::new(std::sync::Arc::new(archive))
        .snapshot()
        .await;
    snapshot.session.key = key;
    snapshot.revision = 1_000_000;
    snapshot.drivers.get_mut(&1).unwrap().position = Some(7);
    snapshot.data_status = "live".into();
    let mut report = pitwall_predict::PredictionLog::default().report();
    report.pace_samples = 4;
    app.store
        .publish_snapshot(pitwall_store::PublishedSnapshot {
            published_at_ms: chrono::Utc::now().timestamp_millis(),
            snapshot: snapshot.clone(),
            evaluation: Some(pitwall_store::PublishedEvaluation {
                forecasts: vec![],
                report,
            }),
        })
        .await
        .unwrap();
    let previous = std::env::var_os("AGON_LIVE_ROLE");
    std::env::set_var("AGON_LIVE_ROLE", "subscriber");
    let job = tokio::spawn(pitwall_api::LiveRuntime::start(app.clone(), key));
    let runtime = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if let Some(runtime) = app.live.read().await.clone() {
                break runtime;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    match previous {
        Some(value) => std::env::set_var("AGON_LIVE_ROLE", value),
        None => std::env::remove_var("AGON_LIVE_ROLE"),
    }
    assert_eq!(runtime.snapshot().await.drivers[&1].position, Some(7));
    assert_eq!(runtime.evaluation().await.1.pace_samples, 4);
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    snapshot.revision += 1;
    snapshot.drivers.get_mut(&1).unwrap().position = Some(2);
    app.store.snapshot(&snapshot).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if runtime.snapshot().await.drivers[&1].position == Some(2) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!path.join(format!("{key}.json")).exists());
    job.abort();
    tokio::fs::remove_dir(path).await.unwrap();
}
