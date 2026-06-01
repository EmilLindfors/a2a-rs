//! Push notification sender implementation

// This module is already conditionally compiled with #[cfg(feature = "server")] in mod.rs

use std::sync::Arc;

use async_trait::async_trait;
#[cfg(feature = "http-client")]
use reqwest::{
    Client,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue},
};
use tokio::sync::Mutex;

use crate::domain::{
    A2AError, TaskArtifactUpdateEvent, TaskPushNotificationConfig, TaskStatusUpdateEvent,
};
use crate::port::AsyncPushNotifier;

/// Interface for a push notification sender
#[async_trait]
pub trait PushNotificationSender: Send + Sync {
    /// Send a status update notification
    async fn send_status_update(
        &self,
        config: &TaskPushNotificationConfig,
        event: &TaskStatusUpdateEvent,
    ) -> Result<(), A2AError>;

    /// Send an artifact update notification
    async fn send_artifact_update(
        &self,
        config: &TaskPushNotificationConfig,
        event: &TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError>;
}

/// HTTP-based push notification sender
#[cfg(feature = "http-client")]
pub struct HttpPushNotificationSender {
    /// HTTP client for sending notifications
    client: Client,
    /// Timeout in seconds
    timeout: u64,
    /// Maximum number of retries
    max_retries: u32,
    /// Backoff factor in milliseconds
    backoff_ms: u64,
}

#[cfg(feature = "http-client")]
impl Default for HttpPushNotificationSender {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "http-client")]
impl HttpPushNotificationSender {
    /// Create a new push notification sender
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            timeout: 30,      // Default timeout in seconds
            max_retries: 3,   // Default max retries
            backoff_ms: 1000, // Default backoff in milliseconds (1 second)
        }
    }

    /// Set the timeout for requests
    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the maximum number of retries
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Set the backoff factor in milliseconds
    pub fn with_backoff_ms(mut self, backoff_ms: u64) -> Self {
        self.backoff_ms = backoff_ms;
        self
    }

    /// Get the headers for a request
    fn get_headers(&self, config: &TaskPushNotificationConfig) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        // Add token if provided
        if !config.token.is_empty() {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", config.token))
                    .unwrap_or_else(|_| HeaderValue::from_static("Invalid token")),
            );
        }

        // Add additional authentication headers if provided
        if let Some(auth) = config.authentication.as_option() {
            // Here we could add specific authentication headers based on the schemes
            // For now we just add the credentials if provided
            if !auth.credentials.is_empty() && !auth.scheme.is_empty() {
                let scheme = &auth.scheme;

                if scheme.to_lowercase() == "basic" {
                    headers.insert(
                        AUTHORIZATION,
                        HeaderValue::from_str(&format!("Basic {}", auth.credentials))
                            .unwrap_or_else(|_| HeaderValue::from_static("Invalid credentials")),
                    );
                } else if scheme.to_lowercase() == "bearer" {
                    headers.insert(
                        AUTHORIZATION,
                        HeaderValue::from_str(&format!("Bearer {}", auth.credentials))
                            .unwrap_or_else(|_| HeaderValue::from_static("Invalid credentials")),
                    );
                }
            }
        }

        headers
    }
}

