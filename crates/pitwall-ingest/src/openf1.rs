use super::*;
use anyhow::{bail, Context};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
const ENDPOINTS: [&str; 12] = [
    "sessions",
    "drivers",
    "laps",
    "stints",
    "position",
    "intervals",
    "pit",
    "race_control",
    "weather",
    "location",
    "car_data",
    "meetings",
];
pub fn openf1_url(endpoint: &str, params: &[(&str, String)]) -> Result<reqwest::Url> {
    anyhow::ensure!(ENDPOINTS.contains(&endpoint), "Unsupported data view");
    let mut url = reqwest::Url::parse(&format!("https://api.openf1.org/v1/{endpoint}"))?;
    for (key, value) in params {
        // The URL's key/value delimiter supplies '=' in OpenF1's comparison expression.
        let key = if key.ends_with(">=") || key.ends_with("<=") {
            key.trim_end_matches('=')
        } else {
            key
        };
        url.query_pairs_mut().append_pair(key, value);
    }
    Ok(url)
}
pub fn attach_locations(samples: &mut [TelemetrySample], events: Vec<RaceEvent>) {
    let mut positions: Vec<_> = events
        .into_iter()
        .filter_map(|e| {
            if let EventData::Location(x, y) = e.payload {
                Some((e.time_ms, x, y))
            } else {
                None
            }
        })
        .collect();
    positions.sort_by_key(|p| p.0);
    for sample in samples {
        let index = positions.partition_point(|p| p.0 < sample.time_ms);
        let closest = [
            index.checked_sub(1).and_then(|i| positions.get(i)),
            positions.get(index),
        ]
        .into_iter()
        .flatten()
        .min_by_key(|p| p.0.abs_diff(sample.time_ms));
        if let Some((time, x, y)) = closest.filter(|p| p.0.abs_diff(sample.time_ms) <= 500) {
            let _ = time;
            sample.x = Some(*x);
            sample.y = Some(*y);
        }
    }
}

type QueryCache = HashMap<String, (Instant, Arc<Vec<Value>>)>;

