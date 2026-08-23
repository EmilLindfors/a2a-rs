//! Where SQLite and PostgreSQL disagree, for the one storage adapter that
//! serves both.
//!
//! [`SqlxTaskStorage`](super::SqlxTaskStorage) runs on sqlx's `Any` driver, so
//! one set of queries reaches either backend — but `Any` passes SQL through
//! verbatim, and the two dialects do not spell everything the same way. This
//! module holds every difference: parameter placeholders, the two upserts, the
//! timestamp comparison, and which migration files to run.
//!
//! Anything not here is identical on both, which is the point. A second copy of
//! the queries is how two backends drift apart.

use std::borrow::Cow;

use super::database_config::DatabaseType;

/// Which SQL dialect this store is talking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Dialect {
    Sqlite,
    Postgres,
}

/// One migration file in one dialect's spelling.
///
/// `Copy`, so the runner can take it by value: an `async fn` that borrows an
/// argument returns a future generic over that borrow, and this one is awaited
/// deep inside futures that get spawned.
#[derive(Debug, Clone, Copy)]
pub(super) struct Migration {
    /// Reported when it fails, so the number in the message matches a file name.
    pub name: &'static str,
    pub sql: &'static str,
    /// Whether re-running this file on an already-migrated database reports the
    /// column as already existing. SQLite cannot write `ALTER TABLE ADD COLUMN`
    /// idempotently and these files re-run on every startup, so that error is
    /// the expected outcome rather than a failure.
    pub tolerates_existing_column: bool,
}

impl Dialect {
    pub(super) fn of(database_type: DatabaseType) -> Option<Self> {
        match database_type {
            DatabaseType::Sqlite => Some(Self::Sqlite),
            DatabaseType::Postgres => Some(Self::Postgres),
            DatabaseType::Mysql => None,
        }
    }

