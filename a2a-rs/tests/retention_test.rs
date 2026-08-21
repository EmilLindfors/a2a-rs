//! Retention sweeps, run against every store that implements one.
//!
//! The stores are meant to model the same thing, so the assertions live in one
//! generic body and each is handed to it: the in-memory store, `SqlxTaskStorage`
//! on SQLite, and — when `A2A_TEST_POSTGRES_URL` names a server — the same
//! adapter on PostgreSQL, where the sweep's SQL is spelled differently. A rule
//! that held for one and not the others would be several retention policies
//! wearing one name.
//!
//! Nothing here sleeps. Idleness is measured against the `now` the caller
//! passes [`AsyncRetention::sweep`], so a test ages a context by sweeping from
//! a month in the future — which is the reason that argument is a parameter and
//! not a call to the clock inside the store.

#![cfg(feature = "server")]

use std::time::Duration;

use a2a_rs::adapter::storage::InMemoryTaskStorage;
use a2a_rs::domain::{
    ContextId, Message, Part, RetentionPolicy, Role, StateKey, StateScope, TaskId, TaskState,
};
use a2a_rs::port::{
    AsyncContextStateStore, AsyncConversationStore, AsyncConversationStoreExt,
    AsyncNotificationManager, AsyncRetention, AsyncTaskLifecycle,
};
use chrono::{TimeDelta, Utc};

/// Everything a store under test has to be able to do.
trait Store:
    AsyncRetention
    + AsyncTaskLifecycle
    + AsyncConversationStore
    + AsyncContextStateStore
    + AsyncNotificationManager
{
}
impl<T> Store for T where
    T: AsyncRetention
        + AsyncTaskLifecycle
        + AsyncConversationStore
        + AsyncContextStateStore
        + AsyncNotificationManager
{
}

fn tid(s: &str) -> TaskId {
    s.parse().unwrap()
}
fn cid(s: &str) -> ContextId {
    s.parse().unwrap()
}
fn said(text: &str) -> Message {
    Message::builder()
        .role(Role::User)
        .parts(vec![Part::text(text.to_string())])
        .message_id(uuid::Uuid::new_v4().to_string())
        .build()
}

const WEEK: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Sweep as though a month had passed, which expires anything written now.
async fn sweep_a_month_on(store: &dyn Store, policy: &RetentionPolicy) -> a2a_rs::domain::Swept {
    let later = Utc::now() + TimeDelta::days(30);
    store.sweep(policy, later).await.unwrap()
}

/// A context with a finished task, a conversation and a remembered fact.
async fn write_a_finished_context(store: &dyn Store, context: &str, task: &str) {
    let (context, task) = (cid(context), tid(task));
    store.create(&task, &context).await.unwrap();
    store
        .update_status(&task, TaskState::Working, Some(said("hello")))
        .await
        .unwrap();
    store
        .update_status(&task, TaskState::Completed, Some(said("goodbye")))
        .await
        .unwrap();
    store
        .remember(
            &context,
            Some("alice"),
            &StateKey::scoped(StateScope::Context, "topic").unwrap(),
            "retention",
        )
        .await
        .unwrap();
}

async fn an_idle_context_is_swept_whole(store: &dyn Store) {
    write_a_finished_context(store, "ctx-idle", "task-idle").await;

    let policy = RetentionPolicy::keep_everything().delete_contexts_idle_for(WEEK);
    let swept = sweep_a_month_on(store, &policy).await;

    assert_eq!(swept.contexts, 1, "the context itself");
    assert_eq!(swept.tasks, 1);
    assert_eq!(swept.messages, 2, "both messages of the conversation");
    assert_eq!(swept.state_keys, 1, "the `context:`-scoped fact");

    assert!(
        store.get(&tid("task-idle"), None).await.is_err(),
        "the task should be gone"
    );
    assert!(
        store
            .load(&cid("ctx-idle"), Some("alice"), None)
            .await
            .unwrap()
            .is_empty(),
        "the conversation should be gone"
    );
    assert!(
        store
            .load_state(&cid("ctx-idle"), Some("alice"))
            .await
            .unwrap()
            .is_empty(),
        "the remembered fact should be gone"
    );
}

/// The window is a window: a context written inside it survives the sweep that
/// expires one written before it.
async fn a_recent_context_survives(store: &dyn Store) {
    write_a_finished_context(store, "ctx-recent", "task-recent").await;

    let policy = RetentionPolicy::keep_everything().delete_contexts_idle_for(WEEK);
    let swept = store.sweep(&policy, Utc::now()).await.unwrap();

    assert!(swept.is_empty(), "nothing written a moment ago is idle");
    assert!(store.get(&tid("task-recent"), None).await.is_ok());
}

