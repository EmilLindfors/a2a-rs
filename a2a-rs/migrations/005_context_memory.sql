-- v0.5.0 Migration, part 2 of 2: conversation memory.
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
CREATE TABLE IF NOT EXISTS contexts (
    id         TEXT PRIMARY KEY,
    owner      TEXT,
    state      TEXT NOT NULL DEFAULT '{}',
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT (datetime('now')),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT (datetime('now'))
);

-- Compaction appends here and deletes nothing. `covers_through_seq` is the
-- highest `task_history.id` folded into `summary`; loading a conversation takes
-- the row with the highest watermark and reads the log after it.
--
-- Append-only is what makes concurrent compaction safe: two turns in one
-- context can both summarize, both rows land, the higher watermark wins, and
-- the loser is duplicated work rather than a corrupted transcript.
CREATE TABLE IF NOT EXISTS context_digests (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    context_id         TEXT NOT NULL,
    covers_through_seq INTEGER NOT NULL,
    summary            TEXT NOT NULL,
    replaced_messages  INTEGER NOT NULL DEFAULT 0,
    model              TEXT NOT NULL DEFAULT '',
    created_at         TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (context_id) REFERENCES contexts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_context_digests_watermark
    ON context_digests(context_id, covers_through_seq DESC);

CREATE TRIGGER IF NOT EXISTS update_contexts_updated_at
    AFTER UPDATE ON contexts
    FOR EACH ROW
BEGIN
    UPDATE contexts SET updated_at = datetime('now') WHERE id = NEW.id;
END;
