//! Units: timestamps in milliseconds, lap times in seconds, distance in metres, speed in km/h.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;
pub fn valid_time_range(start: i64, end: i64, max_ms: i64) -> bool {
    end.checked_sub(start)
        .is_some_and(|length| length > 0 && length <= max_ms)
}

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
    pub speed_trap: Option<f64>,
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
    pub qualifying_phase: Option<u8>,
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
pub struct RiskForecast {
    pub probability: f64,
    pub horizon_laps: u32,
    pub model: String,
    pub label: String,
    pub factors: Vec<String>,
}
impl RiskForecast {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.probability.is_finite()
            && (0.0..=1.0).contains(&self.probability)
            && (1..=30).contains(&self.horizon_laps)
            && !self.model.is_empty()
        {
            Ok(())
        } else {
            Err("Invalid risk forecast")
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub struct OvertakeForecast {
    pub target: u32,
    pub forecast: RiskForecast,
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
    pub qualifying_need: Option<Band>,
    #[serde(default)]
    pub safety_car: Option<RiskForecast>,
    #[serde(default)]
    pub overtake: Option<OvertakeForecast>,
    pub factors: Vec<String>,
    pub insight: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub struct DriverState {
    pub driver: Driver,
    pub position: Option<u32>,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub position_time_ms: Option<i64>,
    pub start_position: Option<u32>,
    pub gap: Option<f64>,
    pub interval: Option<f64>,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub interval_time_ms: Option<i64>,
    pub lap: u32,
    pub laps: Vec<Lap>,
    pub partial_lap: Option<Lap>,
    pub tyre_age: Option<u32>,
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
    pub qualifying_phase: Option<u8>,
    #[ts(type = "number | null")]
    pub qualifying_start_ms: Option<i64>,
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
            qualifying_phase: None,
            qualifying_start_ms: None,
            revision: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RaceEvent {
    #[serde(default)]
    pub source_time_ms: Option<i64>,
    pub time_ms: i64,
    pub driver: Option<u32>,
    pub payload: EventData,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum EventData {
    Lap(Lap),
    Sectors(Lap),
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
    #[serde(default)]
    pub captured_at_ms: Option<i64>,
    pub session: Session,
    pub drivers: Vec<Driver>,
    pub events: Vec<RaceEvent>,
    pub circuit: Circuit,
}
impl SessionArchive {
    pub fn reusable_at(&self, now: i64) -> bool {
        self.session.synthetic
            || self.captured_at_ms.is_some_and(|captured| {
                (self.session.end_ms > 0 && captured.saturating_sub(self.session.end_ms) >= 60_000)
                    || (0..60_000).contains(&now.saturating_sub(captured))
            })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub enum ServerFrame {
    Evaluation {
        generation: u32,
        forecasts: Vec<PaceForecast>,
        report: AccuracyReport,
    },
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
    Live,
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
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub struct PaceForecast {
    pub session: u32,
    pub driver: u32,
    pub issued_lap: u32,
    pub target_lap: u32,
    pub band: Band,
    pub actual: Option<f64>,
    pub model: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../pitwall-web/src/types/")]
pub struct AccuracyReport {
    pub pace_samples: usize,
    pub pace_mae: Option<f64>,
    pub band_coverage: Option<f64>,
    pub pit_samples: usize,
    pub pit_accuracy: Option<f64>,
    pub position_samples: usize,
    pub position_accuracy: Option<f64>,
}

#[cfg(test)]
mod risk_tests {
    use super::*;

    #[test]
    fn risk_forecasts_reject_invalid_probabilities_and_horizons() {
        let mut forecast = RiskForecast {
            probability: 0.2,
            horizon_laps: 5,
            model: "test-v1".into(),
            label: "Experimental model".into(),
            factors: vec![],
        };
        assert!(forecast.validate().is_ok());
        for probability in [-0.1, 1.1, f64::NAN] {
            forecast.probability = probability;
            assert!(forecast.validate().is_err());
        }
        forecast.probability = 0.2;
        forecast.horizon_laps = 0;
        assert!(forecast.validate().is_err());
    }
}
