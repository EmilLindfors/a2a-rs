//! In-memory task storage implementation

#![cfg(feature = "server")]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::domain::{
    task, A2AError, Artifact, Message, Task, TaskArtifactUpdateEvent, TaskIdParams, TaskPushNotificationConfig, TaskQueryParams, TaskSendParams, TaskState, TaskStatus, TaskStatusUpdateEvent
};
use crate::port::server::AsyncTaskHandler;

/// Unique identifier for a subscriber
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SubscriberId(Uuid);

impl SubscriberId {
    fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// A channel-based subscriber for task status updates
struct StatusSubscriber {
    id: SubscriberId,
    task_id: String,
    sender: mpsc::Sender<TaskStatusUpdateEvent>,
}

/// A channel-based subscriber for task artifact updates
struct ArtifactSubscriber {
    id: SubscriberId,
    task_id: String,
    sender: mpsc::Sender<TaskArtifactUpdateEvent>,
}

/// Collection of subscribers for a task
struct TaskSubscribers {
    status: HashMap<SubscriberId, mpsc::Sender<TaskStatusUpdateEvent>>,
    artifacts: HashMap<SubscriberId, mpsc::Sender<TaskArtifactUpdateEvent>>,
}

impl TaskSubscribers {
    fn new() -> Self {
        Self {
            status: HashMap::new(),
            artifacts: HashMap::new(),
        }
    }

    fn add_status_subscriber(&mut self, subscriber: StatusSubscriber) -> SubscriberId {
        let id = subscriber.id;
        self.status.insert(id, subscriber.sender);
        id
    }

    fn add_artifact_subscriber(&mut self, subscriber: ArtifactSubscriber) -> SubscriberId {
        let id = subscriber.id;
        self.artifacts.insert(id, subscriber.sender);
        id
    }

    fn remove_status_subscriber(&mut self, id: SubscriberId) -> bool {
        self.status.remove(&id).is_some()
    }

    fn remove_artifact_subscriber(&mut self, id: SubscriberId) -> bool {
        self.artifacts.remove(&id).is_some()
    }

    fn broadcast_status(&self, event: TaskStatusUpdateEvent) {
        for (id, sender) in &self.status {
            match sender.try_send(event.clone()) {
                Ok(_) => {}
                Err(e) => {
                    match e {
                        mpsc::error::TrySendError::Closed(_) => {
                            // Channel is closed, subscriber is gone
                            debug!("Artifact subscriber channel closed: {:?}", id);
                        }
                        mpsc::error::TrySendError::Full(_) => {
                            // Channel is full, subscriber is backlogged
                            warn!("Artifact subscriber channel full: {:?}", id);
                        }
                      }
                }
            }
        }
    }

    fn broadcast_artifact(&self, event: TaskArtifactUpdateEvent) {
        for (id, sender) in &self.artifacts {
            match sender.try_send(event.clone()) {
                Ok(_) => {}
                Err(e) => {
                  match e {
                    mpsc::error::TrySendError::Closed(_) => {
                        // Channel is closed, subscriber is gone
                        debug!("Artifact subscriber channel closed: {:?}", id);
                    }
                    mpsc::error::TrySendError::Full(_) => {
                        // Channel is full, subscriber is backlogged
                        warn!("Artifact subscriber channel full: {:?}", id);
                    }
                  }
                }
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.status.is_empty() && self.artifacts.is_empty()
    }

    fn clean_closed_channels(&mut self) {
        // Remove status subscribers with closed channels
        self.status.retain(|id, sender| {
            let is_open = !sender.is_closed();
            if !is_open {
                debug!("Removing closed status subscriber channel: {:?}", id);
            }
            is_open
        });

        // Remove artifact subscribers with closed channels
        self.artifacts.retain(|id, sender| {
            let is_open = !sender.is_closed();
            if !is_open {
                debug!("Removing closed artifact subscriber channel: {:?}", id);
            }
            is_open
        });
    }
}

/// Additional metadata for tasks not directly part of the Task struct
struct TaskMetadata {
    /// When the task was created
    created_at: chrono::DateTime<Utc>,
    /// Last access time
    last_accessed: chrono::DateTime<Utc>,
    /// Number of updates to this task
    update_count: u32,
}

impl TaskMetadata {
    fn new() -> Self {
        Self {
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            update_count: 0,
        }
    }

    fn mark_accessed(&mut self) {
        self.last_accessed = Utc::now();
        self.update_count += 1;
    }
}

/// Simple in-memory task storage with channel-based subscriptions
pub struct InMemoryTaskStorage {
    /// Tasks stored by ID
    tasks: Arc<RwLock<HashMap<String, Task>>>,
    /// Subscribers for task updates
    subscribers: Arc<RwLock<HashMap<String, TaskSubscribers>>>,
    /// Push notification configurations by task ID
    push_notifications: Arc<RwLock<HashMap<String, TaskPushNotificationConfig>>>,
    /// Task metadata to track additional info
    task_metadata: Arc<RwLock<HashMap<String, TaskMetadata>>>,
    /// Interval for garbage collection of closed channels
    gc_interval: Duration,
    /// Last time garbage collection was run
    last_gc: Arc<RwLock<chrono::DateTime<Utc>>>,
}

impl InMemoryTaskStorage {
    /// Create a new empty task storage
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            push_notifications: Arc::new(RwLock::new(HashMap::new())),
            task_metadata: Arc::new(RwLock::new(HashMap::new())),
            gc_interval: Duration::from_secs(300), // 5 minutes
            last_gc: Arc::new(RwLock::new(Utc::now())),
        }
    }

