use opentelemetry::trace::TracerProvider;
use std::{net::SocketAddr, path::PathBuf};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let provider = if std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .is_some_and(|v| !v.trim().is_empty())
    {
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .build()?;
        Some(
            opentelemetry_sdk::trace::SdkTracerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(
                    opentelemetry_sdk::Resource::builder()
                        .with_service_name("agon")
                        .build(),
                )
                .build(),
        )
    } else {
        None
    };
    let layer = provider
        .as_ref()
        .map(|p| tracing_opentelemetry::layer().with_tracer(p.tracer("agon")));
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pitwall_api=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(layer)
        .init();
    let app = pitwall_api::App::configured(PathBuf::from(
        std::env::var("AGON_DATA").unwrap_or_else(|_| "data".into()),
    ))
    .await?;
    let address: SocketAddr = std::env::var("AGON_BIND")
        .unwrap_or_else(|_| "127.0.0.1:3001".into())
        .parse()?;
    if let Some(key) = std::env::var("LIVE_SESSION_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
    {
        let key = key.parse::<u32>()?;
        let app = app.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = pitwall_api::LiveRuntime::start(app.clone(), key).await {
                    tracing::warn!(error=%e,"Live startup failed; retrying");
                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                }
            }
        });
    }
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
    if let Some(provider) = provider {
        provider.shutdown()?;
    }
    Ok(())
}
