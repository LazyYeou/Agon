use pitwall_core::*;
use rand::{rngs::StdRng, Rng, SeedableRng};

fn sample(rng: &mut StdRng, band: &Band) -> f64 {
    if band.high > band.low {
        rng.random_range(band.low..band.high)
    } else {
        band.value
    }
}

pub(crate) fn finish(s: &Snapshot, outputs: &mut [Prediction]) {
    let (Some(total), Some(loss)) = (s.session.total_laps, s.circuit.pit_loss.as_ref()) else {
        return;
    };
    if total <= s.lap
        || outputs.is_empty()
        || outputs.iter().any(|p| {
            p.pace.is_empty()
                || p.degradation.is_none()
                || s.drivers[&p.driver].gap.is_none()
                || s.drivers[&p.driver].tyre_age.is_none()
        })
    {
        return;
    }
    let mut counts = vec![vec![0u32; outputs.len()]; outputs.len()];
    let mut rng = StdRng::seed_from_u64(s.session.key as u64 * 10000 + s.lap as u64);
    for _ in 0..1000 {
        let mut times: Vec<_> = outputs
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let d = &s.drivers[&p.driver];
                let remaining = total.saturating_sub(d.lap);
                let deg = sample(&mut rng, p.degradation.as_ref().unwrap()).max(0.0);
                let age = d.tyre_age.unwrap();
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
                            * (deg - p.degradation.as_ref().unwrap().value);
                    elapsed += pace;
                    if let Some(stop) = pit {
                        if h == stop {
                            elapsed += sample(&mut rng, loss) + rng.random_range(0.0..3.0);
                        }
                        if h >= stop {
                            elapsed -= deg * (age + stop - 1) as f64;
                        }
                    }
                }
                (i, elapsed)
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
        // shortcut: one sampled future stop and no calibrated SC model; upgrade after race-level calibration.
        p.factors[2].push_str("; 1,000 simulations: pace/degradation bands, one future pit draw, pit-loss band, assumed 0–3 s warm-up; no calibrated future SC model");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