    /// Rewrite `?` placeholders into `$1..$n` for PostgreSQL.
    ///
    /// The queries in this adapter are written in SQLite's spelling and none of
    /// them contains a `?` inside a string literal, which is what makes a scan
    /// this simple correct. A query that needs one has to be a
    /// [`Dialect`] method instead.
    pub(super) fn bind_params<'a>(self, sql: &'a str) -> Cow<'a, str> {
        match self {
            Self::Sqlite => Cow::Borrowed(sql),
            Self::Postgres => {
                let mut out = String::with_capacity(sql.len() + 8);
                let mut next = 1;
                for ch in sql.chars() {
                    if ch == '?' {
                        out.push('$');
                        out.push_str(&next.to_string());
                        next += 1;
                    } else {
                        out.push(ch);
                    }
                }
                Cow::Owned(out)
            }
        }
    }

    /// Claim a context for an owner, leaving an existing claim alone.
    ///
    /// The whole statement per dialect rather than an "insert or ignore"
    /// fragment: PostgreSQL puts the conflict clause at the end and names the
    /// conflicting column, so there is no shared skeleton to fill in.
    pub(super) fn insert_context_if_absent(self) -> &'static str {
        match self {
            Self::Sqlite => "INSERT OR IGNORE INTO contexts (id, owner) VALUES (?, ?)",
            Self::Postgres => {
                "INSERT INTO contexts (id, owner) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING"
            }
        }
    }

    /// Write a push notification config, replacing one already stored under the
    /// same id.
    pub(super) fn upsert_push_config(self) -> &'static str {
        match self {
            Self::Sqlite => {
                "INSERT OR REPLACE INTO push_notification_configs \
                 (id, task_id, url, token, authentication) VALUES (?, ?, ?, ?, ?)"
            }
            Self::Postgres => {
                "INSERT INTO push_notification_configs \
                 (id, task_id, url, token, authentication) VALUES ($1, $2, $3, $4, $5) \
                 ON CONFLICT (id) DO UPDATE SET \
                 task_id = EXCLUDED.task_id, url = EXCLUDED.url, token = EXCLUDED.token, \
                 authentication = EXCLUDED.authentication"
            }
        }
    }

    /// Write one remembered value, replacing what the key held.
    ///
    /// A row per key, so two turns of one conversation writing different keys
    /// do not overwrite each other — which a read-modify-write over a JSON
    /// document would, without saying so.
    pub(super) fn upsert_context_state(self) -> &'static str {
        match self {
            Self::Sqlite => {
                "INSERT INTO context_state (scope, scope_key, name, value, updated_at) \
                 VALUES (?, ?, ?, ?, datetime('now')) \
                 ON CONFLICT (scope, scope_key, name) DO UPDATE SET \
                 value = excluded.value, updated_at = datetime('now')"
            }
            Self::Postgres => {
                "INSERT INTO context_state (scope, scope_key, name, value, updated_at) \
                 VALUES ($1, $2, $3, $4, now()) \
                 ON CONFLICT (scope, scope_key, name) DO UPDATE SET \
                 value = EXCLUDED.value, updated_at = now()"
            }
        }
    }

    /// Append one event to a task's log, taking the next per-task id and
    /// handing it back.
    ///
    /// The id is computed inside the insert rather than read first and bound:
    /// a read-then-write leaves a window where two appends pick the same id,
    /// and this way the composite primary key is what settles a collision.
    /// `RETURNING` is what makes the assigned id observable without a second
    /// round trip; both backends support it.
    pub(super) fn insert_task_event(self) -> &'static str {
        match self {
            Self::Sqlite => {
                "INSERT INTO task_events (task_id, id, kind, payload)                  SELECT ?, COALESCE(MAX(id), 0) + 1, ?, ? FROM task_events                  WHERE task_id = ? RETURNING id"
            }
            Self::Postgres => {
                "INSERT INTO task_events (task_id, id, kind, payload)                  SELECT $1, COALESCE(MAX(id), 0) + 1, $2, $3 FROM task_events                  WHERE task_id = $4 RETURNING id"
            }
        }
    }

    /// Does `contexts` still carry the unused `state` column?
    ///
    /// Migration 005 created it for a state bag that was never written; 006
    /// gave the bag its own table. The probe makes the drop happen once, on a
    /// database old enough to have the column.
    pub(super) fn dead_context_state_column_probe(self) -> &'static str {
        match self {
            Self::Sqlite => {
                "SELECT 1 AS found FROM pragma_table_info('contexts') WHERE name = 'state'"
            }
            Self::Postgres => {
                "SELECT 1 AS found FROM information_schema.columns \
                 WHERE table_name = 'contexts' AND column_name = 'state'"
            }
        }
    }

    /// Filter tasks by when they were last updated.
    ///
    /// SQLite keeps its timestamps as text and compares them as text.
    /// PostgreSQL has a real timestamp column, and the driver binds every string
    /// as text, so the parameter has to be cast before it can be compared. Pair
    /// with [`format_timestamp`](Self::format_timestamp), which writes the
    /// spelling each side expects.
    pub(super) fn updated_since_predicate(self) -> &'static str {
        match self {
            Self::Sqlite => "updated_at >= ?",
            Self::Postgres => "updated_at >= ?::timestamptz",
        }
    }

    /// Render a timestamp the way [`updated_since_predicate`](Self::updated_since_predicate)
    /// expects to receive it.
    pub(super) fn format_timestamp(self, at: chrono::DateTime<chrono::Utc>) -> String {
        match self {
            // What `datetime('now')` writes, which is what the stored values are
            // compared against character by character.
            Self::Sqlite => at.format("%Y-%m-%d %H:%M:%S").to_string(),
            // Explicit offset, so the cast does not fall back to the server's
            // time zone.
            Self::Postgres => at.to_rfc3339(),
        }
    }

    /// Context ids whose last write is older than the bound cutoff and that hold
    /// no unfinished task.
    ///
    /// "Last write" is the newest timestamp any table carries for the context,
    /// which is why this is a `UNION ALL` rather than a read of
    /// `contexts.updated_at`: nothing updates that row after the claim, and a
    /// context that only ever held tasks has no row there at all.
    ///
    /// The `EXCEPT` is the running-task guard, kept out of the `HAVING` so
    /// neither dialect has to resolve a correlated reference to the grouped
    /// derived table. `input-required` and `auth-required` are deliberately not
    /// in the list — they wait on a caller who, past the retention window, is
    /// not coming back.
    pub(super) fn idle_contexts(self) -> &'static str {
        match self {
            Self::Sqlite => concat!(
                "SELECT ctx FROM (",
                "SELECT id AS ctx, updated_at AS last_write FROM contexts",
                " UNION ALL SELECT context_id, updated_at FROM tasks",
                " UNION ALL SELECT context_id, \"timestamp\" FROM task_history",
                " WHERE context_id IS NOT NULL",
                " UNION ALL SELECT context_id, created_at FROM context_digests",
                " UNION ALL SELECT scope_key, updated_at FROM context_state",
                " WHERE scope = 'context'",
                ") AS activity GROUP BY ctx HAVING MAX(last_write) < ?",
                " EXCEPT SELECT context_id FROM tasks",
                " WHERE status_state IN ('submitted', 'working', 'unknown')",
            ),
            Self::Postgres => concat!(
                "SELECT ctx FROM (",
                "SELECT id AS ctx, updated_at AS last_write FROM contexts",
                " UNION ALL SELECT context_id, updated_at FROM tasks",
                " UNION ALL SELECT context_id, \"timestamp\" FROM task_history",
                " WHERE context_id IS NOT NULL",
                " UNION ALL SELECT context_id, created_at FROM context_digests",
                " UNION ALL SELECT scope_key, updated_at FROM context_state",
                " WHERE scope = 'context'",
                ") AS activity GROUP BY ctx HAVING MAX(last_write) < $1::timestamptz",
                " EXCEPT SELECT context_id FROM tasks",
                " WHERE status_state IN ('submitted', 'working', 'unknown')",
            ),
        }
    }

    /// Principals whose `user:`-scoped state has not been written since the
    /// bound cutoff.
    ///
    /// Grouped by principal rather than filtered per row: the bag is expired
    /// whole or not at all (see
    /// [`RetentionPolicy::delete_user_state_idle_for`](crate::domain::RetentionPolicy::delete_user_state_idle_for)).
    pub(super) fn idle_principals(self) -> &'static str {
        match self {
            Self::Sqlite => {
                "SELECT scope_key FROM context_state WHERE scope = 'user'                  GROUP BY scope_key HAVING MAX(updated_at) < ?"
            }
            Self::Postgres => {
                "SELECT scope_key FROM context_state WHERE scope = 'user'                  GROUP BY scope_key HAVING MAX(updated_at) < $1::timestamptz"
            }
        }
    }

    /// Keep other processes out while the schema is created, if this backend has
    /// anything to keep them out with.
    ///
    /// SQLite serializes writers itself and a file database is normally one
    /// process's. PostgreSQL is shared on purpose, so several agents starting
    /// together is the normal case, and concurrent DDL on related tables
    /// deadlocks rather than no-opping. Taken on the migration pool, which holds
    /// exactly one connection — an advisory lock belongs to a session, and it is
    /// released when that pool closes. The key is arbitrary and constant; it only
    /// has to be the same number in every process running these migrations.
    pub(super) fn migration_lock(self) -> Option<&'static str> {
        match self {
            Self::Sqlite => None,
            Self::Postgres => Some("SELECT pg_advisory_lock(7723510643218)"),
        }
    }

    /// Does the v0.2 push-config table still exist?
    ///
    /// Answered by the legacy `webhook_url` column, which migration 002 replaces.
    /// A database that has already been through 002 must not have its stored
    /// configs dropped, which is what re-running the drop on every startup did.
    pub(super) fn legacy_push_config_probe(self) -> &'static str {
        match self {
            Self::Sqlite => {
                "SELECT 1 AS found FROM pragma_table_info('push_notification_configs') \
                 WHERE name = 'webhook_url'"
            }
            Self::Postgres => {
                "SELECT 1 AS found FROM information_schema.columns \
                 WHERE table_name = 'push_notification_configs' AND column_name = 'webhook_url'"
            }
        }
    }

    /// The base migrations, in order.
    pub(super) fn migrations(self) -> [Migration; 7] {
        match self {
            Self::Sqlite => [
                Migration {
                    name: "001_initial_schema",
                    sql: include_str!("../../../migrations/sqlite/001_initial_schema.sql"),
                    tolerates_existing_column: false,
                },
                Migration {
                    name: "002_v030_push_configs",
                    sql: include_str!("../../../migrations/sqlite/002_v030_push_configs.sql"),
                    tolerates_existing_column: false,
                },
                Migration {
                    name: "003_task_version",
                    sql: include_str!("../../../migrations/sqlite/003_task_version.sql"),
                    tolerates_existing_column: true,
                },
                Migration {
                    name: "004_task_history_context",
                    sql: include_str!("../../../migrations/sqlite/004_task_history_context.sql"),
                    tolerates_existing_column: true,
                },
                Migration {
                    name: "005_context_memory",
                    sql: include_str!("../../../migrations/sqlite/005_context_memory.sql"),
                    tolerates_existing_column: false,
                },
                Migration {
                    name: "006_context_state",
                    sql: include_str!("../../../migrations/sqlite/006_context_state.sql"),
                    tolerates_existing_column: false,
                },
                Migration {
                    name: "007_task_events",
                    sql: include_str!("../../../migrations/sqlite/007_task_events.sql"),
                    tolerates_existing_column: false,
                },
            ],
            Self::Postgres => [
                Migration {
                    name: "001_initial_schema",
                    sql: include_str!("../../../migrations/postgres/001_initial_schema.sql"),
                    tolerates_existing_column: false,
                },
                Migration {
                    name: "002_v030_push_configs",
                    sql: include_str!("../../../migrations/postgres/002_v030_push_configs.sql"),
                    tolerates_existing_column: false,
                },
                Migration {
                    name: "003_task_version",
                    sql: include_str!("../../../migrations/postgres/003_task_version.sql"),
                    tolerates_existing_column: false,
                },
                Migration {
                    name: "004_task_history_context",
                    sql: include_str!("../../../migrations/postgres/004_task_history_context.sql"),
                    tolerates_existing_column: false,
                },
                Migration {
                    name: "005_context_memory",
                    sql: include_str!("../../../migrations/postgres/005_context_memory.sql"),
                    tolerates_existing_column: false,
                },
                Migration {
                    name: "006_context_state",
                    sql: include_str!("../../../migrations/postgres/006_context_state.sql"),
                    tolerates_existing_column: false,
                },
                Migration {
                    name: "007_task_events",
                    sql: include_str!("../../../migrations/postgres/007_task_events.sql"),
                    tolerates_existing_column: false,
                },
            ],
        }
    }
}

