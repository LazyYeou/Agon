-- REST responses have receipt sequences, but no invented OpenF1 document revision.
ALTER TABLE agon_live_arrivals ALTER COLUMN source_key DROP NOT NULL;
ALTER TABLE agon_live_arrivals ALTER COLUMN source_id DROP NOT NULL;
CREATE INDEX IF NOT EXISTS agon_live_rest_receipts ON agon_live_arrivals(session_key,source_topic,received_at) WHERE source_id IS NULL;
CREATE TABLE IF NOT EXISTS agon_live_bootstraps (
    session_key BIGINT PRIMARY KEY,
    archive JSONB NOT NULL
);
