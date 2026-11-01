use super::*;
use anyhow::{bail, Context};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct OpenF1 {
    client: reqwest::Client,
    gate: Arc<Mutex<Instant>>,
    cache: Arc<Mutex<HashMap<String, Arc<Vec<Value>>>>>,
    token: Option<String>,
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
            token: std::env::var("OPENF1_TOKEN").ok(),
        })
    }
    pub async fn query(
        &self,
        endpoint: &str,
        params: &[(&str, String)],
    ) -> Result<Arc<Vec<Value>>> {
        if ![
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
        ]
        .contains(&endpoint)
        {
            bail!("Unsupported data view");
        }
        let key = format!("{endpoint}{params:?}");
        if let Some(hit) = self.cache.lock().await.get(&key).cloned() {
            return Ok(hit);
        }
        for attempt in 0..3 {
            let mut gate = self.gate.lock().await;
            let elapsed = gate.elapsed();
            if elapsed < Duration::from_millis(2100) {
                tokio::time::sleep(Duration::from_millis(2100) - elapsed).await;
            }
            *gate = Instant::now();
            let mut request = self
                .client
                .get(format!("https://api.openf1.org/v1/{endpoint}"))
                .query(params);
            if let Some(token) = &self.token {
                request = request.bearer_auth(token);
            }
            let response = request.send().await?;
            drop(gate);
            if response.status().as_u16() == 429 || response.status().is_server_error() {
                if attempt < 2 {
                    tokio::time::sleep(Duration::from_secs(3 * (attempt + 1))).await;
                    continue;
                }
            }
            let response = response.error_for_status().context(
                "OpenF1 request failed (historical data or sponsor access may be unavailable)",
            )?;
            if response
                .content_length()
                .is_some_and(|n| n > 64 * 1024 * 1024)
            {
                bail!("Source response exceeds session data limit");
            }
            let rows: Vec<Value> = response.json().await?;
            if rows.len() > 500_000 {
                bail!("Source response too large; use a smaller range");
            }
            let rows = Arc::new(rows);
            let mut cache = self.cache.lock().await;
            if cache.len() >= 128 {
                cache.clear();
            }
            cache.insert(key, rows.clone());
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
        if driver == 0 || driver > 999 || end <= start || end - start > 180_000 {
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
        Ok(samples)
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
                events.push(RaceEvent {
                    time_ms,
                    driver: Some(driver),
                    payload: EventData::Stint(Stint {
                        number: row["stint_number"].as_u64().unwrap_or(1) as u32,
                        compound: row["compound"].as_str().unwrap_or("UNKNOWN").into(),
                        start_lap: start,
                        end_lap: None,
                        tyre_age_at_start: row["tyre_age_at_start"].as_u64().unwrap_or(0) as u32,
                        is_new: row["tyre_age_at_start"].as_u64().map(|n| n == 0),
                    }),
                });
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
