use pitwall_core::*;

pub(crate) fn clean_laps<'a>(s: &Snapshot, d: &'a DriverState) -> Vec<&'a Lap> {
    let start = d
        .stints
        .iter()
        .rev()
        .find(|st| st.start_lap <= d.lap.saturating_add(1))
        .map_or(1, |st| st.start_lap);
    let mut laps: Vec<_> = d
        .laps
        .iter()
        .filter(|l| {
            l.number >= start
                && l.number <= d.lap
                && l.time_ms <= s.clock_ms
                && !l.pit_in
                && !l.pit_out
                && l.status == "GREEN"
                && l.interval.is_none_or(|v| v >= 1.5 || v == 0.0)
        })
        .collect();
    laps.sort_by_key(|l| l.number);
    laps
}

// Normal/inverse-gamma posterior: weak zero-slope prior, unknown intercept and noise.
struct Posterior {
    mx: f64,
    my: f64,
    slope: f64,
    variance: f64,
    xx: f64,
    n: f64,
    sse: f64,
}
impl Posterior {
    fn fit(samples: &[(f64, f64)]) -> Self {
        let n = samples.len() as f64;
        let mx = samples.iter().map(|(x, _)| x).sum::<f64>() / n;
        let my = samples.iter().map(|(_, y)| y).sum::<f64>() / n;
        let xx = samples.iter().map(|(x, _)| (x - mx).powi(2)).sum::<f64>();
        let slope = samples
            .iter()
            .map(|(x, y)| (x - mx) * (y - my))
            .sum::<f64>()
            / (xx + 2.0);
        let sse = samples
            .iter()
            .map(|(x, y)| (y - my - slope * (x - mx)).powi(2))
            .sum::<f64>();
        let variance = (0.0324 + 0.5 * (sse + 2.0 * slope.powi(2))) / (1.0 + (n - 1.0) / 2.0);
        Self {
            mx,
            my,
            slope,
            variance,
            xx,
            n,
            sse,
        }
    }
    fn mean(&self, x: f64) -> f64 {
        self.my + self.slope * (x - self.mx)
    }
    fn predictive_sd(&self, x: f64) -> f64 {
        (self.variance * (1.0 + 1.0 / self.n + (x - self.mx).powi(2) / (self.xx + 2.0)))
            .sqrt()
            .max(0.12)
    }
    fn slope_band(&self) -> Band {
        let width = 1.65 * (self.variance / (self.xx + 2.0)).sqrt();
        Band {
            value: self.slope,
            low: self.slope - width,
            high: self.slope + width,
        }
    }
}

pub(crate) struct StintFit {
    pub next: f64,
    pub degradation: Band,
    pub sd: f64,
    pub cliff_lap: Option<u32>,
}

pub(crate) fn regression(s: &Snapshot, d: &DriverState) -> Option<StintFit> {
    let samples: Vec<_> = clean_laps(s, d)
        .into_iter()
        .filter_map(|l| {
            l.duration
                .filter(|v| v.is_finite() && *v > 0.0)
                .map(|y| (l.number as f64, y + l.number as f64 * 0.035))
        })
        .collect();
    if samples.len() < 3 {
        return None;
    }
    let mut posterior = Posterior::fit(&samples);
    let mut cliff_lap = None;
    let mut best_cost = f64::INFINITY;
    for split in 3..=samples.len().saturating_sub(3) {
        let before = Posterior::fit(&samples[..split]);
        let after = Posterior::fit(&samples[split..]);
        let cost = before.sse + after.sse;
        let evidence = samples.len() as f64 * ((posterior.sse + 0.0324) / (cost + 0.0324)).ln();
        let sustained_drop = samples[samples.len() - 3..]
            .iter()
            .all(|(x, y)| y - before.mean(*x) > (3.0 * before.predictive_sd(*x)).max(0.6));
        if cost < best_cost && sustained_drop && evidence > 3.0 * (samples.len() as f64).ln() + 6.0
        {
            best_cost = cost;
            cliff_lap = Some(samples[split].0 as u32);
        }
    }
    if let Some(lap) = cliff_lap {
        let split = samples.iter().position(|(x, _)| *x == lap as f64)?;
        posterior = Posterior::fit(&samples[split..]);
    }
    let next = d.lap as f64 + 1.0;
    Some(StintFit {
        next: posterior.mean(next) - next * 0.035,
        degradation: posterior.slope_band(),
        sd: posterior.predictive_sd(next),
        cliff_lap,
    })
}

