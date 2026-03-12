-- v0.3.0 Migration: Update push notification configs to support multiple configs per task
-- PostgreSQL version
-- This migration enhances the push_notification_configs table to support the v0.3.0 spec

-- Drop the old table (backing up data if needed in production)
DROP TABLE IF EXISTS push_notification_configs;

-- Create new table with support for multiple configs per task
CREATE TABLE IF NOT EXISTS push_notification_configs (
    id TEXT PRIMARY KEY,  -- Unique config ID
    task_id TEXT NOT NULL,  -- Task this config belongs to
    url TEXT NOT NULL,  -- Webhook URL
    token TEXT,  -- Optional authentication token
    authentication JSONB,  -- Optional authentication scheme (OAuth2, etc.)
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
);

-- Index for efficient lookups
CREATE INDEX IF NOT EXISTS idx_push_configs_task_id ON push_notification_configs(task_id);

-- Trigger to automatically update the updated_at timestamp
CREATE OR REPLACE FUNCTION update_push_configs_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

DROP TRIGGER IF EXISTS update_push_configs_updated_at ON push_notification_configs;
CREATE TRIGGER update_push_configs_updated_at
    BEFORE UPDATE ON push_notification_configs
    FOR EACH ROW
    EXECUTE FUNCTION update_push_configs_updated_at_column();
