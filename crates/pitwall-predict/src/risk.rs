use anyhow::{ensure, Context, Result};
use ort::{session::Session as OrtSession, value::Tensor};
use pitwall_core::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Mutex, OnceLock},
};

const SC_FEATURES: [&str; 3] = ["lap", "rain", "circuit_prior"];
const PASS_FEATURES: [&str; 6] = [
    "lap",
    "rain",
    "circuit_prior",
    "interval",
    "tyre_age_delta",
    "compound_delta",
];

struct RiskModel {
    session: OrtSession,
    features: usize,
    priors: BTreeMap<String, f64>,
    brier: f64,
    baseline_brier: f64,
}
impl RiskModel {
    fn probability(&mut self, features: &[f32]) -> Result<f64> {
        ensure!(
            features.len() == self.features && features.iter().all(|v| v.is_finite()),
            "Invalid risk features"
        );
        let input = Tensor::from_array(([1, self.features], features.to_vec().into_boxed_slice()))?;
        let result = self.session.run(ort::inputs![input])?;
        let (_, values) = result["probability"].try_extract_tensor::<f32>()?;
        let value = *values.first().context("Missing risk probability")? as f64;
        ensure!(
            value.is_finite() && (0.0..=1.0).contains(&value),
            "Invalid risk probability"
        );
        Ok(value)
    }
    fn forecast(
        &mut self,
        features: &[f32],
        version: &str,
        label: &str,
        mut factors: Vec<String>,
    ) -> Result<RiskForecast> {
        let probability = self.probability(features)?;
        factors.push(format!(
            "Race-disjoint holdout Brier {:.4}; constant baseline {:.4}{}",
            self.brier,
            self.baseline_brier,
            if self.brier > self.baseline_brier {
                "; below baseline"
            } else {
                ""
            }
        ));
        Ok(RiskForecast {
            probability,
            horizon_laps: 5,
            model: version.into(),
            label: label.into(),
            factors,
        })
    }
}
struct RiskModels {
    sc: RiskModel,
    overtake: RiskModel,
    version: String,
}
impl RiskModels {
    fn load(directory: &Path) -> Result<Self> {
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(directory.join("risk_manifest.json"))?)?;
        ensure!(
            manifest["schema"] == "agon-risk-v1" && manifest["horizon_laps"] == 5,
            "Incompatible risk model manifest"
        );
        let version = manifest["model"]
            .as_str()
            .context("Missing risk version")?
            .to_owned();
        let mut validated = vec![];
        for (name, filename, features) in [
            ("safety_car", "risk_sc.onnx", SC_FEATURES.as_slice()),
            ("overtake", "risk_overtake.onnx", PASS_FEATURES.as_slice()),
        ] {
            let model = &manifest["models"][name];
            ensure!(
                model["features"]
                    .as_array()
                    .context("Missing risk features")?
                    .iter()
                    .map(Value::as_str)
                    .eq(features.iter().map(|feature| Some(*feature))),
                "Incompatible risk feature schema"
            );
            ensure!(model["file"] == filename, "Incompatible risk artifact name");
            let bytes = std::fs::read(directory.join(filename))?;
            ensure!(
                model["sha256"].as_str() == Some(format!("{:x}", Sha256::digest(&bytes)).as_str()),
                "Risk artifact checksum mismatch"
            );
            let priors: BTreeMap<String, f64> =
                serde_json::from_value(model["circuit_priors"].clone())?;
            ensure!(
                !priors.is_empty()
                    && priors
                        .values()
                        .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                "Invalid circuit priors"
            );
            let brier = model["brier"].as_f64().context("Missing risk validation")?;
            let baseline_brier = model["constant_brier"]
                .as_f64()
                .context("Missing risk baseline")?;
            ensure!(
                [brier, baseline_brier]
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                "Invalid risk validation"
            );
            validated.push((bytes, features.len(), priors, brier, baseline_brier));
        }
        let library =
            std::env::var("ONNXRUNTIME_DYLIB_PATH").context("ONNX Runtime library unavailable")?;
        ensure!(
            Path::new(&library).is_file(),
            "ONNX Runtime library unavailable"
        );
        ort::init_from(library).with_name("agon").commit()?;
        let mut models = validated.into_iter().map(
            |(bytes, features, priors, brier, baseline_brier)| -> Result<RiskModel> {
                Ok(RiskModel {
                    session: OrtSession::builder()?
                        .with_intra_threads(1)?
                        .commit_from_memory(&bytes)?,
                    features,
                    priors,
                    brier,
                    baseline_brier,
                })
            },
        );
        Ok(Self {
            sc: models.next().context("SC model missing")??,
            overtake: models.next().context("Pass model missing")??,
            version,
        })
    }
}

