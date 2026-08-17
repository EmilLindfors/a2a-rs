-- v0.3.0 Migration: several push notification configs per task, PostgreSQL
-- dialect.
--
-- The v0.2 table this replaces is dropped by `run_base_migrations`, and only
-- when it is still there — the drop used to live in this file, which re-runs on
-- every startup, so every restart destroyed the configs it had stored.

CREATE TABLE IF NOT EXISTS push_notification_configs (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    url TEXT NOT NULL,
    token TEXT,
    authentication TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_push_configs_task_id ON push_notification_configs(task_id);

DROP TRIGGER IF EXISTS update_push_configs_updated_at ON push_notification_configs;
CREATE TRIGGER update_push_configs_updated_at
    BEFORE UPDATE ON push_notification_configs
    FOR EACH ROW
    EXECUTE FUNCTION a2a_touch_updated_at();
