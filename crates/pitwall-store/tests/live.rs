use pitwall_ingest::LiveArrival;
use pitwall_store::Store;

fn arrival(session: u32, revision: i64, position: u32) -> LiveArrival {
    LiveArrival::decode(
        "position",
        &serde_json::json!({
            "session_key": session, "_key": "driver-position", "_id": revision,
            "driver_number": 1, "position": position, "date": "2026-11-01T10:00:00Z"
        }),
        session,
        1000 + revision,
    )
    .unwrap()
}

#[tokio::test]
#[ignore = "requires PostgreSQL and Redis from docker compose"]
async fn durable_revisions_reject_stale_arrivals_and_resume_after_checkpoint() {
    let key = 8_000_000 + std::process::id();
    let path = std::env::temp_dir().join(format!("agon-live-{key}"));
    let store = Store::connect(path.clone()).await.unwrap();
    let committed = store
        .append_live(&[
            arrival(key, 20, 2),
            arrival(key, 19, 9),
            arrival(key, 21, 1),
        ])
        .await
        .unwrap();
    assert_eq!(
        committed.iter().map(|r| r.accepted).collect::<Vec<_>>(),
        vec![true, false, true]
    );
    assert!(committed[0].sequence < committed[2].sequence);
    let mut snapshot = pitwall_core::Snapshot::empty(pitwall_state::demo_archive().session);
    snapshot.session.key = key;
    store
        .checkpoint_live(&snapshot, committed[0].sequence)
        .await
        .unwrap();
    let (_, watermark) = store.load_live_checkpoint(key).await.unwrap().unwrap();
    let rows = store.live_after(key, watermark).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].arrival.source_id, 19);
    assert!(!rows[0].accepted);
    assert_eq!(rows[1].arrival.received_at_ms, 1021);
    assert!(rows[1].accepted);
    assert!(!store.append_live(&[arrival(key, 20, 2)]).await.unwrap()[0].accepted);
    assert!(store
        .checkpoint_live(&snapshot, rows[1].sequence + 100)
        .await
        .is_err());
    assert_eq!(
        store.load_live_checkpoint(key).await.unwrap().unwrap().1,
        watermark
    );
    let mut invalid = arrival(key, 22, 3);
    invalid.source_key.clear();
    assert!(store
        .append_live(&[arrival(key, 23, 4), invalid])
        .await
        .is_err());
    assert_eq!(
        store.live_after(key, rows[1].sequence).await.unwrap().len(),
        0
    );
    assert!(store
        .append_live(&vec![arrival(key, 30, 4); 257])
        .await
        .is_err());
    let more = (1000..1257)
        .map(|revision| arrival(key, revision, 3))
        .collect::<Vec<_>>();
    for batch in more.chunks(256) {
        store.append_live(batch).await.unwrap();
    }
    let page = store.live_after(key, rows[1].sequence).await.unwrap();
    assert_eq!(page.len(), 256);
    assert_eq!(
        store
            .live_after(key, page[255].sequence)
            .await
            .unwrap()
            .len(),
        1
    );
    // Cleanup only this test's rows; existing archives and volumes are never reset.
    let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    for table in ["agon_live_arrivals", "agon_live_events", "agon_checkpoints"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE session_key=$1"))
            .bind(i64::from(key))
            .execute(&pool)
            .await
            .unwrap();
    }
    tokio::fs::remove_dir(path).await.unwrap();
}

#[tokio::test]
#[ignore = "requires PostgreSQL and Redis from docker compose"]
async fn redis_subscriber_gets_cached_state_then_live_publication() {
    let key = 9_000_000 + std::process::id();
    let path = std::env::temp_dir().join(format!("agon-pubsub-{key}"));
    let store = Store::connect(path.clone()).await.unwrap();
    let mut snapshot = pitwall_core::Snapshot::empty(pitwall_state::demo_archive().session);
    snapshot.session.key = key;
    snapshot.revision = 4;
    store.snapshot(&snapshot).await.unwrap();
    let mut subscription = store.subscribe_snapshots(key).await.unwrap();
    let cached = subscription.recv().await.unwrap().unwrap();
    assert_eq!(cached.snapshot.revision, 4);
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    snapshot.revision = 5;
    store.snapshot(&snapshot).await.unwrap();
    snapshot.revision = 6;
    store.snapshot(&snapshot).await.unwrap();
    snapshot.revision = 7;
    store.snapshot(&snapshot).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let next = tokio::time::timeout(std::time::Duration::from_secs(2), subscription.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(next.snapshot.revision, 7);
    assert_eq!(next.snapshot.session.key, key);
    assert!(next.published_at_ms >= cached.published_at_ms);
    tokio::fs::remove_dir(path).await.unwrap();
}