fn race_features(s: &Snapshot, prior: f64) -> Option<[f32; 3]> {
    if s.session.synthetic
        || !matches!(s.session.name.as_str(), "Race" | "Sprint")
        || s.track_status != "GREEN"
        || s.lap == 0
        || !prior.is_finite()
        || !(0.0..=1.0).contains(&prior)
        || s.session
            .total_laps
            .is_none_or(|total| s.lap.saturating_add(5) > total)
    {
        return None;
    }
    Some([s.lap as f32, u8::from(s.weather.rain?) as f32, prior as f32])
}
fn compound(driver: &DriverState) -> Option<f32> {
    match driver.stints.last()?.compound.as_str() {
        "SOFT" => Some(0.0),
        "MEDIUM" => Some(1.0),
        "HARD" => Some(2.0),
        "INTERMEDIATE" => Some(3.0),
        "WET" => Some(4.0),
        _ => None,
    }
}
fn pass_features(s: &Snapshot, d: &DriverState, prior: f64) -> Option<(u32, [f32; 6])> {
    let race = race_features(s, prior)?;
    if d.in_pit
        || d.pits.iter().any(|pit| {
            s.clock_ms
                .checked_sub(pit.time_ms)
                .is_some_and(|age| (0..=60_000).contains(&age))
        })
        || d.lap == 0
        || d.position_time_ms? > s.clock_ms
        || d.laps.last().is_some_and(|l| l.pit_in || l.pit_out)
    {
        return None;
    }
    let interval_age = s.clock_ms.checked_sub(d.interval_time_ms?)?;
    if !(0..=30_000).contains(&interval_age) {
        return None;
    }
    let interval = d.interval?;
    if !interval.is_finite() || !(0.0 < interval && interval <= 10.0) {
        return None;
    }
    let target_position = d.position?.checked_sub(1)?;
    let mut targets = s.drivers.values().filter(|other| {
        other.position == Some(target_position) && other.driver.number != d.driver.number
    });
    let target = targets.next()?;
    if targets.next().is_some()
        || target.in_pit
        || target.pits.iter().any(|pit| {
            s.clock_ms
                .checked_sub(pit.time_ms)
                .is_some_and(|age| (0..=60_000).contains(&age))
        })
        || target.lap != d.lap
        || target.position_time_ms? > s.clock_ms
        || target.laps.last().is_some_and(|l| l.pit_in || l.pit_out)
    {
        return None;
    }
    Some((
        target.driver.number,
        [
            d.lap as f32,
            race[1],
            race[2],
            interval as f32,
            d.tyre_age? as f32 - target.tyre_age? as f32,
            compound(d)? - compound(target)?,
        ],
    ))
}