/// The guard that makes the whole thing safe to schedule: a task nothing has
/// finished may still be running, however long it has been quiet.
async fn a_context_holding_an_unfinished_task_is_never_swept(store: &dyn Store) {
    let (context, task) = (cid("ctx-busy"), tid("task-busy"));
    store.create(&task, &context).await.unwrap();
    store
        .update_status(&task, TaskState::Working, Some(said("still going")))
        .await
        .unwrap();

    let policy = RetentionPolicy::keep_everything().delete_contexts_idle_for(WEEK);
    let swept = sweep_a_month_on(store, &policy).await;

    assert!(swept.is_empty(), "a working task holds its context back");
    assert!(store.get(&task, None).await.is_ok());
}

/// `input-required` is settled: it waits on a caller who, a month later, is not
/// coming back. That is the difference between this and the test above.
async fn a_context_waiting_on_a_caller_is_swept(store: &dyn Store) {
    let (context, task) = (cid("ctx-waiting"), tid("task-waiting"));
    store.create(&task, &context).await.unwrap();
    store
        .update_status(&task, TaskState::InputRequired, Some(said("which one?")))
        .await
        .unwrap();

    let policy = RetentionPolicy::keep_everything().delete_contexts_idle_for(WEEK);
    let swept = sweep_a_month_on(store, &policy).await;

    assert_eq!(swept.contexts, 1);
    assert!(store.get(&task, None).await.is_err());
}

/// The two knobs are independent in the direction that matters: sweeping every
/// context a principal ever used must not take the facts they carry between
/// contexts, because those outlive any one of them.
async fn sweeping_contexts_leaves_user_state_alone(store: &dyn Store) {
    let context = cid("ctx-with-user-state");
    let remembered = StateKey::scoped(StateScope::User, "name").unwrap();
    store
        .remember(&context, Some("bob"), &remembered, "Bob")
        .await
        .unwrap();

    let policy = RetentionPolicy::keep_everything().delete_contexts_idle_for(WEEK);
    sweep_a_month_on(store, &policy).await;

    let state = store
        .load_state(&cid("some-other-context"), Some("bob"))
        .await
        .unwrap();
    assert_eq!(
        state.get(&remembered),
        Some("Bob"),
        "a `user:` fact belongs to the principal, not to the context it was written from"
    );
}

async fn idle_user_state_is_swept_by_its_own_knob(store: &dyn Store) {
    let context = cid("ctx-for-carol");
    let remembered = StateKey::scoped(StateScope::User, "city").unwrap();
    store
        .remember(&context, Some("carol"), &remembered, "Bergen")
        .await
        .unwrap();

    let policy = RetentionPolicy::keep_everything().delete_user_state_idle_for(WEEK);
    let swept = sweep_a_month_on(store, &policy).await;

    assert_eq!(swept.state_keys, 1);
    assert_eq!(swept.contexts, 0, "this knob does not touch contexts");
    assert!(
        store
            .load_state(&context, Some("carol"))
            .await
            .unwrap()
            .is_empty()
    );
}

/// A webhook registered against a swept task has to go with it. The two stores
/// keep push configs in different places — a SQL table against one, an
/// in-process registry against the other — so this is the assertion most likely
/// to hold on one adapter and not the other.
async fn a_swept_task_takes_its_push_config_with_it(store: &dyn Store) {
    use a2a_rs::domain::{GetTaskPushNotificationConfigParams, TaskPushNotificationConfig};

    let (context, task) = (cid("ctx-webhook"), tid("task-webhook"));
    store.create(&task, &context).await.unwrap();
    store
        .update_status(&task, TaskState::Completed, Some(said("done")))
        .await
        .unwrap();

    let config = TaskPushNotificationConfig {
        id: "cfg-webhook".to_string(),
        task_id: task.as_str().to_string(),
        url: "https://example.invalid/hook".to_string(),
        ..Default::default()
    };
    store.set_config(&config).await.unwrap();

    let policy = RetentionPolicy::keep_everything().delete_contexts_idle_for(WEEK);
    assert_eq!(sweep_a_month_on(store, &policy).await.contexts, 1);

    let looked_up = store
        .get_config(&GetTaskPushNotificationConfigParams {
            id: task.as_str().to_string(),
            push_notification_config_id: None,
            metadata: None,
        })
        .await;
    assert!(
        looked_up.is_err(),
        "the webhook should not outlive the task it was registered against"
    );
}

/// The default is off. A store swept under it is a store nothing happened to,
/// which is what makes retention safe to leave unconfigured.
async fn the_default_policy_deletes_nothing(store: &dyn Store) {
    write_a_finished_context(store, "ctx-kept", "task-kept").await;

    let swept = sweep_a_month_on(store, &RetentionPolicy::default()).await;

    assert!(swept.is_empty());
    assert!(store.get(&tid("task-kept"), None).await.is_ok());
}

