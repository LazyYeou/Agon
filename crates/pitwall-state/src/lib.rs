//! Causal session fold: replay only applies events whose observation time has arrived.
use pitwall_core::*;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};
mod demo;
pub use demo::demo_archive;

pub struct RaceState {
    snapshot: Snapshot,
    seen: BTreeMap<(u32, &'static str), i64>,
    samples: BTreeMap<u32, VecDeque<TelemetrySample>>,
}
impl RaceState {
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
        if kind != "lap"
            && kind != "control"
            && self
                .seen
                .get(&key)
                .is_some_and(|&last| event.time_ms < last)
        {
            return false;
        }
        if let Some(number) = event.driver {
            let Some(d) = self.snapshot.drivers.get_mut(&number) else {
                return false;
            };
            match event.payload {
                EventData::Lap(mut lap) => {
                    if lap.number == 0 || lap.duration.is_some_and(|v| !v.is_finite() || v <= 0.0) {
                        return false;
                    }
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
                        if old.time_ms > lap.time_ms || *old == lap {
                            return false;
                        }
                        *old = lap;
                    } else {
                        d.laps.push(lap);
                        d.laps.sort_by_key(|l| l.number);
                    }
                    d.lap = d.laps.last().map_or(0, |l| l.number);
                    self.snapshot.lap = self.snapshot.lap.max(d.lap);
                }
                EventData::Stint(mut s) => {
                    s.end_lap = None;
                    if let Some(old) = d.stints.iter_mut().find(|old| old.number == s.number) {
                        if *old == s {
                            return false;
                        }
                        *old = s;
                    } else {
                        if let Some(last) = d.stints.last_mut() {
                            last.end_lap = Some(s.start_lap.saturating_sub(1));
                        }
                        d.stints.push(s);
                    }
                }
                EventData::Pit(p) => {
                    if d.pits.iter().any(|old| old.lap == p.lap) {
                        return false;
                    }
                    d.pits.push(p);
                }
                EventData::Position(position) => {
                    if position == 0 || position > 99 {
                        return false;
                    }
                    d.position = Some(position);
                    d.start_position.get_or_insert(position);
                }
                EventData::Interval { gap, interval } => {
                    d.gap = gap.filter(|v| v.is_finite());
                    d.interval = interval.filter(|v| v.is_finite());
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
                    self.snapshot.messages.push(m);
                    if self.snapshot.messages.len() > 400 {
                        self.snapshot.messages.remove(0);
                    }
                }
                _ => return false,
            }
        }
        self.seen.insert(key, event.time_ms);
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