static MODELS: OnceLock<Result<Mutex<RiskModels>, String>> = OnceLock::new();
pub(crate) fn apply(s: &Snapshot, predictions: &mut [Prediction]) {
    for p in predictions.iter_mut() {
        p.safety_car = None;
        p.overtake = None;
    }
    if race_features(s, 0.0).is_none() {
        return;
    }
    let bundle = MODELS.get_or_init(|| {
        RiskModels::load(Path::new(
            &std::env::var("AGON_MODELS").unwrap_or_else(|_| "models".into()),
        ))
        .map(Mutex::new)
        .map_err(|e| e.to_string())
    });
    let Ok(bundle) = bundle else {
        return;
    };
    let Ok(mut bundle) = bundle.lock() else {
        return;
    };
    let version = bundle.version.clone();
    let safety_car = bundle
        .sc
        .priors
        .get(&s.session.circuit)
        .copied()
        .and_then(|prior| {
            let features = race_features(s, prior)?;
            bundle
                .sc
                .forecast(
                    &features,
                    &version,
                    "New SC/VSC deployment within five race laps",
                    vec![format!(
                        "Lap {}; observed rain {}; training circuit prior {:.1}%",
                        s.lap,
                        s.weather.rain?,
                        prior * 100.0
                    )],
                )
                .ok()
        });
    for p in predictions {
        p.safety_car = safety_car.clone();
        let Some(d) = s.drivers.get(&p.driver) else {
            continue;
        };
        let Some(prior) = bundle.overtake.priors.get(&s.session.circuit).copied() else {
            continue;
        };
        let Some((target, features)) = pass_features(s, d, prior) else {
            continue;
        };
        p.overtake = bundle.overtake.forecast(&features, &version, "Position-observed pass; conditional on five green laps without pits/retirement", vec![format!("Gap {:.2} s; tyre age difference {:+.0} laps; compound difference {:+.0}", features[3], features[4], features[5]), "Position timing feed; filtered reversals approximate physical passes. No DRS or pace-delta feature.".into()]).ok().map(|forecast| OvertakeForecast { target, forecast });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn directory() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models")
    }
    #[test]
    fn manifest_schema_and_hash_are_required() {
        let source = directory();
        let destination = std::env::temp_dir().join(format!("agon-risk-{}", std::process::id()));
        std::fs::create_dir_all(&destination).unwrap();
        for name in ["risk_sc.onnx", "risk_overtake.onnx"] {
            std::fs::copy(source.join(name), destination.join(name)).unwrap();
        }
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(source.join("risk_manifest.json")).unwrap())
                .unwrap();
        manifest["models"]["safety_car"]["features"] = json!(["future_lap"]);
        std::fs::write(
            destination.join("risk_manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert!(RiskModels::load(&destination).is_err());
        manifest["models"]["safety_car"]["features"] =
            json!(["lap", null, "rain", "circuit_prior"]);
        std::fs::write(
            destination.join("risk_manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert!(RiskModels::load(&destination).is_err());
        manifest["models"]["safety_car"]["features"] = json!(SC_FEATURES);
        std::fs::write(
            destination.join("risk_manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        std::fs::write(destination.join("risk_sc.onnx"), b"corrupt").unwrap();
        assert!(RiskModels::load(&destination).is_err());
        std::fs::remove_file(destination.join("risk_manifest.json")).unwrap();
        std::fs::remove_file(destination.join("risk_sc.onnx")).unwrap();
        std::fs::remove_file(destination.join("risk_overtake.onnx")).unwrap();
        std::fs::remove_dir(destination).unwrap();
    }
    #[test]
    #[ignore = "requires ONNXRUNTIME_DYLIB_PATH; run with --include-ignored"]
    fn native_onnx_matches_python_and_rejects_invalid_features() {
        let mut models = RiskModels::load(&directory()).unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory().join("risk_manifest.json")).unwrap())
                .unwrap();
        for (name, model) in [
            ("safety_car", &mut models.sc),
            ("overtake", &mut models.overtake),
        ] {
            for case in manifest["models"][name]["parity"].as_array().unwrap() {
                let features: Vec<_> = case["features"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap() as f32)
                    .collect();
                let actual = model.probability(&features).unwrap();
                assert!((actual - case["probability"].as_f64().unwrap()).abs() < 1e-6);
            }
            assert!(model.probability(&[0.0]).is_err());
            assert!(model.probability(&vec![f32::NAN; model.features]).is_err());
            assert!(!model.priors.contains_key("Unknown circuit"));
        }
    }
    #[test]
    fn inputs_withhold_non_races_unknown_circuits_and_stale_gaps() {
        let mut s = Snapshot::empty(Session {
            name: "Race".into(),
            circuit: "Test".into(),
            total_laps: Some(12),
            ..Default::default()
        });
        s.track_status = "GREEN".into();
        s.weather.rain = Some(false);
        s.lap = 4;
        s.clock_ms = 100_000;
        assert!(race_features(&s, 0.1).is_some());
        s.session.total_laps = None;
        assert!(race_features(&s, 0.1).is_none());
        s.session.total_laps = Some(8);
        assert!(race_features(&s, 0.1).is_none());
        s.session.total_laps = Some(12);
        for status in ["SC", "VSC", "RED", "UNKNOWN"] {
            s.track_status = status.into();
            assert!(race_features(&s, 0.1).is_none());
        }
        s.track_status = "GREEN".into();
        s.session.name = "Qualifying".into();
        assert!(race_features(&s, 0.1).is_none());
        s.session.name = "Race".into();
        s.session.synthetic = true;
        assert!(race_features(&s, 0.1).is_none());
        s.session.synthetic = false;
        let mut d = DriverState {
            lap: 4,
            position: Some(2),
            position_time_ms: Some(1),
            interval: Some(0.8),
            interval_time_ms: Some(69_999),
            tyre_age: Some(4),
            ..Default::default()
        };
        d.stints.push(Stint {
            compound: "MEDIUM".into(),
            ..Default::default()
        });
        let mut target = d.clone();
        target.position = Some(1);
        target.driver.number = 1;
        d.driver.number = 2;
        s.drivers.insert(1, target);
        s.drivers.insert(2, d.clone());
        assert!(pass_features(&s, &d, 0.1).is_none());
        d.interval_time_ms = Some(70_000);
        let (target, features) = pass_features(&s, &d, 0.1).unwrap();
        assert_eq!(target, 1);
        assert_eq!(features, [4.0, 0.0, 0.1, 0.8, 0.0, 0.0]);
        d.interval_time_ms = Some(100_001);
        assert!(pass_features(&s, &d, 0.1).is_none());
        d.interval_time_ms = Some(100_000);
        d.in_pit = true;
        assert!(pass_features(&s, &d, 0.1).is_none());
        d.in_pit = false;
        d.pits.push(Pit {
            time_ms: 90_000,
            ..Default::default()
        });
        assert!(pass_features(&s, &d, 0.1).is_none());
    }
}
