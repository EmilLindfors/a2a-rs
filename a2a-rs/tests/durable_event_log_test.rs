//! The stream log in SQL: ids that survive a restart, a bounded tail, and a
//! sweep that takes the events with the context.
//!
//! The in-memory log is unit-tested where it lives. What only a real database
//! can answer is whether a client that reconnects to a *restarted* server gets
//! the events it missed rather than a second event 1 — which is the whole
//! reason the log exists apart from the fan-out.

#![cfg(feature = "sqlx-storage")]

use a2a_rs::adapter::storage::{SqlxStorageBuilder, SqlxTaskStorage};
use a2a_rs::adapter::{InMemoryEventLog, StreamingFanout};
use a2a_rs::domain::{
    A2AError, Artifact, RetentionPolicy, TaskArtifactUpdateEvent, TaskState, TaskStatus,
    TaskStatusUpdateEvent,
};
use a2a_rs::port::{
    AsyncEventLog, AsyncRetention, AsyncStreamingHandler, AsyncTaskLifecycle, UpdateEvent,
};
use futures::StreamExt;

const TASK: &str = "task-log";

fn tid(s: &str) -> a2a_rs::domain::TaskId {
    s.parse().unwrap()
}

fn cid(s: &str) -> a2a_rs::domain::ContextId {
    s.parse().unwrap()
}

fn status(state: TaskState) -> UpdateEvent {
    UpdateEvent::StatusUpdate(TaskStatusUpdateEvent {
        task_id: TASK.to_string(),
        context_id: "ctx".to_string(),
        kind: "status-update".to_string(),
        status: TaskStatus::new(state, None),
        metadata: None,
    })
}

fn state_of(event: &UpdateEvent) -> ::buffa::EnumValue<TaskState> {
    match event {
        UpdateEvent::StatusUpdate(update) => update.status.state,
        UpdateEvent::ArtifactUpdate(_) => panic!("expected a status update"),
    }
}

/// A file-backed store, so dropping it and opening it again is a restart.
fn file_url(dir: &tempfile::TempDir) -> String {
    format!("sqlite:{}?mode=rwc", dir.path().join("a2a.db").display())
}

async fn memory_store() -> Result<SqlxTaskStorage, A2AError> {
    SqlxStorageBuilder::from_config(
        &a2a_rs::adapter::storage::DatabaseConfig::builder()
            .url("sqlite::memory:".to_string())
            .max_connections(1)
            .build(),
    )
    .connect()
    .await
}

/// The point of the durable log: after a restart the ids carry on, so the
/// `Last-Event-ID` a client is holding still means what it meant.
#[tokio::test]
async fn ids_and_events_carry_across_a_restart() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let url = file_url(&dir);

    let store = SqlxTaskStorage::new(&url).await?;
    assert_eq!(
        store.append(TASK, status(TaskState::Submitted)).await?.id,
        1
    );
    assert_eq!(store.append(TASK, status(TaskState::Working)).await?.id, 2);
    drop(store);

    let restarted = SqlxTaskStorage::new(&url).await?;
    assert_eq!(
        restarted
            .append(TASK, status(TaskState::Completed))
            .await?
            .id,
        3,
        "an in-process counter would have handed out 1 again here"
    );

    let replay = restarted.replay(TASK, 1).await?;
    assert!(
        replay.complete,
        "nothing was trimmed, so the gap is covered"
    );
    assert_eq!(
        replay.events.iter().map(|e| e.id).collect::<Vec<_>>(),
        vec![2, 3]
    );
    assert_eq!(
        state_of(&replay.events[1].event),
        ::buffa::EnumValue::from(TaskState::Completed),
        "the payload survives the round trip, not just the id"
    );
    Ok(())
}

/// Both variants go through the same column pair, so both have to come back as
/// what they were.
#[tokio::test]
async fn an_artifact_update_round_trips() -> Result<(), Box<dyn std::error::Error>> {
    let store = memory_store().await?;
    store
        .append(
            TASK,
            UpdateEvent::ArtifactUpdate(TaskArtifactUpdateEvent {
                task_id: TASK.to_string(),
                context_id: "ctx".to_string(),
                kind: "artifact-update".to_string(),
                artifact: Artifact {
                    artifact_id: "a1".to_string(),
                    ..Default::default()
                },
                append: Some(true),
                last_chunk: Some(false),
                metadata: None,
            }),
        )
        .await?;

    let replay = store.replay(TASK, 0).await?;
    match &replay.events[0].event {
        UpdateEvent::ArtifactUpdate(update) => {
            assert_eq!(update.artifact.artifact_id, "a1");
            assert_eq!(update.append, Some(true));
        }
        other => panic!("expected an artifact update, got {other:?}"),
    }
    Ok(())
}

