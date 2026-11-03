use pitwall_core::*;
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::collections::{BTreeMap, BTreeSet};

fn sample(rng: &mut StdRng, band: &Band) -> f64 {
    if band.high > band.low {
        rng.random_range(band.low..band.high)
    } else {
        band.value
    }
}

fn observed_stint_ages(s: &Snapshot, d: &DriverState) -> Vec<u32> {
    let compound = d
        .stints
        .iter()
        .rev()
        .find(|st| st.start_lap <= d.lap.saturating_add(1))
        .map(|st| st.compound.as_str());
    s.drivers
        .values()
        .flat_map(|other| {
            other.stints.iter().filter_map(|st| {
                let end = st.end_lap?;
                (Some(st.compound.as_str()) == compound
                    && end >= st.start_lap
                    && end <= other.lap
                    && end <= s.lap)
                    .then(|| {
                        end.saturating_sub(st.start_lap)
                            .saturating_add(1)
                            .saturating_add(st.tyre_age_at_start)
                            .clamp(2, 1000)
                    })
            })
        })
        .collect()
}

// Only completed, already observed episodes supply duration and slowdown evidence.
fn neutralisations(s: &Snapshot) -> Vec<(u32, f64)> {
    let mut laps: BTreeMap<u32, Vec<f64>> = BTreeMap::new();
    let mut green = BTreeSet::new();
    for d in s.drivers.values() {
        let observed: Vec<_> = d
            .laps
            .iter()
            .filter(|l| {
                l.number <= d.lap
                    && l.number <= s.lap
                    && l.time_ms <= s.clock_ms
                    && !l.pit_in
                    && !l.pit_out
                    && l.duration.is_some_and(|v| v.is_finite() && v > 0.0)
            })
            .collect();
        let clean: Vec<_> = observed
            .iter()
            .filter(|l| l.status == "GREEN")
            .map(|l| l.duration.unwrap())
            .collect();
        if clean.is_empty() {
            continue;
        }
        let baseline = clean.iter().sum::<f64>() / clean.len() as f64;
        for l in observed {
            if l.status == "GREEN" {
                green.insert(l.number);
            }
            if matches!(l.status.as_str(), "SC" | "VSC") {
                let ratio = l.duration.unwrap() / baseline;
                if ratio.is_finite() && ratio >= 1.0 {
                    laps.entry(l.number).or_default().push(ratio);
                }
            }
        }
    }
    let mut episodes = vec![];
    let (mut duration, mut sum, mut previous) = (0, 0.0, 0u32);
    for (lap, ratios) in &laps {
        if *lap != previous.saturating_add(1) {
            duration = 0;
            sum = 0.0;
        }
        duration += 1;
        sum += ratios.iter().sum::<f64>() / ratios.len() as f64;
        previous = *lap;
        if !laps.contains_key(&lap.saturating_add(1)) && green.contains(&lap.saturating_add(1)) {
            episodes.push((duration, sum / duration as f64));
        }
    }
    episodes
}

pub(crate) fn has_current_field(s: &Snapshot) -> bool {
    !s.drivers.is_empty()
        && s.drivers.values().all(|d| {
            let Some(lap) = d
                .laps
                .iter()
                .rev()
                .find(|l| l.number <= d.lap && l.time_ms <= s.clock_ms)
            else {
                return false;
            };
            let Some(duration) = lap.duration.filter(|t| t.is_finite() && *t > 0.0) else {
                return false;
            };
            let gap_time = d.interval_time_ms.or_else(|| {
                (d.position == Some(1))
                    .then_some(d.position_time_ms)
                    .flatten()
            });
            let fresh = |time| {
                s.clock_ms
                    .checked_sub(time)
                    .is_some_and(|age| age >= 0 && age as f64 <= duration * 2000.0)
            };
            d.gap.is_some_and(|g| g.is_finite())
                && fresh(lap.time_ms)
                && gap_time.is_some_and(fresh)
        })
}