#[cfg(feature = "http-client")]
#[async_trait]
impl PushNotificationSender for HttpPushNotificationSender {
    async fn send_status_update(
        &self,
        config: &TaskPushNotificationConfig,
        event: &TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        let mut last_error = None;

        #[cfg(feature = "tracing")]
        tracing::debug!(
            task_id = %event.task_id,
            url = %config.url,
            "Preparing to send HTTP push notification"
        );

        // Try with retries
        for attempt in 0..=self.max_retries {
            // If this is a retry, wait with exponential backoff
            if attempt > 0 {
                let backoff = self.backoff_ms * (1 << (attempt - 1));
                #[cfg(feature = "tracing")]
                tracing::debug!(
                    task_id = %event.task_id,
                    attempt = attempt,
                    backoff_ms = backoff,
                    "Retrying push notification after backoff"
                );
                tokio::time::sleep(tokio::time::Duration::from_millis(backoff)).await;
            }

            // Send the notification
            #[cfg(feature = "tracing")]
            tracing::debug!(
                task_id = %event.task_id,
                attempt = attempt,
                url = %config.url,
                "Sending HTTP POST request for push notification"
            );

            match self
                .client
                .post(&config.url)
                .headers(self.get_headers(config))
                .json(event)
                .timeout(std::time::Duration::from_secs(self.timeout))
                .send()
                .await
            {
                Ok(response) => {
                    let status = response.status();
                    #[cfg(feature = "tracing")]
                    tracing::debug!(
                        task_id = %event.task_id,
                        status = %status,
                        "Received response from push notification endpoint"
                    );

                    // Check if the request was successful
                    if status.is_success() {
                        #[cfg(feature = "tracing")]
                        tracing::info!(
                            task_id = %event.task_id,
                            status = %status,
                            "Push notification HTTP request succeeded"
                        );
                        return Ok(());
                    } else {
                        let body = response.text().await.unwrap_or_default();
                        #[cfg(feature = "tracing")]
                        tracing::warn!(
                            task_id = %event.task_id,
                            status = %status,
                            body = %body,
                            "Push notification HTTP request failed"
                        );
                        last_error = Some(A2AError::Internal(format!(
                            "Push notification failed with status {}: {}",
                            status, body
                        )));

                        // Don't retry on client errors (4xx)
                        if status.is_client_error() {
                            break;
                        }
                    }
                }
                Err(e) => {
                    #[cfg(feature = "tracing")]
                    tracing::warn!(
                        task_id = %event.task_id,
                        error = %e,
                        "Failed to send HTTP request for push notification"
                    );
                    // Store the error but continue retrying
                    last_error = Some(A2AError::Internal(format!(
                        "Failed to send push notification: {}",
                        e
                    )));
                }
            }
        }

        // Return the last error if we had one
        Err(last_error.unwrap_or_else(|| {
            A2AError::Internal("Unknown error sending push notification".to_string())
        }))
    }

    async fn send_artifact_update(
        &self,
        config: &TaskPushNotificationConfig,
        event: &TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        let mut last_error = None;

        // Try with retries
        for attempt in 0..=self.max_retries {
            // If this is a retry, wait with exponential backoff
            if attempt > 0 {
                let backoff = self.backoff_ms * (1 << (attempt - 1));
                tokio::time::sleep(tokio::time::Duration::from_millis(backoff)).await;
            }

            // Send the notification
            match self
                .client
                .post(&config.url)
                .headers(self.get_headers(config))
                .json(event)
                .timeout(std::time::Duration::from_secs(self.timeout))
                .send()
                .await
            {
                Ok(response) => {
                    // Check if the request was successful
                    if response.status().is_success() {
                        return Ok(());
                    } else {
                        let status = response.status();
                        let body = response.text().await.unwrap_or_default();
                        last_error = Some(A2AError::Internal(format!(
                            "Push notification failed with status {}: {}",
                            status, body
                        )));

                        // Don't retry on client errors (4xx)
                        if status.is_client_error() {
                            break;
                        }
                    }
                }
                Err(e) => {
                    // Store the error but continue retrying
                    last_error = Some(A2AError::Internal(format!(
                        "Failed to send push notification: {}",
                        e
                    )));
                }
            }
        }

        // Return the last error if we had one
        Err(last_error.unwrap_or_else(|| {
            A2AError::Internal("Unknown error sending push notification".to_string())
        }))
    }
}

/// No-op push notification sender that does nothing
#[derive(Default)]
pub struct NoopPushNotificationSender;