/// The cap bounds one task's rows, and a client that fell past it is told so
/// rather than handed the fragment that is left.
#[tokio::test]
async fn the_capacity_trims_the_oldest_events() -> Result<(), Box<dyn std::error::Error>> {
    let store = SqlxStorageBuilder::from_config(
        &a2a_rs::adapter::storage::DatabaseConfig::builder()
            .url("sqlite::memory:".to_string())
            .max_connections(1)
            .build(),
    )
    .event_log_capacity(Some(3))
    .connect()
    .await?;

    for _ in 0..8 {
        store.append(TASK, status(TaskState::Working)).await?;
    }

    let fallen_behind = store.replay(TASK, 2).await?;
    assert!(
        !fallen_behind.complete,
        "events 3..5 were trimmed, so this is a fragment of the gap"
    );
    assert_eq!(
        fallen_behind
            .events
            .iter()
            .map(|e| e.id)
            .collect::<Vec<_>>(),
        vec![6, 7, 8]
    );

    let covered = store.replay(TASK, 5).await?;
    assert!(covered.complete, "id 6 is still there, so the gap is whole");
    Ok(())
}

/// A `SubscribeToTask` that resumes over a durable log gets the same tail the
/// in-memory one would give — the fan-out does not care which log it holds.
#[tokio::test]
async fn the_fanout_resumes_over_a_durable_log() -> Result<(), Box<dyn std::error::Error>> {
    let store = memory_store().await?;
    let handler = StreamingFanout::over(store.clone());

    handler
        .broadcast_status_update(
            TASK,
            match status(TaskState::Working) {
                UpdateEvent::StatusUpdate(update) => update,
                _ => unreachable!(),
            },
        )
        .await?;
    handler
        .broadcast_status_update(
            TASK,
            match status(TaskState::Completed) {
                UpdateEvent::StatusUpdate(update) => update,
                _ => unreachable!(),
            },
        )
        .await?;

    let mut stream = handler.combined_update_stream(TASK, Some(1)).await?;
    let replayed = stream.next().await.expect("an event")?;
    assert_eq!(replayed.id, 2);
    assert_eq!(
        state_of(&replayed.event),
        ::buffa::EnumValue::from(TaskState::Completed)
    );

    // And the events are in the database, not only in this process.
    assert_eq!(store.replay(TASK, 0).await?.events.len(), 2);
    Ok(())
}

/// The log has no foreign key, so nothing deletes it on its own. A retention
/// sweep of the context is what reclaims it.
#[tokio::test]
async fn a_swept_context_takes_its_events_with_it() -> Result<(), Box<dyn std::error::Error>> {
    let store = memory_store().await?;
    store.create(&tid(TASK), &cid("ctx")).await?;
    // Finished, because an unfinished task holds its context back from a sweep.
    store
        .update_status(&tid(TASK), TaskState::Completed, None)
        .await?;
    store.append(TASK, status(TaskState::Completed)).await?;
    assert_eq!(store.replay(TASK, 0).await?.events.len(), 1);

    let policy = RetentionPolicy::keep_everything()
        .delete_contexts_idle_for(std::time::Duration::from_secs(7 * 24 * 3600));
    let swept = store
        .sweep(&policy, chrono::Utc::now() + chrono::Duration::days(30))
        .await?;
    assert_eq!(swept.contexts, 1);

    let replay = store.replay(TASK, 1).await?;
    assert!(replay.events.is_empty(), "the events went with the context");
    assert!(
        !replay.complete,
        "a client still holding an id from before the sweep cannot be covered"
    );
    Ok(())
}

/// The contrast the durable log exists for, as a test rather than a claim: a
/// second fan-out over the same database resumes a client that the in-memory
/// default would have to start over.
#[tokio::test]
async fn a_restarted_fanout_resumes_only_over_the_durable_log()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let url = file_url(&dir);
    let working = match status(TaskState::Working) {
        UpdateEvent::StatusUpdate(update) => update,
        _ => unreachable!(),
    };

    let store = SqlxTaskStorage::new(&url).await?;
    StreamingFanout::over(store.clone())
        .broadcast_status_update(TASK, working.clone())
        .await?;
    drop(store);

    // The server restarts: new process, new fan-out, same database.
    let restarted = StreamingFanout::over(SqlxTaskStorage::new(&url).await?);
    let mut stream = restarted.combined_update_stream(TASK, Some(0)).await?;
    let replayed = stream.next().await.expect("an event")?;
    assert_eq!(
        replayed.id, 1,
        "the event outlived the process that sent it"
    );

    // The same restart over the in-memory default has nothing to replay, and
    // the next event it logs reuses an id the client has already seen.
    let fresh = InMemoryEventLog::new();
    assert!(fresh.replay(TASK, 1).await?.events.is_empty());
    assert_eq!(
        fresh.append(TASK, status(TaskState::Completed)).await?.id,
        1
    );
    Ok(())
}
