//! Push notification sender implementation

// This module is already conditionally compiled with #[cfg(feature = "server")] in mod.rs

use std::sync::Arc;

use async_trait::async_trait;
#[cfg(feature = "http-client")]
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE},
    Client,
};
use tokio::sync::Mutex;

use crate::domain::{
    A2AError, PushNotificationConfig, TaskArtifactUpdateEvent, TaskStatusUpdateEvent,
};

/// Interface for a push notification sender
#[async_trait]
pub trait PushNotificationSender: Send + Sync {
    /// Send a status update notification
    async fn send_status_update(
        &self,
        config: &PushNotificationConfig,
        event: &TaskStatusUpdateEvent,
    ) -> Result<(), A2AError>;

    /// Send an artifact update notification
    async fn send_artifact_update(
        &self,
        config: &PushNotificationConfig,
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
    fn get_headers(&self, config: &PushNotificationConfig) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        // Add token if provided
        if let Some(token) = &config.token {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", token))
                    .unwrap_or_else(|_| HeaderValue::from_static("Invalid token")),
            );
        }

        // Add additional authentication headers if provided
        if let Some(auth) = &config.authentication {
            // Here we could add specific authentication headers based on the schemes
            // For now we just add the credentials if provided
            if let Some(credentials) = &auth.credentials {
                if !auth.schemes.is_empty() {
                    // Use the first scheme for simplicity
                    let scheme = &auth.schemes[0];

                    if scheme.to_lowercase() == "basic" {
                        headers.insert(
                            AUTHORIZATION,
                            HeaderValue::from_str(&format!("Basic {}", credentials))
                                .unwrap_or_else(|_| {
                                    HeaderValue::from_static("Invalid credentials")
                                }),
                        );
                    } else if scheme.to_lowercase() == "bearer" {
                        headers.insert(
                            AUTHORIZATION,
                            HeaderValue::from_str(&format!("Bearer {}", credentials))
                                .unwrap_or_else(|_| {
                                    HeaderValue::from_static("Invalid credentials")
                                }),
                        );
                    }
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
        config: &PushNotificationConfig,
        event: &TaskStatusUpdateEvent,
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

    async fn send_artifact_update(
        &self,
        config: &PushNotificationConfig,
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
        _config: &PushNotificationConfig,
        _event: &TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        // Do nothing - no-op implementation
        Ok(())
    }

    async fn send_artifact_update(
        &self,
        _config: &PushNotificationConfig,
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
    registry: Arc<Mutex<std::collections::HashMap<String, PushNotificationConfig>>>,
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
        config: PushNotificationConfig,
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
    ) -> Result<Option<PushNotificationConfig>, A2AError> {
        let registry = self.registry.lock().await;
        Ok(registry.get(task_id).cloned())
    }

    /// Send a status update notification for a task
    pub async fn send_status_update(
        &self,
        task_id: &str,
        event: &TaskStatusUpdateEvent,
    ) -> Result<(), A2AError> {
        let registry = self.registry.lock().await;

        if let Some(config) = registry.get(task_id) {
            self.sender.send_status_update(config, event).await?;
            Ok(())
        } else {
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
