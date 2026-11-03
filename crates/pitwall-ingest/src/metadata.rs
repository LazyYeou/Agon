//! Curated metadata; never infer scheduled laps from completed race results.
use anyhow::{ensure, Context, Result};
use chrono::Datelike;
use pitwall_core::{Band, Circuit, Session};
use serde::Deserialize;
use std::{collections::HashSet, path::Path};

const MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataManifest {
    version: u32,
    entries: Vec<Entry>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    session_key: u32,
    year: u16,
    circuit: String,
    session_name: String,
    start_ms: i64,
    configuration: String,
    schedule: Option<Schedule>,
    geometry: Option<Geometry>,
    pit_loss: Option<PitLoss>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Provenance {
    source: String,
    licence: String,
    known_at_ms: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Schedule {
    total_laps: u32,
    provenance: Provenance,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Geometry {
    coordinate_system: String,
    reference_session_key: u32,
    alignment: String,
    fraction_basis: String,
    approximate: bool,
    outline: Vec<(f64, f64)>,
    #[serde(default)]
    corners: Vec<(String, f64, f64)>,
    #[serde(default)]
    sectors: Vec<f64>,
    #[serde(default)]
    drs_zones: Vec<(f64, f64)>,
    #[serde(default)]
    pit_lane: Vec<(f64, f64)>,
    provenance: Provenance,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PitLoss {
    band: Band,
    data_through_ms: i64,
    calibration_sessions: Vec<CalibrationSession>,
    method: String,
    provenance: Provenance,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CalibrationSession {
    session_key: u32,
    end_ms: i64,
}

impl MetadataManifest {
    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= MAX_BYTES, "Metadata exceeds 8 MiB");
        let manifest: Self = serde_json::from_slice(bytes).context("Invalid metadata manifest")?;
        ensure!(manifest.version == 1, "Unsupported metadata version");
        ensure!(
            manifest.entries.len() <= 10_000,
            "Too many metadata entries"
        );
        let mut keys = HashSet::new();
        for entry in &manifest.entries {
            ensure!(
                keys.insert(entry.session_key),
                "Duplicate metadata session {}",
                entry.session_key
            );
            entry
                .validate()
                .with_context(|| format!("Metadata session {}", entry.session_key))?;
        }
        Ok(manifest)
    }
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        use std::io::Read;
        let file = std::fs::File::open(path.as_ref()).context("Open metadata manifest")?;
        let mut bytes = Vec::new();
        file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes)?;
        Self::from_slice(&bytes)
    }
    /// Unknown sessions remain unchanged; a known key with conflicting identity is an error.
    pub fn apply(&self, session: &mut Session, circuit: &mut Circuit) -> Result<bool> {
        let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.session_key == session.key)
        else {
            return Ok(false);
        };
        ensure!(
            !session.synthetic
                && entry.year == session.year
                && entry.circuit == session.circuit
                && entry.session_name == session.name
                && entry.start_ms == session.start_ms,
            "Metadata identity mismatch for session {}",
            session.key
        );
        if let Some(schedule) = &entry.schedule {
            session.total_laps = Some(schedule.total_laps);
        }
        if let Some(geometry) = &entry.geometry {
            circuit.outline.clone_from(&geometry.outline);
            circuit.corners.clone_from(&geometry.corners);
            circuit.sectors.clone_from(&geometry.sectors);
            circuit.drs_zones.clone_from(&geometry.drs_zones);
            circuit.pit_lane.clone_from(&geometry.pit_lane);
            circuit.approximate = geometry.approximate;
        }
        if let Some(loss) = &entry.pit_loss {
            circuit.pit_loss = Some(loss.band.clone());
        }
        for (kind, provenance) in [
            ("Schedule", entry.schedule.as_ref().map(|s| &s.provenance)),
            ("Geometry", entry.geometry.as_ref().map(|s| &s.provenance)),
            ("Pit loss", entry.pit_loss.as_ref().map(|s| &s.provenance)),
        ] {
            if let Some(p) = provenance {
                let attribution = format!(
                    "{kind} ({}): {} [{}; known at {}]",
                    entry.configuration, p.source, p.licence, p.known_at_ms
                );
                if !circuit.source.contains(&attribution) {
                    if !circuit.source.is_empty() {
                        circuit.source.push_str(" | ");
                    }
                    circuit.source.push_str(&attribution);
                }
            }
        }
        Ok(true)
    }
}

