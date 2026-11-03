use anyhow::{ensure, Context, Result};
use ort::{session::Session, value::Tensor};
use pitwall_core::*;
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{Mutex, OnceLock},
};
const FEATURES: [&str; 7] = [
    "baseline",
    "tyre_age",
    "compound",
    "track_temperature",
    "interval",
    "lap",
    "stint",
];

pub struct ModelBundle {
    pace: Session,
    pit: Session,
    pub version: String,
    pub width: f64,
    pub mae: f64,
}
impl ModelBundle {
    pub fn load(directory: &Path) -> Result<Self> {
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.join("manifest.json"))?)?;
        let features = manifest["features"]
            .as_array()
            .context("Model feature schema missing")?;
        ensure!(
            features.iter().filter_map(|v| v.as_str()).eq(FEATURES),
            "Model feature schema incompatible"
        );
        ensure!(
            manifest["baseline_model"] == "bayesian-changepoint-v2",
            "Model baseline schema incompatible"
        );
        let pace_bytes = std::fs::read(directory.join("pace.onnx"))?;
        let pit_bytes = std::fs::read(directory.join("hazard.onnx"))?;
        for (name, bytes) in [("pace.onnx", &pace_bytes), ("hazard.onnx", &pit_bytes)] {
            let expected = manifest["artifact_sha256"][name]
                .as_str()
                .context("Model checksum missing")?;
            ensure!(
                format!("{:x}", Sha256::digest(bytes)) == expected,
                "Model artifact checksum mismatch: {name}"
            );
        }
        let library = std::env::var("ONNXRUNTIME_DYLIB_PATH")
            .context("Set ONNXRUNTIME_DYLIB_PATH to the native runtime library")?;
        ensure!(
            Path::new(&library).is_file(),
            "ONNX Runtime library not found"
        );
        ort::init_from(library).with_name("agon").commit()?;
        let pace = Session::builder()?
            .with_intra_threads(1)?
            .commit_from_memory(&pace_bytes)?;
        let pit = Session::builder()?
            .with_intra_threads(1)?
            .commit_from_memory(&pit_bytes)?;
        let width = manifest["pace_p10_p90_half_width"]
            .as_f64()
            .context("Calibration width missing")?;
        ensure!(width.is_finite() && width > 0.0, "Invalid calibration band");
        Ok(Self {
            pace,
            pit,
            width,
            version: manifest["model"]
                .as_str()
                .context("Model version missing")?
                .into(),
            mae: manifest["pace_mae"].as_f64().unwrap_or(f64::INFINITY),
        })
    }
    pub fn residual(&mut self, features: &[f32]) -> Result<f64> {
        ensure!(features.len() == FEATURES.len(), "Wrong feature count");
        let input =
            Tensor::from_array(([1, FEATURES.len()], features.to_vec().into_boxed_slice()))?;
        let result = self.pace.run(ort::inputs![input])?;
        let (_, values) = result[0].try_extract_tensor::<f32>()?;
        let residual = *values.first().context("Empty model result")? as f64;
        ensure!(residual.is_finite(), "Nonfinite model output");
        Ok(residual)
    }
    pub fn hazard(&mut self, features: &[f32]) -> Result<f64> {
        ensure!(features.len() == FEATURES.len(), "Wrong feature count");
        Ok(self.hazards(features)?[0])
    }
    pub fn hazards(&mut self, features: &[f32]) -> Result<Vec<f64>> {
        ensure!(
            !features.is_empty() && features.len().is_multiple_of(FEATURES.len()),
            "Wrong hazard batch feature count"
        );
        let rows = features.len() / FEATURES.len();
        let input =
            Tensor::from_array(([rows, FEATURES.len()], features.to_vec().into_boxed_slice()))?;
        let result = self.pit.run(ort::inputs![input])?;
        let (_, values) = result["probabilities"].try_extract_tensor::<f32>()?;
        ensure!(values.len() == rows * 2, "Hazard probability shape mismatch");
        let probabilities: Vec<_> = values.chunks_exact(2).map(|row| row[1] as f64).collect();
        ensure!(
            probabilities.iter().all(|p| p.is_finite() && (0.0..=1.0).contains(p)),
            "Invalid hazard probability"
        );
        Ok(probabilities)
    }
}
static MODELS: OnceLock<Result<Mutex<ModelBundle>, String>> = OnceLock::new();
pub fn apply_models(state: &Snapshot, predictions: &mut [Prediction]) {
    if state.session.synthetic {
        return;
    }
    let bundle = MODELS.get_or_init(|| {
        ModelBundle::load(Path::new(
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
    for p in predictions {
        let Some(first) = p.pace.first() else {
            continue;
        };
        let d = &state.drivers[&p.driver];
        let Some(age) = d.tyre_age else {
            continue;
        };
        let compound = match d.stints.last().map(|s| s.compound.as_str()) {
            Some("SOFT") => 0.0,
            Some("MEDIUM") => 1.0,
            Some("HARD") => 2.0,
            Some("INTERMEDIATE") => 3.0,
            Some("WET") => 4.0,
            _ => -1.0,
        };
        let features = [
            first.value as f32,
            age as f32,
            compound,
            state.weather.track.map_or(f32::NAN, |n| n as f32),
            d.interval.map_or(f32::NAN, |n| n as f32),
            d.lap as f32,
            d.stints.last().map_or(1.0, |s| s.number as f32),
        ];
        let Ok(residual) = bundle.residual(&features) else {
            continue;
        };
        let mut survival = 1.0;
        let mut distribution = vec![];
        let mut horizon_features = Vec::with_capacity(30 * FEATURES.len());
        for horizon in 0..30 {
            let mut row = features;
            row[1] = (age + horizon) as f32;
            row[5] = (d.lap + horizon) as f32;
            horizon_features.extend_from_slice(&row);
        }
        let Ok(hazards) = bundle.hazards(&horizon_features) else {
            continue;
        };
        for (index, hazard) in hazards.into_iter().enumerate() {
            let mass = survival * hazard;
            survival *= 1.0 - hazard;
            distribution.push((d.lap + index as u32 + 1, mass));
        }
        for (i, band) in p.pace.iter_mut().enumerate() {
            band.value += residual;
            let width = bundle.width
                * (i as f64 + 1.0).sqrt()
                * if p.status == "low confidence" {
                    2.0
                } else {
                    1.0
                };
            band.low = band.value - width;
            band.high = band.value + width;
        }
        if let Some((index, window)) =
            distribution
                .windows(4)
                .enumerate()
                .max_by(|(_, a), (_, b)| {
                    a.iter()
                        .map(|(_, v)| v)
                        .sum::<f64>()
                        .total_cmp(&b.iter().map(|(_, v)| v).sum::<f64>())
                })
        {
            p.pit_window = Some(PitWindow {
                from_lap: window[0].0,
                to_lap: window[3].0,
                probability: window.iter().map(|(_, v)| v).sum(),
                distribution: distribution.clone(),
            });
            let _ = index;
        }
        p.model = bundle.version.clone();
        p.label = if bundle.mae > 0.35 {
            "model estimate · backtest below target"
        } else {
            "model estimate"
        }
        .into();
        if p.status == "basic estimate" {
            p.status = "model estimate".into();
        }
        p.factors[2] = format!(
            "Historic race-trained residual/hazard model; holdout MAE {:.2} s",
            bundle.mae
        );
    }
}
