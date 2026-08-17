-- v0.5.0 Migration, part 1 of 2: make `task_history` addressable by context,
-- PostgreSQL dialect.
--
-- `task_history` is already the conversation event log — append-only, ordered by
-- its identity column, one full Message per row. It just could not be read by
-- context, only by task. A task's context never changes, so denormalizing the
-- column here is safe and avoids joining `tasks` on the hottest read in the
-- system (rebuilding a conversation for the model on every turn).
--
-- The backfill lives in `run_base_migrations`, guarded by `context_id IS NULL`
-- so it costs nothing on a database that has already run it.

ALTER TABLE task_history ADD COLUMN IF NOT EXISTS context_id TEXT;
