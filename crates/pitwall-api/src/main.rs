use std::{net::SocketAddr, path::PathBuf};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pitwall_api=info,tower_http=info".into()),
        )
        .init();
    let app = pitwall_api::App::configured(PathBuf::from(
        std::env::var("AGON_DATA").unwrap_or_else(|_| "data".into()),
    ))
    .await?;
    let address: SocketAddr = std::env::var("AGON_BIND")
        .unwrap_or_else(|_| "127.0.0.1:3001".into())
        .parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(%address,"Agon timing gateway ready");
    axum::serve(
        listener,
        pitwall_api::router(app).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}
