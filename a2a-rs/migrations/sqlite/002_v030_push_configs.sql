-- v0.3.0 Migration: several push notification configs per task, SQLite dialect.
--
-- The v0.2 table this replaces is dropped by `run_base_migrations`, and only
-- when it is still there — the drop used to live in this file, which re-runs on
-- every startup, so every restart destroyed the configs it had stored.
CREATE TABLE IF NOT EXISTS push_notification_configs (
    id TEXT PRIMARY KEY,  -- Unique config ID
    task_id TEXT NOT NULL,  -- Task this config belongs to
    url TEXT NOT NULL,  -- Webhook URL
    token TEXT,  -- Optional authentication token
    authentication JSONB,  -- Optional authentication scheme (OAuth2, etc.)
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT (datetime('now')),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
);

-- Index for efficient lookups
CREATE INDEX IF NOT EXISTS idx_push_configs_task_id ON push_notification_configs(task_id);

-- Trigger to automatically update the updated_at timestamp
CREATE TRIGGER IF NOT EXISTS update_push_configs_updated_at
    AFTER UPDATE ON push_notification_configs
    FOR EACH ROW
BEGIN
    UPDATE push_notification_configs SET updated_at = datetime('now') WHERE id = NEW.id;
END;
