//! Private archives and forecast logs, with atomic files locally and PostgreSQL in deployment.
use anyhow::{Context, Result};
use pitwall_core::*;
use pitwall_ingest::LiveArrival;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tokio::sync::{watch, Mutex};

pub const LIVE_BATCH_LIMIT: usize = 256;

#[derive(Debug)]
pub struct StoredArrival {
    pub sequence: i64,
    pub arrival: LiveArrival,
    pub accepted: bool,
}

pub struct JournalEvent {
    pub sequence: i64,
    pub event: RaceEvent,
    pub received_at_ms: i64,
    pub mqtt: bool,
    pub accepted: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PublishedEvaluation {
    pub forecasts: Vec<PaceForecast>,
    pub report: AccuracyReport,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PublishedSnapshot {
    pub published_at_ms: i64,
    pub snapshot: Snapshot,
    #[serde(default)]
    pub evaluation: Option<PublishedEvaluation>,
}
pub struct SnapshotSubscription {
    key: u32,
    initial: Option<PublishedSnapshot>,
    messages: watch::Receiver<Option<redis::PushInfo>>,
    _connection: redis::aio::MultiplexedConnection,
}
impl SnapshotSubscription {
    pub async fn recv(&mut self) -> Result<Option<PublishedSnapshot>> {
        if let Some(initial) = self.initial.take() {
            return Ok(Some(initial));
        }
        loop {
            self.messages.changed().await?;
            let message = self.messages.borrow_and_update().clone().unwrap();
            if message.kind == redis::PushKind::Disconnection {
                return Ok(None);
            }
            let payload = redis::from_redis_value::<String>(
                message
                    .data
                    .get(1)
                    .context("Missing Redis snapshot payload")?,
            )?;
            let value = serde_json::from_str::<PublishedSnapshot>(&payload)?;
            if value.snapshot.session.key == self.key {
                return Ok(Some(value));
            }
        }
    }
}

pub struct Store {
    root: PathBuf,
    pool: Option<sqlx::PgPool>,
    redis: Option<redis::Client>,
    write: Mutex<()>,
    next: AtomicU64,
}
impl Store {
    pub async fn local(root: PathBuf) -> Result<Self> {
        tokio::fs::create_dir_all(&root).await?;
        Ok(Self {
            root,
            pool: None,
            redis: None,
            write: Mutex::new(()),
            next: AtomicU64::new(0),
        })
    }
    pub async fn connect(root: PathBuf) -> Result<Self> {
        let mut store = Self::local(root).await?;
        if let Ok(url) = std::env::var("DATABASE_URL") {
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(8)
                .connect(&url)
                .await
                .context("Connect PostgreSQL")?;
            let mut migration = pool.begin().await?;
            sqlx::query("SELECT pg_advisory_xact_lock(-1)")
                .execute(&mut *migration)
                .await?;
            sqlx::raw_sql(include_str!("../../../migrations/001_initial.sql"))
                .execute(&mut *migration)
                .await?;
            sqlx::raw_sql(include_str!("../../../migrations/002_live_arrivals.sql"))
                .execute(&mut *migration)
                .await?;
            sqlx::raw_sql(include_str!("../../../migrations/003_rest_receipts.sql"))
                .execute(&mut *migration)
                .await?;
            migration.commit().await?;
            store.pool = Some(pool);
        }
        if let Ok(url) = std::env::var("REDIS_URL") {
            let client = redis::Client::open(url)?;
            let mut connection = client.get_multiplexed_async_connection().await?;
            redis::cmd("PING")
                .query_async::<String>(&mut connection)
                .await?;
            store.redis = Some(client);
        }
        Ok(store)
    }
    pub async fn save(&self, archive: &SessionArchive) -> Result<()> {
        let value = serde_json::to_value(archive)?;
        if let Some(pool) = &self.pool {
            sqlx::query("INSERT INTO agon_archives(session_key,archive) VALUES($1,$2) ON CONFLICT(session_key) DO UPDATE SET archive=EXCLUDED.archive,updated_at=now()")
            .bind(archive.session.key as i64).bind(&value).execute(pool).await?;
        }
        self.atomic(
            &format!("{}.json", archive.session.key),
            &serde_json::to_vec(&value)?,
        )
        .await
    }
    pub async fn load(&self, key: u32) -> Result<Option<SessionArchive>> {
        if let Some(pool) = &self.pool {
            if let Some(value) = sqlx::query_scalar::<_, serde_json::Value>(
                "SELECT archive FROM agon_archives WHERE session_key=$1",
            )
            .bind(key as i64)
            .fetch_optional(pool)
            .await?
            {
                return Ok(Some(serde_json::from_value(value)?));
            }
        }
        match tokio::fs::read(self.root.join(format!("{key}.json"))).await {
            Ok(bytes) => Ok(Some(
                serde_json::from_slice(&bytes)
                    .context("Cached archive is corrupt; retained for inspection")?,
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    pub async fn log<T: Serialize>(&self, run: &str, session: u32, records: &T) -> Result<()> {
        if run.is_empty()
            || run.len() > 80
            || !run.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            anyhow::bail!("Invalid run identity");
        }
        let value = serde_json::to_value(records)?;
        if let Some(pool) = &self.pool {
            sqlx::query("INSERT INTO agon_forecast_runs(run_id,session_key,records) VALUES($1,$2,$3) ON CONFLICT(run_id) DO UPDATE SET records=EXCLUDED.records,updated_at=now()")
            .bind(run).bind(session as i64).bind(&value).execute(pool).await?;
        }
        self.atomic(
            &format!("forecast-{run}.json"),
            &serde_json::to_vec(&value)?,
        )
        .await
    }
    pub async fn load_log<T: serde::de::DeserializeOwned>(&self, run: &str) -> Result<Option<T>> {
        anyhow::ensure!(
            !run.is_empty()
                && run.len() <= 80
                && run.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "Invalid run identity"
        );
        if let Some(pool) = &self.pool {
            if let Some(value) = sqlx::query_scalar::<_, serde_json::Value>(
                "SELECT records FROM agon_forecast_runs WHERE run_id=$1",
            )
            .bind(run)
            .fetch_optional(pool)
            .await?
            {
                return Ok(Some(serde_json::from_value(value)?));
            }
        }
        match tokio::fs::read(self.root.join(format!("forecast-{run}.json"))).await {
            Ok(bytes) => Ok(Some(
                serde_json::from_slice(&bytes)
                    .context("Cached forecast log is corrupt; retained for inspection")?,
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    pub async fn save_live_bootstrap(&self, archive: &SessionArchive) -> Result<()> {
        self.require_live_storage()?;
        let mut bootstrap = archive.clone();
        let captured = bootstrap
            .captured_at_ms
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
        bootstrap.captured_at_ms = Some(captured);
        bootstrap.events.retain(|event| event.time_ms <= captured);
        sqlx::query("INSERT INTO agon_live_bootstraps(session_key,archive) VALUES($1,$2) ON CONFLICT(session_key) DO NOTHING")
            .bind(i64::from(archive.session.key)).bind(serde_json::to_value(bootstrap)?).execute(self.pool.as_ref().unwrap()).await?;
        Ok(())
    }
    pub async fn load_live_archive(&self, key: u32) -> Result<Option<SessionArchive>> {
        let Some(pool) = &self.pool else {
            return Ok(None);
        };
        let Some(value) = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT archive FROM agon_live_bootstraps WHERE session_key=$1",
        )
        .bind(i64::from(key))
        .fetch_optional(pool)
        .await?
        else {
            return Ok(None);
        };
        let mut archive: SessionArchive = serde_json::from_value(value)?;
        let floor = archive
            .captured_at_ms
            .context("Live bootstrap missing capture time")?;
        let mut watermark = 0;
        loop {
            let rows = self.journal_after(key, watermark).await?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                watermark = row.sequence;
                if !row.accepted || row.received_at_ms < floor {
                    continue;
                }
                let mut event = row.event;
                event.time_ms = row.received_at_ms;
                archive.session.end_ms = archive.session.end_ms.max(event.time_ms);
                if let EventData::Control(message) = &event.payload {
                    if let Some(status) = pitwall_ingest::status_from_control(message) {
                        archive.events.push(RaceEvent {
                            time_ms: event.time_ms,
                            source_time_ms: event.source_time_ms,
                            driver: None,
                            payload: EventData::Status(status),
                        });
                    }
                }
                archive.events.push(event);
            }
        }
        archive.events.sort_by_key(|event| event.time_ms);
        Ok(Some(archive))
    }
    pub async fn append_rest(
        &self,
        key: u32,
        topic: &str,
        documents: &[serde_json::Value],
        started_at_ms: i64,
        received_at_ms: i64,
    ) -> Result<()> {
        self.require_live_storage()?;
        anyhow::ensure!(
            !documents.is_empty() && documents.len() <= LIVE_BATCH_LIMIT,
            "REST batch must contain 1..=256 documents"
        );
        anyhow::ensure!(
            matches!(
                topic,
                "laps" | "position" | "intervals" | "pit" | "race_control" | "weather" | "stints"
            ),
            "Invalid REST repair topic"
        );
        anyhow::ensure!(
            received_at_ms >= started_at_ms,
            "REST receipt precedes request"
        );
        let mut repairs = Vec::with_capacity(documents.len());
        let mut revisions = Vec::new();
        for document in documents {
            anyhow::ensure!(
                document["session_key"].as_u64() == Some(u64::from(key))
                    && serde_json::to_vec(document)?.len() <= 1_048_576,
                "Invalid REST repair session or size"
            );
            if document.get("_id").is_some() || document.get("_key").is_some() {
                revisions.push(
                    LiveArrival::decode(topic, document, key, received_at_ms)
                        .context("Invalid REST document revision")?,
                );
            } else if let Some(mut event) = pitwall_ingest::normalize(
                topic,
                document,
                if matches!(topic, "laps" | "stints" | "pit") {
                    Some(received_at_ms)
                } else {
                    None
                },
            ) {
                if event.time_ms <= received_at_ms {
                    event.time_ms = received_at_ms;
                    repairs.push(serde_json::json!({"source":"rest","session_key":key,"source_topic":topic,"received_at_ms":received_at_ms,"document":document,"event":event}));
                }
            }
        }
        if !revisions.is_empty() {
            self.append_revisions(&revisions, false).await?;
        }
        let mut transaction = self.pool.as_ref().unwrap().begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(i64::from(key))
            .execute(&mut *transaction)
            .await?;
        for receipt in repairs {
            // A response already in flight cannot supersede a later MQTT observation.
            let superseded = sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM agon_live_events WHERE session_key=$1 AND source_topic=$2 AND event->'driver'=$3 AND received_at>=to_timestamp($4::double precision/1000))")
                .bind(i64::from(key)).bind(topic).bind(&receipt["event"]["driver"]).bind(started_at_ms as f64).fetch_one(&mut *transaction).await?;
            sqlx::query("INSERT INTO agon_live_arrivals(session_key,received_at,source_topic,source_key,source_id,arrival,accepted) SELECT $1,to_timestamp($2::double precision/1000),$3,NULL,NULL,$4,$5 WHERE NOT EXISTS(SELECT 1 FROM agon_live_arrivals WHERE session_key=$1 AND source_topic=$3 AND source_id IS NULL AND received_at=to_timestamp($2::double precision/1000) AND arrival->'document'=$4::jsonb->'document')")
                .bind(i64::from(key)).bind(received_at_ms as f64).bind(topic).bind(receipt).bind(!superseded).execute(&mut *transaction).await?;
        }
        transaction.commit().await?;
        Ok(())
    }
    pub async fn snapshot(&self, snapshot: &Snapshot) -> Result<()> {
        self.publish_snapshot(PublishedSnapshot {
            published_at_ms: chrono::Utc::now().timestamp_millis(),
            snapshot: snapshot.clone(),
            evaluation: None,
        })
        .await
    }
    pub async fn publish_snapshot(&self, mut published: PublishedSnapshot) -> Result<()> {
        if let Some(evaluation) = &mut published.evaluation {
            let excess = evaluation.forecasts.len().saturating_sub(500);
            evaluation.forecasts.drain(..excess);
        }
        if let Some(client) = &self.redis {
            let mut conn = client.get_multiplexed_async_connection().await?;
            let value = serde_json::to_string(&published)?;
            redis::cmd("SET")
                .arg(format!("agon:snapshot:{}", published.snapshot.session.key))
                .arg(&value)
                .arg("EX")
                .arg(120)
                .query_async::<()>(&mut conn)
                .await?;
            redis::cmd("PUBLISH")
                .arg(format!("agon:session:{}", published.snapshot.session.key))
                .arg(value)
                .query_async::<i64>(&mut conn)
                .await?;
        }
        Ok(())
    }
    pub fn require_live_storage(&self) -> Result<()> {
        anyhow::ensure!(
            self.pool.is_some(),
            "Primary live ingest requires DATABASE_URL for durable revision history"
        );
        Ok(())
    }
    pub async fn subscribe_snapshots(&self, key: u32) -> Result<SnapshotSubscription> {
        let client = self
            .redis
            .as_ref()
            .context("Live subscriber requires REDIS_URL")?;
        let mut info = client.get_connection_info().clone();
        info.redis.protocol = redis::ProtocolVersion::RESP3;
        let client = redis::Client::open(info)?;
        let (sender, messages) = watch::channel(None);
        // RESP2 PubSub internally uses an unbounded queue; coalesce RESP3 pushes instead.
        let config =
            redis::AsyncConnectionConfig::new().set_push_sender(move |message: redis::PushInfo| {
                if matches!(
                    message.kind,
                    redis::PushKind::Message | redis::PushKind::Disconnection
                ) {
                    sender.send_replace(Some(message));
                }
                Ok::<(), redis::aio::SendError>(())
            });
        let mut connection = client
            .get_multiplexed_async_connection_with_config(&config)
            .await?;
        connection.subscribe(format!("agon:session:{key}")).await?;
        // Subscribe before reading the cache so an intervening publication cannot be missed.
        let cached: Option<String> = redis::cmd("GET")
            .arg(format!("agon:snapshot:{key}"))
            .query_async(&mut connection)
            .await?;
        let initial = cached
            .map(|value| serde_json::from_str::<PublishedSnapshot>(&value))
            .transpose()?
            .filter(|value| value.snapshot.session.key == key);
        Ok(SnapshotSubscription {
            key,
            initial,
            messages,
            _connection: connection,
        })
    }
    pub async fn append_live(&self, arrivals: &[LiveArrival]) -> Result<Vec<StoredArrival>> {
        self.append_revisions(arrivals, true).await
    }
    async fn append_revisions(
        &self,
        arrivals: &[LiveArrival],
        mqtt: bool,
    ) -> Result<Vec<StoredArrival>> {
        self.require_live_storage()?;
        anyhow::ensure!(
            !arrivals.is_empty() && arrivals.len() <= LIVE_BATCH_LIMIT,
            "Live batch must contain 1..=256 arrivals"
        );
        let key = arrivals[0].session_key;
        for arrival in arrivals {
            anyhow::ensure!(
                arrival.session_key == key
                    && arrival.source_key.len() <= 256
                    && !arrival.source_key.is_empty()
                    && arrival.source_id >= 0
                    && arrival.source_topic.len() <= 32
                    && serde_json::to_vec(arrival)?.len() <= 1_048_576,
                "Invalid live arrival identity or size"
            );
            let decoded = LiveArrival::decode(
                &arrival.source_topic,
                &arrival.document,
                key,
                arrival.received_at_ms,
            )
            .context("Invalid original live document")?;
            anyhow::ensure!(
                decoded.source_key == arrival.source_key
                    && decoded.source_id == arrival.source_id
                    && decoded.event == arrival.event,
                "Live arrival disagrees with original document"
            );
        }
        let mut transaction = self.pool.as_ref().unwrap().begin().await?;
        // Allocate journal sequences in commit order for this session, including across primaries.
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(i64::from(key))
            .execute(&mut *transaction)
            .await?;
        let mut committed = Vec::with_capacity(arrivals.len());
        for arrival in arrivals {
            if let Some(sequence) = sqlx::query_scalar::<_, i64>("SELECT sequence FROM agon_live_arrivals WHERE session_key=$1 AND source_topic=$2 AND source_key=$3 AND source_id=$4")
                .bind(i64::from(key)).bind(&arrival.source_topic).bind(&arrival.source_key).bind(arrival.source_id)
                .fetch_optional(&mut *transaction).await? {
                committed.push(StoredArrival { sequence, arrival: arrival.clone(), accepted: false });
                continue;
            }
            let accepted = sqlx::query("INSERT INTO agon_live_events(session_key,received_at,source_topic,source_key,source_id,event) VALUES($1,to_timestamp($2::double precision/1000),$3,$4,$5,$6) ON CONFLICT(session_key,source_topic,source_key) DO UPDATE SET received_at=EXCLUDED.received_at,source_id=EXCLUDED.source_id,event=EXCLUDED.event WHERE agon_live_events.source_id<EXCLUDED.source_id")
                .bind(i64::from(key)).bind(arrival.received_at_ms as f64).bind(&arrival.source_topic).bind(&arrival.source_key).bind(arrival.source_id).bind(serde_json::to_value(&arrival.event)?)
                .execute(&mut *transaction).await?.rows_affected() == 1;
            let mut receipt = serde_json::to_value(arrival)?;
            if !mqtt {
                receipt["source"] = serde_json::json!("rest");
            }
            let sequence = sqlx::query_scalar::<_, i64>("INSERT INTO agon_live_arrivals(session_key,received_at,source_topic,source_key,source_id,arrival,accepted) VALUES($1,to_timestamp($2::double precision/1000),$3,$4,$5,$6,$7) RETURNING sequence")
                .bind(i64::from(key)).bind(arrival.received_at_ms as f64).bind(&arrival.source_topic).bind(&arrival.source_key).bind(arrival.source_id).bind(receipt).bind(accepted)
                .fetch_one(&mut *transaction).await?;
            committed.push(StoredArrival {
                sequence,
                arrival: arrival.clone(),
                accepted,
            });
        }
        transaction.commit().await?;
        Ok(committed)
    }
    pub async fn live_after(&self, key: u32, watermark: i64) -> Result<Vec<StoredArrival>> {
        self.require_live_storage()?;
        let rows = sqlx::query_as::<_, (i64, serde_json::Value, bool)>("SELECT sequence,arrival,accepted FROM agon_live_arrivals WHERE session_key=$1 AND sequence>$2 AND source_id IS NOT NULL ORDER BY sequence LIMIT 256")
            .bind(i64::from(key)).bind(watermark).fetch_all(self.pool.as_ref().unwrap()).await?;
        rows.into_iter()
            .map(|(sequence, value, accepted)| {
                Ok(StoredArrival {
                    sequence,
                    arrival: serde_json::from_value(value)?,
                    accepted,
                })
            })
            .collect()
    }
    pub async fn journal_after(&self, key: u32, watermark: i64) -> Result<Vec<JournalEvent>> {
        self.require_live_storage()?;
        let rows = sqlx::query_as::<_,(i64,serde_json::Value,bool)>("SELECT sequence,arrival,accepted FROM agon_live_arrivals WHERE session_key=$1 AND sequence>$2 ORDER BY sequence LIMIT 256")
            .bind(i64::from(key)).bind(watermark).fetch_all(self.pool.as_ref().unwrap()).await?;
        rows.into_iter()
            .map(|(sequence, receipt, accepted)| {
                Ok(JournalEvent {
                    sequence,
                    event: serde_json::from_value(receipt["event"].clone())?,
                    received_at_ms: receipt["received_at_ms"]
                        .as_i64()
                        .context("Missing journal receipt time")?,
                    mqtt: receipt["source"] != "rest",
                    accepted,
                })
            })
            .collect()
    }
    pub async fn checkpoint_live(&self, snapshot: &Snapshot, watermark: i64) -> Result<()> {
        self.require_live_storage()?;
        let mut transaction = self.pool.as_ref().unwrap().begin().await?;
        let max = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(sequence),0) FROM agon_live_arrivals WHERE session_key=$1",
        )
        .bind(i64::from(snapshot.session.key))
        .fetch_one(&mut *transaction)
        .await?;
        anyhow::ensure!(
            watermark >= 0 && watermark <= max,
            "Checkpoint watermark exceeds durable arrivals"
        );
        let result = sqlx::query("INSERT INTO agon_checkpoints(session_key,state,live_watermark) VALUES($1,$2,$3) ON CONFLICT(session_key) DO UPDATE SET state=EXCLUDED.state,live_watermark=EXCLUDED.live_watermark,updated_at=now() WHERE agon_checkpoints.live_watermark<=EXCLUDED.live_watermark")
            .bind(i64::from(snapshot.session.key)).bind(serde_json::to_value(snapshot)?).bind(watermark).execute(&mut *transaction).await?;
        anyhow::ensure!(
            result.rows_affected() == 1,
            "Checkpoint watermark would move backwards"
        );
        transaction.commit().await?;
        Ok(())
    }
    pub async fn load_live_checkpoint(&self, key: u32) -> Result<Option<(Snapshot, i64)>> {
        self.require_live_storage()?;
        let row = sqlx::query_as::<_, (serde_json::Value, i64)>(
            "SELECT state,live_watermark FROM agon_checkpoints WHERE session_key=$1",
        )
        .bind(i64::from(key))
        .fetch_optional(self.pool.as_ref().unwrap())
        .await?;
        row.map(|(value, watermark)| Ok((serde_json::from_value(value)?, watermark)))
            .transpose()
    }
    pub async fn checkpoint(&self, snapshot: &Snapshot) -> Result<()> {
        let value = serde_json::to_value(snapshot)?;
        if let Some(pool) = &self.pool {
            sqlx::query("INSERT INTO agon_checkpoints(session_key,state) VALUES($1,$2) ON CONFLICT(session_key) DO UPDATE SET state=EXCLUDED.state,updated_at=now()")
            .bind(snapshot.session.key as i64).bind(&value).execute(pool).await?;
        }
        self.atomic(
            &format!("checkpoint-{}.json", snapshot.session.key),
            &serde_json::to_vec(&value)?,
        )
        .await
    }
    pub async fn load_checkpoint(&self, key: u32) -> Result<Option<Snapshot>> {
        if let Some(pool) = &self.pool {
            if let Some(value) = sqlx::query_scalar::<_, serde_json::Value>(
                "SELECT state FROM agon_checkpoints WHERE session_key=$1",
            )
            .bind(key as i64)
            .fetch_optional(pool)
            .await?
            {
                return Ok(Some(serde_json::from_value(value)?));
            }
        }
        match tokio::fs::read(self.root.join(format!("checkpoint-{key}.json"))).await {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    async fn atomic(&self, name: &str, bytes: &[u8]) -> Result<()> {
        let _guard = self.write.lock().await;
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let temp = self
            .root
            .join(format!(".{name}-{}-{id}.tmp", std::process::id()));
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .await?;
        use tokio::io::AsyncWriteExt;
        file.write_all(bytes).await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&temp, self.root.join(name)).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn forecast_log_survives_restart() {
        let path = std::env::temp_dir().join(format!("agon-log-restart-{}", std::process::id()));
        let store = Store::local(path.clone()).await.unwrap();
        store
            .log(
                "live-42",
                42,
                &vec![serde_json::json!({"issued_lap":3,"target_lap":4})],
            )
            .await
            .unwrap();
        drop(store);
        let restarted = Store::local(path.clone()).await.unwrap();
        let records: Vec<serde_json::Value> = restarted.load_log("live-42").await.unwrap().unwrap();
        assert_eq!(
            records,
            vec![serde_json::json!({"issued_lap":3,"target_lap":4})]
        );
        assert!(restarted
            .load_log::<serde_json::Value>("../invalid")
            .await
            .is_err());
        tokio::fs::remove_file(path.join("forecast-live-42.json"))
            .await
            .unwrap();
        tokio::fs::remove_dir(path).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL and Redis from docker compose"]
    async fn rest_repairs_are_durable_without_inventing_source_revisions() {
        let key = 5_000_000 + std::process::id();
        let path = std::env::temp_dir().join(format!("agon-rest-repair-{key}"));
        let store = Store::connect(path.clone()).await.unwrap();
        let row = serde_json::json!({"session_key":key,"driver_number":1,"position":3,"date":"2026-11-01T10:00:00Z"});
        let received = chrono::Utc::now().timestamp_millis();
        store
            .append_rest(key, "position", std::slice::from_ref(&row), 1000, received)
            .await
            .unwrap();
        store
            .append_rest(key, "position", std::slice::from_ref(&row), 1000, received)
            .await
            .unwrap();
        let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap())
            .await
            .unwrap();
        let rows = sqlx::query_as::<_,(Option<String>,Option<i64>,serde_json::Value,bool)>("SELECT source_key,source_id,arrival,accepted FROM agon_live_arrivals WHERE session_key=$1 ORDER BY sequence")
            .bind(i64::from(key)).fetch_all(&pool).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, None);
        assert_eq!(rows[0].1, None);
        assert_eq!(rows[0].2["document"], row);
        assert!(rows[0].3);
        let mqtt = |revision, received| {
            LiveArrival::decode("position",&serde_json::json!({"session_key":key,"_key":"driver-1","_id":revision,"driver_number":1,"position":2,"date":"2026-11-01T10:00:01Z"}),key,received).unwrap()
        };
        store.append_live(&[mqtt(20, 2000)]).await.unwrap();
        store
            .append_rest(key, "position", &[row], 1500, received + 1)
            .await
            .unwrap();
        let accepted = sqlx::query_scalar::<_,bool>("SELECT accepted FROM agon_live_arrivals WHERE session_key=$1 ORDER BY sequence DESC LIMIT 1").bind(i64::from(key)).fetch_one(&pool).await.unwrap();
        assert!(
            !accepted,
            "REST request started before MQTT update must not overwrite it"
        );
        let stale = mqtt(19, 3000);
        store
            .append_rest(key, "position", &[stale.document], 3000, received + 2)
            .await
            .unwrap();
        assert!(
            !store
                .live_after(key, 0)
                .await
                .unwrap()
                .last()
                .unwrap()
                .accepted
        );
        assert!(store
            .append_rest(
                key,
                "position",
                &vec![serde_json::json!({}); 257],
                4000,
                received + 3
            )
            .await
            .is_err());
        assert!(store
            .append_rest(
                key,
                "position",
                &[serde_json::json!({"session_key":key+1})],
                4000,
                received + 3
            )
            .await
            .is_err());
        for table in ["agon_live_arrivals", "agon_live_events"] {
            sqlx::query(&format!("DELETE FROM {table} WHERE session_key=$1"))
                .bind(i64::from(key))
                .execute(&pool)
                .await
                .unwrap();
        }
        tokio::fs::remove_dir(path).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL and Redis from docker compose"]
    async fn captured_replay_keeps_bootstrap_and_delays_corrections_until_receipt() {
        let key = 4_000_000 + std::process::id();
        let path = std::env::temp_dir().join(format!("agon-captured-replay-{key}"));
        let store = Store::connect(path.clone()).await.unwrap();
        let captured = chrono::Utc::now().timestamp_millis() - 5000;
        let mut bootstrap = pitwall_state::demo_archive();
        bootstrap.session.key = key;
        bootstrap.session.start_ms = captured - 1000;
        bootstrap.session.end_ms = captured + 10000;
        bootstrap.captured_at_ms = Some(captured);
        bootstrap.events = vec![RaceEvent {
            time_ms: captured - 100,
            source_time_ms: Some(captured - 100),
            driver: Some(1),
            payload: EventData::Position(2),
        }];
        store.save_live_bootstrap(&bootstrap).await.unwrap();
        let mut revised_bootstrap = bootstrap.clone();
        revised_bootstrap.events[0].payload = EventData::Position(9);
        store.save_live_bootstrap(&revised_bootstrap).await.unwrap();
        let row = serde_json::json!({"session_key":key,"_key":"lap-1","_id":20,"driver_number":1,"lap_number":1,"date_start":chrono::DateTime::from_timestamp_millis(captured-91000).unwrap().to_rfc3339(),"lap_duration":90.0});
        let arrival = LiveArrival::decode("laps", &row, key, captured + 1000).unwrap();
        store.append_live(&[arrival]).await.unwrap();
        let archive = store.load_live_archive(key).await.unwrap().unwrap();
        let mut replay = pitwall_state::Replay::new(std::sync::Arc::new(archive));
        replay.seek(captured + 999);
        assert_eq!(replay.state().view().drivers[&1].position, Some(2));
        assert!(replay.state().view().drivers[&1].laps.is_empty());
        replay.advance(captured + 1000);
        assert_eq!(
            replay.state().view().drivers[&1].laps[0].duration,
            Some(90.0)
        );
        assert_eq!(
            replay.state().view().drivers[&1].laps[0].time_ms,
            captured - 1000
        );
        let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap())
            .await
            .unwrap();
        for table in [
            "agon_live_arrivals",
            "agon_live_events",
            "agon_live_bootstraps",
        ] {
            sqlx::query(&format!("DELETE FROM {table} WHERE session_key=$1"))
                .bind(i64::from(key))
                .execute(&pool)
                .await
                .unwrap();
        }
        tokio::fs::remove_dir(path).await.unwrap();
    }
}
mod export;
