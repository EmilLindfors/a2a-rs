-- Initial schema for A2A task storage, PostgreSQL dialect.
--
-- The sibling under `../sqlite/` is the same schema in SQLite's spelling. Two
-- files rather than one, because the differences are not cosmetic: identity
-- columns, the `updated_at` trigger and the timestamp default all have to be
-- written per backend.
--
-- JSON payloads are TEXT here, not JSONB. The store reads and writes them as
-- serialized strings and never queries into them, and the runtime driver
-- (sqlx's `Any`) decodes text, integers, floats, booleans and bytes only — a
-- JSONB column would have to be cast on the way in and would fail to decode on
-- the way out. One query path across both backends is worth more than operators
-- nothing uses.

CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    context_id TEXT NOT NULL,
    status_state TEXT NOT NULL CHECK (status_state IN ('submitted', 'working', 'input-required', 'completed', 'canceled', 'failed', 'rejected', 'auth-required', 'unknown')),
    status_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    metadata TEXT,
    artifacts TEXT
);

-- `id` is the sequence number the conversation log is ordered by, and it is read
-- back as a 64-bit integer — hence BIGSERIAL rather than SERIAL.
CREATE TABLE IF NOT EXISTS task_history (
    id BIGSERIAL PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT now(),
    status_state TEXT NOT NULL CHECK (status_state IN ('submitted', 'working', 'input-required', 'completed', 'canceled', 'failed', 'rejected', 'auth-required', 'unknown')),
    message TEXT
);

-- The v0.2 shape, replaced by migration 002. Created here so a database that
-- has never seen this schema follows the same path as one being upgraded.
CREATE TABLE IF NOT EXISTS push_notification_configs (
    task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    webhook_url TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_tasks_context_id ON tasks(context_id);
CREATE INDEX IF NOT EXISTS idx_tasks_created_at ON tasks(created_at);
CREATE INDEX IF NOT EXISTS idx_tasks_status_state ON tasks(status_state);
CREATE INDEX IF NOT EXISTS idx_task_history_task_id ON task_history(task_id);
CREATE INDEX IF NOT EXISTS idx_task_history_timestamp ON task_history(timestamp);

-- One function for every `updated_at` column in this schema. `BEFORE UPDATE`
-- rather than SQLite's `AFTER UPDATE` plus a second UPDATE: PostgreSQL can
-- amend the row on its way to disk, so there is no recursion to guard against.
CREATE OR REPLACE FUNCTION a2a_touch_updated_at() RETURNS trigger AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- PostgreSQL has no `CREATE TRIGGER IF NOT EXISTS`, and these migrations re-run
-- on every startup, so each trigger is dropped and recreated.
DROP TRIGGER IF EXISTS update_tasks_updated_at ON tasks;
CREATE TRIGGER update_tasks_updated_at
    BEFORE UPDATE ON tasks
    FOR EACH ROW
    EXECUTE FUNCTION a2a_touch_updated_at();
