//! Push notification management port definitions

use async_trait::async_trait;

use crate::domain::{
    A2AError, DeleteTaskPushNotificationConfigParams, GetTaskPushNotificationConfigParams,
    ListTaskPushNotificationConfigsParams, TaskArtifactUpdateEvent, TaskPushNotificationConfig,
    TaskStatusUpdateEvent,
};

/// Validate a push notification config URL.
///
/// Checks that the URL is non-empty, well-formed, and uses HTTPS
/// (HTTP is allowed only for localhost for development purposes).
fn validate_push_notification_url(config: &TaskPushNotificationConfig) -> Result<(), A2AError> {
    if config.url.trim().is_empty() {
        return Err(A2AError::ValidationError {
            field: "url".to_string(),
            message: "Webhook URL cannot be empty".to_string(),
        });
    }

    match url::Url::parse(&config.url) {
        Ok(parsed_url) => {
            let scheme = parsed_url.scheme();
            if scheme != "https" {
                let is_localhost = parsed_url
                    .host_str()
                    .map(|h| h == "localhost" || h == "127.0.0.1" || h == "::1")
                    .unwrap_or(false);

                if scheme != "http" || !is_localhost {
                    return Err(A2AError::ValidationError {
                        field: "url".to_string(),
                        message: "Webhook URL must use HTTPS (HTTP is only allowed for localhost)"
                            .to_string(),
                    });
                }
            }
        }
        Err(_) => {
            return Err(A2AError::ValidationError {
                field: "url".to_string(),
                message: "Invalid webhook URL format".to_string(),
            });
        }
    }

    Ok(())
}

/// Async management of push-notification configurations.
///
/// Expressed in terms of the A2A v1.0.0 multi-config CRUD model — the richest
/// shape — so a single capability covers both single- and multi-config storage.
/// Validation conveniences (URL/task-id checks) live on
/// [`AsyncNotificationManagerExt`], which is blanket-implemented for every
/// `AsyncNotificationManager`.
#[async_trait]
pub trait AsyncNotificationManager: Send + Sync {
    /// Create or replace a push-notification config, returning it with any
    /// server-assigned ID populated.
    async fn set_config(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError>;

    /// Get a push-notification config for a task.
    async fn get_config(
        &self,
        params: &GetTaskPushNotificationConfigParams,
    ) -> Result<TaskPushNotificationConfig, A2AError>;

    /// List all push-notification configs for a task.
    async fn list_configs(
        &self,
        params: &ListTaskPushNotificationConfigsParams,
    ) -> Result<Vec<TaskPushNotificationConfig>, A2AError>;

    /// Delete a push-notification config. Idempotent per the v1.0.0 spec.
    async fn delete_config(
        &self,
        params: &DeleteTaskPushNotificationConfigParams,
    ) -> Result<(), A2AError>;
}

/// Validation conveniences over [`AsyncNotificationManager`].
///
/// Blanket-implemented for every `AsyncNotificationManager`, so implementors
/// only stub the core CRUD primitives.
#[async_trait]
pub trait AsyncNotificationManagerExt: AsyncNotificationManager {
    /// Validate a push-notification config's webhook URL.
    fn validate_config(&self, config: &TaskPushNotificationConfig) -> Result<(), A2AError> {
        validate_push_notification_url(config)
    }

    /// Validate the task ID and webhook URL, then store the config.
    async fn set_validated(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        if config.task_id.trim().is_empty() {
            return Err(A2AError::ValidationError {
                field: "task_id".to_string(),
                message: "Task ID cannot be empty".to_string(),
            });
        }
        self.validate_config(config)?;
        self.set_config(config).await
    }
}

impl<T: AsyncNotificationManager + ?Sized> AsyncNotificationManagerExt for T {}

/// Out-of-band delivery of task updates to a task's configured push endpoint.
///
/// This is the **delivery** half of push notifications, deliberately separate
/// from the config-CRUD capability ([`AsyncNotificationManager`]) and from the
/// in-process streaming fan-out ([`AsyncStreamingHandler`](crate::port::AsyncStreamingHandler)).
/// Keeping delivery behind its own port is what lets the orchestration layer
/// (the [`TaskStatusBroadcast`](crate::application::TaskStatusBroadcast) mixin)
/// "commit, announce to subscribers, then notify the webhook" without any one
/// adapter taking on a second job — and lets the notification backend be swapped
/// freely (HTTP webhook, no-op, a queue, a test spy) at the composition edge.
///
/// Errors are surfaced to the caller, but the orchestration layer treats
/// delivery as best-effort: a webhook that is down must not fail the task
/// mutation that triggered it.
#[async_trait]
pub trait AsyncPushNotifier: Send + Sync {
    /// Deliver a status update to the task's configured push endpoint, if any.
    ///
    /// A task with no registered config is not an error — implementations
    /// return `Ok(())`.
    async fn notify_status(
        &self,
        task_id: &str,
        event: &TaskStatusUpdateEvent,
    ) -> Result<(), A2AError>;

    /// Deliver an artifact update to the task's configured push endpoint, if any.
    async fn notify_artifact(
        &self,
        task_id: &str,
        event: &TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError>;
}

/// Deref-forwarding impl so an `Arc<dyn AsyncPushNotifier>` (e.g. the value
/// handed out by `InMemoryTaskStorage::push_notifier`) satisfies `impl
/// AsyncPushNotifier` bounds directly, without re-wrapping.
#[async_trait]
impl<T: AsyncPushNotifier + ?Sized> AsyncPushNotifier for std::sync::Arc<T> {
    async fn notify_status(
        &self,
        task_id: &str,
        event: &TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        (**self).notify_status(task_id, event).await
    }

    async fn notify_artifact(
        &self,
        task_id: &str,
        event: &TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        (**self).notify_artifact(task_id, event).await
    }
}

/// A no-op [`AsyncPushNotifier`] for compositions with no push backend wired.
///
/// Every method succeeds without doing anything, mirroring `NoopStreamingHandler`
/// on the streaming side.
#[derive(Clone, Debug, Default)]
pub struct NoopPushNotifier;

#[async_trait]
impl AsyncPushNotifier for NoopPushNotifier {
    async fn notify_status(
        &self,
        _task_id: &str,
        _event: &TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        Ok(())
    }

    async fn notify_artifact(
        &self,
        _task_id: &str,
        _event: &TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        Ok(())
    }
}
