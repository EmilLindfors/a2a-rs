//! `SqlxTaskStorage` against a real PostgreSQL server.
//!
//! Skipped unless `A2A_TEST_POSTGRES_URL` names one:
//!
//! ```text
//! docker run --rm -e POSTGRES_PASSWORD=a2a -p 5432:5432 postgres:17
//! A2A_TEST_POSTGRES_URL=postgres://postgres:a2a@localhost/postgres \
//!   cargo test -p a2a-rs --features full --test postgres_storage_test
//! ```
//!
//! What is tested here is deliberately not the whole storage suite — that runs
//! on SQLite in `sqlx_storage_test.rs` and the queries are the same file. These
//! cover what can only differ by backend: the schema, the two upserts, the
//! placeholder rewrite, the timestamp comparison, and every column the driver
//! has to decode.

#![cfg(feature = "sqlx-storage")]

use a2a_rs::adapter::storage::SqlxTaskStorage;
use a2a_rs::domain::{
    ContextId, Digest, ListTasksParams, Message, Part, Role, Seq, StateKey, TaskId, TaskState,
};
use a2a_rs::port::{
    AsyncContextStateStore, AsyncConversationStore, AsyncNotificationManager, AsyncTaskLifecycle,
    AsyncTaskQuery, AsyncTaskVersioning,
};
use a2a_rs::{A2AError, TaskPushNotificationConfig};
use uuid::Uuid;

/// The server to test against, or `None` when this run is skipping.
fn postgres_url() -> Option<String> {
    std::env::var("A2A_TEST_POSTGRES_URL")
        .ok()
        .filter(|url| !url.is_empty())
}

/// Guards every test in this file. Returns `None` — and says why once per test —
/// when no server is configured, so a plain `cargo test` stays green on a
/// machine with no PostgreSQL.
macro_rules! storage_or_skip {
    () => {
        match postgres_url() {
            Some(url) => match SqlxTaskStorage::new(&url).await {
                Ok(storage) => storage,
                Err(e) => panic!("A2A_TEST_POSTGRES_URL is set but unusable: {e}"),
            },
            None => {
                eprintln!("skipped: set A2A_TEST_POSTGRES_URL to run the PostgreSQL storage tests");
                return;
            }
        }
    };
}

fn tid() -> TaskId {
    Uuid::new_v4().to_string().parse().unwrap()
}

fn ctx() -> ContextId {
    Uuid::new_v4().to_string().parse().unwrap()
}

fn said(text: &str) -> Message {
    Message::builder()
        .role(Role::User)
        .parts(vec![Part::text(text.to_string())])
        .message_id(Uuid::new_v4().to_string())
        .build()
}

fn texts(conversation: &a2a_rs::domain::Conversation) -> Vec<String> {
    use a2a_rs::domain::part;
    conversation
        .tail
        .iter()
        .flat_map(|entry| {
            entry.message.parts.iter().filter_map(|p| match &p.content {
                Some(part::Content::Text(text)) => Some(text.clone()),
                _ => None,
            })
        })
        .collect()
}

/// Connecting runs the migrations, and running them twice has to be the same as
/// running them once — they re-run on every construction.
#[tokio::test]
async fn migrations_are_idempotent() {
    let storage = storage_or_skip!();
    let id = tid();
    let context = ctx();
    storage.create(&id, &context).await.expect("create");

    // A second store over the same database re-runs every migration file.
    let url = postgres_url().unwrap();
    let second = SqlxTaskStorage::new(&url)
        .await
        .expect("second migration pass");

    let task = second.get(&id, None).await.expect("the task survives");
    assert_eq!(task.id, id.as_str());
}

/// The whole round trip through the JSON columns and the state check
/// constraint. Every column `row_to_task` reads has to decode, which is the
/// thing that breaks when a schema uses a type the driver cannot map.
#[tokio::test]
async fn a_task_round_trips_through_every_column() {
    let storage = storage_or_skip!();
    let id = tid();
    let context = ctx();

    storage.create(&id, &context).await.expect("create");
    storage
        .update_status(&id, TaskState::Working, Some(said("on it")))
        .await
        .expect("update");
    let task = storage.get(&id, None).await.expect("get");

    assert_eq!(task.id, id.as_str());
    assert_eq!(task.context_id, context.as_str());
    assert_eq!(task.status.state, TaskState::Working);
    assert!(
        task.history.iter().any(|m| m.parts.iter().any(
            |p| matches!(&p.content, Some(a2a_rs::domain::part::Content::Text(t)) if t == "on it")
        )),
        "the message written to history has to come back: {:?}",
        task.history
    );
}

