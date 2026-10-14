//! One set of assertions, run against every storage backend.
//!
//! The adapters each had their own test file asserting what that adapter does,
//! which is exactly the shape that lets two implementations of one port drift
//! apart while both suites stay green. They did: until 2026-08-26 a completed
//! task read back from `SqlxTaskStorage` had no `status.message` at all —
//! `update_status` wrote `status_state` and left the message column holding
//! whatever `create` put there — while `InMemoryTaskStorage` carried the
//! agent's reply, as `Task::update_status` in the domain does. Found from
//! downstream, by pointing one agent at both backends and reading the two
//! answers side by side.
//!
//! So the assertions live once and the backends are the parameter. Anything
//! added here has to hold for both, which is the property the `Any`-driver work
//! was for and the one nothing was checking.

use a2a_rs::domain::{A2AError, ListTasksParams, Message, TaskState};
use a2a_rs::port::{AsyncTaskArtifacts, AsyncTaskLifecycle, AsyncTaskQuery, AsyncTaskVersioning};

fn tid(s: &str) -> a2a_rs::domain::TaskId {
    s.parse().unwrap()
}
fn cid(s: &str) -> a2a_rs::domain::ContextId {
    s.parse().unwrap()
}

fn agent_says(text: &str) -> Message {
    Message::agent_text(text.to_string(), uuid::Uuid::new_v4().to_string())
}