pub(crate) fn finish(s: &Snapshot, outputs: &mut [Prediction]) {
    for p in outputs.iter_mut() {
        p.finish.clear();
    }
    let (Some(total), Some(loss)) = (s.session.total_laps, s.circuit.pit_loss.as_ref()) else {
        return;
    };
    if !has_current_field(s) {
        for p in outputs.iter_mut() {
            p.factors.push("Finishing order unavailable: a participant may be inactive or delayed; complete-field lap/gap observations must be within two observed lap durations. No retirement is inferred.".into());
        }
        return;
    }
    let name = s.session.name.trim().to_lowercase();
    if total <= s.lap
        || total > 1000
        || loss.validate().is_err()
        || loss.low < 0.0
        || s.track_status == "RED"
        || !(matches!(name.as_str(), "sprint" | "sprint race")
            || name.split_whitespace().next() == Some("race")
            || (s.session.synthetic && name.is_empty()))
        || outputs.is_empty()
        || outputs.iter().any(|p| {
            p.pace.is_empty()
                || p.pace.iter().any(|b| b.validate().is_err() || b.low <= 0.0)
                || p.degradation.as_ref().is_none_or(|b| b.validate().is_err())
                || s.drivers.get(&p.driver).is_none_or(|d| {
                    d.gap.is_none_or(|gap| !gap.is_finite())
                        || d.tyre_age.is_none()
                        || d.lap > s.lap
                        || d.tyre_age.is_some_and(|age| age > 1000)
                })
                || p.pit_window.as_ref().is_some_and(|w| {
                    w.distribution
                        .iter()
                        .any(|(_, probability)| !probability.is_finite() || *probability < 0.0)
                        || w.distribution
                            .iter()
                            .map(|(_, probability)| probability)
                            .sum::<f64>()
                            > 1.000001
                })
        })
    {
        return;
    }
    let stint_ages: Vec<_> = outputs
        .iter()
        .map(|p| observed_stint_ages(s, &s.drivers[&p.driver]))
        .collect();
    let episodes = neutralisations(s);
    let risk = outputs
        .iter()
        .filter_map(|p| p.safety_car.as_ref())
        .find(|r| r.validate().is_ok())
        .map(|r| (r.probability, r.horizon_laps));
    let mut counts = vec![vec![0u32; outputs.len()]; outputs.len()];
    let mut rng = StdRng::seed_from_u64(s.session.key as u64 * 10000 + s.lap as u64);
    for _ in 0..1000 {
        // shortcut: onset is uniform within the calibrated horizon; upgrade with an onset model.
        let scenario = risk
            .filter(|(probability, _)| !episodes.is_empty() && rng.random::<f64>() < *probability)
            .map(|(_, horizon)| {
                let start = rng.random_range(1..=horizon);
                let (duration, ratio) = episodes[rng.random_range(0..episodes.len())];
                (start, start.saturating_add(duration), ratio)
            });
        let mut times: Vec<_> = outputs
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let d = &s.drivers[&p.driver];
                let remaining = total.saturating_sub(d.lap);
                let deg = sample(&mut rng, p.degradation.as_ref().unwrap()).max(0.0);
                let age = d.tyre_age.unwrap();
                let mut tyre_age = age;
                let mut next_age = if stint_ages[i].is_empty() {
                    // Economic replacement interval for linear degradation, conditional on this run's fit.
                    if deg > 0.0 {
                        (2.0 * loss.value / deg).sqrt().ceil().clamp(2.0, 1000.0) as u32
                    } else {
                        1000
                    }
                } else {
                    stint_ages[i][rng.random_range(0..stint_ages[i].len())]
                };
                let draw = rng.random::<f64>();
                let mut mass = 0.0;
                let pit = p.pit_window.as_ref().and_then(|window| {
                    window.distribution.iter().find_map(|(lap, probability)| {
                        if *lap <= d.lap || *lap > total {
                            return None;
                        }
                        mass += probability;
                        if draw < mass {
                            Some(*lap - d.lap)
                        } else {
                            None
                        }
                    })
                });
                let hazard_end = p
                    .pit_window
                    .as_ref()
                    .and_then(|w| w.distribution.iter().map(|(lap, _)| *lap).max())
                    .unwrap_or(d.lap);
                let mut stopped = false;
                let pace_quantile = rng.random::<f64>();
                let mut elapsed = d.gap.unwrap();
                for h in 1..=remaining {
                    let forecast = p
                        .pace
                        .get((h - 1) as usize)
                        .unwrap_or_else(|| p.pace.last().unwrap());
                    let horizon = p.pace.len() as u32;
                    let pace = forecast.low
                        + pace_quantile * (forecast.high - forecast.low)
                        + h.saturating_sub(horizon) as f64 * (deg - 0.035)
                        + (h - 1).min(horizon - 1) as f64
                            * (deg - p.degradation.as_ref().unwrap().value)
                        - deg * (age + h - 1 - tyre_age) as f64;
                    let ratio = scenario
                        .filter(|(start, end, _)| h >= *start && h < *end)
                        .map_or(1.0, |(_, _, ratio)| ratio);
                    elapsed += pace.max(1.0) * ratio;
                    tyre_age += 1;
                    let policy_due = (stopped || (pit.is_none() && d.lap + h > hazard_end))
                        && tyre_age >= next_age;
                    let worthwhile =
                        deg * tyre_age as f64 * (remaining - h) as f64 > loss.value + 1.5;
                    if pit == Some(h) || (h < remaining && policy_due && worthwhile) {
                        elapsed += sample(&mut rng, loss) + rng.random_range(0.0..3.0);
                        tyre_age = 0;
                        stopped = true;
                        if !stint_ages[i].is_empty() {
                            next_age = stint_ages[i][rng.random_range(0..stint_ages[i].len())];
                        }
                    }
                }
                (i, elapsed)
            })
            .collect();
        if times.iter().any(|(_, elapsed)| !elapsed.is_finite()) {
            return;
        }
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
        let sc = if risk.is_none() {
            "future SC/VSC omitted: calibrated deployment forecast unavailable"
        } else if episodes.is_empty() {
            "future SC/VSC effects omitted: deployment probability alone does not calibrate duration/slowdown"
        } else {
            "one shared SC/VSC deployment within forecast horizon; uniformly assumed onset, observed completed episode duration/slowdown; no later deployments, bunching or pit-loss discount"
        };
        p.factors[2].push_str(&format!("; 1,000 simulations: first-stop hazard then sequential observed same-compound stint ages (economic replacement interval if unavailable); tyre age resets; fresh same-compound pace assumes the fitted degradation, inventory unknown; pit-loss band, assumed 0–3 s warm-up; lapped cars extrapolated to scheduled distance; policy uncalibrated; {sc}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safety_car_uses_completed_observations_and_one_shared_scenario() {
        let mut s = Snapshot::empty(Session {
            total_laps: Some(6),
            synthetic: true,
            ..Default::default()
        });
        s.lap = 3;
        s.track_status = "GREEN".into();
        s.circuit.pit_loss = Some(crate::band(20.0, 0.0));
        for number in 1..=2 {
            s.drivers.insert(
                number,
                DriverState {
                    driver: Driver {
                        number,
                        ..Default::default()
                    },
                    lap: 3,
                    tyre_age: Some(3),
                    interval_time_ms: Some(0),
                    gap: Some(if number == 1 { 0.0 } else { 35.0 }),
                    laps: (1..=3)
                        .map(|number| Lap {
                            number,
                            duration: Some(if number == 1 { 180.0 } else { 90.0 }),
                            status: if number == 1 { "SC" } else { "GREEN" }.into(),
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
                },
            );
        }
        let mut predictions = crate::predict(&s);
        for p in &mut predictions {
            p.pace = vec![crate::band(if p.driver == 1 { 100.0 } else { 90.0 }, 0.0); 5];
            p.degradation = Some(crate::band(0.0, 0.0));
            p.pit_window = None;
            p.safety_car = Some(RiskForecast {
                probability: 0.5,
                horizon_laps: 5,
                model: "test-calibrated".into(),
                label: "test".into(),
                factors: vec![],
            });
        }
        let original = predictions.clone();
        finish(&s, &mut predictions);
        let win = predictions[0]
            .finish
            .iter()
            .find(|(pos, _)| *pos == 1)
            .unwrap()
            .1;
        assert!(
            (0.6..0.8).contains(&win),
            "deployment uncertainty must reach finishing order: {win}"
        );
        // Identical pace must receive identical neutralisation, never independent driver events.
        s.drivers.get_mut(&2).unwrap().gap = Some(2.0);
        predictions[1].pace = predictions[0].pace.clone();
        finish(&s, &mut predictions);
        assert_eq!(predictions[0].finish, vec![(1, 1.0)]);
        // An episode without its observed ending cannot provide a duration.
        for d in s.drivers.values_mut() {
            d.laps.retain(|l| l.number == 1);
        }
        assert!(neutralisations(&s).is_empty());
        s.drivers.get_mut(&2).unwrap().gap = Some(35.0);
        let mut unavailable = original;
        finish(&s, &mut unavailable);
        assert_eq!(unavailable[0].finish, vec![(1, 1.0)]);
    }

    #[test]
    fn repeated_stops_reset_tyre_age_and_finish_is_deterministic() {
        let mut s = Snapshot::empty(Session {
            total_laps: Some(11),
            synthetic: true,
            ..Default::default()
        });
        s.lap = 3;
        s.track_status = "GREEN".into();
        s.circuit.pit_loss = Some(crate::band(0.0, 0.0));
        for number in 1..=2 {
            s.drivers.insert(
                number,
                DriverState {
                    driver: Driver {
                        number,
                        ..Default::default()
                    },
                    lap: 3,
                    tyre_age: Some(2),
                    interval_time_ms: Some(0),
                    gap: Some(if number == 1 { 0.0 } else { 2.0 }),
                    laps: vec![Lap {
                        number: 3,
                        duration: Some(100.0),
                        ..Default::default()
                    }],
                    stints: vec![
                        Stint {
                            compound: "MEDIUM".into(),
                            start_lap: 1,
                            end_lap: Some(2),
                            ..Default::default()
                        },
                        Stint {
                            compound: "MEDIUM".into(),
                            start_lap: 3,
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                },
            );
        }
        let mut predictions = crate::predict(&s);
        for p in &mut predictions {
            p.pace = vec![crate::band(100.0, 0.0)];
            p.degradation = Some(crate::band(if p.driver == 1 { 1.0 } else { 0.0 }, 0.0));
            p.pit_window = if p.driver == 1 {
                Some(PitWindow {
                    from_lap: 5,
                    to_lap: 5,
                    probability: 1.0,
                    distribution: vec![(5, 1.0)],
                })
            } else {
                None
            };
        }
        let mut repeated = predictions.clone();
        finish(&s, &mut predictions);
        finish(&s, &mut repeated);
        // Three stops save 36 s of tyre ageing; one stop saves only 24 s.
        assert_eq!(predictions[0].finish, vec![(1, 1.0)]);
        for (a, b) in predictions.iter().zip(&repeated) {
            assert_eq!(a.finish, b.finish);
            assert!((a.finish.iter().map(|(_, p)| p).sum::<f64>() - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn finish_propagates_later_pace_horizon_uncertainty() {
        let mut s = Snapshot::empty(Session {
            total_laps: Some(6),
            synthetic: true,
            ..Default::default()
        });
        s.lap = 3;
        s.track_status = "GREEN".into();
        s.circuit.pit_loss = Some(Band {
            value: 20.0,
            low: 20.0,
            high: 20.0,
        });
        for number in 1..=2 {
            s.drivers.insert(
                number,
                DriverState {
                    driver: Driver {
                        number,
                        ..Default::default()
                    },
                    lap: 3,
                    tyre_age: Some(3),
                    interval_time_ms: Some(0),
                    gap: Some((number - 1) as f64 * 10.0),
                    laps: (1..=3)
                        .map(|n| Lap {
                            number: n,
                            duration: Some(90.0),
                            status: "GREEN".into(),
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
                },
            );
        }
        let mut predictions = crate::predict(&s);
        assert_eq!(predictions[0].finish, vec![(1, 1.0)]);
        for pace in &mut predictions[0].pace[1..] {
            pace.low = 60.0;
            pace.high = 120.0;
        }
        finish(&s, &mut predictions);
        let win = predictions[0]
            .finish
            .iter()
            .find(|(position, _)| *position == 1)
            .unwrap()
            .1;
        assert!(
            (0.2..0.8).contains(&win),
            "later-horizon uncertainty should broaden final positions: {win}"
        );
    }
}
