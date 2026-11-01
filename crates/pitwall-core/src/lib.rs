//! Units: timestamps in milliseconds, lap times in seconds, distance in metres, speed in km/h.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub struct Session {
    pub key: u32,
    pub year: u16,
    pub event: String,
    pub name: String,
    pub circuit: String,
    pub country: String,
    #[ts(type = "number")]
    pub start_ms: i64,
    #[ts(type = "number")]
    pub end_ms: i64,
    pub total_laps: Option<u32>,
    pub synthetic: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub struct Driver {
    pub number: u32,
    pub code: String,
    pub name: String,
    pub team: String,
    pub color: String,
}
impl Driver {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.number == 0 || self.number > 999 || self.code.is_empty() || self.code.len() > 8 {
            Err("Invalid driver identity")
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub struct TelemetrySample {
    #[ts(type = "number")]
    pub time_ms: i64,
    pub speed: Option<f64>,
    pub rpm: Option<f64>,
    pub gear: Option<u8>,
    pub throttle: Option<f64>,
    pub brake: Option<bool>,
    pub drs: Option<bool>,
    pub x: Option<f64>,
    pub y: Option<f64>,
}
impl TelemetrySample {
    pub fn validate(&self) -> Result<(), &'static str> {
        if [self.speed, self.rpm, self.throttle, self.x, self.y]
            .iter()
            .flatten()
            .any(|v| !v.is_finite())
            || self.speed.is_some_and(|v| !(0.0..=450.0).contains(&v))
            || self.throttle.is_some_and(|v| !(0.0..=100.0).contains(&v))
            || self.gear.is_some_and(|v| v > 9)
        {
            Err("Invalid telemetry")
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub struct Lap {
    pub number: u32,
    #[ts(type = "number")]
    pub time_ms: i64,
    pub duration: Option<f64>,
    pub sectors: Vec<Option<f64>>,
    pub segments: Vec<u16>,
    pub pit_out: bool,
    pub pit_in: bool,
    pub compound: String,
    pub tyre_age: Option<u32>,
    pub position: Option<u32>,
    pub gap: Option<f64>,
    pub interval: Option<f64>,
    pub status: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub struct Stint {
    pub number: u32,
    pub compound: String,
    pub start_lap: u32,
    pub end_lap: Option<u32>,
    pub tyre_age_at_start: u32,
    pub is_new: Option<bool>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub struct Pit {
    #[ts(type = "number")]
    pub time_ms: i64,
    pub lap: u32,
    pub lane_duration: Option<f64>,
    pub stop_duration: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub struct Weather {
    pub air: Option<f64>,
    pub track: Option<f64>,
    pub humidity: Option<f64>,
    pub wind_speed: Option<f64>,
    pub wind_direction: Option<f64>,
    pub rain: Option<bool>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub struct ControlMessage {
    #[ts(type = "number")]
    pub time_ms: i64,
    pub driver: Option<u32>,
    pub message: String,
    pub category: String,
    pub flag: Option<String>,
    pub sector: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub struct Band {
    pub value: f64,
    pub low: f64,
    pub high: f64,
}
impl Band {
    pub fn validate(&self) -> Result<(), &'static str> {
        if [self.low, self.value, self.high]
            .iter()
            .all(|v| v.is_finite())
            && self.low <= self.value
            && self.value <= self.high
        {
            Ok(())
        } else {
            Err("Invalid prediction band")
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub struct PitWindow {
    pub from_lap: u32,
    pub to_lap: u32,
    pub probability: f64,
    pub distribution: Vec<(u32, f64)>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub struct Prediction {
    pub driver: u32,
    pub lap: u32,
    pub model: String,
    pub label: String,
    pub status: String,
    pub confidence: f64,
    pub pace: Vec<Band>,
    pub degradation: Option<Band>,
    pub laps_to_cliff: Option<Band>,
    pub tyre_health: Option<Band>,
    pub pit_window: Option<PitWindow>,
    pub finish: Vec<(u32, f64)>,
    pub rejoin: Option<Band>,
    pub factors: Vec<String>,
    pub insight: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub struct DriverState {
    pub driver: Driver,
    pub position: Option<u32>,
    pub start_position: Option<u32>,
    pub gap: Option<f64>,
    pub interval: Option<f64>,
    pub lap: u32,
    pub laps: Vec<Lap>,
    pub stints: Vec<Stint>,
    pub pits: Vec<Pit>,
    pub in_pit: bool,
    pub telemetry: Option<TelemetrySample>,
    pub location: Option<(f64, f64)>,
    pub prediction: Option<Prediction>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub struct Circuit {
    pub outline: Vec<(f64, f64)>,
    pub corners: Vec<(String, f64, f64)>,
    pub sectors: Vec<f64>,
    pub drs_zones: Vec<(f64, f64)>,
    pub pit_lane: Vec<(f64, f64)>,
    pub pit_loss: Option<Band>,
    pub source: String,
    pub approximate: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub struct Snapshot {
    pub session: Session,
    #[ts(type = "number")]
    pub clock_ms: i64,
    pub playing: bool,
    pub speed: f64,
    pub lap: u32,
    pub track_status: String,
    pub data_status: String,
    pub weather: Weather,
    pub drivers: BTreeMap<u32, DriverState>,
    pub messages: Vec<ControlMessage>,
    pub circuit: Circuit,
    #[ts(type = "number")]
    pub revision: u64,
}
impl Snapshot {
    pub fn empty(session: Session) -> Self {
        Self {
            clock_ms: session.start_ms,
            session,
            playing: false,
            speed: 1.0,
            lap: 0,
            track_status: "UNKNOWN".into(),
            data_status: "loading".into(),
            weather: Weather::default(),
            drivers: BTreeMap::new(),
            messages: vec![],
            circuit: Circuit::default(),
            revision: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RaceEvent {
    pub time_ms: i64,
    pub driver: Option<u32>,
    pub payload: EventData,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum EventData {
    Lap(Lap),
    Stint(Stint),
    Pit(Pit),
    Telemetry(TelemetrySample),
    Location(f64, f64),
    Position(u32),
    Interval {
        gap: Option<f64>,
        interval: Option<f64>,
    },
    Weather(Weather),
    Control(ControlMessage),
    Status(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionArchive {
    pub session: Session,
    pub drivers: Vec<Driver>,
    pub events: Vec<RaceEvent>,
    pub circuit: Circuit,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub enum ServerFrame {
    Snapshot {
        sequence: u32,
        generation: u32,
        state: Box<Snapshot>,
    },
    Delta {
        sequence: u32,
        generation: u32,
        changes: serde_json::Value,
    },
    Telemetry {
        driver: u32,
        samples: Vec<TelemetrySample>,
    },
    Error {
        message: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub enum ClientCommand {
    Open {
        session: u32,
    },
    Play {
        playing: bool,
    },
    Speed {
        speed: f64,
    },
    Seek {
        #[ts(type = "number")]
        time_ms: i64,
    },
    Subscribe {
        driver: Option<u32>,
    },
    Resync,
}
