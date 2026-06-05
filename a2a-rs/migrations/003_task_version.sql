-- v0.4.0 Migration: add an optimistic-concurrency version column to tasks.
-- The version is a monotonic counter bumped on every task mutation; conditional
-- updates (AsyncTaskVersioning::update_status_checked) compare it to detect and
-- reject lost updates.

ALTER TABLE tasks ADD COLUMN version INTEGER NOT NULL DEFAULT 1;
