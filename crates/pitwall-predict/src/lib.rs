//! Uncalibrated fallback forecasts; trained artifacts must explicitly replace this model version.
use pitwall_core::*;
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde::{Deserialize, Serialize};
mod score;
pub use score::*;

fn band(value: f64, width: f64) -> Band {
    Band {
        value,
        low: value - width,
        high: value + width,
    }
}
fn regression(d: &DriverState) -> Option<(f64, f64, f64)> {
    let start = d.stints.last().map_or(1, |s| s.start_lap);
    let samples: Vec<_> = d
        .laps
        .iter()
        .filter(|l| {
            l.number >= start
                && !l.pit_in
                && !l.pit_out
                && l.status == "GREEN"
                && l.interval.is_none_or(|v| v >= 1.5 || v == 0.0)
        })
        .filter_map(|l| {
            l.duration
                .map(|y| (l.number as f64, y + l.number as f64 * 0.035))
        })
        .collect();
    if samples.len() < 3 {
        return None;
    }
    let n = samples.len() as f64;
    let mx = samples.iter().map(|(x, _)| x).sum::<f64>() / n;
    let my = samples.iter().map(|(_, y)| y).sum::<f64>() / n;
    let xx = samples.iter().map(|(x, _)| (x - mx).powi(2)).sum::<f64>();
    // A weak zero-slope prior stabilizes short stints; fuel correction remains an explicit approximation.
    let slope = samples
        .iter()
        .map(|(x, y)| (x - mx) * (y - my))
        .sum::<f64>()
        / (xx + 2.0);
    let residual = (samples
        .iter()
        .map(|(x, y)| (y - (my + slope * (x - mx))).powi(2))
        .sum::<f64>()
        / n)
        .sqrt()
        .max(0.18);
    Some((
        my + slope * (d.lap as f64 + 1.0 - mx) - (d.lap as f64 + 1.0) * 0.035,
        slope,
        residual,
    ))
}

