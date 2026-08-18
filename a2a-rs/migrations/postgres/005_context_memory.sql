-- v0.5.0 Migration, part 2 of 2: conversation memory, PostgreSQL dialect.
--
-- Every statement here is idempotent, so this file re-runs on each `new()` with
-- the rest of the base migrations.

-- Reads the conversation for one context in insertion order. `task_history.id`
-- is the sequence number and the digest watermark, so this index serves both
-- "everything in this context" and "everything after the last summary".
CREATE INDEX IF NOT EXISTS idx_task_history_context_seq
    ON task_history(context_id, id);

-- A conversation as an entity, rather than a column repeated across tasks.
--
-- `owner` is the authenticated principal that first wrote to the context. NULL
-- means unowned — an agent running without an authenticator — and stays
-- readable by anyone. A non-NULL owner is enforced on every read, because
-- projecting a conversation into a prompt turns `context_id` into a capability:
-- whoever holds one would otherwise read what was said in it.
-- This table carried a `state TEXT` column that nothing read or wrote. The
-- state bag went to its own table in 006, one row per key; 006 also drops the
-- column from a database that already has it.
CREATE TABLE IF NOT EXISTS contexts (
    id         TEXT PRIMARY KEY,
    owner      TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Compaction appends here and deletes nothing. `covers_through_seq` is the
-- highest `task_history.id` folded into `summary`; loading a conversation takes
-- the row with the highest watermark and reads the log after it.
--
-- Append-only is what makes concurrent compaction safe: two turns in one
-- context can both summarize, both rows land, the higher watermark wins, and
-- the loser is duplicated work rather than a corrupted transcript.
CREATE TABLE IF NOT EXISTS context_digests (
    id                 BIGSERIAL PRIMARY KEY,
    context_id         TEXT NOT NULL REFERENCES contexts(id) ON DELETE CASCADE,
    covers_through_seq BIGINT NOT NULL,
    summary            TEXT NOT NULL,
    replaced_messages  BIGINT NOT NULL DEFAULT 0,
    model              TEXT NOT NULL DEFAULT '',
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_context_digests_watermark
    ON context_digests(context_id, covers_through_seq DESC);

DROP TRIGGER IF EXISTS update_contexts_updated_at ON contexts;
CREATE TRIGGER update_contexts_updated_at
    BEFORE UPDATE ON contexts
    FOR EACH ROW
    EXECUTE FUNCTION a2a_touch_updated_at();
