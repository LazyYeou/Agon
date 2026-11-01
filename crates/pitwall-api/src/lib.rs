//! HTTP and bounded binary WebSocket gateway.
use axum::{
    extract::{ws::WebSocketUpgrade, ConnectInfo, Path, Query, State},
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use pitwall_core::*;
use pitwall_ingest::OpenF1;
use pitwall_state::Replay;
use pitwall_store::Store;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{Mutex as AsyncMutex, Semaphore};
mod connection;
mod live;
mod replay_window;
pub use live::LiveRuntime;

pub struct App {
    pub live: tokio::sync::RwLock<Option<Arc<LiveRuntime>>>,
    pub source: OpenF1,
    pub store: Store,
    archives: AsyncMutex<HashMap<u32, Arc<SessionArchive>>>,
    load: AsyncMutex<()>,
    rates: Mutex<HashMap<IpAddr, (Instant, u32, u32)>>,
    pub jobs: Arc<Semaphore>,
    pub connections: std::sync::atomic::AtomicUsize,
    pub sent: std::sync::atomic::AtomicU64,
    pub errors: std::sync::atomic::AtomicU64,
}
impl App {
    pub async fn local(path: PathBuf) -> anyhow::Result<Arc<Self>> {
        Self::with_store(Store::local(path).await?).await
    }
    pub async fn configured(path: PathBuf) -> anyhow::Result<Arc<Self>> {
        Self::with_store(Store::connect(path).await?).await
    }
    async fn with_store(store: Store) -> anyhow::Result<Arc<Self>> {
        let archive = Arc::new(pitwall_state::demo_archive());
        Ok(Arc::new(Self {
            live: tokio::sync::RwLock::new(None),
            source: OpenF1::new()?,
            store,
            archives: AsyncMutex::new(HashMap::from([(0, archive)])),
            load: AsyncMutex::new(()),
            rates: Mutex::new(HashMap::new()),
            jobs: Arc::new(Semaphore::new(4)),
            connections: 0.into(),
            sent: 0.into(),
            errors: 0.into(),
        }))
    }
    pub async fn archive(&self, key: u32) -> anyhow::Result<Arc<SessionArchive>> {
        if let Some(a) = self
            .archives
            .lock()
            .await
            .get(&key)
            .filter(|a| a.reusable_at(chrono::Utc::now().timestamp_millis()))
            .cloned()
        {
            return Ok(a);
        }
        let _load = self.load.lock().await;
        if let Some(a) = self
            .archives
            .lock()
            .await
            .get(&key)
            .filter(|a| a.reusable_at(chrono::Utc::now().timestamp_millis()))
            .cloned()
        {
            return Ok(a);
        }
        let archive = if let Some(archive) = self
            .store
            .load(key)
            .await?
            .filter(|a| a.reusable_at(chrono::Utc::now().timestamp_millis()))
        {
            archive
        } else {
            let archive = self.source.load_archive(key).await?;
            self.store.save(&archive).await?;
            archive
        };
        let a = Arc::new(archive);
        let mut archives = self.archives.lock().await;
        if archives.len() >= 8 {
            archives.retain(|key, _| *key == 0);
        }
        archives.insert(key, a.clone());
        Ok(a)
    }
}
type ApiError = (StatusCode, Json<Value>);
fn bad(message: impl ToString) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error":message.to_string()})),
    )
}
fn source_error(error: anyhow::Error) -> ApiError {
    tracing::warn!(error=%error,"data source request failed");
    (
        StatusCode::BAD_GATEWAY,
        Json(
            json!({"error":"OpenF1 data unavailable. Retry later; live sessions require sponsor access."}),
        ),
    )
}
pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/health", get(|| async { Json(json!({"status":"ok"})) }))
        .route("/metrics", get(metrics))
        .route("/api/sessions", get(sessions))
        .route("/api/session/{key}", get(session))
        .route("/api/telemetry", get(telemetry))
        .route("/api/strategy", get(strategy))
        .route("/api/accuracy", get(accuracy))
        .route("/api/live", get(live_status))
        .route("/ws", get(websocket))
        .fallback_service(
            tower_http::services::ServeDir::new("pitwall-web/dist").not_found_service(
                tower_http::services::ServeFile::new("pitwall-web/dist/index.html"),
            ),
        )
        .layer(middleware::from_fn_with_state(app.clone(), limit))
        .layer(
            tower_http::trace::TraceLayer::new_for_http().make_span_with(
                tower_http::trace::DefaultMakeSpan::new().level(tracing::Level::INFO),
            ),
        )
        .with_state(app)
}
async fn limit(
    State(app): State<Arc<App>>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|v| v.0.ip())
        .unwrap_or(IpAddr::from([127, 0, 0, 1]));
    let allowed = {
        let mut rates = app.rates.lock().unwrap_or_else(|e| e.into_inner());
        if rates.len() > 20000 {
            rates.retain(|_, (t, _, active)| t.elapsed() < Duration::from_secs(60) || *active > 0);
        }
        let r = rates.entry(ip).or_insert((Instant::now(), 0, 0));
        if r.0.elapsed() > Duration::from_secs(60) {
            r.0 = Instant::now();
            r.1 = 0;
        }
        r.1 += 1;
        r.1 <= 180
    };
    if !allowed {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "Request limit reached. Retry in a minute.",
        )
            .into_response();
    }
    next.run(req).await
}
#[derive(Deserialize)]
struct Year {
    year: u16,
}
async fn sessions(
    State(app): State<Arc<App>>,
    Query(q): Query<Year>,
) -> Result<Json<Vec<Session>>, ApiError> {
    if !(2023..=2100).contains(&q.year) {
        return Err(bad("Choose a year from 2023 onward"));
    }
    Ok(Json(
        app.source.sessions(q.year).await.map_err(source_error)?,
    ))
}
async fn session(
    State(app): State<Arc<App>>,
    Path(key): Path<u32>,
) -> Result<Json<Session>, ApiError> {
    Ok(Json(
        app.archive(key)
            .await
            .map_err(source_error)?
            .session
            .clone(),
    ))
}
#[derive(Deserialize)]
struct TelemetryQuery {
    session: u32,
    driver: u32,
    from: i64,
    to: i64,
}
async fn telemetry(
    State(app): State<Arc<App>>,
    Query(q): Query<TelemetryQuery>,
) -> Result<Json<Vec<TelemetrySample>>, ApiError> {
    if q.driver == 0 || q.driver > 999 || !valid_time_range(q.from, q.to, 180_000) {
        return Err(bad("Choose a valid driver and a range up to 180 seconds"));
    }
    if let Some(live) = app
        .live
        .read()
        .await
        .clone()
        .filter(|live| live.key == q.session)
    {
        let state = live.snapshot().await;
        if !state.drivers.contains_key(&q.driver)
            || q.from < state.session.start_ms
            || q.to > state.session.end_ms
        {
            return Err(bad("Telemetry range or driver outside session"));
        }
        let retained = live.telemetry(q.driver).await;
        let covers_range = retained
            .first()
            .is_some_and(|s| s.time_ms <= q.from.saturating_add(1000))
            && retained
                .last()
                .is_some_and(|s| s.time_ms >= q.to.saturating_sub(1000));
        let samples: Vec<_> = retained
            .into_iter()
            .filter(|s| s.time_ms >= q.from && s.time_ms <= q.to)
            .collect();
        if covers_range {
            return Ok(Json(samples));
        }
        return app
            .source
            .telemetry(q.session, q.driver, q.from, q.to)
            .await
            .map(Json)
            .map_err(source_error);
    }
    let a = app.archive(q.session).await.map_err(source_error)?;
    if !a.drivers.iter().any(|d| d.number == q.driver)
        || q.from < a.session.start_ms
        || q.to > a.session.end_ms
    {
        return Err(bad("Telemetry range or driver outside session"));
    }
    let samples = if q.session == 0 {
        a.events
            .iter()
            .filter(|e| e.driver == Some(q.driver) && e.time_ms >= q.from && e.time_ms <= q.to)
            .filter_map(|e| {
                if let EventData::Telemetry(s) = &e.payload {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .take(1200)
            .collect()
    } else {
        app.source
            .telemetry(q.session, q.driver, q.from, q.to)
            .await
            .map_err(source_error)?
    };
    Ok(Json(samples))
}
#[derive(Deserialize)]
struct StrategyQuery {
    session: u32,
    time_ms: i64,
    a: u32,
    b: u32,
}
async fn strategy(
    State(app): State<Arc<App>>,
    Query(q): Query<StrategyQuery>,
) -> Result<Json<pitwall_predict::StrategyEstimate>, ApiError> {
    if q.a == q.b {
        return Err(bad("Select two different drivers"));
    }
    if let Some(live) = app
        .live
        .read()
        .await
        .clone()
        .filter(|live| live.key == q.session)
    {
        let state = live.snapshot().await;
        if state.clock_ms.abs_diff(q.time_ms) <= 5000 {
            if ![q.a, q.b].iter().all(|n| state.drivers.contains_key(n)) {
                return Err(bad("Driver not in session"));
            }
            return Ok(Json(pitwall_predict::undercut(&state, q.a, q.b)));
        }
    }
    let a = app.archive(q.session).await.map_err(source_error)?;
    if q.time_ms < a.session.start_ms || q.time_ms > a.session.end_ms {
        return Err(bad("Time outside session"));
    }
    if ![q.a, q.b]
        .iter()
        .all(|n| a.drivers.iter().any(|d| d.number == *n))
    {
        return Err(bad("Driver not in session"));
    }
    let permit = app.jobs.clone().try_acquire_owned().map_err(|_| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"Strategy workers busy; retry shortly"})),
        )
    })?;
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut replay = Replay::new(a);
        replay.seek(q.time_ms);
        pitwall_predict::undercut(replay.state().view(), q.a, q.b)
    })
    .await
    .map_err(|e| bad(e.to_string()))?;
    Ok(Json(result))
}
async fn accuracy() -> Json<Value> {
    let path = PathBuf::from(std::env::var("AGON_MODELS").unwrap_or_else(|_| "models".into()))
        .join("manifest.json");
    let value = match tokio::fs::read(path).await {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes).ok(),
        Err(_) => None,
    };
    Json(value.map_or_else(||json!({"status":"unavailable","message":"No backtest report available. Playback scores accumulate from recorded forecasts."}),|v|json!({"status":"measured","model":v["model"],"years":v["training_years"],"pace_samples":v["pace_samples"],"pace_mae":v["pace_mae"],"pace_1_to_5_mae":v["pace_1_to_5_mae"],"pace_1_to_5_samples":v["pace_1_to_5_samples"],"band_coverage":v["band_coverage"],"pit_samples":v["pit_samples"],"pit_candidates":v["pit_candidates"],"pit_forecast_coverage":v["pit_forecast_coverage"],"pit_accuracy":v["pit_horizon_10_accuracy"],"position_accuracy":v["finish_half_distance_accuracy"],"limitations":v["limitations"]})))
}
async fn live_status(State(app): State<Arc<App>>) -> Json<Value> {
    Json(
        json!({"configured":app.live.read().await.as_ref().map(|r|r.key),"message":"Live ingest requires sponsor credentials on the server. Feed delay is approximately 3 seconds."}),
    )
}
async fn websocket(
    State(app): State<Arc<App>>,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
) -> Response {
    let accepted = {
        let mut rates = app.rates.lock().unwrap_or_else(|e| e.into_inner());
        let r = rates.entry(address.ip()).or_insert((Instant::now(), 0, 0));
        if r.2 >= 16 {
            false
        } else {
            r.2 += 1;
            true
        }
    };
    if !accepted {
        return (StatusCode::TOO_MANY_REQUESTS, "Too many open timing views").into_response();
    }
    ws.max_message_size(2048)
        .max_frame_size(2048)
        .on_upgrade(move |socket| connection::run(socket, app, address.ip()))
}
async fn metrics(State(app): State<Arc<App>>) -> String {
    use std::sync::atomic::Ordering::Relaxed;
    format!("# TYPE agon_ws_clients gauge\nagon_ws_clients {}\n# TYPE agon_ws_bytes_total counter\nagon_ws_bytes_total {}\n# TYPE agon_ws_errors_total counter\nagon_ws_errors_total {}\n",app.connections.load(Relaxed),app.sent.load(Relaxed),app.errors.load(Relaxed))
}
