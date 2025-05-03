//! Push notification sender implementation

#![cfg(feature = "server")]

use std::sync::Arc;

use async_trait::async_trait;
use reqwest::{Client, header::{HeaderMap, HeaderValue, CONTENT_TYPE, AUTHORIZATION}};
use tokio::sync::Mutex;

use crate::domain::{
    A2AError, PushNotificationConfig, TaskArtifactUpdateEvent, TaskStatusUpdateEvent,
};

/// Interface for a push notification sender
#[async_trait]
pub trait PushNotificationSender: Send + Sync {
    /// Send a status update notification
    async fn send_status_update(&self, config: &PushNotificationConfig, event: &TaskStatusUpdateEvent) -> Result<(), A2AError>;
    
    /// Send an artifact update notification
    async fn send_artifact_update(&self, config: &PushNotificationConfig, event: &TaskArtifactUpdateEvent) -> Result<(), A2AError>;
}

/// HTTP-based push notification sender
pub struct HttpPushNotificationSender {
    /// HTTP client for sending notifications
    client: Client,
    /// Timeout in seconds
    timeout: u64,
}

impl Default for HttpPushNotificationSender {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpPushNotificationSender {
    /// Create a new push notification sender
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            timeout: 30, // Default timeout in seconds
        }
    }
    
    /// Set the timeout for requests
    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = timeout;
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
                                .unwrap_or_else(|_| HeaderValue::from_static("Invalid credentials")),
                        );
                    } else if scheme.to_lowercase() == "bearer" {
                        headers.insert(
                            AUTHORIZATION,
                            HeaderValue::from_str(&format!("Bearer {}", credentials))
                                .unwrap_or_else(|_| HeaderValue::from_static("Invalid credentials")),
                        );
                    }
                }
            }
        }
        
        headers
    }
}

#[async_trait]
impl PushNotificationSender for HttpPushNotificationSender {
    async fn send_status_update(&self, config: &PushNotificationConfig, event: &TaskStatusUpdateEvent) -> Result<(), A2AError> {
        // Send the notification
        let response = self.client
            .post(&config.url)
            .headers(self.get_headers(config))
            .json(event)
            .timeout(std::time::Duration::from_secs(self.timeout))
            .send()
            .await
            .map_err(|e| A2AError::Internal(format!("Failed to send push notification: {}", e)))?;
        
        // Check if the request was successful
        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(A2AError::Internal(format!(
                "Push notification failed with status {}: {}",
                status, body
            )))
        }
    }
    
    async fn send_artifact_update(&self, config: &PushNotificationConfig, event: &TaskArtifactUpdateEvent) -> Result<(), A2AError> {
        // Send the notification
        let response = self.client
            .post(&config.url)
            .headers(self.get_headers(config))
            .json(event)
            .timeout(std::time::Duration::from_secs(self.timeout))
            .send()
            .await
            .map_err(|e| A2AError::Internal(format!("Failed to send push notification: {}", e)))?;
        
        // Check if the request was successful
        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(A2AError::Internal(format!(
                "Push notification failed with status {}: {}",
                status, body
            )))
        }
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
    pub async fn register(&self, task_id: &str, config: PushNotificationConfig) -> Result<(), A2AError> {
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
    
    /// Send a status update notification for a task
    pub async fn send_status_update(&self, task_id: &str, event: &TaskStatusUpdateEvent) -> Result<(), A2AError> {
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
    pub async fn send_artifact_update(&self, task_id: &str, event: &TaskArtifactUpdateEvent) -> Result<(), A2AError> {
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