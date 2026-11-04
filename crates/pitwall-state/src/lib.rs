//! Causal session fold: replay only applies events whose observation time has arrived.
use pitwall_core::*;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};
mod demo;
pub use demo::demo_archive;
fn refresh_stint(driver: &mut DriverState) {
    driver
        .stints
        .sort_by_key(|stint| (stint.start_lap, stint.number));
    for i in 0..driver.stints.len().saturating_sub(1) {
        driver.stints[i].end_lap = Some(driver.stints[i + 1].start_lap.saturating_sub(1));
    }
    driver.tyre_age = driver.stints.last().map(|stint| {
        stint.tyre_age_at_start
            + if driver.lap >= stint.start_lap {
                driver.lap - stint.start_lap + 1
            } else {
                0
            }
    });
    for lap in &mut driver.laps {
        if let Some(stint) = driver
            .stints
            .iter()
            .rev()
            .find(|stint| stint.start_lap <= lap.number)
        {
            lap.compound = stint.compound.clone();
            lap.tyre_age = Some(stint.tyre_age_at_start + lap.number - stint.start_lap + 1);
        }
    }
}

pub struct RaceState {
    snapshot: Snapshot,
    seen: BTreeMap<(u32, &'static str), i64>,
    lap_receipts: BTreeMap<(u32, u32), i64>,
    samples: BTreeMap<u32, VecDeque<TelemetrySample>>,
}
impl RaceState {
    pub fn restore(snapshot: Snapshot) -> Self {
        let mut state = Self::new(
            snapshot.session.clone(),
            snapshot
                .drivers
                .values()
                .map(|d| d.driver.clone())
                .collect(),
            snapshot.circuit.clone(),
        );
        // A snapshot has no per-field receipt history; its clock is a safe recovery floor.
        for (&number, driver) in &snapshot.drivers {
            for kind in [
                "sectors",
                "stint",
                "pit",
                "telemetry",
                "location",
                "position",
                "interval",
            ] {
                state.seen.insert((number, kind), snapshot.clock_ms);
            }
            for lap in &driver.laps {
                state
                    .lap_receipts
                    .insert((number, lap.number), snapshot.clock_ms);
            }
            if let Some(sample) = &driver.telemetry {
                state
                    .samples
                    .entry(number)
                    .or_default()
                    .push_back(sample.clone());
            }
        }
        for kind in ["weather", "status"] {
            state.seen.insert((0, kind), snapshot.clock_ms);
        }
        state.snapshot = snapshot;
        state
    }
    pub fn new(session: Session, drivers: Vec<Driver>, circuit: Circuit) -> Self {
        let mut snapshot = Snapshot::empty(session);
        snapshot.drivers = drivers
            .into_iter()
            .filter(|d| d.validate().is_ok())
            .map(|driver| {
                (
                    driver.number,
                    DriverState {
                        driver,
                        ..Default::default()
                    },
                )
            })
            .collect();
        snapshot.circuit = circuit;
        snapshot.data_status = "replay".into();
        Self {
            snapshot,
            seen: BTreeMap::new(),
            lap_receipts: BTreeMap::new(),
            samples: BTreeMap::new(),
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.clone()
    }
    pub fn view(&self) -> &Snapshot {
        &self.snapshot
    }
    pub fn view_mut(&mut self) -> &mut Snapshot {
        &mut self.snapshot
    }
    pub fn telemetry(&self, driver: u32) -> Vec<TelemetrySample> {
        self.samples
            .get(&driver)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }
    pub fn set_clock(&mut self, clock: i64) {
        self.snapshot.clock_ms = clock;
        for d in self.snapshot.drivers.values_mut() {
            d.in_pit = d.pits.last().is_some_and(|p| {
                p.lane_duration.is_some_and(|duration| {
                    clock >= p.time_ms && clock < p.time_ms + (duration * 1000.0) as i64
                })
            });
        }
    }
    pub fn apply(&mut self, event: RaceEvent) -> bool {
        let kind = match &event.payload {
            EventData::Lap(_) => "lap",
            EventData::Sectors(_) => "sectors",
            EventData::Stint(_) => "stint",
            EventData::Pit(_) => "pit",
            EventData::Telemetry(_) => "telemetry",
            EventData::Location(..) => "location",
            EventData::Position(_) => "position",
            EventData::Interval { .. } => "interval",
            EventData::Weather(_) => "weather",
            EventData::Control(_) => "control",
            EventData::Status(_) => "status",
        };
        let key = (event.driver.unwrap_or(0), kind);
        let ordering_time = if matches!(kind, "lap" | "stint" | "pit") {
            event.time_ms
        } else {
            event.source_time_ms.unwrap_or(event.time_ms)
        };
        if kind != "lap"
            && kind != "control"
            && self
                .seen
                .get(&key)
                .is_some_and(|&last| ordering_time < last)
        {
            return false;
        }
        if let Some(number) = event.driver {
            let Some(d) = self.snapshot.drivers.get_mut(&number) else {
                return false;
            };
            match event.payload {
                EventData::Sectors(lap) => {
                    if lap.number <= d.lap
                        || lap.duration.is_some()
                        || lap
                            .sectors
                            .iter()
                            .flatten()
                            .any(|v| !v.is_finite() || *v <= 0.0)
                    {
                        return false;
                    }
                    if d.partial_lap
                        .as_ref()
                        .is_some_and(|old| old.number > lap.number || old == &lap)
                    {
                        return false;
                    }
                    d.partial_lap = Some(lap);
                }
                EventData::Lap(mut lap) => {
                    if lap.number == 0
                        || lap.number > 500
                        || lap.duration.is_none_or(|v| !v.is_finite() || v <= 0.0)
                    {
                        return false;
                    }
                    let lap_key = (number, lap.number);
                    if self
                        .lap_receipts
                        .get(&lap_key)
                        .is_some_and(|&last| event.time_ms < last)
                    {
                        return false;
                    }
                    self.lap_receipts.insert(lap_key, event.time_ms);
                    lap.pit_in |= d.pits.iter().any(|pit| pit.lap == lap.number);
                    lap.position = d.position;
                    lap.gap = d.gap;
                    lap.interval = d.interval;
                    lap.status = self.snapshot.track_status.clone();
                    if let Some(stint) = d.stints.iter().rev().find(|s| s.start_lap <= lap.number) {
                        lap.compound = stint.compound.clone();
                        lap.tyre_age = Some(
                            stint.tyre_age_at_start
                                + lap.number.saturating_sub(stint.start_lap)
                                + 1,
                        );
                    }
                    if let Some(old) = d.laps.iter_mut().find(|l| l.number == lap.number) {
                        lap.position = old.position;
                        lap.gap = old.gap;
                        lap.interval = old.interval;
                        lap.status = old.status.clone();
                        if *old == lap {
                            return false;
                        }
                        *old = lap;
                    } else {
                        d.laps.push(lap);
                        d.laps.sort_by_key(|l| l.number);
                    }
                    d.lap = d.laps.last().map_or(0, |l| l.number);
                    refresh_stint(d);
                    if d.partial_lap
                        .as_ref()
                        .is_some_and(|lap| lap.number <= d.lap)
                    {
                        d.partial_lap = None;
                    }
                    self.snapshot.lap = self.snapshot.lap.max(d.lap);
                }
                EventData::Stint(mut s) => {
                    if s.number == 0
                        || s.start_lap == 0
                        || s.start_lap > 500
                        || s.tyre_age_at_start > 500
                    {
                        return false;
                    }
                    s.end_lap = None;
                    if let Some(old) = d.stints.iter_mut().find(|old| old.number == s.number) {
                        s.end_lap = old.end_lap;
                        if *old == s {
                            return false;
                        }
                        *old = s;
                    } else {
                        d.stints.push(s);
                    }
                    refresh_stint(d);
                }
                EventData::Pit(p) => {
                    if let Some(lap) = d.laps.iter_mut().find(|lap| lap.number == p.lap) {
                        lap.pit_in = true;
                    }
                    if let Some(old) = d.pits.iter_mut().find(|old| old.lap == p.lap) {
                        if *old == p {
                            return false;
                        }
                        *old = p;
                    } else {
                        d.pits.push(p);
                    }
                }
                EventData::Position(position) => {
                    if position == 0 || position > 99 {
                        return false;
                    }
                    if position == 1 {
                        d.gap = Some(0.0);
                        d.interval = Some(0.0);
                    } else if d.position == Some(1) {
                        d.gap = None;
                        d.interval = None;
                    }
                    d.position = Some(position);
                    d.position_time_ms = Some(ordering_time);
                    d.start_position.get_or_insert(position);
                }
                EventData::Interval { gap, interval } => {
                    d.interval_time_ms = Some(ordering_time);
                    d.gap = if d.position == Some(1) {
                        Some(0.0)
                    } else {
                        gap.filter(|v| v.is_finite())
                    };
                    d.interval = if d.position == Some(1) {
                        Some(0.0)
                    } else {
                        interval.filter(|v| v.is_finite())
                    };
                }
                EventData::Telemetry(s) => {
                    if s.validate().is_err() {
                        return false;
                    }
                    d.telemetry = Some(s.clone());
                    let samples = self.samples.entry(number).or_default();
                    if samples.back().is_some_and(|v| *v == s) {
                        return false;
                    }
                    samples.push_back(s);
                    if samples.len() > 1200 {
                        samples.pop_front();
                    }
                }
                EventData::Location(x, y) if x.is_finite() && y.is_finite() => {
                    d.location = Some((x, y));
                }
                _ => return false,
            }
        } else {
            match event.payload {
                EventData::Weather(w) => self.snapshot.weather = w,
                EventData::Status(s) => self.snapshot.track_status = s,
                EventData::Control(m) => {
                    if self
                        .snapshot
                        .messages
                        .iter()
                        .any(|old| old.time_ms == m.time_ms && old.message == m.message)
                    {
                        return false;
                    }
                    let name = self.snapshot.session.name.to_lowercase();
                    let phase = m.qualifying_phase.or_else(|| {
                        ((name.contains("qualifying") || name.contains("shootout"))
                            && m.category == "SessionStatus"
                            && m.message == "SESSION STARTED"
                            && self
                                .snapshot
                                .qualifying_start_ms
                                .is_none_or(|start| m.time_ms > start))
                        .then(|| {
                            self.snapshot
                                .qualifying_phase
                                .unwrap_or(0)
                                .saturating_add(1)
                                .min(3)
                        })
                    });
                    if let Some(phase) = phase {
                        if self.snapshot.qualifying_phase != Some(phase) {
                            self.snapshot.qualifying_phase = Some(phase);
                            self.snapshot.qualifying_start_ms = Some(m.time_ms);
                        }
                    }
                    self.snapshot.messages.push(m);
                    if self.snapshot.messages.len() > 400 {
                        self.snapshot.messages.remove(0);
                    }
                }
                _ => return false,
            }
        }
        self.seen.insert(key, ordering_time);
        self.snapshot.clock_ms = self.snapshot.clock_ms.max(event.time_ms);
        self.snapshot.revision += 1;
        true
    }
}

pub struct Replay {
    pub archive: Arc<SessionArchive>,
    state: RaceState,
    cursor: usize,
}
impl Replay {
    pub fn new(archive: Arc<SessionArchive>) -> Self {
        let state = RaceState::new(
            archive.session.clone(),
            archive.drivers.clone(),
            archive.circuit.clone(),
        );
        Self {
            archive,
            state,
            cursor: 0,
        }
    }
    pub fn state(&self) -> &RaceState {
        &self.state
    }
    pub fn state_mut(&mut self) -> &mut RaceState {
        &mut self.state
    }
    pub fn seek(&mut self, time_ms: i64) {
        self.state = RaceState::new(
            self.archive.session.clone(),
            self.archive.drivers.clone(),
            self.archive.circuit.clone(),
        );
        self.cursor = 0;
        self.advance(time_ms);
    }
    pub fn advance(&mut self, time_ms: i64) {
        let time = time_ms.clamp(self.archive.session.start_ms, self.archive.session.end_ms);
        if time < self.state.view().clock_ms {
            self.seek(time);
            return;
        }
        while let Some(event) = self
            .archive
            .events
            .get(self.cursor)
            .filter(|e| e.time_ms <= time)
        {
            self.state.apply(event.clone());
            self.cursor += 1;
        }
        self.state.set_clock(time);
    }
}
