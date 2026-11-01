-- Retain every distinct source revision, including rejected stale revisions.
-- agon_live_events remains the latest revision index; no existing rows are removed.
CREATE TABLE IF NOT EXISTS agon_live_arrivals (
    sequence BIGSERIAL PRIMARY KEY,
    session_key BIGINT NOT NULL,
    received_at TIMESTAMPTZ NOT NULL,
    source_topic TEXT NOT NULL,
    source_key TEXT NOT NULL,
    source_id BIGINT NOT NULL,
    arrival JSONB NOT NULL,
    accepted BOOLEAN NOT NULL,
    UNIQUE(session_key, source_topic, source_key, source_id)
);
CREATE INDEX IF NOT EXISTS agon_live_arrivals_resume ON agon_live_arrivals(session_key, sequence);
ALTER TABLE agon_checkpoints ADD COLUMN IF NOT EXISTS live_watermark BIGINT NOT NULL DEFAULT 0;