/// A compacted conversation is a summary plus what was said after it, and the
/// sweep takes both. The only case that writes a `context_digests` row: that
/// table is one of the five the last-write query unions over, and
/// `Swept::digests` counts nothing anywhere else.
async fn a_compacted_context_takes_its_digest_with_it(store: &dyn Store) {
    let (context, task) = (cid("ctx-digest"), tid("task-digest"));
    store.create(&task, &context).await.unwrap();
    store
        .update_status(&task, TaskState::Completed, Some(said("the long version")))
        .await
        .unwrap();

    let conversation = store.load(&context, Some("alice"), None).await.unwrap();
    store
        .compact_through(
            &context,
            Some("alice"),
            &conversation,
            "the short version".to_string(),
            "test-model".to_string(),
        )
        .await
        .unwrap();

    let policy = RetentionPolicy::keep_everything().delete_contexts_idle_for(WEEK);
    let swept = sweep_a_month_on(store, &policy).await;

    assert_eq!(swept.contexts, 1);
    assert_eq!(swept.digests, 1, "the summary goes with what it summarized");
    assert_eq!(swept.messages, 1);
    assert!(
        store
            .load(&context, Some("alice"), None)
            .await
            .unwrap()
            .is_empty()
    );
}

/// Every case, against one store. Each gets a fresh one — a sweep is global, so
/// sharing would let one case's leftovers answer another's assertion. A fixture
/// that answers `None` is a backend this run cannot reach, and the case skips.
macro_rules! for_each_case {
    ($suite:ident, $fresh:path) => {
        mod $suite {
            use super::*;

            for_each_case!(@cases $fresh:
                an_idle_context_is_swept_whole,
                a_recent_context_survives,
                a_context_holding_an_unfinished_task_is_never_swept,
                a_context_waiting_on_a_caller_is_swept,
                sweeping_contexts_leaves_user_state_alone,
                idle_user_state_is_swept_by_its_own_knob,
                a_swept_task_takes_its_push_config_with_it,
                a_compacted_context_takes_its_digest_with_it,
                the_default_policy_deletes_nothing,
            );
        }
    };
    (@cases $fresh:path: $($case:ident),+ $(,)?) => {
        $(
            #[tokio::test]
            async fn $case() {
                if let Some(store) = $fresh(stringify!($case)).await {
                    super::$case(&store).await;
                }
            }
        )+
    };
}

async fn in_memory(_case: &str) -> Option<InMemoryTaskStorage> {
    Some(InMemoryTaskStorage::new())
}

for_each_case!(in_memory_store, in_memory);

#[cfg(feature = "sqlx-storage")]
async fn sqlite(_case: &str) -> Option<a2a_rs::adapter::storage::SqlxTaskStorage> {
    use a2a_rs::adapter::storage::{DatabaseConfig, SqlxStorageBuilder};

    let config = DatabaseConfig::builder()
        .url("sqlite::memory:".to_string())
        .max_connections(1)
        .build();
    Some(
        SqlxStorageBuilder::from_config(&config)
            .connect()
            .await
            .unwrap(),
    )
}

#[cfg(feature = "sqlx-storage")]
for_each_case!(sqlite_store, sqlite);

/// The same cases against a real PostgreSQL server, skipped unless
/// `A2A_TEST_POSTGRES_URL` names one:
///
/// ```text
/// docker run --rm -e POSTGRES_PASSWORD=a2a -p 5432:5432 postgres:17
/// A2A_TEST_POSTGRES_URL=postgres://postgres:a2a@localhost/postgres \
///   cargo test -p a2a-rs --features full --test retention_test
/// ```
///
/// The sweep is the one part of the storage adapter written twice — the
/// `UNION ALL` over five tables that finds the last write, the `EXCEPT` that
/// holds back a context with a running task, and the `$1::timestamptz` the
/// cutoff is bound through all have a separate PostgreSQL spelling. Only a
/// server can say whether they mean what the SQLite ones mean.
///
/// A database per case, created from the one the URL names: a sweep is global,
/// so cases sharing a server would delete each other's contexts and then
/// disagree about the count. The name is the case's, so a run reuses the same
/// databases rather than accumulating them, and no case can drop one another is
/// using. `DROP DATABASE … WITH (FORCE)` needs PostgreSQL 13 or newer.
#[cfg(feature = "postgres")]
async fn postgres(case: &str) -> Option<a2a_rs::adapter::storage::SqlxTaskStorage> {
    let base = std::env::var("A2A_TEST_POSTGRES_URL")
        .ok()
        .filter(|url| !url.is_empty());
    let Some(base) = base else {
        eprintln!("skipped: set A2A_TEST_POSTGRES_URL to sweep against PostgreSQL");
        return None;
    };

    let admin = sqlx::postgres::PgPool::connect(&base)
        .await
        .expect("A2A_TEST_POSTGRES_URL is set but unusable");

    // `raw_sql` rather than `query`: CREATE DATABASE cannot run in a
    // transaction block, and the extended protocol sqlx prepares statements
    // with wraps it in one.
    let database = format!("a2a_ret_{case}");
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

    let mut url = url::Url::parse(&base).expect("A2A_TEST_POSTGRES_URL is not a URL");
    url.set_path(&database);
    Some(
        a2a_rs::adapter::storage::SqlxTaskStorage::new(url.as_str())
            .await
            .expect("connect to the case's database"),
    )
}

#[cfg(feature = "postgres")]
for_each_case!(postgres_store, postgres);
