use pitwall_core::*;
use pitwall_ingest::OpenF1;
use pitwall_state::RaceState;
use std::time::{Duration, Instant};
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Channel {
    Positions,
    Telemetry(u32),
}
pub(crate) struct ReplayWindow {
    events: Vec<RaceEvent>,
    cursor: usize,
    from: Option<i64>,
    generation: u32,
    channel: Option<Channel>,
    retry_at: Instant,
    job: Option<tokio::task::JoinHandle<anyhow::Result<Vec<RaceEvent>>>>,
}
impl ReplayWindow {
    pub fn new() -> Self {
        Self {
            events: vec![],
            cursor: 0,
            from: None,
            generation: 0,
            channel: None,
            retry_at: Instant::now(),
            job: None,
        }
    }
    pub fn reset(&mut self) {
        if let Some(job) = self.job.take() {
            job.abort();
        }
        self.events.clear();
        self.cursor = 0;
        self.from = None;
        self.channel = None;
        self.retry_at = Instant::now();
    }
    pub async fn update(
        &mut self,
        state: &mut RaceState,
        source: &OpenF1,
        generation: u32,
        channel: Channel,
    ) {
        if self.generation != generation || self.channel != Some(channel) {
            self.reset();
            self.generation = generation;
            self.channel = Some(channel);
        }
        let view = state.view();
        if view.session.synthetic {
            return;
        }
        let clock = view.clock_ms;
        let start = view.session.start_ms;
        let end = view.session.end_ms;
        let key = view.session.key;
        if self.job.as_ref().is_some_and(|job| job.is_finished()) {
            match self.job.take().unwrap().await {
                Ok(Ok(events)) => {
                    self.events = events;
                    self.cursor = 0;
                }
                _ => {
                    self.from = None;
                    self.retry_at = Instant::now() + Duration::from_secs(10);
                    tracing::warn!("Replay source window unavailable; retaining last samples");
                }
            }
        }
        while let Some(event) = self
            .events
            .get(self.cursor)
            .filter(|event| event.time_ms <= clock)
        {
            state.apply(event.clone());
            self.cursor += 1;
        }
        let span = match channel {
            Channel::Positions => 120_000,
            Channel::Telemetry(_) => 180_000,
        };
        let from = start + (clock - start) / span * span;
        if self.from != Some(from)
            && self.job.is_none()
            && from < end
            && Instant::now() >= self.retry_at
        {
            let source = source.clone();
            let to = (from + span).min(end);
            self.from = Some(from);
            self.job = Some(tokio::spawn(async move {
                let mut events = match channel {
                    Channel::Positions => source.locations(key, from, to).await?,
                    Channel::Telemetry(driver) => source
                        .telemetry(key, driver, from, to)
                        .await?
                        .into_iter()
                        .map(|sample| RaceEvent {
                            source_time_ms: None,
                            time_ms: sample.time_ms,
                            driver: Some(driver),
                            payload: EventData::Telemetry(sample),
                        })
                        .collect(),
                };
                events.sort_by_key(|event| event.time_ms);
                Ok(events)
            }));
        }
    }
}
impl Drop for ReplayWindow {
    fn drop(&mut self) {
        if let Some(job) = self.job.take() {
            job.abort();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn prefetched_samples_are_not_applied_before_replay_clock() {
        let mut state = pitwall_state::RaceState::new(
            Session {
                key: 1,
                start_ms: 0,
                end_ms: 180000,
                ..Default::default()
            },
            vec![Driver {
                number: 1,
                code: "AAA".into(),
                ..Default::default()
            }],
            Circuit::default(),
        );
        state.set_clock(1500);
        let mut window = ReplayWindow::new();
        window.channel = Some(Channel::Positions);
        window.from = Some(0);
        window.events = vec![
            RaceEvent {
                source_time_ms: None,
                time_ms: 1000,
                driver: Some(1),
                payload: EventData::Location(3.0, 4.0),
            },
            RaceEvent {
                source_time_ms: None,
                time_ms: 2000,
                driver: Some(1),
                payload: EventData::Location(7.0, 8.0),
            },
        ];
        let source = pitwall_ingest::OpenF1::new().unwrap();
        window
            .update(&mut state, &source, 0, Channel::Positions)
            .await;
        assert_eq!(state.view().drivers[&1].location, Some((3.0, 4.0)));
        state.set_clock(2500);
        window
            .update(&mut state, &source, 0, Channel::Positions)
            .await;
        assert_eq!(state.view().drivers[&1].location, Some((7.0, 8.0)));
    }
}