/// Private manifests replace the bundled coverage; read failures are surfaced, never ignored.
pub fn apply_metadata(session: &mut Session, circuit: &mut Circuit) -> Result<bool> {
    let manifest = match std::env::var_os("AGON_METADATA_FILE") {
        Some(path) => MetadataManifest::from_path(path)?,
        None => MetadataManifest::from_slice(include_bytes!("../../../metadata/verified.json"))?,
    };
    manifest.apply(session, circuit)
}

impl Provenance {
    fn validate(&self, start_ms: i64) -> Result<()> {
        let url = reqwest::Url::parse(&self.source).context("Metadata needs a source URL")?;
        ensure!(
            matches!(url.scheme(), "https" | "http") && url.host_str().is_some(),
            "Invalid metadata source URL"
        );
        ensure!(
            !self.licence.trim().is_empty(),
            "Metadata licence/permission is required"
        );
        ensure!(
            self.known_at_ms > 0 && self.known_at_ms < start_ms,
            "Metadata must be known before session start"
        );
        Ok(())
    }
}
impl Entry {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.session_key > 0
                && self.year >= 2023
                && !self.circuit.trim().is_empty()
                && !self.session_name.trim().is_empty()
                && !self.configuration.trim().is_empty(),
            "Incomplete metadata identity/configuration"
        );
        ensure!(
            chrono::DateTime::from_timestamp_millis(self.start_ms)
                .is_some_and(|d| d.year() == i32::from(self.year)),
            "Metadata year/start mismatch"
        );
        ensure!(
            self.schedule.is_some() || self.geometry.is_some() || self.pit_loss.is_some(),
            "Empty metadata entry"
        );
        if let Some(schedule) = &self.schedule {
            ensure!(
                matches!(self.session_name.as_str(), "Race" | "Sprint")
                    && (1..=500).contains(&schedule.total_laps),
                "Invalid scheduled race distance"
            );
            schedule.provenance.validate(self.start_ms)?;
        }
        if let Some(loss) = &self.pit_loss {
            loss.provenance.validate(self.start_ms)?;
            loss.band.validate().map_err(anyhow::Error::msg)?;
            ensure!(
                loss.band.low > 0.0 && loss.band.high <= 300.0,
                "Invalid strategic pit-loss seconds"
            );
            ensure!(
                loss.data_through_ms > 0 && loss.data_through_ms <= loss.provenance.known_at_ms,
                "Pit-loss data must predate source availability"
            );
            ensure!(
                !loss.method.trim().is_empty()
                    && !loss.calibration_sessions.is_empty()
                    && loss.calibration_sessions.iter().all(|s| s.session_key > 0
                        && s.session_key != self.session_key
                        && s.end_ms > 0
                        && s.end_ms <= loss.data_through_ms),
                "Pit loss requires prior-session calibration, excluding target session"
            );
        }
        if let Some(g) = &self.geometry {
            g.provenance.validate(self.start_ms)?;
            ensure!(g.coordinate_system == "openf1_location" && g.reference_session_key == self.session_key
                && !g.alignment.trim().is_empty() && g.fraction_basis == "outline_index", "Geometry must be registered to this session's OpenF1 coordinate frame and outline indices");
            let point = |(x, y): (f64, f64)| {
                x.is_finite() && y.is_finite() && x.abs() <= 1e9 && y.abs() <= 1e9
            };
            ensure!(
                (3..=20_000).contains(&g.outline.len()) && g.outline.iter().copied().all(point),
                "Invalid geometry outline"
            );
            let bounds = g.outline.iter().fold(
                (
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                ),
                |(xmin, xmax, ymin, ymax), (x, y)| {
                    (xmin.min(*x), xmax.max(*x), ymin.min(*y), ymax.max(*y))
                },
            );
            ensure!(
                bounds.1 > bounds.0 && bounds.3 > bounds.2,
                "Degenerate geometry outline"
            );
            let aligned = |p: (f64, f64)| {
                point(p)
                    && p.0 >= bounds.0 - (bounds.1 - bounds.0) * 0.2
                    && p.0 <= bounds.1 + (bounds.1 - bounds.0) * 0.2
                    && p.1 >= bounds.2 - (bounds.3 - bounds.2) * 0.2
                    && p.1 <= bounds.3 + (bounds.3 - bounds.2) * 0.2
            };
            let mut names = HashSet::new();
            ensure!(
                g.corners.len() <= 100
                    && g.corners.iter().all(|(name, x, y)| !name.trim().is_empty()
                        && names.insert(name)
                        && aligned((*x, *y))),
                "Invalid or unaligned corner markers"
            );
            ensure!(
                g.pit_lane.is_empty()
                    || ((2..=20_000).contains(&g.pit_lane.len())
                        && g.pit_lane.iter().copied().all(aligned)),
                "Invalid or unaligned pit lane"
            );
            ensure!(
                g.sectors.is_empty()
                    || (g.sectors.len() == 3
                        && g.sectors[0] == 0.0
                        && g.sectors
                            .iter()
                            .all(|v| v.is_finite() && (0.0..1.0).contains(v))
                        && g.sectors.windows(2).all(|pair| pair[0] < pair[1])),
                "Timing sectors require three ordered outline-index starts including zero"
            );
            ensure!(
                g.drs_zones.len() <= 10
                    && g.drs_zones.iter().all(|(a, b)| a.is_finite()
                        && b.is_finite()
                        && *a >= 0.0
                        && *a < *b
                        && *b <= 1.0),
                "Invalid DRS outline-index range"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn session() -> Session {
        Session {
            key: 9165,
            year: 2023,
            circuit: "Singapore".into(),
            name: "Race".into(),
            start_ms: 1_694_952_000_000,
            ..Default::default()
        }
    }
    fn fixture() -> Value {
        let provenance = json!({"source":"https://example.org/pre-race", "licence":"Private permission for test fixture", "known_at_ms":1_694_520_000_000i64});
        json!({"version":1,"entries":[{
            "session_key":9165,"year":2023,"circuit":"Singapore","session_name":"Race",
            "start_ms":1_694_952_000_000i64,"configuration":"2023 revised layout",
            "schedule":{"total_laps":62,"provenance":provenance},
            "geometry":{"coordinate_system":"openf1_location","reference_session_key":9165,
                "alignment":"Registered to this session's location feed; test fixture only",
                "fraction_basis":"outline_index", "approximate":true,
                "outline":[[0,0],[10,0],[10,10],[0,10],[0,0]],
                "corners":[["T1",10,0]],"sectors":[0,0.25,0.75],"drs_zones":[[0.2,0.4]],"pit_lane":[[0,0],[1,1]],
                "provenance":provenance},
            "pit_loss":{"band":{"low":20,"value":22,"high":24},"data_through_ms":1_694_000_000_000i64,
                "calibration_sessions":[{"session_key":9000,"end_ms":1_693_000_000_000i64}],"method":"Prior-session green-flag paired lap estimate", "provenance":provenance}
        }]})
    }
    fn parse(value: &Value) -> Result<MetadataManifest> {
        MetadataManifest::from_slice(&serde_json::to_vec(value).unwrap())
    }
    #[test]
    fn applies_only_to_exact_identity_and_keeps_provenance() {
        let manifest = parse(&fixture()).unwrap();
        let mut session = session();
        let mut circuit = Circuit::default();
        assert!(manifest.apply(&mut session, &mut circuit).unwrap());
        assert_eq!(session.total_laps, Some(62));
        assert_eq!(circuit.corners, vec![("T1".into(), 10.0, 0.0)]);
        assert_eq!(circuit.pit_loss.as_ref().unwrap().value, 22.0);
        assert!(circuit.source.contains("https://example.org/pre-race"));
        let source = circuit.source.clone();
        manifest.apply(&mut session, &mut circuit).unwrap();
        assert_eq!(circuit.source, source);
        session.circuit = "Different".into();
        assert!(manifest.apply(&mut session, &mut circuit).is_err());
        session.key = 42;
        assert!(!manifest.apply(&mut session, &mut circuit).unwrap());
    }
    #[test]
    fn rejects_future_sources_invalid_geometry_and_same_race_calibration() {
        for (path, bad) in [
            (
                "/entries/0/schedule/provenance/known_at_ms",
                json!(1_694_952_000_000i64),
            ),
            ("/entries/0/geometry/reference_session_key", json!(9161)),
            ("/entries/0/geometry/coordinate_system", json!("metres")),
            (
                "/entries/0/geometry/outline",
                json!([[0, 0], [0, 0], [0, 0]]),
            ),
            ("/entries/0/geometry/sectors", json!([0, 0.7, 0.3])),
            ("/entries/0/geometry/drs_zones", json!([[0.9, 1.2]])),
            (
                "/entries/0/pit_loss/calibration_sessions/0/session_key",
                json!(9165),
            ),
            (
                "/entries/0/pit_loss/calibration_sessions/0/end_ms",
                json!(1_694_952_000_000i64),
            ),
            (
                "/entries/0/pit_loss/data_through_ms",
                json!(1_694_952_000_000i64),
            ),
            ("/entries/0/pit_loss/band/low", json!(30)),
            ("/entries/0/schedule/total_laps", json!(0)),
            ("/entries/0/schedule/provenance/licence", json!("")),
        ] {
            let mut value = fixture();
            *value.pointer_mut(path).unwrap() = bad;
            assert!(parse(&value).is_err(), "accepted {path}");
        }
        let mut duplicate = fixture();
        let entry = duplicate["entries"][0].clone();
        duplicate["entries"].as_array_mut().unwrap().push(entry);
        assert!(parse(&duplicate).is_err());
    }
    #[test]
    fn absent_fields_preserve_approximate_outline_and_unknown_schedule() {
        let mut value = fixture();
        value["entries"][0]
            .as_object_mut()
            .unwrap()
            .remove("schedule");
        value["entries"][0]
            .as_object_mut()
            .unwrap()
            .remove("geometry");
        let manifest = parse(&value).unwrap();
        let mut session = session();
        let mut circuit = Circuit {
            outline: vec![(1.0, 2.0), (3.0, 4.0)],
            approximate: true,
            ..Default::default()
        };
        manifest.apply(&mut session, &mut circuit).unwrap();
        assert_eq!(session.total_laps, None);
        assert_eq!(circuit.outline, vec![(1.0, 2.0), (3.0, 4.0)]);
        assert!(circuit.approximate);
    }

    #[test]
    fn bundled_schedule_is_available_without_inventing_geometry_or_pit_loss() {
        let manifest =
            MetadataManifest::from_slice(include_bytes!("../../../metadata/verified.json"))
                .unwrap();
        let mut session = session();
        let mut circuit = Circuit::default();
        manifest.apply(&mut session, &mut circuit).unwrap();
        assert_eq!(session.total_laps, Some(62));
        assert!(circuit.outline.is_empty() && circuit.corners.is_empty());
        assert!(circuit.pit_loss.is_none());
    }
}