#[async_trait]
impl PushNotificationSender for NoopPushNotificationSender {
    async fn send_status_update(
        &self,
        _config: &TaskPushNotificationConfig,
        _event: &TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        // Do nothing - no-op implementation
        Ok(())
    }

    async fn send_artifact_update(
        &self,
        _config: &TaskPushNotificationConfig,
        _event: &TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        // Do nothing - no-op implementation
        Ok(())
    }
}

/// In-memory push notification sender registry
pub struct PushNotificationRegistry {
    /// Sender for push notifications
    sender: Arc<dyn PushNotificationSender>,
    /// Registry of task IDs to push notification configs
    registry: Arc<Mutex<std::collections::HashMap<String, TaskPushNotificationConfig>>>,
}

impl PushNotificationRegistry {
    /// Create a new push notification registry
    pub fn new(sender: impl PushNotificationSender + 'static) -> Self {
        Self {
            sender: Arc::new(sender),
            registry: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Register a push notification configuration for a task
    pub async fn register(
        &self,
        task_id: &str,
        config: TaskPushNotificationConfig,
    ) -> Result<(), A2AError> {
        let mut registry = self.registry.lock().await;
        registry.insert(task_id.to_string(), config);
        Ok(())
    }

    /// Unregister a push notification configuration for a task
    pub async fn unregister(&self, task_id: &str) -> Result<(), A2AError> {
        let mut registry = self.registry.lock().await;
        registry.remove(task_id);
        Ok(())
    }

    /// Get the push notification configuration for a task
    pub async fn get_config(
        &self,
        task_id: &str,
    ) -> Result<Option<TaskPushNotificationConfig>, A2AError> {
        let registry = self.registry.lock().await;
        Ok(registry.get(task_id).cloned())
    }

    /// Send a status update notification for a task
    pub async fn send_status_update(
        &self,
        task_id: &str,
        event: &TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        let config = {
            let registry = self.registry.lock().await;
            registry.get(task_id).cloned()
        };

        if let Some(config) = config {
            #[cfg(feature = "tracing")]
            tracing::info!(
                task_id = %task_id,
                url = %config.url,
                state = ?event.status.state,
                "📤 Sending push notification for status update"
            );

            match self.sender.send_status_update(&config, event).await {
                Ok(()) => {
                    #[cfg(feature = "tracing")]
                    tracing::info!(
                        task_id = %task_id,
                        "✅ Push notification sent successfully"
                    );
                    Ok(())
                }
                Err(e) => {
                    #[cfg(feature = "tracing")]
                    tracing::error!(
                        task_id = %task_id,
                        error = %e,
                        "❌ Failed to send push notification"
                    );
                    Err(e)
                }
            }
        } else {
            #[cfg(feature = "tracing")]
            tracing::debug!(
                task_id = %task_id,
                "⚠️  No push notification config registered for task"
            );
            // No push notification configured for this task
            Ok(())
        }
    }

    /// Send an artifact update notification for a task
    pub async fn send_artifact_update(
        &self,
        task_id: &str,
        event: &TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        let registry = self.registry.lock().await;

        if let Some(config) = registry.get(task_id) {
            self.sender.send_artifact_update(config, event).await?;
            Ok(())
        } else {
            // No push notification configured for this task
            Ok(())
        }
    }
}

/// The registry is the in-house [`AsyncPushNotifier`] adapter: it looks up the
/// per-task config it holds and dispatches through its pluggable
/// [`PushNotificationSender`] backend. The two trait methods are exactly the
/// existing inherent `send_*` methods.
#[async_trait]
impl AsyncPushNotifier for PushNotificationRegistry {
    async fn notify_status(
        &self,
        task_id: &str,
        event: &TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        self.send_status_update(task_id, event).await
    }

    async fn notify_artifact(
        &self,
        task_id: &str,
        event: &TaskArtifactUpdateEvent,
    ) -> Result<(), A2AError> {
        self.send_artifact_update(task_id, event).await
    }
}
