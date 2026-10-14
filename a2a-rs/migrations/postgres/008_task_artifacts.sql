-- v0.11.0 Migration: a task's artifacts, PostgreSQL dialect.
--
-- Every statement here is idempotent, so this file re-runs on each `new()` with
-- the rest of the base migrations.

-- What a task produced, one row per artifact, written as each one finishes.
--
-- Rows rather than the `artifacts` column on `tasks`, which only `create`
-- ever wrote. A column would be read, changed and written back whole for every
-- artifact, and two artifacts finishing at once would each write over the
-- other. A row is one upsert on (task_id, artifact_id). The column is left in
-- place and no longer read.
--
-- `seq` is the order an artifact was first written in. A replaced artifact
-- keeps its place.
CREATE TABLE IF NOT EXISTS task_artifacts (
    task_id     TEXT   NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    artifact_id TEXT   NOT NULL,
    seq         BIGINT NOT NULL,
    body        TEXT   NOT NULL,
    PRIMARY KEY (task_id, artifact_id)
);
