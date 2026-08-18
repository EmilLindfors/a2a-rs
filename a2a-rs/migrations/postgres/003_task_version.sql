-- v0.4.0 Migration: add an optimistic-concurrency version column to tasks,
-- PostgreSQL dialect.
--
-- The version is a monotonic counter bumped on every task mutation; conditional
-- updates (AsyncTaskVersioning::update_status_checked) compare it to detect and
-- reject lost updates.
--
-- BIGINT rather than INTEGER: the column is read back as a 64-bit integer, and
-- PostgreSQL's INTEGER is 32 bits, which the driver refuses to widen.

ALTER TABLE tasks ADD COLUMN IF NOT EXISTS version BIGINT NOT NULL DEFAULT 1;