/// `update_status_checked` is a conditional UPDATE whose row count decides the
/// answer, and the version column is read back as a 64-bit integer — which is
/// why the PostgreSQL schema declares it BIGINT.
#[tokio::test]
async fn a_stale_version_is_refused() {
    let storage = storage_or_skip!();
    let id = tid();
    storage.create(&id, &ctx()).await.expect("create");

    let version = storage.version(&id).await.expect("version");
    storage
        .update_status_checked(&id, version, TaskState::Working, None)
        .await
        .expect("the current version applies");

    let conflict = storage
        .update_status_checked(&id, version, TaskState::Completed, None)
        .await;
    assert!(
        matches!(conflict, Err(A2AError::VersionConflict { .. })),
        "a stale version must be refused, got {conflict:?}"
    );
}

/// The list query is where the placeholders, the count and the timestamp
/// comparison all land in one statement.
#[tokio::test]
async fn listing_filters_by_context_status_and_time() {
    let storage = storage_or_skip!();
    let context = ctx();
    let before = chrono::Utc::now() - chrono::Duration::minutes(5);

    let working = tid();
    storage.create(&working, &context).await.expect("create");
    storage
        .update_status(&working, TaskState::Working, None)
        .await
        .expect("update");
    let completed = tid();
    storage.create(&completed, &context).await.expect("create");
    storage
        .update_status(&completed, TaskState::Completed, None)
        .await
        .expect("update");

    let listed = storage
        .list(&ListTasksParams {
            context_id: Some(context.as_str().to_string()),
            status: Some(TaskState::Working),
            status_timestamp_after: Some(before.to_rfc3339()),
            ..Default::default()
        })
        .await
        .expect("list");

    assert_eq!(listed.total_size, 1, "one working task in this context");
    assert_eq!(listed.tasks.len(), 1);
    assert_eq!(listed.tasks[0].id, working.as_str());
}

