//! Uncalibrated fallback forecasts; trained artifacts must explicitly replace this model version.
use pitwall_core::*;
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde::{Deserialize, Serialize};
mod analysis;
mod model;
mod score;
mod simulation;
pub use model::ModelBundle;
pub use score::*;

fn band(value: f64, width: f64) -> Band {
    Band {
        value,
        low: value - width,
        high: value + width,
    }
}

pub fn predict(s: &Snapshot) -> Vec<Prediction> {
    let paused = s.track_status == "RED";
    let low = matches!(s.track_status.as_str(), "SC" | "VSC" | "UNKNOWN")
        || s.weather.rain.unwrap_or(false);
    let mut outputs: Vec<_> = s
        .drivers
        .values()
        .map(|d| {
            let fitted = analysis::regression(s, d);
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
                    .as_ref().map(|fit| {
                        (1..=5)
                            .map(|h| {
                                band(
                                    fit.next + (h - 1) as f64 * (fit.degradation.value - 0.035),
                                    fit.sd * (h as f64).sqrt() * if low { 3.0 } else { 1.65 },
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let deg = fitted.as_ref().map(|fit| fit.degradation.clone());
            let age=d.tyre_age;
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
                            st.end_lap.filter(|end| *end <= other.lap).map(|end| {
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
                typical.zip(age).map(|(length,age)| {
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
            let cliff = fitted.as_ref().and_then(|fit| fit.cliff_lap.map(|_| Band {
                value: 0.0, low: 0.0, high: 3.0,
            }));
            let tyre_health = typical.zip(age).map(|(length,age)| {
                let v = (1.0 - age as f64 / length.max(1) as f64).clamp(0.0, 1.0);
                Band {
                    value: v,
                    low: (v - 0.25).max(0.0),
                    high: (v + 0.25).min(1.0),
                }
            });
            let factors = vec![
                age.map_or_else(||"Tyre age: unavailable".into(),|age|format!("Tyre age: {age} completed laps")),
                fitted.as_ref().map_or(
                    "Insufficient clean laps in this stint".into(),
                    |fit| {
                        format!("Bayesian fuel-corrected degradation: {:+.3} s/lap (approximate)", fit.degradation.value)
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
            } else if let Some(fit) = &fitted {
                if let Some(lap) = fit.cliff_lap {
                    format!("{}: observed stint changepoint near lap {lap}; sustained pace loss may indicate a tyre cliff. Uncalibrated estimate.", d.driver.code)
                } else {
                    format!("{}: Bayesian stint trend {:+.2} s/lap. No observed cliff changepoint; estimates are not calibrated.", d.driver.code, fit.degradation.value)
                }
            } else {
                "Complete at least three clean laps in this stint for a pace estimate.".into()
            };
            Prediction {
                driver: d.driver.number,
                lap: d.lap,
                model: "basic-bayesian-v2".into(),
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
                qualifying_need: None,
                factors,
                insight,
            }
        })
        .collect();
    if paused {
        return outputs;
    }
    model::apply_models(s, &mut outputs);
    for p in &mut outputs {
        if let Some(alert) = analysis::anomaly(s, &s.drivers[&p.driver]) {
            p.factors[2] = alert.clone();
            p.insight = format!("{} {alert}", p.insight);
        }
    }
    if s.session.name.to_lowercase().contains("qualifying") {
        let phase = s.qualifying_phase.or_else(|| {
            if s.session.name.contains("Q1") {
                Some(1)
            } else if s.session.name.contains("Q2") {
                Some(2)
            } else {
                None
            }
        });
        if let Some(phase @ 1..=2) = phase {
            analysis::qualifying(s, &mut outputs, phase);
        }
        return outputs;
    }
    simulation::finish(s, &mut outputs);
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
    pub undercut_success: Option<f64>,
    pub overcut_success: Option<f64>,
    pub assumptions: Vec<String>,
}
pub fn undercut(s: &Snapshot, a: u32, b: u32) -> StrategyEstimate {
    let ps = predict(s);
    let first = ps.iter().find(|p| p.driver == a);
    let second = ps.iter().find(|p| p.driver == b);
    let mut out=StrategyEstimate {undercut:None,overcut:None,undercut_success:None,overcut_success:None,assumptions:vec!["Two-lap simulation with one-lap pit offset, pace/degradation bands and independent pit-loss uncertainty.".into(),"Out-lap warm-up penalty assumed 0–3 seconds; fresh-tyre pace extrapolates the stint fit. Basic, uncalibrated estimate.".into(),"Positive advantage is time gained after both drivers pit. Success means the simulated relative gap crosses zero.".into()]};
    if let (Some(pa), Some(pb), Some(loss)) = (first, second, &s.circuit.pit_loss) {
        if let (Some(x), Some(y)) = (pa.pace.first(), pb.pace.first()) {
            let (Some(da), Some(db)) = (s.drivers.get(&a), s.drivers.get(&b)) else {
                return out;
            };
            let (Some(ga), Some(gb), Some(dega), Some(degb)) =
                (da.gap, db.gap, &pa.degradation, &pb.degradation)
            else {
                return out;
            };
            let gap = ga - gb;
            let (Some(agea), Some(ageb)) = (da.tyre_age, db.tyre_age) else {
                return out;
            };
            let agea = agea as f64;
            let ageb = ageb as f64;
            let mut rng = StdRng::seed_from_u64(a as u64 * 10000 + b as u64 + s.lap as u64);
            let mut under = vec![];
            let mut over = vec![];
            let mut under_success = 0;
            let mut over_success = 0;
            let sample = |rng: &mut StdRng, b: &Band| {
                if b.high > b.low {
                    rng.random_range(b.low..b.high)
                } else {
                    b.value
                }
            };
            for _ in 0..1000 {
                let olda = sample(&mut rng, x);
                let oldb = sample(&mut rng, y);
                let fresha = olda - sample(&mut rng, dega).max(0.0) * agea;
                let freshb = oldb - sample(&mut rng, degb).max(0.0) * ageb;
                let outa = fresha + rng.random_range(0.0..3.0);
                let outb = freshb + rng.random_range(0.0..3.0);
                let lossa = sample(&mut rng, loss);
                let lossb = sample(&mut rng, loss);
                let after_under = gap + lossa + outa + fresha - oldb - lossb - outb;
                let after_over = gap + olda + lossa + outa - lossb - outb - freshb;
                under.push(gap - after_under);
                over.push(gap - after_over);
                if after_under < 0.0 {
                    under_success += 1;
                }
                if after_over < 0.0 {
                    over_success += 1;
                }
            }
            let quantile = |values: &mut Vec<f64>| {
                values.sort_by(f64::total_cmp);
                Band {
                    value: values[500],
                    low: values[100],
                    high: values[900],
                }
            };
            out.undercut = Some(quantile(&mut under));
            out.overcut = Some(quantile(&mut over));
            out.undercut_success = Some(under_success as f64 / 1000.0);
            out.overcut_success = Some(over_success as f64 / 1000.0);
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
