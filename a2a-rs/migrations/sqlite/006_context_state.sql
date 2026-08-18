-- v0.7.0 Migration: the state bag.
--
-- Every statement here is idempotent, so this file re-runs on each `new()` with
-- the rest of the base migrations.

-- The facts an agent was asked to remember, apart from what was said.
--
-- One row per key rather than a JSON document per context. Two turns of one
-- conversation can run at once — the same reason `context_digests` is
-- append-only — and a read-modify-write over a document loses whichever write
-- lands second, silently. An upsert per key has nothing to lose.
--
-- `scope_key` is a context id for `scope = 'context'` and an authenticated
-- principal for `scope = 'user'`, which is what makes a `user:` key readable
-- from a context that principal has not opened yet. Hence no foreign key to
-- `contexts`: half these rows are not about a context at all.
--
-- `temp:` keys never reach this table. That is the whole content of the scope.
CREATE TABLE IF NOT EXISTS context_state (
    scope      TEXT NOT NULL,
    scope_key  TEXT NOT NULL,
    name       TEXT NOT NULL,
    value      TEXT NOT NULL,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (scope, scope_key, name)
);
