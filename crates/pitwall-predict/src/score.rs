use pitwall_core::*;
pub use pitwall_core::{AccuracyReport, PaceForecast as ForecastRecord};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct PredictionLog {
    pub records: Vec<ForecastRecord>,
    #[serde(default)]
    strategy: Vec<StrategyRecord>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct StrategyRecord {
    session: u32,
    issued_ms: i64,
    forecast: Prediction,
    total_laps: Option<u32>,
    at_half_distance: bool,
    pit_actual: Option<u32>,
    position_actual: Option<u32>,
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
            if (p.pit_window.is_some() || !p.finish.is_empty())
                && !self.strategy.iter().any(|r| {
                    r.session == s.session.key
                        && r.forecast.driver == p.driver
                        && r.forecast.lap == p.lap
                })
            {
                self.strategy.push(StrategyRecord {
                    session: s.session.key,
                    issued_ms: s.clock_ms,
                    forecast: p.clone(),
                    total_laps: s.session.total_laps,
                    at_half_distance: s
                        .session
                        .total_laps
                        .is_some_and(|total| s.lap == total.div_ceil(2)),
                    pit_actual: None,
                    position_actual: None,
                });
            }
        }
    }
    pub fn observe(&mut self, s: &Snapshot) {
        for r in &mut self.records {
            if r.session == s.session.key && r.actual.is_none() {
                r.actual = s
                    .drivers
                    .get(&r.driver)
                    .and_then(|d| {
                        d.laps.iter().find(|l| {
                            l.number == r.target_lap
                                && l.time_ms <= s.clock_ms
                                && l.status == "GREEN"
                                && !l.pit_in
                                && !l.pit_out
                        })
                    })
                    .and_then(|l| l.duration);
            }
        }
        for r in &mut self.strategy {
            if r.session != s.session.key {
                continue;
            }
            let Some(d) = s.drivers.get(&r.forecast.driver) else {
                continue;
            };
            if r.pit_actual.is_none() {
                if let Some(pit) = d
                    .pits
                    .iter()
                    .filter(|pit| pit.lap > r.forecast.lap && pit.time_ms <= s.clock_ms)
                    .min_by_key(|pit| pit.lap)
                {
                    if pit.lap - r.forecast.lap == 10 {
                        r.pit_actual = Some(pit.lap);
                    }
                }
            }
            if let Some(total) = r.total_laps {
                if r.position_actual.is_none() && s.lap >= total && r.at_half_distance {
                    r.position_actual = d
                        .laps
                        .iter()
                        .find(|l| l.number == total && l.time_ms <= s.clock_ms)
                        .and_then(|l| l.position);
                }
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
        let pits: Vec<_> = self
            .strategy
            .iter()
            .filter_map(|r| {
                let actual = r.pit_actual?;
                let window = r.forecast.pit_window.as_ref()?;
                let predicted = window
                    .distribution
                    .iter()
                    .max_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(lap, _)| *lap)?;
                Some(predicted.abs_diff(actual) <= 2)
            })
            .collect();
        let positions: Vec<_> = self
            .strategy
            .iter()
            .filter_map(|r| {
                let actual = r.position_actual?;
                let (predicted, _) = r
                    .forecast
                    .finish
                    .iter()
                    .max_by(|a, b| a.1.total_cmp(&b.1))?;
                Some(predicted.abs_diff(actual) <= 1)
            })
            .collect();
        let accuracy = |values: &[bool]| {
            if values.is_empty() {
                None
            } else {
                Some(values.iter().filter(|v| **v).count() as f64 / values.len() as f64)
            }
        };
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
            pit_samples: pits.len(),
            pit_accuracy: accuracy(&pits),
            position_samples: positions.len(),
            position_accuracy: accuracy(&positions),
        }
    }
}
