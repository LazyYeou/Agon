use pitwall_store::*;
#[tokio::test]
#[ignore = "requires isolated PostgreSQL and Redis from docker compose"]
async fn postgres_and_redis_roundtrip_checkpoints_and_forecasts() {
    let path = std::env::temp_dir().join(format!("agon-pg-{}", std::process::id()));
    let store = Store::connect(path.clone()).await.unwrap();
    let mut archive = pitwall_state::demo_archive();
    let key = 5_000_000 + std::process::id();
    archive.session.key = key;
    store.save(&archive).await.unwrap();
    let state = pitwall_core::Snapshot::empty(archive.session.clone());
    store.checkpoint(&state).await.unwrap();
    store.snapshot(&state).await.unwrap();
    store
        .log(&format!("integration-{key}"), key, &vec!["known forecast"])
        .await
        .unwrap();
    assert_eq!(
        store
            .load_checkpoint(key)
            .await
            .unwrap()
            .unwrap()
            .session
            .key,
        key
    );
    tokio::fs::remove_file(path.join(format!("{key}.json")))
        .await
        .unwrap();
    assert!(store.load(key).await.unwrap().unwrap().session.synthetic);
}
#[tokio::test]
async fn atomic_cache_roundtrip_and_corruption_is_an_error() {
    let path = std::env::temp_dir().join(format!("agon-store-{}", std::process::id()));
    let store = Store::local(path.clone()).await.unwrap();
    let archive = pitwall_state::demo_archive();
    store.save(&archive).await.unwrap();
    assert_eq!(store.load(0).await.unwrap().unwrap().session.key, 0);
    tokio::fs::write(path.join("0.json"), b"broken")
        .await
        .unwrap();
    assert!(store.load(0).await.is_err());
    tokio::fs::remove_file(path.join("0.json")).await.unwrap();
    tokio::fs::remove_dir(path).await.unwrap();
}
