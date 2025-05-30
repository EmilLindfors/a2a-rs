-- Initial schema for A2A task storage
-- This creates the core tables needed for persistent task storage

-- Tasks table - stores main task information
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    context_id TEXT NOT NULL,
    status_state TEXT NOT NULL CHECK (status_state IN ('submitted', 'working', 'input-required', 'completed', 'canceled', 'failed', 'rejected', 'auth-required', 'unknown')),
    status_message TEXT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT (datetime('now')),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT (datetime('now')),
    metadata JSONB,
    artifacts JSONB
);

-- Task history table - stores chronological task updates
CREATE TABLE IF NOT EXISTS task_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL,
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT (datetime('now')),
    status_state TEXT NOT NULL CHECK (status_state IN ('submitted', 'working', 'input-required', 'completed', 'canceled', 'failed', 'rejected', 'auth-required', 'unknown')),
    message JSONB,
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
);

-- Push notification configurations
CREATE TABLE IF NOT EXISTS push_notification_configs (
    task_id TEXT PRIMARY KEY,
    webhook_url TEXT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
);

-- Indexes for better query performance
CREATE INDEX IF NOT EXISTS idx_tasks_context_id ON tasks(context_id);
CREATE INDEX IF NOT EXISTS idx_tasks_created_at ON tasks(created_at);
CREATE INDEX IF NOT EXISTS idx_tasks_status_state ON tasks(status_state);
CREATE INDEX IF NOT EXISTS idx_task_history_task_id ON task_history(task_id);
CREATE INDEX IF NOT EXISTS idx_task_history_timestamp ON task_history(timestamp);

-- Trigger to automatically update the updated_at timestamp
CREATE TRIGGER IF NOT EXISTS update_tasks_updated_at 
    AFTER UPDATE ON tasks
    FOR EACH ROW
BEGIN
    UPDATE tasks SET updated_at = datetime('now') WHERE id = NEW.id;
END;