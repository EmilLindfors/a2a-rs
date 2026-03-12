//! Push notification management port definitions

#[cfg(feature = "server")]
use async_trait::async_trait;

use crate::domain::{A2AError, PushNotificationConfig, TaskIdParams, TaskPushNotificationConfig};

/// Validate a push notification config URL.
///
/// Checks that the URL is non-empty, well-formed, and uses HTTPS
/// (HTTP is allowed only for localhost for development purposes).
fn validate_push_notification_url(config: &PushNotificationConfig) -> Result<(), A2AError> {
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
        config: &PushNotificationConfig,
    ) -> Result<(), A2AError> {
        validate_push_notification_url(config)
    }

    /// Send a test notification to verify configuration
    fn send_test_notification(&self, config: &PushNotificationConfig) -> Result<(), A2AError> {
        // Default implementation - can be overridden
        self.validate_notification_config(config)?;
        // In a real implementation, this would send a test notification
        Ok(())
    }
}

#[cfg(feature = "server")]
#[async_trait]
/// An async trait for managing push notification configurations and delivery
pub trait AsyncNotificationManager: Send + Sync {
    /// Set up push notifications for a task
    async fn set_task_notification(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError>;

    /// Get the push notification configuration for a task
    async fn get_task_notification(
        &self,
        task_id: &str,
    ) -> Result<TaskPushNotificationConfig, A2AError>;

    /// Remove push notification configuration for a task
    async fn remove_task_notification(&self, task_id: &str) -> Result<(), A2AError>;

    /// Check if push notifications are configured for a task
    async fn has_task_notification(&self, task_id: &str) -> Result<bool, A2AError> {
        match self.get_task_notification(task_id).await {
            Ok(_) => Ok(true),
            Err(A2AError::TaskNotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Validate push notification configuration
    async fn validate_notification_config(
        &self,
        config: &PushNotificationConfig,
    ) -> Result<(), A2AError> {
        validate_push_notification_url(config)
    }

    /// Send a test notification to verify configuration
    async fn send_test_notification(
        &self,
        config: &PushNotificationConfig,
    ) -> Result<(), A2AError> {
        // Default implementation - can be overridden
        self.validate_notification_config(config).await?;
        // In a real implementation, this would send a test notification
        Ok(())
    }

    /// Set task notification with validation
    async fn set_task_notification_validated(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        // Validate the task ID
        if config.task_id.trim().is_empty() {
            return Err(A2AError::ValidationError {
                field: "task_id".to_string(),
                message: "Task ID cannot be empty".to_string(),
            });
        }

        // Validate the notification config
        self.validate_notification_config(&config.push_notification_config)
            .await?;

        // Set the notification
        self.set_task_notification(config).await
    }

    /// Get task notification with validation
    async fn get_task_notification_validated(
        &self,
        params: &TaskIdParams,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        if params.id.trim().is_empty() {
            return Err(A2AError::ValidationError {
                field: "task_id".to_string(),
                message: "Task ID cannot be empty".to_string(),
            });
        }

        self.get_task_notification(&params.id).await
    }

    /// Send notification for task status update
    async fn notify_task_status_update(
        &self,
        task_id: &str,
        _status_update: &crate::domain::TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        // Default implementation - can be overridden
        // Check if notifications are configured for this task
        if !self.has_task_notification(task_id).await? {
            return Ok(()); // No notification configured, silently succeed
        }

        // In a real implementation, this would send the actual notification
        // For now, just validate that we have the configuration
        let _config = self.get_task_notification(task_id).await?;

        Ok(())
    }

    /// Send notification for task artifact update
    async fn notify_task_artifact_update(
        &self,
        task_id: &str,
        _artifact_update: &crate::domain::TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        // Default implementation - can be overridden
        // Check if notifications are configured for this task
        if !self.has_task_notification(task_id).await? {
            return Ok(()); // No notification configured, silently succeed
        }

        // In a real implementation, this would send the actual notification
        // For now, just validate that we have the configuration
        let _config = self.get_task_notification(task_id).await?;

        Ok(())
    }
}