pub(crate) fn anomaly(s: &Snapshot, d: &DriverState) -> Option<String> {
    if s.track_status != "GREEN" || s.weather.rain.unwrap_or(false) {
        return None;
    }
    let laps = clean_laps(s, d);
    if laps.len() < 8 {
        return None;
    }
    let laps = &laps[laps.len().saturating_sub(15)..];
    let split = laps.len() - 3;
    let mut alerts = vec![];
    let mut check =
        |name: String, values: Vec<Option<f64>>, floor: f64, slowing: f64, units: &str| {
            let baseline: Vec<_> = values[..split]
                .iter()
                .flatten()
                .copied()
                .filter(|v| v.is_finite())
                .collect();
            let recent: Vec<_> = values[split..]
                .iter()
                .flatten()
                .copied()
                .filter(|v| v.is_finite())
                .collect();
            if baseline.len() < 5 || recent.len() != 3 {
                return;
            }
            let mean = baseline.iter().sum::<f64>() / baseline.len() as f64;
            let sd = (baseline.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
                / (baseline.len() - 1) as f64)
                .sqrt()
                .max(floor);
            let delta = recent.iter().sum::<f64>() / 3.0 - mean;
            let z = delta * slowing / sd;
            if z >= 3.0 && recent.iter().all(|v| (v - mean) * slowing >= 2.0 * sd) {
                alerts.push(format!(
                "{name} {delta:+.2} {units} across the last 3 clean laps (rolling z-score {z:.1})"
            ));
            }
        };
    for sector in 0..3 {
        check(
            format!("sector {}", sector + 1),
            laps.iter()
                .map(|l| l.sectors.get(sector).copied().flatten())
                .collect(),
            0.05,
            1.0,
            "s",
        );
    }
    check(
        "speed trap".into(),
        laps.iter().map(|l| l.speed_trap).collect(),
        1.0,
        -1.0,
        "km/h",
    );
    if alerts.is_empty() {
        None
    } else {
        Some(format!(
            "Statistical anomaly: {}. Cause unconfirmed; estimate is uncalibrated.",
            alerts.join("; ")
        ))
    }
}

pub(crate) fn qualifying(s: &Snapshot, outputs: &mut [Prediction], phase: u8) {
    // Q2 times cannot be separated from Q1 without the phase boundary.
    if phase == 2 && s.qualifying_start_ms.is_none() {
        return;
    }
    let mut drivers = vec![];
    let mut xx = 0.0;
    let mut xy = 0.0;
    let mut residual_samples = vec![];
    let mut durations = vec![];
    for d in s.drivers.values() {
        let laps: Vec<_> = d
            .laps
            .iter()
            .filter(|l| {
                l.number <= d.lap
                    && l.time_ms <= s.clock_ms
                    && !l.pit_in
                    && !l.pit_out
                    && l.status == "GREEN"
                    && s.qualifying_start_ms.is_none_or(|start| l.time_ms >= start)
            })
            .filter_map(|l| {
                l.duration
                    .filter(|v| v.is_finite() && *v > 0.0)
                    .map(|v| (l.time_ms, v))
            })
            .collect();
        if laps.len() >= 2 {
            let n = laps.len() as f64;
            let mx = laps
                .iter()
                .map(|(t, _)| (*t - s.clock_ms) as f64 / 60_000.0)
                .sum::<f64>()
                / n;
            let my = laps.iter().map(|(_, y)| y).sum::<f64>() / n;
            for (t, y) in &laps {
                let x = (*t - s.clock_ms) as f64 / 60_000.0 - mx;
                xx += x * x;
                xy += x * (y - my);
                residual_samples.push((x, y - my));
            }
        }
        if let Some((t, best)) = laps.iter().min_by(|a, b| a.1.total_cmp(&b.1)).copied() {
            durations.extend(laps.iter().map(|(_, v)| *v));
            drivers.push((d.driver.number, t, best));
        }
    }
    if residual_samples.len() < 6 || xx <= 0.0 {
        return;
    }
    let slope = xy / xx;
    let variance = (residual_samples
        .iter()
        .map(|(x, y)| (y - slope * x).powi(2))
        .sum::<f64>()
        / (residual_samples.len() - 2) as f64)
        .max(0.01);
    durations.sort_by(f64::total_cmp);
    let horizon_minutes = durations[durations.len() / 2] / 60.0;
    let mut projected: Vec<_> = drivers
        .iter()
        .map(|(n, t, best)| {
            let ahead = (s.clock_ms - t) as f64 / 60_000.0 + horizon_minutes;
            (*n, best + (slope * ahead).min(0.0), ahead)
        })
        .collect();
    projected.sort_by(|a, b| a.1.total_cmp(&b.1));
    let retained = if phase == 1 {
        (s.drivers.len() + 10) / 2
    } else {
        10
    };
    let Some((_, cutoff, ahead)) = projected.get(retained.saturating_sub(1)) else {
        return;
    };
    let width = 1.65 * (variance * (1.0 + ahead.powi(2) / xx)).sqrt();
    for p in outputs {
        if let Some((_, _, best)) = drivers.iter().find(|(n, _, _)| *n == p.driver) {
            let need = best - cutoff;
            p.qualifying_need = Some(Band {
                value: need.max(0.0),
                low: (need - width).max(0.0),
                high: (need + width).max(0.0),
            });
            p.factors[2] = format!("Within-driver track evolution regression {slope:+.3} s/min; next flying lap in approximately {:.0} s, uncalibrated", horizon_minutes * 60.0);
        }
    }
}
