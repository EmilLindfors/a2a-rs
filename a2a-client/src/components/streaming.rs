//! Server-Sent Events (SSE) streaming components

use a2a_rs::{RetryPolicy, StreamItem};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::StreamExt;
use std::{convert::Infallible, sync::Arc};
use tracing::{error, warn};

use crate::WebA2AClient;

/// Create an SSE stream of task updates for an Axum endpoint.
///
/// This is a thin serialization adapter over
/// [`WebA2AClient::subscribe_resilient`]: the reusable core owns reconnect +
/// exponential backoff and `Last-Event-ID` resumption, so this function only
/// maps each [`StreamItem`] to a typed Axum [`Event`] (`task-update` /
/// `task-status` / `artifact`), tagging it with the server event id so a browser
/// `EventSource` resumes automatically. The stream ends when the task reaches a
/// terminal state (or retries are exhausted).
pub fn create_sse_stream(
    client: Arc<WebA2AClient>,
    task_id: String,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let updates = client.subscribe_resilient(&task_id, RetryPolicy::default());

    let stream = updates.filter_map(|result| async move {
        let event = match result {
            Ok(event) => event,
            Err(e) => {
                warn!("Stream error (ending): {}", e);
                return None;
            }
        };

        let (event_type, data) = match &event.item {
            StreamItem::Task(task) => ("task-update", serde_json::to_string(task)),
            StreamItem::StatusUpdate(status) => ("task-status", serde_json::to_string(status)),
            StreamItem::ArtifactUpdate(artifact) => ("artifact", serde_json::to_string(artifact)),
        };

        match data {
            Ok(json) => {
                let mut sse = Event::default().event(event_type).data(json);
                if let Some(id) = event.event_id {
                    sse = sse.id(id.to_string());
                }
                Some(Ok(sse))
            }
            Err(e) => {
                error!("Failed to serialize {event_type}: {e}");
                None
            }
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}