/// Did this fail because another process was creating the same schema?
///
/// The advisory lock is the real defence; this is what catches the case it
/// cannot cover — a database that was already being migrated by a process
/// holding no lock, such as one running an older build. PostgreSQL's `CREATE
/// TABLE IF NOT EXISTS` checks and creates in two steps, so the loser of that
/// race either finds the object appear underneath it (a duplicate-object
/// SQLSTATE) or deadlocks against the other session's locks on the same tables.
/// Every one of them is answered by running the file again, since by then the
/// other process is done. SQLite serializes writers itself and never gets here.
pub(super) fn is_concurrent_ddl_conflict(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|db| db.code())
        .is_some_and(|code| {
            matches!(
                &*code,
                // duplicate object, in its various spellings
                "23505" | "42P07" | "42P06" | "42710" | "42P16"
                // deadlock detected, serialization failure
                | "40P01" | "40001"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_keeps_its_own_placeholders() {
        let sql = "SELECT id FROM tasks WHERE id = ? AND version = ?";
        assert_eq!(Dialect::Sqlite.bind_params(sql), sql);
    }

    #[test]
    fn postgres_placeholders_are_numbered_in_order() {
        assert_eq!(
            Dialect::Postgres.bind_params("SELECT id FROM tasks WHERE id = ? AND version = ?"),
            "SELECT id FROM tasks WHERE id = $1 AND version = $2"
        );
    }

    /// The rewrite has to reach the placeholders inside a subquery too — the
    /// history insert takes its context from one, and binding them out of order
    /// would file every message under the wrong conversation.
    #[test]
    fn placeholders_inside_a_subquery_are_numbered_with_the_rest() {
        assert_eq!(
            Dialect::Postgres.bind_params(
                "INSERT INTO task_history (task_id, context_id, status_state) \
                 VALUES (?, (SELECT context_id FROM tasks WHERE id = ?), ?)"
            ),
            "INSERT INTO task_history (task_id, context_id, status_state) \
             VALUES ($1, (SELECT context_id FROM tasks WHERE id = $2), $3)"
        );
    }

    /// A cast written into a predicate has to survive the rewrite, since `::` is
    /// the one piece of PostgreSQL syntax the shared queries carry.
    #[test]
    fn a_cast_survives_the_rewrite() {
        assert_eq!(
            Dialect::Postgres.bind_params(Dialect::Postgres.updated_since_predicate()),
            "updated_at >= $1::timestamptz"
        );
    }

    /// Both retention queries take exactly one bound cutoff, spelled the way
    /// that dialect needs it. They bypass [`Dialect::bind_params`] — they are
    /// written per dialect already — so a stray `?` in the PostgreSQL spelling
    /// would reach the server verbatim.
    #[test]
    fn the_retention_queries_bind_one_cutoff_each() {
        for query in [
            Dialect::Sqlite.idle_contexts(),
            Dialect::Sqlite.idle_principals(),
        ] {
            assert_eq!(query.matches('?').count(), 1, "{query}");
        }
        for query in [
            Dialect::Postgres.idle_contexts(),
            Dialect::Postgres.idle_principals(),
        ] {
            assert_eq!(query.matches('?').count(), 0, "{query}");
            assert_eq!(query.matches("$1::timestamptz").count(), 1, "{query}");
        }
    }

    /// The running-task guard is what keeps a sweep from deleting work in
    /// progress, and it turns on a list of state names that the schema's CHECK
    /// constraint also spells. `input-required` and `auth-required` are
    /// deliberately absent: they wait on a caller.
    #[test]
    fn only_unfinished_states_hold_a_context_back() {
        for dialect in [Dialect::Sqlite, Dialect::Postgres] {
            let query = dialect.idle_contexts();
            assert!(
                query.contains("'submitted', 'working', 'unknown'"),
                "{query}"
            );
            assert!(!query.contains("input-required"), "{query}");
        }
    }

    /// MySQL is recognized from a URL so the error can name it, and there is no
    /// dialect behind it.
    #[test]
    fn mysql_has_no_dialect() {
        assert_eq!(Dialect::of(DatabaseType::Mysql), None);
    }
}