    /// Create a new task storage with a custom garbage collection interval
    pub fn with_gc_interval(gc_interval: Duration) -> Self {
        let mut storage = Self::new();
        storage.gc_interval = gc_interval;
        storage
    }

    /// Update task metadata
    async fn update_task_metadata(&self, task_id: &str) {
        let mut metadata = self.task_metadata.write().await;
        if let Some(task_meta) = metadata.get_mut(task_id) {
            task_meta.mark_accessed();
        } else {
            metadata.insert(task_id.to_string(), TaskMetadata::new());
        }
    }

    /// Run garbage collection if needed
    async fn maybe_run_gc(&self) {
        let now = Utc::now();
        let should_run = {
            let last_gc = self.last_gc.read().await;
            (now - *last_gc).to_std().unwrap_or_default() > self.gc_interval
        };

        if should_run {
            debug!("Running garbage collection for task subscribers");
            let mut last_gc = self.last_gc.write().await;
            *last_gc = now;

            let mut subscribers = self.subscribers.write().await;
            
            // Clean subscribers for each task
            for (task_id, task_subs) in subscribers.iter_mut() {
                task_subs.clean_closed_channels();
            }

            // Remove empty subscriber collections
            subscribers.retain(|_, subs| !subs.is_empty());
        }
    }

    /// Send a status update to all subscribers for a task
    async fn broadcast_status_update(
        &self,
        task_id: &str,
        status: TaskStatus,
        final_: bool,
    ) -> Result<(), A2AError> {
        // Create the update event
        let event = TaskStatusUpdateEvent {
            id: task_id.to_string(),
            status,
            final_,
            metadata: None,
        };

        // Get subscribers and broadcast
        {
            let subscribers = self.subscribers.read().await;
            if let Some(task_subscribers) = subscribers.get(task_id) {
                // We don't need to hold the lock for long - just get the subscribers
                task_subscribers.broadcast_status(event);
            }
        }

        // Periodically clean up subscribers
        self.maybe_run_gc().await;

        Ok(())
    }

    /// Send an artifact update to all subscribers for a task
    async fn broadcast_artifact_update(
        &self,
        task_id: &str,
        artifact: Artifact,
    ) -> Result<(), A2AError> {
        // Create the update event
        let event = TaskArtifactUpdateEvent {
            id: task_id.to_string(),
            artifact,
            metadata: None,
        };

        // Get subscribers and broadcast
        {
            let subscribers = self.subscribers.read().await;
            if let Some(task_subscribers) = subscribers.get(task_id) {
                // We don't need to hold the lock for long - just get the subscribers
                task_subscribers.broadcast_artifact(event);
            }
        }

        Ok(())
    }

    /// Add a status subscriber to a task
    pub async fn subscribe_to_task_status(
        &self,
        task_id: &str,
        buffer_size: usize,
    ) -> Result<mpsc::Receiver<TaskStatusUpdateEvent>, A2AError> {
        // Create a channel for the subscriber
        let (sender, receiver) = mpsc::channel(buffer_size);

        // Create a subscriber
        let subscriber = StatusSubscriber {
            id: SubscriberId::new(),
            task_id: task_id.to_string(),
            sender,
        };

        // Add the subscriber
        {
            let mut subscribers = self.subscribers.write().await;
            let task_subscribers = subscribers
                .entry(task_id.to_string())
                .or_insert_with(TaskSubscribers::new);

            task_subscribers.add_status_subscriber(subscriber);
        }

        // Send initial status
        let task = self.get_task(task_id, None).await?;
        self.broadcast_status_update(task_id, task.status, false).await?;

        Ok(receiver)
    }