pub fn predict(s: &Snapshot) -> Vec<Prediction> {
    let paused = s.track_status == "RED";
    let low = matches!(s.track_status.as_str(), "SC" | "VSC" | "UNKNOWN")
        || s.weather.rain.unwrap_or(false);
    let mut outputs: Vec<_> = s
        .drivers
        .values()
        .map(|d| {
            let fitted = regression(d);
            let confidence = if paused {
                0.0
            } else if low {
                0.2
            } else {
                0.5
            };
            let pace = if paused {
                vec![]
            } else {
                fitted
                    .map(|(next, deg, sd)| {
                        (1..=5)
                            .map(|h| {
                                band(
                                    next + (h - 1) as f64 * (deg - 0.035),
                                    sd * (h as f64).sqrt() * if low { 3.0 } else { 1.65 },
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let deg = fitted.map(|(_, v, sd)| band(v, sd / (d.laps.len() as f64).sqrt().max(1.0)));
            let age = d.laps.last().and_then(|l| l.tyre_age).unwrap_or(0);
            let mut lengths: Vec<_> = s
                .drivers
                .values()
                .flat_map(|other| {
                    other
                        .stints
                        .iter()
                        .filter(|st| {
                            d.stints
                                .last()
                                .is_some_and(|current| current.compound == st.compound)
                        })
                        .filter_map(|st| {
                            st.end_lap.map(|end| {
                                end.saturating_sub(st.start_lap) + 1 + st.tyre_age_at_start
                            })
                        })
                })
                .collect();
            lengths.sort_unstable();
            let typical = lengths.get(lengths.len() / 2).copied();
            let pit_window = if paused {
                None
            } else {
                typical.map(|length| {
                    let remaining = length.saturating_sub(age).max(1);
                    let center = d.lap + remaining;
                    let from = center.saturating_sub(3).max(d.lap + 1);
                    let to = center + 3;
                    let distribution = (from..=to)
                        .map(|lap| (lap, 1.0 / (to - from + 1) as f64))
                        .collect();
                    PitWindow {
                        from_lap: from,
                        to_lap: to,
                        probability: 0.0,
                        distribution,
                    }
                })
            };
            let cliff = fitted
                .filter(|(_, deg, _)| *deg > 0.02)
                .map(|(_, deg, _)| band((2.0 / deg).clamp(0.0, 60.0), 10.0));
            let cliff = cliff.map(|mut b| {
                b.low = b.low.max(0.0);
                b
            });
            let tyre_health = typical.map(|length| {
                let v = (1.0 - age as f64 / length.max(1) as f64).clamp(0.0, 1.0);
                Band {
                    value: v,
                    low: (v - 0.25).max(0.0),
                    high: (v + 0.25).min(1.0),
                }
            });
            let factors = vec![
                format!("Tyre age: {age} completed laps"),
                fitted.map_or(
                    "Insufficient clean laps in this stint".into(),
                    |(_, deg, _)| {
                        format!("Fuel-corrected degradation: {deg:+.3} s/lap (approximate)")
                    },
                ),
                format!(
                    "Track {}, gap ahead {}",
                    s.track_status,
                    d.interval
                        .map_or("unavailable".into(), |v| format!("{v:.1} s"))
                ),
            ];
            let insight = if paused {
                "Predictions paused during red flag".into()
            } else if let Some((_, deg, _)) = fitted {
                format!(
                    "{}: observed stint trend {deg:+.2} s/lap. Estimates are not calibrated.",
                    d.driver.code
                )
            } else {
                "Complete at least three clean laps in this stint for a pace estimate.".into()
            };
            Prediction {
                driver: d.driver.number,
                lap: d.lap,
                model: "basic-v1".into(),
                label: "basic estimate · uncalibrated".into(),
                status: if paused {
                    "paused"
                } else if fitted.is_none() {
                    "insufficient data"
                } else if low {
                    "low confidence"
                } else {
                    "basic estimate"
                }
                .into(),
                confidence,
                pace,
                degradation: if paused { None } else { deg },
                laps_to_cliff: if paused { None } else { cliff },
                tyre_health: if paused { None } else { tyre_health },
                pit_window,
                finish: vec![],
                rejoin: None,
                factors,
                insight,
            }
        })
        .collect();
    if paused {
        return outputs;
    }
    if let Some(total) = s.session.total_laps {
        let mut counts = vec![vec![0u32; outputs.len()]; outputs.len()];
        let mut rng = StdRng::seed_from_u64(s.session.key as u64 * 10000 + s.lap as u64);
        for _ in 0..1000 {
            let mut times: Vec<_> = outputs
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    let d = &s.drivers[&p.driver];
                    let pace = p.pace.first()?;
                    let remaining = total.saturating_sub(d.lap) as f64;
                    let noise =
                        rng.random_range(-1.0..1.0) * (pace.high - pace.low) * remaining.sqrt();
                    Some((i, d.gap.unwrap_or(0.0) + remaining * pace.value + noise))
                })
                .collect();
            times.sort_by(|a, b| a.1.total_cmp(&b.1));
            for (position, (i, _)) in times.iter().enumerate() {
                counts[*i][position] += 1;
            }
        }
        for (i, p) in outputs.iter_mut().enumerate() {
            p.finish = counts[i]
                .iter()
                .enumerate()
                .filter(|(_, n)| **n > 0)
                .map(|(pos, n)| (pos as u32 + 1, *n as f64 / 1000.0))
                .collect();
        }
    }
    if let Some(loss) = &s.circuit.pit_loss {
        for p in &mut outputs {
            let gap = s.drivers[&p.driver].gap;
            if let Some(gap) = gap {
                let position = |v: f64| {
                    1 + s
                        .drivers
                        .values()
                        .filter(|d| {
                            d.gap.is_some_and(|g| g < gap + v) && d.driver.number != p.driver
                        })
                        .count() as u32
                };
                p.rejoin = Some(Band {
                    value: position(loss.value) as f64,
                    low: position(loss.low) as f64,
                    high: position(loss.high) as f64,
                });
            }
        }
    }
    outputs
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StrategyEstimate {
    pub undercut: Option<Band>,
    pub overcut: Option<Band>,
    pub assumptions: Vec<String>,
}
pub fn undercut(s: &Snapshot, a: u32, b: u32) -> StrategyEstimate {
    let ps = predict(s);
    let first = ps.iter().find(|p| p.driver == a);
    let second = ps.iter().find(|p| p.driver == b);
    let mut out=StrategyEstimate {undercut:None,overcut:None,assumptions:vec!["One-lap pit offset; equal pit losses; out-lap warm-up is not calibrated.".into(),"Positive advantage means time gained on the other driver, not guaranteed track position.".into()]};
    if let (Some(pa), Some(pb), Some(loss)) = (first, second, &s.circuit.pit_loss) {
        if let (Some(x), Some(y)) = (pa.pace.first(), pb.pace.first()) {
            let advantage = y.value - x.value;
            let width = (x.high - x.low) + (y.high - y.low) + (loss.high - loss.low);
            out.undercut = Some(band(advantage, width));
            out.overcut = Some(band(-advantage, width));
            out.assumptions.push(format!(
                "Pit loss {:.1}–{:.1} s; gap {} s",
                loss.low,
                loss.high,
                s.drivers
                    .get(&a)
                    .and_then(|d| d.gap)
                    .zip(s.drivers.get(&b).and_then(|d| d.gap))
                    .map(|(x, y)| format!("{:.1}", x - y))
                    .unwrap_or("unknown".into())
            ));
        }
    }
    out
}
