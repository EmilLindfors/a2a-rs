//! Push notification management port definitions

#[cfg(feature = "server")]
use async_trait::async_trait;

use crate::domain::{
    A2AError, DeleteTaskPushNotificationConfigParams, GetTaskPushNotificationConfigParams,
    ListTaskPushNotificationConfigsParams, TaskPushNotificationConfig,
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

/// A trait for managing push notification configurations and delivery
pub trait NotificationManager {
    /// Set up push notifications for a task
    fn set_task_notification(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError>;

    /// Get the push notification configuration for a task
    fn get_task_notification(&self, task_id: &str) -> Result<TaskPushNotificationConfig, A2AError>;

    /// Remove push notification configuration for a task
    fn remove_task_notification(&self, task_id: &str) -> Result<(), A2AError>;

    /// Check if push notifications are configured for a task
    fn has_task_notification(&self, task_id: &str) -> Result<bool, A2AError> {
        match self.get_task_notification(task_id) {
            Ok(_) => Ok(true),
            Err(A2AError::TaskNotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Validate push notification configuration
    fn validate_notification_config(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<(), A2AError> {
        validate_push_notification_url(config)
    }

    /// Send a test notification to verify configuration
    fn send_test_notification(&self, config: &TaskPushNotificationConfig) -> Result<(), A2AError> {
        // Default implementation - can be overridden
        self.validate_notification_config(config)?;
        // In a real implementation, this would send a test notification
        Ok(())
    }
}

/// Async management of push-notification configurations.
///
/// Expressed in terms of the A2A v1.0.0 multi-config CRUD model — the richest
/// shape — so a single capability covers both single- and multi-config storage.
/// Validation conveniences (URL/task-id checks) live on
/// [`AsyncNotificationManagerExt`], which is blanket-implemented for every
/// `AsyncNotificationManager`.
#[cfg(feature = "server")]
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
#[cfg(feature = "server")]
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

#[cfg(feature = "server")]
impl<T: AsyncNotificationManager + ?Sized> AsyncNotificationManagerExt for T {}
