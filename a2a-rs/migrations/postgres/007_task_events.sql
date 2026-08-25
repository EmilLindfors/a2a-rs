-- v0.7.0 Migration: the durable stream log, PostgreSQL dialect.
--
-- Every statement here is idempotent, so this file re-runs on each `new()` with
-- the rest of the base migrations.

-- What a task's update stream already said, so a client that reconnects can be
-- told the part it missed — including after the server that said it restarted.
--
-- `id` is per task and starts at 1, which is the id a client echoes back in
-- `Last-Event-ID`. It is assigned from this table rather than from a counter in
-- the process, because a counter starts again at 1 after a restart and hands
-- out ids a resuming client has already seen. The composite primary key is what
-- makes that assignment safe: two writers computing the same next id collide
-- here instead of both claiming it.
--
-- No foreign key to `tasks`. The streaming handler and the task store are
-- separate ports and need not be the same database, and an append that failed
-- because the task row had not landed yet would drop an event on the one path
-- whose job is not to drop events. Sweeping is explicit instead: a retention
-- sweep deletes a context's events with the rest of it.
CREATE TABLE IF NOT EXISTS task_events (
    task_id    TEXT   NOT NULL,
    id         BIGINT NOT NULL,
    kind       TEXT   NOT NULL,
    payload    TEXT   NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (task_id, id)
);
