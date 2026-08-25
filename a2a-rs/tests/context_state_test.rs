//! The state bag, run against every store that keeps one.
//!
//! Same shape as `retention_test.rs`, and for the same reason: the stores are
//! meant to model one thing, so the assertions live in a generic body and each
//! store is handed to it — the in-memory store, `SqlxTaskStorage` on SQLite,
//! and, when `A2A_TEST_POSTGRES_URL` names a server, the same adapter on
//! PostgreSQL, where both halves added here are spelled differently. `remember`
//! reads the replaced value in a transaction, and a read refresh binds its
//! cutoff through the dialect; SQLite passing says nothing about either.
//!
//! Nothing sleeps. What a read refresh *saves* cannot be asserted from here:
//! a refresh writes the clock's `now`, and ageing a bag past a window without
//! waiting one out means writing a timestamp in the past, which no port lets a
//! caller do. That half is tested inside each adapter, where the store's own
//! state is reachable — `ReadRefresh`'s rule has unit tests of its own in
//! `domain::retention`. What belongs here is the direction both stores must
//! agree on and that a caller can reach: with no refresh configured a read
//! saves nothing, and with one configured it still saves nothing that is not a
//! `user:` bag.

#![cfg(feature = "server")]

use std::time::Duration;

use a2a_rs::adapter::storage::InMemoryTaskStorage;
use a2a_rs::domain::{
    ContextId, ReadRefresh, Remembered, RetentionPolicy, StateKey, StateScope, Swept,
};
use a2a_rs::port::{AsyncContextStateStore, AsyncRetention};
use chrono::{TimeDelta, Utc};

/// Everything a store under test has to be able to do.
trait Store: AsyncContextStateStore + AsyncRetention {}
impl<T> Store for T where T: AsyncContextStateStore + AsyncRetention {}

fn cid(s: &str) -> ContextId {
    s.parse().unwrap()
}
fn key(scope: StateScope, name: &str) -> StateKey {
    StateKey::scoped(scope, name).unwrap()
}

const WEEK: Duration = Duration::from_secs(7 * 24 * 60 * 60);

async fn sweep_a_month_on(store: &dyn Store, policy: &RetentionPolicy) -> Swept {
    store
        .sweep(policy, Utc::now() + TimeDelta::days(30))
        .await
        .unwrap()
}

// ---------------------------------------------------------------- `Remembered`

/// The four answers, in the order one key walks through them.
async fn a_write_says_what_the_key_held(store: &dyn Store) {
    let context = cid("ctx-remembered");
    let topic = key(StateScope::Context, "topic");

    assert_eq!(
        store
            .remember(&context, Some("alice"), &topic, "retention")
            .await
            .unwrap(),
        Remembered::Stored,
        "nothing was there"
    );

    assert_eq!(
        store
            .remember(&context, Some("alice"), &topic, "retention")
            .await
            .unwrap(),
        Remembered::Unchanged,
        "an agent repeating itself has overwritten nothing"
    );

    let replaced = store
        .remember(&context, Some("alice"), &topic, "ownership")
        .await
        .unwrap();
    assert_eq!(
        replaced,
        Remembered::Replaced {
            previous: "retention".to_string()
        }
    );
    assert_eq!(
        replaced.replaced_value(),
        Some("retention"),
        "the value the row no longer holds, which nothing else records"
    );

    // And the write landed, rather than the report having cost it.
    let bag = store.load_state(&context, Some("alice")).await.unwrap();
    assert_eq!(bag.get(&topic), Some("ownership"));
}

/// `temp:` is the scope that stores nothing, so `Stored` would be the one
/// answer that is untrue — the caller will read the key back and find nothing.
async fn a_temp_key_reports_that_it_was_not_stored(store: &dyn Store) {
    let context = cid("ctx-temp");
    let draft = key(StateScope::Temp, "draft");

    assert_eq!(
        store
            .remember(&context, Some("alice"), &draft, "half a sentence")
            .await
            .unwrap(),
        Remembered::NotStored
    );
    assert!(
        store
            .load_state(&context, Some("alice"))
            .await
            .unwrap()
            .is_empty()
    );
}

/// A `user:` key is filed under the principal, so the report has to be about
/// that bucket rather than about the context the write came from.
async fn a_user_key_reports_across_contexts(store: &dyn Store) {
    let tone = key(StateScope::User, "tone");

    assert_eq!(
        store
            .remember(&cid("ctx-user-a"), Some("alice"), &tone, "brief")
            .await
            .unwrap(),
        Remembered::Stored
    );
    assert_eq!(
        store
            .remember(&cid("ctx-user-b"), Some("alice"), &tone, "thorough")
            .await
            .unwrap(),
        Remembered::Replaced {
            previous: "brief".to_string()
        },
        "a second context writes the same bag"
    );
}

// --------------------------------------------------------------- `ReadRefresh`

/// The default, pinned: reads record nothing, so a bag that is only ever read
/// expires. This is the behaviour `ReadRefresh` is opt-in *from*.
async fn without_a_refresh_a_read_bag_still_expires(store: &dyn Store) {
    let context = cid("ctx-no-refresh");
    store
        .remember(
            &context,
            Some("alice"),
            &key(StateScope::User, "name"),
            "Emil",
        )
        .await
        .unwrap();

    store.load_state(&context, Some("alice")).await.unwrap();

    let policy = RetentionPolicy::keep_everything().delete_user_state_idle_for(WEEK);
    assert_eq!(
        sweep_a_month_on(store, &policy).await.state_keys,
        1,
        "a read did not keep it alive"
    );
}

