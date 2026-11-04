use super::*;
use pitwall_predict::input_signature as prediction_signature;
use pitwall_state::RaceState;
use pitwall_store::{PublishedSnapshot, LIVE_BATCH_LIMIT};
use tokio::sync::RwLock;

pub struct LiveRuntime {
    pub key: u32,
    state: RwLock<RaceState>,
    last: AsyncMutex<Instant>,
    published: AsyncMutex<i64>,
    log: RwLock<pitwall_predict::PredictionLog>,
    published_evaluation: RwLock<Option<pitwall_store::PublishedEvaluation>>,
}
impl LiveRuntime {
    pub fn new(archive: Arc<SessionArchive>) -> Self {
        let mut replay = Replay::new(archive.clone());
        replay.advance(chrono::Utc::now().timestamp_millis());
        let mut state = RaceState::restore(replay.state().snapshot());
        state.view_mut().data_status = "data delayed".into();
        Self {
            key: archive.session.key,
            state: RwLock::new(state),
            last: AsyncMutex::new(Instant::now() - Duration::from_secs(60)),
            published: AsyncMutex::new(0),
            log: RwLock::new(pitwall_predict::PredictionLog::default()),
            published_evaluation: RwLock::new(None),
        }
    }
    pub async fn apply(&self, event: RaceEvent) {
        let time = event.time_ms;
        if self.apply_event(event).await
            && (chrono::Utc::now().timestamp_millis() - time).abs() <= 15_000
        {
            self.mark_live().await;
        }
    }
    async fn mark_live(&self) {
        self.state.write().await.view_mut().data_status = "live".into();
        *self.last.lock().await = Instant::now();
    }
    async fn apply_event(&self, event: RaceEvent) -> bool {
        let mut state = self.state.write().await;
        let time_ms = event.time_ms;
        let source_time_ms = event.source_time_ms;
        let status = if let EventData::Control(message) = &event.payload {
            pitwall_ingest::status_from_control(message)
        } else {
            None
        };
        let accepted = state.apply(event);
        if accepted {
            if let Some(status) = status {
                state.apply(RaceEvent {
                    source_time_ms,
                    time_ms,
                    driver: None,
                    payload: EventData::Status(status),
                });
            }
        }
        let clock = state.view().clock_ms;
        state.set_clock(clock);
        accepted
    }
    pub async fn snapshot(&self) -> Snapshot {
        self.state.read().await.snapshot()
    }
    pub async fn evaluation(&self) -> (Vec<PaceForecast>, AccuracyReport) {
        if let Some(evaluation) = &*self.published_evaluation.read().await {
            return (evaluation.forecasts.clone(), evaluation.report.clone());
        }
        let log = self.log.read().await;
        (
            log.records
                .iter()
                .skip(log.records.len().saturating_sub(500))
                .cloned()
                .collect(),
            log.report(),
        )
    }
    pub async fn telemetry(&self, driver: u32) -> Vec<TelemetrySample> {
        self.state.read().await.telemetry(driver)
    }
    pub async fn mark_delayed(&self) {
        self.state.write().await.view_mut().data_status = "data delayed".into();
    }
    async fn recover_arrivals(&self, store: &Store, watermark: &mut i64) -> anyhow::Result<()> {
        loop {
            let rows = store.journal_after(self.key, *watermark).await?;
            if rows.is_empty() {
                return Ok(());
            }
            for row in rows {
                if row.accepted
                    && self.apply_event(row.event).await
                    && row.mqtt
                    && (chrono::Utc::now().timestamp_millis() - row.received_at_ms).abs() <= 15_000
                {
                    self.mark_live().await;
                }
                *watermark = (*watermark).max(row.sequence);
            }
        }
    }
    pub async fn commit_arrivals(
        &self,
        store: &Store,
        arrivals: &[pitwall_ingest::LiveArrival],
        watermark: &mut i64,
    ) -> anyhow::Result<()> {
        store.append_live(arrivals).await?;
        // Read the committed journal: a prior COMMIT may have succeeded despite a lost response.
        self.recover_arrivals(store, watermark).await
    }
    pub async fn receive_snapshot(&self, published: PublishedSnapshot) -> bool {
        let now = chrono::Utc::now().timestamp_millis();
        if published.snapshot.session.key != self.key
            || now - published.published_at_ms > 15_000
            || published.published_at_ms > now + 5_000
        {
            return false;
        }
        let mut last = self.published.lock().await;
        if published.published_at_ms <= *last {
            return false;
        }
        let mut state = self.state.write().await;
        if published.snapshot.revision < state.view().revision {
            return false;
        }
        for (driver, view) in &published.snapshot.drivers {
            if let Some(sample) = &view.telemetry {
                state.apply(RaceEvent {
                    source_time_ms: None,
                    time_ms: sample.time_ms,
                    driver: Some(*driver),
                    payload: EventData::Telemetry(sample.clone()),
                });
            }
        }
        *state.view_mut() = published.snapshot;
        if let Some(mut evaluation) = published.evaluation {
            let excess = evaluation.forecasts.len().saturating_sub(500);
            evaluation.forecasts.drain(..excess);
            *self.published_evaluation.write().await = Some(evaluation);
        }
        *last = published.published_at_ms;
        *self.last.lock().await = Instant::now();
        true
    }
    async fn subscriber(app: Arc<App>, key: u32) -> anyhow::Result<()> {
        let mut subscription = app.store.subscribe_snapshots(key).await?;
        let initial = tokio::time::timeout(Duration::from_secs(15), subscription.recv())
            .await??
            .ok_or_else(|| {
                anyhow::anyhow!("Live Redis subscription closed before initial snapshot")
            })?;
        anyhow::ensure!(
            initial.snapshot.session.key == key,
            "Redis snapshot session mismatch"
        );
        let mut state = RaceState::restore(initial.snapshot.clone());
        state.view_mut().data_status = "data delayed".into();
        let runtime = Arc::new(Self {
            key,
            state: RwLock::new(state),
            last: AsyncMutex::new(Instant::now() - Duration::from_secs(60)),
            published: AsyncMutex::new(0),
            log: RwLock::new(pitwall_predict::PredictionLog::default()),
            published_evaluation: RwLock::new(None),
        });
        runtime.receive_snapshot(initial).await;
        *app.live.write().await = Some(runtime.clone());
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                next=subscription.recv()=>match next {
                    Ok(Some(value))=>{runtime.receive_snapshot(value).await;},
                    _=>{
                        runtime.mark_delayed().await;
                        tracing::warn!("Redis live subscription interrupted; retaining last state");
                        loop {
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            if let Ok(next)=app.store.subscribe_snapshots(key).await {subscription=next;break;}
                        }
                    }
                },
                _=tick.tick()=>if runtime.last.lock().await.elapsed()>Duration::from_secs(15){runtime.mark_delayed().await;}
            }
        }
    }
    pub async fn start(app: Arc<App>, key: u32) -> anyhow::Result<()> {
        match std::env::var("AGON_LIVE_ROLE")
            .as_deref()
            .unwrap_or("primary")
        {
            "subscriber" => return Self::subscriber(app, key).await,
            "primary" => {}
            _ => anyhow::bail!("AGON_LIVE_ROLE must be primary or subscriber"),
        }
        app.store.require_live_storage()?;
        let archive = app.archive(key).await?;
        app.store.save_live_bootstrap(&archive).await?;
        let runtime = Arc::new(Self::new(archive.clone()));
        let mut watermark = 0;
        if let Some((mut snapshot, saved_watermark)) = app.store.load_live_checkpoint(key).await? {
            if snapshot.session.key == key {
                pitwall_ingest::metadata::apply_metadata(
                    &mut snapshot.session,
                    &mut snapshot.circuit,
                )?;
                let mut state = runtime.state.write().await;
                *state = RaceState::restore(snapshot);
                state.view_mut().data_status = "data delayed".into();
                watermark = saved_watermark;
            }
        }
        let run = format!("live-{key}");
        *runtime.log.write().await = app.store.load_log(&run).await?.unwrap_or_default();
        runtime.recover_arrivals(&app.store, &mut watermark).await?;
        runtime.mark_delayed().await;
        *app.live.write().await = Some(runtime.clone());
        let feed = pitwall_ingest::LiveFeed::new();
        let mut events = feed.take_arrivals().await.unwrap();
        tokio::spawn(async move {
            loop {
                if let Err(e) = feed.run(key).await {
                    tracing::warn!(error=%e,"Live feed interrupted; retrying");
                    tokio::time::sleep(Duration::from_secs(15)).await;
                }
            }
        });
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        let mut last_signature = String::new();
        let mut job: Option<(String, tokio::task::JoinHandle<Vec<Prediction>>)> = None;
        let mut saved = Instant::now();
        let mut pending = Vec::with_capacity(LIVE_BATCH_LIMIT);
        let mut retry_at = Instant::now();
        let mut reconcile = tokio::time::interval_at(
            tokio::time::Instant::now() + Duration::from_secs(60),
            Duration::from_secs(60),
        );
        let mut repair: Option<tokio::task::JoinHandle<anyhow::Result<()>>> = None;
        loop {
            tokio::select! {
                count=events.recv_many(&mut pending,LIVE_BATCH_LIMIT),if pending.is_empty()=>{
                    if count==0{break;}
                    retry_at=Instant::now();
                },
                _=reconcile.tick()=>{if repair.is_none(){let app=app.clone();repair=Some(tokio::spawn(async move{
                    let now=chrono::Utc::now().timestamp_millis();let from=chrono::DateTime::from_timestamp_millis(now-180_000).unwrap().to_rfc3339();
                    for topic in ["laps","position","intervals","pit","race_control","weather","stints"] {
                        let mut params=vec![("session_key",key.to_string())];if topic!="stints"{params.push((if topic=="laps"{"date_start>="}else{"date>="},from.clone()));}
                        let started_at=chrono::Utc::now().timestamp_millis();
                        let rows=app.source.query(topic,&params).await?;
                        let received_at=chrono::Utc::now().timestamp_millis();
                        for batch in rows.chunks(LIVE_BATCH_LIMIT){loop {
                            match app.store.append_rest(key,topic,batch,started_at,received_at).await {
                                Ok(())=>break,
                                Err(error) if error.downcast_ref::<sqlx::Error>().is_some()=>{tracing::error!(%error,"Live REST durability failed; retaining bounded response for retry");tokio::time::sleep(Duration::from_secs(5)).await;}
                                Err(error)=>return Err(error),
                            }
                        }}
                    }
                    Ok(())
                }));}},
                _=tick.tick()=>{
                    if !pending.is_empty()&&Instant::now()>=retry_at {
                        match runtime.commit_arrivals(&app.store,&pending,&mut watermark).await {
                            Ok(())=>{pending.clear();},
                            Err(e)=>{runtime.mark_delayed().await;retry_at=Instant::now()+Duration::from_secs(5);tracing::error!(error=%e,"Live durability failed; retaining bounded batch for retry");}
                        }
                    }
                    if repair.as_ref().is_some_and(|h|h.is_finished()){
                        if !matches!(repair.take().unwrap().await,Ok(Ok(()))){tracing::warn!("Live REST reconciliation failed; retaining committed repairs");}
                        if let Err(e)=runtime.recover_arrivals(&app.store,&mut watermark).await{tracing::error!(error=%e,"Live repair recovery failed");runtime.mark_delayed().await;}
                    }
                    if runtime.last.lock().await.elapsed()>Duration::from_secs(15){runtime.mark_delayed().await;}
                    let view=runtime.snapshot().await;let signature=prediction_signature(&view);
                    runtime.log.write().await.observe(&view);
                    if job.as_ref().is_some_and(|(_,h)|h.is_finished()) {let (sig,h)=job.take().unwrap();if let Ok(predictions)=h.await{if sig==signature {runtime.log.write().await.record(&view,&predictions);let mut state=runtime.state.write().await;for p in predictions{if let Some(d)=state.view_mut().drivers.get_mut(&p.driver){d.prediction=Some(p);}}}}}
                    if signature!=last_signature&&job.is_none(){let snapshot=view.clone();job=Some((signature.clone(),tokio::task::spawn_blocking(move||pitwall_predict::predict(&snapshot))));last_signature=signature;}
                let (forecasts,report)=runtime.evaluation().await;
                if let Err(e)=app.store.publish_snapshot(PublishedSnapshot{published_at_ms:chrono::Utc::now().timestamp_millis(),snapshot:view.clone(),evaluation:Some(pitwall_store::PublishedEvaluation{forecasts,report})}).await{tracing::warn!(error=%e,"Redis snapshot failed");}
                if saved.elapsed()>Duration::from_secs(10){
                    if let Err(e)=app.store.checkpoint_live(&view,watermark).await{tracing::error!(error=%e,"Live checkpoint persistence failed");}
                    if let Err(e)=app.store.log(&run,key,&*runtime.log.read().await).await{tracing::error!(error=%e,"Live prediction persistence failed");}
                    saved=Instant::now();
                }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn prediction_signature_tracks_corrections_but_ignores_telemetry() {
        let runtime = LiveRuntime::new(Arc::new(pitwall_state::demo_archive()));
        let mut snapshot = runtime.snapshot().await;
        let original = prediction_signature(&snapshot);
        snapshot.clock_ms += 1000;
        snapshot.revision += 1;
        snapshot.drivers.get_mut(&1).unwrap().telemetry = Some(TelemetrySample::default());
        assert_eq!(prediction_signature(&snapshot), original);
        let driver = snapshot.drivers.get_mut(&1).unwrap();
        driver.laps[0].duration = Some(123.0);
        assert_ne!(prediction_signature(&snapshot), original);
        let lap_correction = prediction_signature(&snapshot);
        snapshot.drivers.get_mut(&1).unwrap().stints[0].tyre_age_at_start += 1;
        assert_ne!(prediction_signature(&snapshot), lap_correction);
        let tyre_correction = prediction_signature(&snapshot);
        snapshot.drivers.get_mut(&1).unwrap().pits.push(Pit {
            lap: 2,
            lane_duration: Some(20.0),
            ..Default::default()
        });
        let pit_added = prediction_signature(&snapshot);
        assert_ne!(pit_added, tyre_correction);
        snapshot
            .drivers
            .get_mut(&1)
            .unwrap()
            .pits
            .last_mut()
            .unwrap()
            .lane_duration = Some(24.0);
        assert_ne!(prediction_signature(&snapshot), pit_added);
    }

    #[tokio::test]
    async fn subscriber_receives_the_primary_accuracy_report() {
        let runtime = LiveRuntime::new(Arc::new(pitwall_state::demo_archive()));
        let mut snapshot = runtime.snapshot().await;
        snapshot.revision += 1;
        let mut report = pitwall_predict::PredictionLog::default().report();
        report.pace_samples = 4;
        report.pace_mae = Some(0.4);
        assert!(
            runtime
                .receive_snapshot(PublishedSnapshot {
                    published_at_ms: chrono::Utc::now().timestamp_millis(),
                    snapshot,
                    evaluation: Some(pitwall_store::PublishedEvaluation {
                        forecasts: vec![],
                        report
                    })
                })
                .await
        );
        let (_, report) = runtime.evaluation().await;
        assert_eq!(report.pace_samples, 4);
        assert_eq!(report.pace_mae, Some(0.4));
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL and Redis from docker compose"]
    async fn rest_recovery_survives_restart_without_marking_mqtt_healthy() {
        let key = 3_000_000 + std::process::id();
        let path = std::env::temp_dir().join(format!("agon-rest-runtime-{key}"));
        let store = Store::connect(path.clone()).await.unwrap();
        let mut archive = pitwall_state::demo_archive();
        archive.session.key = key;
        let runtime = LiveRuntime::new(Arc::new(archive.clone()));
        let checkpoint = runtime.snapshot().await;
        store.checkpoint_live(&checkpoint, 0).await.unwrap();
        let pit = serde_json::json!({"session_key":key,"driver_number":1,"lap_number":13,"date":"2026-11-01T10:00:00Z","lane_duration":23.0});
        let received = chrono::Utc::now().timestamp_millis();
        store
            .append_rest(key, "pit", &[pit], received, received)
            .await
            .unwrap();
        let restarted = LiveRuntime::new(Arc::new(archive));
        let (saved, mut watermark) = store.load_live_checkpoint(key).await.unwrap().unwrap();
        *restarted.state.write().await = RaceState::restore(saved);
        restarted
            .recover_arrivals(&store, &mut watermark)
            .await
            .unwrap();
        let snapshot = restarted.snapshot().await;
        assert_eq!(
            snapshot.drivers[&1].pits.last().unwrap().lane_duration,
            Some(23.0)
        );
        assert_eq!(snapshot.data_status, "data delayed");
        assert!(watermark > 0);
        tokio::fs::remove_dir(path).await.unwrap();
    }
}
