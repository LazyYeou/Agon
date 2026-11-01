//! Private archives and forecast logs, with atomic files locally and PostgreSQL in deployment.
use anyhow::{Context, Result};
use pitwall_core::*;
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tokio::sync::Mutex;

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
            sqlx::raw_sql(include_str!("../../../migrations/001_initial.sql"))
                .execute(&pool)
                .await?;
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
    pub async fn snapshot(&self, snapshot: &Snapshot) -> Result<()> {
        if let Some(client) = &self.redis {
            let mut conn = client.get_multiplexed_async_connection().await?;
            let value = serde_json::to_string(snapshot)?;
            redis::cmd("SET")
                .arg(format!("agon:snapshot:{}", snapshot.session.key))
                .arg(&value)
                .arg("EX")
                .arg(120)
                .query_async::<()>(&mut conn)
                .await?;
            redis::cmd("PUBLISH")
                .arg(format!("agon:session:{}", snapshot.session.key))
                .arg(value)
                .query_async::<i64>(&mut conn)
                .await?;
        }
        Ok(())
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