#[derive(Clone)]
pub struct OpenF1 {
    client: reqwest::Client,
    gate: Arc<Mutex<Instant>>,
    cache: Arc<Mutex<QueryCache>>,
    token: Arc<Mutex<(Option<String>, Instant)>>,
}
impl OpenF1 {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(45))
                .user_agent("Agon non-commercial fan timing")
                .build()?,
            gate: Arc::new(Mutex::new(Instant::now() - Duration::from_secs(3))),
            cache: Arc::new(Mutex::new(HashMap::new())),
            token: Arc::new(Mutex::new((
                std::env::var("OPENF1_TOKEN")
                    .ok()
                    .filter(|v| !v.trim().is_empty()),
                Instant::now(),
            ))),
        })
    }
    pub async fn access_token(&self) -> Result<Option<String>> {
        let mut token = self.token.lock().await;
        if token.0.is_some() && token.1.elapsed() < Duration::from_secs(3500) {
            return Ok(token.0.clone());
        }
        if let (Some(user), Some(password)) = (
            std::env::var("OPENF1_USERNAME")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            std::env::var("OPENF1_PASSWORD")
                .ok()
                .filter(|v| !v.trim().is_empty()),
        ) {
            let response = self
                .client
                .post("https://api.openf1.org/token")
                .form(&[("username", user), ("password", password)])
                .send()
                .await?
                .error_for_status()?;
            let value: Value = response.json().await?;
            let access = value["access_token"]
                .as_str()
                .context("OpenF1 authentication did not return a token")?
                .to_string();
            *token = (Some(access.clone()), Instant::now());
            return Ok(Some(access));
        }
        Ok(token.0.clone())
    }
    pub async fn query(
        &self,
        endpoint: &str,
        params: &[(&str, String)],
    ) -> Result<Arc<Vec<Value>>> {
        let url = openf1_url(endpoint, params)?;
        let key = format!("{endpoint}{params:?}");
        if let Some((at, hit)) = self.cache.lock().await.get(&key).cloned() {
            if at.elapsed() < Duration::from_secs(60) {
                return Ok(hit);
            }
        }
        for attempt in 0..3 {
            let mut gate = self.gate.lock().await;
            let elapsed = gate.elapsed();
            if elapsed < Duration::from_millis(2100) {
                tokio::time::sleep(Duration::from_millis(2100) - elapsed).await;
            }
            if let Some((at, hit)) = self.cache.lock().await.get(&key).cloned() {
                if at.elapsed() < Duration::from_secs(60) {
                    return Ok(hit);
                }
            }
            *gate = Instant::now();
            let mut request = self.client.get(url.clone());
            if let Some(token) = self.access_token().await? {
                request = request.bearer_auth(token);
            }
            let mut response = request.send().await?;
            drop(gate);
            if (response.status().as_u16() == 429 || response.status().is_server_error())
                && attempt < 2
            {
                tokio::time::sleep(Duration::from_secs(3 * (attempt + 1))).await;
                continue;
            }
            if response
                .content_length()
                .is_some_and(|n| n > 64 * 1024 * 1024)
            {
                bail!("Source response exceeds session data limit");
            }
            let status = response.status().as_u16();
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                if bytes.len() + chunk.len() > 64 * 1024 * 1024 {
                    bail!("Source response exceeds session data limit");
                }
                bytes.extend_from_slice(&chunk);
            }
            let rows = decode_rows(status, &bytes)?;
            if rows.len() > 500_000 {
                bail!("Source response too large; use a smaller range");
            }
            let rows = Arc::new(rows);
            let mut cache = self.cache.lock().await;
            if cache.len() >= 128 {
                cache.clear();
            }
            cache.insert(key, (Instant::now(), rows.clone()));
            return Ok(rows);
        }
        bail!("OpenF1 unavailable after retries")
    }
    pub async fn sessions(&self, year: u16) -> Result<Vec<Session>> {
        if !(2023..=2100).contains(&year) {
            bail!("Year must be 2023 or later");
        }
        Ok(self
            .query("sessions", &[("year", year.to_string())])
            .await?
            .iter()
            .filter_map(session)
            .collect())
    }
    pub async fn telemetry(
        &self,
        key: u32,
        driver: u32,
        start: i64,
        end: i64,
    ) -> Result<Vec<TelemetrySample>> {
        if driver == 0 || driver > 999 || !valid_time_range(start, end, 180_000) {
            bail!("Choose a driver and a range up to 180 seconds");
        }
        let date = |v| {
            chrono::DateTime::from_timestamp_millis(v)
                .map(|d| d.to_rfc3339())
                .context("Invalid time")
        };
        let rows = self
            .query(
                "car_data",
                &[
                    ("session_key", key.to_string()),
                    ("driver_number", driver.to_string()),
                    ("date>=", date(start)?),
                    ("date<=", date(end)?),
                ],
            )
            .await?;
        let mut samples: Vec<_> = rows
            .iter()
            .filter_map(|r| normalize("car_data", r, None))
            .filter_map(|e| {
                if let EventData::Telemetry(s) = e.payload {
                    Some(s)
                } else {
                    None
                }
            })
            .collect();
        samples.sort_by_key(|s| s.time_ms);
        if !samples.is_empty() {
            let positions = self
                .query(
                    "location",
                    &[
                        ("session_key", key.to_string()),
                        ("driver_number", driver.to_string()),
                        ("date>=", date(start)?),
                        ("date<=", date(end)?),
                    ],
                )
                .await?;
            attach_locations(
                &mut samples,
                positions
                    .iter()
                    .filter_map(|row| normalize("location", row, None))
                    .collect(),
            );
        }
        Ok(samples)
    }
    pub async fn locations(&self, key: u32, start: i64, end: i64) -> Result<Vec<RaceEvent>> {
        if !valid_time_range(start, end, 120_000) {
            bail!("Position window must be within 120 seconds");
        }
        let date = |v| {
            chrono::DateTime::from_timestamp_millis(v)
                .map(|d| d.to_rfc3339())
                .context("Invalid time")
        };
        let rows = self
            .query(
                "location",
                &[
                    ("session_key", key.to_string()),
                    ("date>=", date(start)?),
                    ("date<=", date(end)?),
                ],
            )
            .await?;
        let mut events: Vec<_> = rows
            .iter()
            .filter_map(|row| normalize("location", row, None))
            .collect();
        events.sort_by_key(|e| e.time_ms);
        Ok(events)
    }
    pub async fn load_archive(&self, key: u32) -> Result<SessionArchive> {
        if key == 0 {
            return Ok(pitwall_state::demo_archive());
        }
        let params = [("session_key", key.to_string())];
        let sessions = self.query("sessions", &params).await?;
        let session = sessions
            .first()
            .and_then(session)
            .context("Session not found")?;
        let rows = self.query("drivers", &params).await?;
        let drivers = rows
            .iter()
            .filter_map(|v| {
                let d = Driver {
                    number: v["driver_number"].as_u64()? as u32,
                    code: v["name_acronym"].as_str()?.into(),
                    name: v["full_name"].as_str().unwrap_or("").into(),
                    team: v["team_name"].as_str().unwrap_or("").into(),
                    color: v["team_colour"]
                        .as_str()
                        .filter(|c| c.len() == 6 && c.chars().all(|v| v.is_ascii_hexdigit()))
                        .unwrap_or("a5b5c9")
                        .into(),
                };
                d.validate().ok()?;
                Some(d)
            })
            .collect();
        let lap_rows = self.query("laps", &params).await?;
        let mut events: Vec<_> = lap_rows
            .iter()
            .filter_map(|v| normalize("laps", v, None))
            .collect();
        for endpoint in ["position", "intervals", "pit", "race_control", "weather"] {
            let rows = self.query(endpoint, &params).await?;
            for row in rows.iter() {
                if let Some(event) = normalize(endpoint, row, None) {
                    if let EventData::Control(message) = &event.payload {
                        if let Some(status) = status_from_control(message) {
                            events.push(RaceEvent {
                                source_time_ms: None,
                                time_ms: event.time_ms,
                                driver: None,
                                payload: EventData::Status(status),
                            });
                        }
                    }
                    events.push(event);
                }
            }
        }
        for row in self.query("stints", &params).await?.iter() {
            let Some(driver) = row["driver_number"].as_u64().map(|v| v as u32) else {
                continue;
            };
            let Some(start) = row["lap_start"].as_u64().map(|v| v as u32) else {
                continue;
            };
            let time = lap_rows
                .iter()
                .find(|v| {
                    v["driver_number"].as_u64() == Some(driver as u64)
                        && v["lap_number"].as_u64() == Some(start as u64)
                })
                .and_then(|v| timestamp(&v["date_start"]));
            if let Some(time_ms) = time {
                if let Some(event) = normalize("stints", row, Some(time_ms)) {
                    events.push(event);
                }
            }
        }
        // One reference lap is enough for an approximate outline; full telemetry stays on demand.
        let mut circuit = Circuit {
            source: "OpenF1 session location; approximate. Corner/DRS/pit metadata not available."
                .into(),
            approximate: true,
            ..Default::default()
        };
        if let Some(row) = lap_rows.iter().find(|v| {
            v["lap_duration"].as_f64().is_some_and(|d| d > 30.0) && v["is_pit_out_lap"] == false
        }) {
            if let (Some(start), Some(duration), Some(driver)) = (
                timestamp(&row["date_start"]),
                row["lap_duration"].as_f64(),
                row["driver_number"].as_u64(),
            ) {
                let date = |v| {
                    chrono::DateTime::from_timestamp_millis(v)
                        .unwrap()
                        .to_rfc3339()
                };
                if let Ok(rows) = self
                    .query(
                        "location",
                        &[
                            ("session_key", key.to_string()),
                            ("driver_number", driver.to_string()),
                            ("date>=", date(start)),
                            ("date<=", date(start + (duration * 1000.0) as i64)),
                        ],
                    )
                    .await
                {
                    let mut points: Vec<_> = rows
                        .iter()
                        .filter_map(|r| normalize("location", r, None))
                        .collect();
                    points.sort_by_key(|e| e.time_ms);
                    circuit.outline = points
                        .iter()
                        .filter_map(|e| {
                            if let EventData::Location(x, y) = e.payload {
                                Some((x, y))
                            } else {
                                None
                            }
                        })
                        .collect();
                }
            }
        }
        events.sort_by_key(|e| e.time_ms);
        Ok(SessionArchive {
            captured_at_ms: Some(chrono::Utc::now().timestamp_millis()),
            session,
            drivers,
            events,
            circuit,
        })
    }
}
impl DataSource for OpenF1 {
    fn load(&self, key: u32) -> Pin<Box<dyn Future<Output = Result<SessionArchive>> + Send + '_>> {
        Box::pin(self.load_archive(key))
    }
}
pub fn session(v: &Value) -> Option<Session> {
    let start = timestamp(&v["date_start"])?;
    let end = timestamp(&v["date_end"])?;
    Some(Session {
        key: v["session_key"].as_u64()? as u32,
        year: v["year"].as_u64()? as u16,
        event: v["location"].as_str().unwrap_or("Event").into(),
        name: v["session_name"].as_str()?.into(),
        circuit: v["circuit_short_name"].as_str().unwrap_or("").into(),
        country: v["country_name"].as_str().unwrap_or("").into(),
        start_ms: start,
        end_ms: end,
        total_laps: None,
        synthetic: false,
    })
}