    /// Add an artifact subscriber to a task
    pub async fn subscribe_to_task_artifacts(
        &self,
        task_id: &str,
        buffer_size: usize,
    ) -> Result<mpsc::Receiver<TaskArtifactUpdateEvent>, A2AError> {
        // Create a channel for the subscriber
        let (sender, receiver) = mpsc::channel(buffer_size);

        // Create a subscriber
        let subscriber = ArtifactSubscriber {
            id: SubscriberId::new(),
            task_id: task_id.to_string(),
            sender,
        };

        // Add the subscriber
        {
            let mut subscribers = self.subscribers.write().await;
            let task_subscribers = subscribers
                .entry(task_id.to_string())
                .or_insert_with(TaskSubscribers::new);

            task_subscribers.add_artifact_subscriber(subscriber);
        }

        // Send initial artifacts
        let task = self.get_task(task_id, None).await?;
        if let Some(artifacts) = task.artifacts {
            for artifact in artifacts {
                self.broadcast_artifact_update(task_id, artifact).await?;
            }
        }

        Ok(receiver)
    }
}

#[async_trait]
impl AsyncTaskHandler for InMemoryTaskStorage {
    async fn handle_message<'a>(
        &self,
        task_id: &'a str,
        message: &'a Message,
        session_id: Option<&'a str>,
    ) -> Result<Task, A2AError> {
        // Get or create the task
        let mut task = {
            // First try with a read lock to check if the task exists
            let tasks_guard = self.tasks.read().await;

            if let Some(existing_task) = tasks_guard.get(task_id) {
                existing_task.clone()
            } else {
                // Task doesn't exist, drop read lock and acquire write lock
                drop(tasks_guard);

                let mut tasks_guard = self.tasks.write().await;

                // Check again in case another thread created it while we were waiting
                if let Some(existing_task) = tasks_guard.get(task_id) {
                    existing_task.clone()
                } else {
                    // Create a new task
                    let mut new_task = Task::new(task_id.to_string());

                    // Add session ID if provided
                    if let Some(sid) = session_id {
                        new_task.session_id = Some(sid.to_string());
                    }

                    // Add user message to history
                    new_task.add_to_history(message.clone());

                    // Insert it
                    tasks_guard.insert(task_id.to_string(), new_task.clone());

                    // Create metadata for the new task
                    self.task_metadata
                        .write()
                        .await
                        .insert(task_id.to_string(), TaskMetadata::new());

                    new_task
                }
            }
        }; // Lock is dropped here

        // Update the task with the message
        task.update_status(TaskState::Working, Some(message.clone()));

        // Update the task in storage
        {
            let mut tasks_guard = self.tasks.write().await;
            tasks_guard.insert(task_id.to_string(), task.clone());
        } // Lock is dropped here

        // Update metadata
        self.update_task_metadata(task_id).await;

        // Broadcast status update
        self.broadcast_status_update(task_id, task.status.clone(), false)
            .await?;

        Ok(task)
    }