/// The text of a task's status message, if it has one.
fn status_text(task: &a2a_rs::domain::Task) -> Option<String> {
    task.status.as_option()?.message.as_option().map(|message| {
        message
            .parts
            .iter()
            .filter_map(|part| match &part.content {
                Some(a2a_rs::domain::part::Content::Text(text)) => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ")
    })
}

/// A settled task carries the message that settled it.
///
/// This is what a client reads to get the answer: `status.message` is where the
/// agent's reply belongs, and `history` is the transcript it is also appended
/// to. A backend that files it only under the second leaves every client that
/// reads the first with a completed task and nothing in it.
async fn a_completed_task_keeps_its_message(storage: &impl AsyncTaskLifecycle, label: &str) {
    let id = tid("11111111-1111-1111-1111-111111111111");
    storage.create(&id, &cid("ctx-parity")).await.unwrap();

    storage
        .update_status(&id, TaskState::Working, Some(agent_says("thinking")))
        .await
        .unwrap();
    storage
        .update_status(&id, TaskState::Completed, Some(agent_says("the answer")))
        .await
        .unwrap();

    let task = storage.get(&id, None).await.unwrap();
    assert_eq!(task.status.state, TaskState::Completed, "{label}");
    assert_eq!(
        status_text(&task).as_deref(),
        Some("the answer"),
        "{label}: a completed task must carry the message that completed it"
    );
}

/// ...and the message belongs to the *current* status, so a transition that
/// carries none clears the last one.
///
/// The alternative — leaving the previous message in place — attributes it to a
/// state it was never about, which reads as a stale answer rather than as an
/// absent one.
async fn a_status_with_no_message_clears_the_last(storage: &impl AsyncTaskLifecycle, label: &str) {
    let id = tid("22222222-2222-2222-2222-222222222222");
    storage.create(&id, &cid("ctx-parity")).await.unwrap();

    storage
        .update_status(&id, TaskState::Working, Some(agent_says("halfway")))
        .await
        .unwrap();
    storage
        .update_status(&id, TaskState::InputRequired, None)
        .await
        .unwrap();

    let task = storage.get(&id, None).await.unwrap();
    assert_eq!(task.status.state, TaskState::InputRequired, "{label}");
    assert_eq!(
        status_text(&task),
        None,
        "{label}: a status carrying no message must not keep the previous one"
    );
}

/// A canceled task says so in its status, not only in its history.
async fn a_canceled_task_says_why(storage: &impl AsyncTaskLifecycle, label: &str) {
    let id = tid("33333333-3333-3333-3333-333333333333");
    storage.create(&id, &cid("ctx-parity")).await.unwrap();
    storage
        .update_status(&id, TaskState::Working, None)
        .await
        .unwrap();

    storage.cancel(&id).await.unwrap();

    let task = storage.get(&id, None).await.unwrap();
    assert_eq!(task.status.state, TaskState::Canceled, "{label}");
    let text = status_text(&task)
        .unwrap_or_else(|| panic!("{label}: a canceled task must carry a cancellation message"));
    assert!(
        text.contains("canceled"),
        "{label}: the cancellation message should say so, got {text:?}"
    );
}

/// A text artifact with one part.
fn said(id: &str, text: &str) -> a2a_rs::domain::Artifact {
    a2a_rs::domain::Artifact {
        artifact_id: id.to_string(),
        name: id.to_string(),
        parts: vec![a2a_rs::domain::Part::text(text.to_string())],
        ..Default::default()
    }
}

/// The ids and first text of a task's artifacts, in order.
fn artifacts_of(task: &a2a_rs::domain::Task) -> Vec<(String, String)> {
    task.artifacts
        .iter()
        .map(|artifact| {
            let text = artifact
                .parts
                .iter()
                .find_map(|part| part.get_text())
                .unwrap_or_default();
            (artifact.artifact_id.clone(), text.to_string())
        })
        .collect()
}

/// A kept artifact is read back by `get`, and by `list` when it asks for
/// artifacts. One with a known id replaces the old one in its place, and each
/// write bumps the version. Nothing wrote an artifact before, so a client that
/// was not listening to the stream never saw one.
async fn a_kept_artifact_is_read_back<S>(storage: &S, label: &str)
where
    S: AsyncTaskLifecycle + AsyncTaskArtifacts + AsyncTaskQuery + AsyncTaskVersioning,
{
    let id = tid("kept-artifacts");
    storage.create(&id, &cid("kept-ctx")).await.unwrap();
    let created = storage.version(&id).await.unwrap();

    storage.put_artifact(&id, said("a", "one")).await.unwrap();
    storage.put_artifact(&id, said("b", "two")).await.unwrap();
    storage.put_artifact(&id, said("a", "uno")).await.unwrap();

    let expected = vec![
        ("a".to_string(), "uno".to_string()),
        ("b".to_string(), "two".to_string()),
    ];
    let task = storage.get(&id, None).await.unwrap();
    assert_eq!(artifacts_of(&task), expected, "[{label}] get");
    assert_eq!(
        storage.version(&id).await.unwrap(),
        created + 3,
        "[{label}] version"
    );

    let listed = |include| ListTasksParams {
        context_id: Some("kept-ctx".to_string()),
        include_artifacts: Some(include),
        ..Default::default()
    };
    let with = storage.list(&listed(true)).await.unwrap();
    assert_eq!(
        artifacts_of(&with.tasks[0]),
        expected,
        "[{label}] list with"
    );
    let without = storage.list(&listed(false)).await.unwrap();
    assert!(
        without.tasks[0].artifacts.is_empty(),
        "[{label}] list without"
    );

    let missing = storage
        .put_artifact(&tid("no-such-task"), said("a", "one"))
        .await;
    assert!(
        matches!(missing, Err(A2AError::TaskNotFound(_))),
        "[{label}] an unknown task: {missing:?}"
    );
}

/// Every backend runs every assertion above. A backend added later gets one
/// call here and inherits the lot.
macro_rules! parity_suite {
    ($name:ident, $label:expr, $build:expr) => {
        #[tokio::test]
        async fn $name() {
            let storage = $build.await;
            a_completed_task_keeps_its_message(&storage, $label).await;
            a_status_with_no_message_clears_the_last(&storage, $label).await;
            a_canceled_task_says_why(&storage, $label).await;
            a_kept_artifact_is_read_back(&storage, $label).await;
        }
    };
}

parity_suite!(in_memory_storage_is_consistent, "inmemory", async {
    a2a_rs::InMemoryTaskStorage::new()
});

#[cfg(feature = "sqlx-storage")]
parity_suite!(sqlx_storage_is_consistent, "sqlx/sqlite", async {
    use a2a_rs::adapter::storage::{DatabaseConfig, SqlxStorageBuilder};
    let config = DatabaseConfig::builder()
        .url("sqlite::memory:".to_string())
        .max_connections(1)
        .build();
    SqlxStorageBuilder::from_config(&config)
        .connect()
        .await
        .expect("sqlite in-memory storage")
});
