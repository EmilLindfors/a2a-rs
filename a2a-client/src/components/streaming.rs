//! Server-Sent Events (SSE) streaming components

use a2a_rs::StreamItem;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::StreamExt;
use std::{convert::Infallible, sync::Arc, time::Duration};
use tracing::{error, info, warn};

use crate::WebA2AClient;

/// Create an SSE stream for task updates
///
/// This function handles:
/// - WebSocket streaming if available
/// - Fallback to HTTP polling
/// - Automatic retry logic
/// - Serialization to JSON events
pub fn create_sse_stream(
    client: Arc<WebA2AClient>,
    task_id: String,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        info!("Attempting to subscribe to task {} via HTTP stream", task_id);

        let mut retry_count: u32 = 0;
        let max_retries = 15; // Covers ~2 minutes using exponential backoff
        let base_delay = Duration::from_millis(500);
        let max_delay = Duration::from_secs(10);
        let mut is_terminal = false;

        loop {
            match client.transport.subscribe_to_task(&task_id, Some(50)).await {
                Ok(mut event_stream) => {
                    info!("Successfully subscribed to task {} via HTTP stream", task_id);
                    retry_count = 0; // Reset retries on successful connection

                    while let Some(result) = event_stream.next().await {
                        match result {
                            Ok(stream_item) => {
                                use a2a_rs::domain::TaskStateExt;
                                let (event_type, event_data) = match &stream_item {
                                    StreamItem::Task(task) => {
                                        if let Some(status) = task.status.as_option() {
                                            if status.state.is_terminal() {
                                                is_terminal = true;
                                            }
                                        }
                                        match serde_json::to_string(task) {
                                            Ok(json) => ("task-update", json),
                                            Err(e) => {
                                                error!("Failed to serialize task: {}", e);
                                                continue;
                                            }
                                        }
                                    }
                                    StreamItem::StatusUpdate(status) => {
                                        if status.status.state.is_terminal() {
                                            is_terminal = true;
                                        }
                                        match serde_json::to_string(status) {
                                            Ok(json) => ("task-status", json),
                                            Err(e) => {
                                                error!("Failed to serialize status: {}", e);
                                                continue;
                                            }
                                        }
                                    }
                                    StreamItem::ArtifactUpdate(artifact) => {
                                        match serde_json::to_string(artifact) {
                                            Ok(json) => ("artifact", json),
                                            Err(e) => {
                                                error!("Failed to serialize artifact: {}", e);
                                                continue;
                                            }
                                        }
                                    }
                                };

                                yield Ok(Event::default()
                                    .event(event_type)
                                    .data(event_data));
                            }
                            Err(e) => {
                                warn!("Stream error (continuing): {}", e);
                                continue;
                            }
                        }
                    }

                    if is_terminal {
                        info!("Task {} reached terminal state. Ending stream gracefully.", task_id);
                        break;
                    } else {
                        warn!("Stream ended prematurely for task {}. Retrying...", task_id);
                        // Do not break; it will loop around and reconnect.
                    }
                }
                Err(e) => {
                    retry_count += 1;

                    if retry_count <= max_retries {
                        // Exponential delay: base_delay * 2^(retry_count - 1)
                        let factor = 2u64.saturating_pow(retry_count.saturating_sub(1).min(6));
                        let delay = base_delay.saturating_mul(factor as u32);

                        // Jitter calculation (0-200ms) based on task_id bytes and system time
                        let jitter_ms = {
                            let mut state = 0u64;
                            for &b in task_id.as_bytes() {
                                state = state.wrapping_mul(6364136223846793005).wrapping_add(b as u64);
                            }
                            let time_ms = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64;
                            state = state.wrapping_mul(6364136223846793005).wrapping_add(time_ms);
                            state % 200
                        };
                        let final_delay = delay.saturating_add(Duration::from_millis(jitter_ms)).min(max_delay);

                        warn!(
                            "Failed to subscribe to task {} (attempt {}/{}): {}. Retrying in {:?}...",
                            task_id, retry_count, max_retries, e, final_delay
                        );

                        tokio::time::sleep(final_delay).await;
                        continue;
                    } else {
                        warn!("Failed to subscribe after {} retries: {}, aborting stream", max_retries, e);
                        break;
                    }
                }
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}
