#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let key = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("Usage: export <session-key>"))?
        .parse::<u32>()?;
    let store = pitwall_store::Store::connect(
        std::env::var("AGON_DATA")
            .unwrap_or_else(|_| "data".into())
            .into(),
    )
    .await?;
    println!("{}", store.export_parquet(key).await?.display());
    Ok(())
}
