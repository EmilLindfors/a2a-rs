-- v0.5.0 Migration, part 1 of 2: make `task_history` addressable by context.
--
-- `task_history` is already the conversation event log — append-only, ordered by
-- its autoincrement id, one full Message per row. It just could not be read by
-- context, only by task. A task's context never changes, so denormalizing the
-- column here is safe and avoids joining `tasks` on the hottest read in the
-- system (rebuilding a conversation for the model on every turn).
--
-- Kept in its own file because SQLite cannot express ADD COLUMN idempotently;
-- see `run_base_migrations`, which tolerates the duplicate-column error and
-- backfills only when the column was actually added.

ALTER TABLE task_history ADD COLUMN context_id TEXT;
