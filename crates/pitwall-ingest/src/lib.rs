//! OpenF1 source boundary. No unfiltered raw-feed proxy is exposed to browsers.
use anyhow::Result;
use pitwall_core::*;
use serde_json::Value;
use std::{future::Future, pin::Pin};
mod live;
pub mod metadata;
mod openf1;
pub use live::{LiveArrival, LiveFeed};
pub use openf1::session as session_from_openf1;
pub use openf1::{attach_locations, openf1_url, OpenF1};
pub fn decode_rows(status: u16, body: &[u8]) -> Result<Vec<Value>> {
    if status == 404 {
        let value: Value = serde_json::from_slice(body)?;
        if value["detail"] == "No results found." {
            return Ok(vec![]);
        }
    }
    anyhow::ensure!((200..300).contains(&status), "OpenF1 HTTP {status}");
    Ok(serde_json::from_slice(body)?)
}

pub trait DataSource: Send + Sync {
    fn load(&self, key: u32) -> Pin<Box<dyn Future<Output = Result<SessionArchive>> + Send + '_>>;
}
pub fn timestamp(value: &Value) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value.as_str()?)
        .ok()
        .map(|v| v.timestamp_millis())
}
fn number(v: &Value) -> Option<f64> {
    v.as_f64().filter(|v| v.is_finite())
}
pub fn normalize(topic: &str, row: &Value, observed: Option<i64>) -> Option<RaceEvent> {
    let driver = row["driver_number"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v > 0 && *v < 1000);
    let dated = timestamp(&row["date"]);
    let payload = match topic {
        "laps" => {
            let number = u32::try_from(row["lap_number"].as_u64()?).ok()?;
            if number == 0 || number > 500 {
                return None;
            }
            let sectors: Vec<_> = (1..=3)
                .map(|i| number_value(row.get(format!("duration_sector_{i}"))).filter(|v| *v > 0.0))
                .collect();
            let segments: Vec<_> = (1..=3)
                .flat_map(|i| {
                    row[format!("segments_sector_{i}")]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v.as_u64())
                        .filter_map(|v| u16::try_from(v).ok())
                })
                .collect();
            let Some(duration) = number_value(row.get("lap_duration")) else {
                let time_ms = observed?;
                driver?;
                if sectors.iter().all(Option::is_none) && segments.is_empty() {
                    return None;
                }
                return Some(RaceEvent {
                    source_time_ms: timestamp(&row["date_start"]),
                    time_ms,
                    driver,
                    payload: EventData::Sectors(Lap {
                        number,
                        time_ms,
                        sectors,
                        segments,
                        ..Default::default()
                    }),
                });
            };
            if duration <= 0.0 || duration > 7200.0 {
                return None;
            }
            let completed_ms = timestamp(&row["date_start"])
                .and_then(|start| start.checked_add((duration * 1000.0).round() as i64))
                .or(observed)?;
            return Some(RaceEvent {
                source_time_ms: timestamp(&row["date_start"]),
                time_ms: observed.unwrap_or(completed_ms),
                driver,
                payload: EventData::Lap(Lap {
                    number,
                    time_ms: completed_ms,
                    duration: Some(duration),
                    speed_trap: number_value(row.get("st_speed")),
                    sectors,
                    segments,
                    pit_out: row["is_pit_out_lap"].as_bool().unwrap_or(false),
                    ..Default::default()
                }),
            });
        }
        "position" => EventData::Position(u32::try_from(row["position"].as_u64()?).ok()?),
        "stints" => EventData::Stint(Stint {
            number: u32::try_from(row["stint_number"].as_u64()?).ok()?,
            compound: row["compound"].as_str().unwrap_or("UNKNOWN").into(),
            start_lap: u32::try_from(row["lap_start"].as_u64()?).ok()?,
            end_lap: None,
            tyre_age_at_start: row["tyre_age_at_start"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())?,
            is_new: row["tyre_age_at_start"].as_u64().map(|n| n == 0),
        }),
        "intervals" => EventData::Interval {
            gap: number(&row["gap_to_leader"]),
            interval: number(&row["interval"]),
        },
        "car_data" => {
            let s = TelemetrySample {
                time_ms: dated.or(observed)?,
                speed: number(&row["speed"]),
                rpm: number(&row["rpm"]),
                gear: row["n_gear"].as_u64().and_then(|v| u8::try_from(v).ok()),
                throttle: number(&row["throttle"]),
                brake: row["brake"].as_f64().map(|v| v > 0.0),
                drs: match row["drs"].as_i64() {
                    Some(10 | 12 | 14) => Some(true),
                    Some(0 | 1) => Some(false),
                    _ => None,
                },
                ..Default::default()
            };
            s.validate().ok()?;
            EventData::Telemetry(s)
        }
        "location" => EventData::Location(number(&row["x"])?, number(&row["y"])?),
        "weather" => EventData::Weather(Weather {
            air: number(&row["air_temperature"]),
            track: number(&row["track_temperature"]),
            humidity: number(&row["humidity"]),
            wind_speed: number(&row["wind_speed"]),
            wind_direction: number(&row["wind_direction"]),
            rain: row["rainfall"].as_f64().map(|v| v > 0.0),
        }),
        "pit" => EventData::Pit(Pit {
            time_ms: dated.or(observed)?,
            lap: u32::try_from(row["lap_number"].as_u64()?).ok()?,
            lane_duration: number(&row["lane_duration"]).or_else(|| number(&row["pit_duration"])),
            stop_duration: number(&row["stop_duration"]),
        }),
        "race_control" => {
            let message = row["message"].as_str()?.chars().take(2000).collect();
            let category = row["category"].as_str().unwrap_or("Message").to_string();
            return Some(RaceEvent {
                source_time_ms: dated,
                time_ms: observed.or(dated)?,
                driver: None,
                payload: EventData::Control(ControlMessage {
                    time_ms: dated.or(observed)?,
                    driver,
                    message,
                    category,
                    flag: row["flag"].as_str().map(str::to_string),
                    sector: row["sector"].as_u64().and_then(|v| u32::try_from(v).ok()),
                    qualifying_phase: row["qualifying_phase"]
                        .as_u64()
                        .and_then(|v| u8::try_from(v).ok())
                        .filter(|v| (1..=3).contains(v)),
                }),
            });
        }
        _ => return None,
    };
    Some(RaceEvent {
        source_time_ms: dated,
        time_ms: observed.or(dated)?,
        driver: if topic == "weather" { None } else { driver },
        payload,
    })
}
fn number_value(v: Option<&Value>) -> Option<f64> {
    v.and_then(number)
}

pub fn status_from_control(message: &ControlMessage) -> Option<String> {
    let text = message.message.to_uppercase();
    if text.contains("VIRTUAL SAFETY CAR DEPLOYED") {
        Some("VSC".into())
    } else if text.contains("SAFETY CAR DEPLOYED") {
        Some("SC".into())
    } else if text.contains("VIRTUAL SAFETY CAR ENDED")
        || text.contains("TRACK CLEAR")
        || text.contains("GREEN LIGHT")
    {
        Some("GREEN".into())
    } else if message.flag.as_deref() == Some("RED") {
        Some("RED".into())
    } else {
        None
    }
}
