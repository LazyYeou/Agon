use pitwall_store::*;
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
