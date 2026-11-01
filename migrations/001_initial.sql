-- Additive migration; no automatic destructive reset.
CREATE TABLE IF NOT EXISTS agon_archives (
    session_key BIGINT PRIMARY KEY,
    archive JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS agon_forecast_runs (
    run_id TEXT PRIMARY KEY,
    session_key BIGINT NOT NULL,
    records JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS agon_forecasts_session ON agon_forecast_runs(session_key);
CREATE TABLE IF NOT EXISTS agon_live_events (
    session_key BIGINT NOT NULL,
    received_at TIMESTAMPTZ NOT NULL,
    source_topic TEXT NOT NULL,
    source_key TEXT NOT NULL,
    source_id BIGINT NOT NULL,
    event JSONB NOT NULL,
    PRIMARY KEY(session_key,source_topic,source_key)
);
