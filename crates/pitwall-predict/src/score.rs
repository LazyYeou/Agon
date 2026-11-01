use pitwall_core::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForecastRecord {
    pub session: u32,
    pub driver: u32,
    pub issued_lap: u32,
    pub target_lap: u32,
    pub band: Band,
    pub actual: Option<f64>,
    pub model: String,
}
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct PredictionLog {
    pub records: Vec<ForecastRecord>,
}
#[derive(Serialize, Debug)]
pub struct AccuracyReport {
    pub pace_samples: usize,
    pub pace_mae: Option<f64>,
    pub band_coverage: Option<f64>,
    pub pit_samples: usize,
    pub pit_accuracy: Option<f64>,
    pub position_samples: usize,
    pub position_accuracy: Option<f64>,
}
impl PredictionLog {
    pub fn record(&mut self, s: &Snapshot, predictions: &[Prediction]) {
        let mut keys: BTreeSet<_> = self
            .records
            .iter()
            .map(|r| (r.session, r.driver, r.issued_lap, r.target_lap))
            .collect();
        for p in predictions {
            for (h, band) in p.pace.iter().enumerate() {
                let target = p.lap + h as u32 + 1;
                if keys.insert((s.session.key, p.driver, p.lap, target)) {
                    self.records.push(ForecastRecord {
                        session: s.session.key,
                        driver: p.driver,
                        issued_lap: p.lap,
                        target_lap: target,
                        band: band.clone(),
                        actual: None,
                        model: p.model.clone(),
                    });
                }
            }
        }
    }
    pub fn observe(&mut self, s: &Snapshot) {
        for r in &mut self.records {
            if r.session == s.session.key && r.actual.is_none() {
                r.actual = s
                    .drivers
                    .get(&r.driver)
                    .and_then(|d| d.laps.iter().find(|l| l.number == r.target_lap))
                    .and_then(|l| l.duration);
            }
        }
    }
    pub fn report(&self) -> AccuracyReport {
        let rows: Vec<_> = self
            .records
            .iter()
            .filter_map(|r| r.actual.map(|actual| (actual, &r.band)))
            .collect();
        let n = rows.len();
        AccuracyReport {
            pace_samples: n,
            pace_mae: if n == 0 {
                None
            } else {
                Some(rows.iter().map(|(a, b)| (a - b.value).abs()).sum::<f64>() / n as f64)
            },
            band_coverage: if n == 0 {
                None
            } else {
                Some(
                    rows.iter()
                        .filter(|(a, b)| *a >= b.low && *a <= b.high)
                        .count() as f64
                        / n as f64,
                )
            },
            pit_samples: 0,
            pit_accuracy: None,
            position_samples: 0,
            position_accuracy: None,
        }
    }
}