/// A timestamp filter in the future matches nothing — which is the assertion
/// that the parameter is compared as a timestamp rather than as text.
#[tokio::test]
async fn a_future_timestamp_filter_matches_nothing() {
    let storage = storage_or_skip!();
    let context = ctx();
    storage.create(&tid(), &context).await.expect("create");

    let listed = storage
        .list(&ListTasksParams {
            context_id: Some(context.as_str().to_string()),
            status_timestamp_after: Some(
                (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
            ),
            ..Default::default()
        })
        .await
        .expect("list");

    assert_eq!(listed.total_size, 0, "nothing was updated in the future");
}

/// `set_config` is an upsert, spelled `ON CONFLICT ... DO UPDATE` here and
/// `INSERT OR REPLACE` on SQLite. Writing the same id twice has to overwrite
/// rather than fail on the primary key.
#[tokio::test]
async fn a_push_config_is_overwritten_by_id() {
    let storage = storage_or_skip!();
    let id = tid();
    storage.create(&id, &ctx()).await.expect("create");

    let config = TaskPushNotificationConfig {
        task_id: id.as_str().to_string(),
        id: "webhook-1".to_string(),
        url: "https://example.test/first".to_string(),
        ..Default::default()
    };
    storage.set_config(&config).await.expect("first write");
    storage
        .set_config(&TaskPushNotificationConfig {
            url: "https://example.test/second".to_string(),
            ..config.clone()
        })
        .await
        .expect("the same id overwrites");

    let stored = storage
        .list_configs(&a2a_rs::domain::ListTaskPushNotificationConfigsParams {
            id: id.as_str().to_string(),
            metadata: None,
        })
        .await
        .expect("list configs");

    assert_eq!(stored.len(), 1, "one config, not two: {stored:?}");
    assert_eq!(stored[0].url, "https://example.test/second");
}

/// Push configs used to be dropped and recreated by migration 002 on every
/// startup, so a restart lost them. A second store over the same database is
/// exactly that restart.
#[tokio::test]
async fn push_configs_survive_a_restart() {
    let storage = storage_or_skip!();
    let id = tid();
    storage.create(&id, &ctx()).await.expect("create");
    storage
        .set_config(&TaskPushNotificationConfig {
            task_id: id.as_str().to_string(),
            id: "kept".to_string(),
            url: "https://example.test/kept".to_string(),
            ..Default::default()
        })
        .await
        .expect("set config");

    let url = postgres_url().unwrap();
    let restarted = SqlxTaskStorage::new(&url).await.expect("reopen");

    let stored = restarted
        .get_config(&a2a_rs::domain::GetTaskPushNotificationConfigParams {
            id: id.as_str().to_string(),
            push_notification_config_id: Some("kept".to_string()),
            ..Default::default()
        })
        .await
        .expect("the config survives a restart");
    assert_eq!(stored.url, "https://example.test/kept");
}

/// The conversation read: `contexts` ownership, the digest watermark, and the
/// `id > ?` window over `task_history` — the query the LLM handler makes on
/// every turn.
#[tokio::test]
async fn a_conversation_reads_back_after_its_digest() {
    let storage = storage_or_skip!();
    let context = ctx();
    let id = tid();

    storage.create(&id, &context).await.expect("create");
    for text in ["first", "second", "third"] {
        storage
            .update_status(&id, TaskState::Working, Some(said(text)))
            .await
            .expect("update");
    }

    let full = storage.load(&context, None, None).await.expect("load");
    assert_eq!(texts(&full), vec!["first", "second", "third"]);

    let watermark = full.tail[1].seq;
    storage
        .compact(
            &context,
            None,
            Digest {
                covers_through: watermark,
                summary: "they said two things".to_string(),
                replaced_messages: 2,
                model: "test".to_string(),
            },
        )
        .await
        .expect("compact");

    let compacted = storage.load(&context, None, None).await.expect("reload");
    assert_eq!(
        compacted.digest.as_ref().map(|d| d.summary.as_str()),
        Some("they said two things")
    );
    assert_eq!(
        texts(&compacted),
        vec!["third"],
        "everything through the watermark is behind the summary"
    );
    assert_eq!(
        compacted.digest.map(|d| d.covers_through),
        Some(Seq::new(watermark.get()))
    );
}

/// The context claim is an insert that ignores a conflict, and the owner is
/// enforced on every read.
#[tokio::test]
async fn a_context_belongs_to_whoever_started_it() {
    let storage = storage_or_skip!();
    let context = ctx();

    storage
        .load(&context, Some("alice"), None)
        .await
        .expect("alice claims it");
    storage
        .load(&context, Some("alice"), None)
        .await
        .expect("and can read it again");

    let denied = storage.load(&context, Some("bob"), None).await;
    assert!(
        matches!(denied, Err(A2AError::ContextAccessDenied { .. })),
        "bob must not read alice's conversation, got {denied:?}"
    );
}

/// The state bag's own table and its own upsert — the third `ON CONFLICT` in
/// this adapter, and the one with a composite key.
#[tokio::test]
async fn remembered_values_round_trip_through_both_scopes() {
    let storage = storage_or_skip!();
    let context = ctx();
    let caller = Uuid::new_v4().to_string();
    let project: StateKey = "project".parse().unwrap();
    let tone: StateKey = "user:tone".parse().unwrap();

    storage
        .remember(&context, Some(&caller), &project, "a2a-rs")
        .await
        .expect("write a context-scoped value");
    storage
        .remember(&context, Some(&caller), &tone, "brief")
        .await
        .expect("write a user-scoped value");
    // Again, to exercise the conflict branch rather than the insert.
    storage
        .remember(&context, Some(&caller), &project, "a2a-agents")
        .await
        .expect("replace it");

    let state = storage
        .load_state(&context, Some(&caller))
        .await
        .expect("read both scopes back");
    assert_eq!(state.get(&project), Some("a2a-agents"));
    assert_eq!(state.get(&tone), Some("brief"));

    // The user-scoped half is filed under the principal, so another context of
    // the same caller reads it and nothing else.
    let elsewhere = storage
        .load_state(&ctx(), Some(&caller))
        .await
        .expect("read from a context that has never seen it");
    assert_eq!(elsewhere.get(&tone), Some("brief"));
    assert_eq!(elsewhere.get(&project), None);

    assert!(
        storage
            .forget(&context, Some(&caller), &project)
            .await
            .expect("delete it")
    );
    assert!(
        !storage
            .forget(&context, Some(&caller), &project)
            .await
            .expect("and report that it was already gone")
    );
}