    async fn get_task<'a>(
        &self,
        task_id: &'a str,
        history_length: Option<u32>,
    ) -> Result<Task, A2AError> {
        // Get the task - only need read lock
        let mut task = {
            let tasks_guard = self.tasks.read().await;

            if let Some(task) = tasks_guard.get(task_id) {
                task.clone()
            } else {
                return Err(A2AError::TaskNotFound(task_id.to_string()));
            }
        }; // Lock is dropped here

        // Update metadata
        self.update_task_metadata(task_id).await;

        // Limit history if requested
        if let Some(length) = history_length {
            task.limit_history(length);
        }

        Ok(task)
    }

    async fn cancel_task<'a>(&self, task_id: &'a str) -> Result<Task, A2AError> {
        // Get the task with a read lock first
        let mut task = {
            let tasks_guard = self.tasks.read().await;
            
            match tasks_guard.get(task_id) {
                Some(task) => task.clone(),
                None => return Err(A2AError::TaskNotFound(task_id.to_string())),
            }
        }; // Read lock is dropped here
        
        // Verify if the task can be canceled
        if task.status.state != TaskState::Working {
            return Err(A2AError::TaskNotCancelable(format!(
                "Task {} is in state {:?} and cannot be canceled",
                task_id, task.status.state
            )));
        }
        
        // Update the task status
        task.update_status(TaskState::Canceled, None);
        
        // Update the task in storage with write lock
        {
            let mut tasks_guard = self.tasks.write().await;
            tasks_guard.insert(task_id.to_string(), task.clone());
        } // Write lock is dropped here
        
        // Update metadata
        self.update_task_metadata(task_id).await;

        // Broadcast status update (with final flag set to true)
        self.broadcast_status_update(task_id, task.status.clone(), true)
            .await?;

        Ok(task)
    }

    async fn set_push_notification<'a>(
        &self,
        config: &'a TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        // Store the push notification config
        {
            let mut push_guard = self.push_notifications.write().await;
            push_guard.insert(config.id.clone(), config.clone());
        } // Lock is dropped here

        Ok(config.clone())
    }

    async fn get_push_notification<'a>(
        &self,
        task_id: &'a str,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        // Get the push notification config - only need read lock
        let config = {
            let push_guard = self.push_notifications.read().await;

            if let Some(config) = push_guard.get(task_id) {
                config.clone()
            } else {
                return Err(A2AError::PushNotificationNotSupported);
            }
        }; // Lock is dropped here

        Ok(config)
    }

    async fn add_status_subscriber<'a>(
        &self,
        task_id: &'a str,
        subscriber: Box<dyn crate::port::server::Subscriber<TaskStatusUpdateEvent> + Send + Sync>,
    ) -> Result<(), A2AError> {
        // Create a channel for the new subscriber pattern
        let (tx, mut rx) = mpsc::channel::<TaskStatusUpdateEvent>(16);
        
        // Add the channel subscriber
        {
            let mut subscribers = self.subscribers.write().await;
            let task_subscribers = subscribers
                .entry(task_id.to_string())
                .or_insert_with(TaskSubscribers::new);
            
            task_subscribers.add_status_subscriber(StatusSubscriber {
                id: SubscriberId::new(),
                task_id: task_id.to_string(),
                sender: tx,
            });
        }
        
        // Spawn a task to forward events from the channel to the subscriber
        let task_id = task_id.to_string();
        let task_id_clone = task_id.clone();
        tokio::spawn(async move {
            
            while let Some(event) = rx.recv().await {
                if let Err(e) = subscriber.on_update(event).await {
                    warn!("Error notifying status subscriber for task {}: {}", task_id, e);
                }
            }
        });
        
        // Send initial update
        let task = self.get_task(&task_id_clone, None).await?;
        self.broadcast_status_update(&task_id_clone, task.status, false).await?;
        
        Ok(())
    }

    async fn add_artifact_subscriber<'a>(
        &self,
        task_id: &'a str,
        subscriber: Box<dyn crate::port::server::Subscriber<TaskArtifactUpdateEvent> + Send + Sync>,
    ) -> Result<(), A2AError> {
        // Create a channel for the new subscriber pattern
        let (tx, mut rx) = mpsc::channel::<TaskArtifactUpdateEvent>(16);
        
        // Add the channel subscriber
        {
            let mut subscribers = self.subscribers.write().await;
            let task_subscribers = subscribers
                .entry(task_id.to_string())
                .or_insert_with(TaskSubscribers::new);
            
            task_subscribers.add_artifact_subscriber(ArtifactSubscriber {
                id: SubscriberId::new(),
                task_id: task_id.to_string(),
                sender: tx,
            });
        }
        
        // Spawn a task to forward events from the channel to the subscriber
        let task_id = task_id.to_string();
        let task_id_clone = task_id.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let Err(e) = subscriber.on_update(event).await {
                    warn!("Error notifying artifact subscriber for task {}: {}", task_id, e);
                }
            }
        });
        
        // Send initial artifacts
        let task = self.get_task(&task_id_clone, None).await?;
        if let Some(artifacts) = task.artifacts {
            for artifact in artifacts {
                self.broadcast_artifact_update(&task_id_clone, artifact).await?;
            }
        }
        
        Ok(())
    }

    async fn remove_subscribers<'a>(&self, task_id: &'a str) -> Result<(), A2AError> {
        // Simply remove all subscribers
        let mut subscribers = self.subscribers.write().await;
        subscribers.remove(task_id);
        Ok(())
    }
}

impl Clone for InMemoryTaskStorage {
    fn clone(&self) -> Self {
        Self {
            tasks: self.tasks.clone(),
            subscribers: self.subscribers.clone(),
            push_notifications: self.push_notifications.clone(),
            task_metadata: self.task_metadata.clone(),
            gc_interval: self.gc_interval,
            last_gc: self.last_gc.clone(),
        }
    }
}