/// A context is not a principal. Its idleness is maintained by its own writes,
/// and a read of the state bag must not stand in for one — the refresh is
/// scoped to the `user:` bucket precisely so this stays true.
async fn a_refresh_does_not_keep_the_context_alive(store: &dyn Store) {
    let context = cid("ctx-refresh-context-scope");
    store
        .remember(
            &context,
            Some("alice"),
            &key(StateScope::Context, "topic"),
            "memory",
        )
        .await
        .unwrap();

    store.load_state(&context, Some("alice")).await.unwrap();

    let policy = RetentionPolicy::keep_everything().delete_contexts_idle_for(WEEK);
    assert_eq!(
        sweep_a_month_on(store, &policy).await.contexts,
        1,
        "reading a context's own keys is still not a write"
    );
}

// -------------------------------------------------------------------- fixtures

/// Every case, against one store. Each gets a fresh one, for the reason
/// `retention_test.rs` gives: a sweep is global. `$fresh` takes the case name
/// and whether that case wants a read refresh configured.
macro_rules! for_each_case {
    ($suite:ident, $fresh:path) => {
        mod $suite {
            use super::*;

            for_each_case!(@plain $fresh:
                a_write_says_what_the_key_held,
                a_temp_key_reports_that_it_was_not_stored,
                a_user_key_reports_across_contexts,
                without_a_refresh_a_read_bag_still_expires,
            );
            for_each_case!(@refreshing $fresh:
                a_refresh_does_not_keep_the_context_alive,
            );
        }
    };
    (@plain $fresh:path: $($case:ident),+ $(,)?) => {
        $(
            #[tokio::test]
            async fn $case() {
                if let Some(store) = $fresh(stringify!($case), ReadRefresh::never()).await {
                    super::$case(&store).await;
                }
            }
        )+
    };
    (@refreshing $fresh:path: $($case:ident),+ $(,)?) => {
        $(
            #[tokio::test]
            async fn $case() {
                // A zero window makes every bag due a refresh, which is what
                // lets these assert the write without waiting one out.
                let refresh = ReadRefresh::after(Duration::ZERO);
                if let Some(store) = $fresh(stringify!($case), refresh).await {
                    super::$case(&store).await;
                }
            }
        )+
    };
}

async fn in_memory(_case: &str, refresh: ReadRefresh) -> Option<InMemoryTaskStorage> {
    Some(InMemoryTaskStorage::new().with_read_refresh(refresh))
}

for_each_case!(in_memory_store, in_memory);

#[cfg(feature = "sqlx-storage")]
async fn sqlite(
    _case: &str,
    refresh: ReadRefresh,
) -> Option<a2a_rs::adapter::storage::SqlxTaskStorage> {
    Some(
        a2a_rs::adapter::storage::SqlxTaskStorage::builder("sqlite::memory:")
            .max_connections(1)
            .read_refresh(refresh)
            .connect()
            .await
            .unwrap(),
    )
}

#[cfg(feature = "sqlx-storage")]
for_each_case!(sqlite_store, sqlite);

/// The same cases against a real PostgreSQL server, skipped unless
/// `A2A_TEST_POSTGRES_URL` names one. A database per case, for the reason
/// `retention_test.rs` documents at length: a sweep is global.
///
/// Both halves this file covers are written twice. `remember`'s read-back runs
/// inside a transaction the two backends open differently, and the refresh
/// binds its cutoff through `$1`/`$2::timestamptz` against a `timestamptz`
/// column rather than SQLite's text. Only a server can say they agree.
#[cfg(feature = "postgres")]
async fn postgres(
    case: &str,
    refresh: ReadRefresh,
) -> Option<a2a_rs::adapter::storage::SqlxTaskStorage> {
    let base = std::env::var("A2A_TEST_POSTGRES_URL")
        .ok()
        .filter(|url| !url.is_empty())?;

    let admin = sqlx::postgres::PgPool::connect(&base)
        .await
        .expect("A2A_TEST_POSTGRES_URL is set but unusable");

    let database = format!("a2a_state_{case}");
    // `raw_sql` rather than `query`: CREATE DATABASE cannot run in a
    // transaction block, and the extended protocol wraps prepared statements in
    // one. `WITH (FORCE)` needs PostgreSQL 13 or newer.
    for statement in [
        format!("DROP DATABASE IF EXISTS \"{database}\" WITH (FORCE)"),
        format!("CREATE DATABASE \"{database}\""),
    ] {
        sqlx::raw_sql(&statement)
            .execute(&admin)
            .await
            .unwrap_or_else(|e| panic!("{statement}: {e}"));
    }
    admin.close().await;

    let url = {
        let mut url = url::Url::parse(&base).expect("A2A_TEST_POSTGRES_URL is not a URL");
        url.set_path(&database);
        url.to_string()
    };

    Some(
        a2a_rs::adapter::storage::SqlxTaskStorage::builder(url)
            .max_connections(2)
            .read_refresh(refresh)
            .connect()
            .await
            .unwrap(),
    )
}

#[cfg(feature = "postgres")]
for_each_case!(postgres_store, postgres);
