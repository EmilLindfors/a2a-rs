//! In-memory task storage implementation

// This module is already conditionally compiled with #[cfg(feature = "server")] in mod.rs

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use tokio::sync::Mutex; // Changed from std::sync::Mutex

use crate::adapter::business::push_notification::{
    PushNotificationRegistry, PushNotificationSender,
};

#[cfg(feature = "http-client")]
use crate::adapter::business::push_notification::HttpPushNotificationSender;
#[cfg(not(feature = "http-client"))]
use crate::adapter::business::push_notification::NoopPushNotificationSender;
use crate::domain::{
    A2AError, ContextId, Conversation, Digest, Message, Seq, SequencedMessage, Task, TaskId,
    TaskPushNotificationConfig, TaskState, TaskStateExt, VersionedTask,
};
use crate::port::{
    AsyncConversationStore, AsyncNotificationManager, AsyncPushNotifier, AsyncTaskLifecycle,
    AsyncTaskQuery, AsyncTaskVersioning,
};

/// Simple in-memory task storage for testing and example purposes.
///
/// Persistence-only: streaming fan-out lives in
/// [`InMemoryStreamingHandler`](crate::adapter::InMemoryStreamingHandler) and
/// push-webhook delivery behind the [`AsyncPushNotifier`] port (this struct hands
/// out its registry via [`push_notifier`](Self::push_notifier)). The store still
/// owns push-config CRUD ([`AsyncNotificationManager`]) because that is config
/// *persistence*.
pub struct InMemoryTaskStorage {
    /// Tasks stored by ID
    pub(crate) tasks: Arc<Mutex<HashMap<String, Task>>>,
    /// Per-task optimistic-concurrency version, bumped on every mutation.
    ///
    /// A separate map keyed by the same task id. Mutators always lock `tasks`
    /// first and `versions` second, so the two stay consistent and never
    /// deadlock (see [`AsyncTaskVersioning`]).
    pub(crate) versions: Arc<Mutex<HashMap<String, u64>>>,
    /// The conversation log, keyed by context id.
    ///
    /// A separate append-only list rather than something derived from `tasks`,
    /// mirroring what the SQL adapter keeps in `task_history`. Deriving it would
    /// need a total order across tasks that `Task` does not carry, and the point
    /// of having both adapters is that they model the same thing.
    ///
    /// The only nesting that exists is `tasks` → `versions` → `conversations`,
    /// taken in that order by `update_status`. `digests` and `context_owners`
    /// are always taken alone, and `load` releases the digest lock before
    /// acquiring this one.
    pub(crate) conversations: Arc<Mutex<HashMap<String, Vec<SequencedMessage>>>>,
    /// Appended digests, keyed by context id. Newest wins on load, by watermark
    /// rather than by position, since two concurrent compactions can append out
    /// of watermark order.
    pub(crate) digests: Arc<Mutex<HashMap<String, Vec<Digest>>>>,
    /// The principal that first wrote to each context. `None` is unowned and
    /// stays readable by anyone.
    pub(crate) context_owners: Arc<Mutex<HashMap<String, Option<String>>>>,
    /// Hands out conversation sequence numbers. Shared across contexts, which is
    /// harmless: `Seq` only has to be monotonic *within* one.
    pub(crate) next_seq: Arc<AtomicU64>,
    /// Push notification registry (config store + delivery backend)
    pub(crate) push_notification_registry: Arc<PushNotificationRegistry>,
}

impl InMemoryTaskStorage {
    /// Create a new empty task storage
    pub fn new() -> Self {
        // Use the appropriate push notification sender based on available features
        #[cfg(feature = "http-client")]
        let push_sender = HttpPushNotificationSender::new();
        #[cfg(not(feature = "http-client"))]
        let push_sender = NoopPushNotificationSender;

        let push_registry = PushNotificationRegistry::new(push_sender);

        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            versions: Arc::new(Mutex::new(HashMap::new())),
            conversations: Arc::new(Mutex::new(HashMap::new())),
            digests: Arc::new(Mutex::new(HashMap::new())),
            context_owners: Arc::new(Mutex::new(HashMap::new())),
            next_seq: Arc::new(AtomicU64::new(1)),
            push_notification_registry: Arc::new(push_registry),
        }
    }

    /// Create a new task storage with a custom push notification sender
    pub fn with_push_sender(push_sender: impl PushNotificationSender + 'static) -> Self {
        let push_registry = PushNotificationRegistry::new(push_sender);

        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            versions: Arc::new(Mutex::new(HashMap::new())),
            conversations: Arc::new(Mutex::new(HashMap::new())),
            digests: Arc::new(Mutex::new(HashMap::new())),
            context_owners: Arc::new(Mutex::new(HashMap::new())),
            next_seq: Arc::new(AtomicU64::new(1)),
            push_notification_registry: Arc::new(push_registry),
        }
    }

    /// Bump (or initialize) the stored version for a task, returning the new
    /// value. Callers already hold the `tasks` lock; this acquires `versions`
    /// second, preserving the global lock order.
    async fn bump_version(&self, task_id: &str) -> u64 {
        let mut versions = self.versions.lock().await;
        let v = versions.entry(task_id.to_string()).or_insert(0);
        *v += 1;
        *v
    }

    /// Hand out this store's push-notification registry as an
    /// [`AsyncPushNotifier`].
    ///
    /// The returned notifier shares the same config registry the store writes to
    /// via [`AsyncNotificationManager::set_config`], so a config registered on
    /// the store is immediately visible to the notifier at the composition edge.
    pub fn push_notifier(&self) -> Arc<dyn AsyncPushNotifier> {
        self.push_notification_registry.clone()
    }
}

impl Default for InMemoryTaskStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryTaskStorage {
    /// Record `message` at the end of `context_id`'s conversation.
    ///
    /// Callers hold the `tasks` lock; this takes `conversations` after it,
    /// preserving the order documented on the field.
    async fn append_to_conversation(&self, context_id: &str, message: Message) {
        let seq = Seq::new(self.next_seq.fetch_add(1, Ordering::Relaxed));
        let mut conversations = self.conversations.lock().await;
        conversations
            .entry(context_id.to_string())
            .or_default()
            .push(SequencedMessage { seq, message });
    }

    /// Claim `context_id` for `caller` if nobody holds it, then refuse a caller
    /// that is not the holder.
    ///
    /// One method because claim and check race otherwise: two first-turn
    /// requests would both see "unclaimed" and both write an owner.
    async fn claim_or_check_context(
        &self,
        context_id: &str,
        caller: Option<&str>,
        claim: bool,
    ) -> Result<(), A2AError> {
        let mut owners = self.context_owners.lock().await;
        match owners.get(context_id) {
            // Unowned, either because nothing claimed it or because it was
            // claimed with no principal. Both stay open.
            Some(None) => Ok(()),
            Some(Some(owner)) if Some(owner.as_str()) == caller => Ok(()),
            Some(Some(_)) => Err(A2AError::ContextAccessDenied {
                context_id: context_id.to_string(),
            }),
            None => {
                if claim {
                    owners.insert(context_id.to_string(), caller.map(str::to_string));
                }
                Ok(())
            }
        }
    }
}

#[async_trait]
impl AsyncConversationStore for InMemoryTaskStorage {
    async fn load(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Conversation, A2AError> {
        let context_id = context_id.as_str();
        // Claims on read, not only on write. A handler loads history at the top
        // of every turn, so the first turn of a conversation is what establishes
        // who owns it; claiming only on compaction would leave a context
        // readable by anyone until it first grew long enough to summarize.
        self.claim_or_check_context(context_id, caller, true)
            .await?;

        // Highest watermark, not newest appended: two concurrent compactions can
        // land out of order, and the one covering more is the one to use.
        let digest = {
            let digests = self.digests.lock().await;
            digests.get(context_id).and_then(|digests| {
                digests
                    .iter()
                    .max_by_key(|digest| digest.covers_through)
                    .cloned()
            })
        };

        let watermark = digest
            .as_ref()
            .map(|digest| digest.covers_through)
            .unwrap_or(Seq::START);

        let conversations = self.conversations.lock().await;
        let mut tail: Vec<SequencedMessage> = conversations
            .get(context_id)
            .map(|log| {
                log.iter()
                    .filter(|entry| entry.seq > watermark)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        // Keep the newest when limiting: the older part is what a summary
        // stands in for, and dropping the recent end would leave the model
        // answering with the least relevant half of the conversation.
        if let Some(limit) = limit {
            let limit = limit as usize;
            if tail.len() > limit {
                tail.drain(..tail.len() - limit);
            }
        }

        Ok(Conversation { digest, tail })
    }

    async fn compact(
        &self,
        context_id: &ContextId,
        caller: Option<&str>,
        digest: Digest,
    ) -> Result<(), A2AError> {
        let context_id = context_id.as_str();
        self.claim_or_check_context(context_id, caller, true)
            .await?;

        let mut digests = self.digests.lock().await;
        digests
            .entry(context_id.to_string())
            .or_default()
            .push(digest);
        Ok(())
    }
}

#[async_trait]
impl AsyncTaskLifecycle for InMemoryTaskStorage {
    async fn create(&self, id: &TaskId, context_id: &ContextId) -> Result<Task, A2AError> {
        let task_id = id.as_str();
        let context_id = context_id.as_str();
        let mut tasks_guard = self.tasks.lock().await;

        if tasks_guard.contains_key(task_id) {
            return Err(A2AError::TaskNotFound(format!(
                "Task {} already exists",
                task_id
            )));
        }

        let task = Task::new(task_id.to_string(), context_id.to_string());
        tasks_guard.insert(task_id.to_string(), task.clone());
        self.bump_version(task_id).await; // version 0 -> 1

        Ok(task)
    }

    async fn update_status(
        &self,
        id: &TaskId,
        state: TaskState,
        message: Option<Message>,
    ) -> Result<Task, A2AError> {
        let task_id = id.as_str();
        let mut tasks_guard = self.tasks.lock().await;

        let task = tasks_guard
            .get_mut(task_id)
            .ok_or_else(|| A2AError::TaskNotFound(task_id.to_string()))?;

        let context_id = task.context_id.clone();
        let logged = message.clone();

        // Update the task status with the optional message
        task.update_status(state, message);
        let updated = task.clone();
        self.bump_version(task_id).await;

        // The same message goes onto the context's conversation log, which is
        // what a later turn reads back as history. Only messages: a status
        // transition carrying none has nothing to record.
        if let Some(message) = logged {
            self.append_to_conversation(&context_id, message).await;
        }

        // Persistence only: announcing the change to streaming subscribers is
        // the orchestration layer's job (see `TaskStatusBroadcast`), not a side
        // effect of the mutator.
        Ok(updated)
    }

    async fn exists(&self, id: &TaskId) -> Result<bool, A2AError> {
        let task_id = id.as_str();
        let tasks_guard = self.tasks.lock().await;
        Ok(tasks_guard.contains_key(task_id))
    }

    async fn get(&self, id: &TaskId, history_length: Option<u32>) -> Result<Task, A2AError> {
        let task_id = id.as_str();
        // Get the task
        let task = {
            let tasks_guard = self.tasks.lock().await;

            let Some(task) = tasks_guard.get(task_id) else {
                return Err(A2AError::TaskNotFound(task_id.to_string()));
            };

            // Apply history length limitation if specified
            task.with_limited_history(history_length)
        }; // Lock is dropped here

        Ok(task)
    }

    async fn cancel(&self, id: &TaskId) -> Result<Task, A2AError> {
        let task_id = id.as_str();
        let mut tasks_guard = self.tasks.lock().await;

        let Some(task) = tasks_guard.get(task_id) else {
            return Err(A2AError::TaskNotFound(task_id.to_string()));
        };

        let mut updated_task = task.clone();

        // Anything that has not finished can be canceled — a queued
        // (`Submitted`) task most of all, and an `InputRequired` one, where
        // cancelling is how a client says "never mind". See
        // `TaskState::is_cancelable`.
        if !updated_task.status.state.is_cancelable() {
            return Err(A2AError::TaskNotCancelable(format!(
                "Task {} has already finished in state {:?} and cannot be canceled",
                task_id, updated_task.status.state
            )));
        }

        // Create a cancellation message to add to history
        let cancel_message = Message {
            role: ::buffa::EnumValue::from(crate::domain::Role::Agent),
            parts: vec![crate::domain::Part::text(format!(
                "Task {} canceled.",
                task_id
            ))],
            message_id: uuid::Uuid::new_v4().to_string(),
            task_id: task_id.to_string(),
            context_id: updated_task.context_id.clone(),
            ..Default::default()
        };

        // Update the status with the cancellation message to track in history
        updated_task.update_status(TaskState::Canceled, Some(cancel_message));
        tasks_guard.insert(task_id.to_string(), updated_task.clone());
        self.bump_version(task_id).await;

        // Persistence only: the orchestration layer announces the cancellation
        // to streaming subscribers (see `TaskStatusBroadcast`).
        Ok(updated_task)
    }
}

#[async_trait]
impl AsyncTaskVersioning for InMemoryTaskStorage {
    async fn version(&self, id: &TaskId) -> Result<u64, A2AError> {
        let task_id = id.as_str();
        let tasks_guard = self.tasks.lock().await;
        if !tasks_guard.contains_key(task_id) {
            return Err(A2AError::TaskNotFound(task_id.to_string()));
        }
        let versions = self.versions.lock().await;
        Ok(versions.get(task_id).copied().unwrap_or(0))
    }

    async fn get_versioned(
        &self,
        id: &TaskId,
        history_length: Option<u32>,
    ) -> Result<VersionedTask, A2AError> {
        let task_id = id.as_str();
        let tasks_guard = self.tasks.lock().await;
        let Some(task) = tasks_guard.get(task_id) else {
            return Err(A2AError::TaskNotFound(task_id.to_string()));
        };
        let task = task.with_limited_history(history_length);
        let versions = self.versions.lock().await;
        let version = versions.get(task_id).copied().unwrap_or(0);
        Ok(VersionedTask::new(task, version))
    }

    async fn update_status_checked(
        &self,
        id: &TaskId,
        expected: u64,
        state: TaskState,
        message: Option<Message>,
    ) -> Result<VersionedTask, A2AError> {
        let task_id = id.as_str();
        // Lock order: tasks, then versions — the compare-and-swap holds both so
        // the check and the bump are atomic against every other mutator.
        let mut tasks_guard = self.tasks.lock().await;
        let task = tasks_guard
            .get_mut(task_id)
            .ok_or_else(|| A2AError::TaskNotFound(task_id.to_string()))?;
        let mut versions = self.versions.lock().await;
        let current = versions.get(task_id).copied().unwrap_or(0);
        if current != expected {
            return Err(A2AError::VersionConflict {
                id: task_id.to_string(),
                expected,
                actual: current,
            });
        }
        task.update_status(state, message);
        let new_version = current + 1;
        versions.insert(task_id.to_string(), new_version);
        Ok(VersionedTask::new(task.clone(), new_version))
    }
}

#[async_trait]
impl AsyncTaskQuery for InMemoryTaskStorage {
    async fn list(
        &self,
        params: &crate::domain::ListTasksParams,
    ) -> Result<crate::domain::ListTasksResult, A2AError> {
        use crate::domain::ListTasksResult;

        let tasks_guard = self.tasks.lock().await;

        // Filter tasks based on parameters
        let mut filtered_tasks: Vec<_> = tasks_guard
            .values()
            .filter(|task| {
                // Filter by context_id if provided
                if let Some(ref context_id) = params.context_id
                    && &task.context_id != context_id
                {
                    return false;
                }

                // Filter by status if provided
                if let Some(ref status) = params.status
                    && &task.status.state != status
                {
                    return false;
                }

                // Filter by status_timestamp_after if provided
                if let Some(status_timestamp_after) = &params.status_timestamp_after
                    && let Ok(after_dt) =
                        chrono::DateTime::parse_from_rfc3339(status_timestamp_after)
                    && let Some(timestamp) = task.status.timestamp_utc()
                    && timestamp <= after_dt.with_timezone(&chrono::Utc)
                {
                    return false;
                }

                true
            })
            .cloned()
            .collect();

        // Sort by timestamp (most recent first)
        filtered_tasks.sort_by(|a, b| {
            let a_time = a
                .status
                .timestamp_utc()
                .map(|t| t.timestamp_millis())
                .unwrap_or(0);
            let b_time = b
                .status
                .timestamp_utc()
                .map(|t| t.timestamp_millis())
                .unwrap_or(0);
            b_time.cmp(&a_time)
        });

        let total_size = filtered_tasks.len() as i32;

        // Handle pagination
        let page_size = params.page_size.unwrap_or(50).clamp(1, 100) as usize;
        let page_start = if let Some(ref token) = params.page_token {
            // Parse page token as a number (simple implementation)
            token.parse::<usize>().unwrap_or(0)
        } else {
            0
        };

        let page_end = (page_start + page_size).min(filtered_tasks.len());
        let has_more = page_end < filtered_tasks.len();

        // Get the page of tasks
        let mut page_tasks: Vec<_> = filtered_tasks[page_start..page_end].to_vec();

        // Apply history length limit
        let history_length = params.history_length.unwrap_or(0);
        for task in &mut page_tasks {
            *task = task.with_limited_history(Some(history_length as u32));

            // Remove artifacts if not requested
            if !params.include_artifacts.unwrap_or(false) {
                task.artifacts.clear();
            }
        }

        // Generate next page token
        let next_page_token = if has_more {
            page_end.to_string()
        } else {
            String::new()
        };

        Ok(ListTasksResult {
            tasks: page_tasks,
            total_size,
            page_size: page_size as i32,
            next_page_token,
        })
    }
}

// AsyncNotificationManager implementation.
//
// In-memory storage keeps a single config per task in the push-notification
// registry, so the multi-config CRUD surface is expressed in those terms.
#[async_trait]
impl AsyncNotificationManager for InMemoryTaskStorage {
    async fn set_config(
        &self,
        config: &TaskPushNotificationConfig,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        #[cfg(feature = "tracing")]
        tracing::info!(
            task_id = %config.task_id,
            url = %config.url,
            "🚀 Registering push notification config for task"
        );

        // Register with the push notification registry
        self.push_notification_registry
            .register(&config.task_id, config.clone())
            .await?;

        #[cfg(feature = "tracing")]
        tracing::info!(
            task_id = %config.task_id,
            "✅ Push notification config registered successfully"
        );

        Ok(config.clone())
    }

    async fn get_config(
        &self,
        params: &crate::domain::GetTaskPushNotificationConfigParams,
    ) -> Result<TaskPushNotificationConfig, A2AError> {
        match self
            .push_notification_registry
            .get_config(&params.id)
            .await?
        {
            Some(config) => Ok(config),
            None => Err(A2AError::PushNotificationNotSupported),
        }
    }

    async fn list_configs(
        &self,
        params: &crate::domain::ListTaskPushNotificationConfigsParams,
    ) -> Result<Vec<TaskPushNotificationConfig>, A2AError> {
        // In-memory storage supports one config per task; return it as a
        // single-item vec (or empty if none registered).
        match self
            .push_notification_registry
            .get_config(&params.id)
            .await?
        {
            Some(config) => Ok(vec![config]),
            None => Ok(vec![]),
        }
    }

    async fn delete_config(
        &self,
        params: &crate::domain::DeleteTaskPushNotificationConfigParams,
    ) -> Result<(), A2AError> {
        // In-memory storage keeps a single config per task, so config_id is
        // not used for lookup. Idempotent per the v1.0.0 spec.
        self.push_notification_registry
            .unregister(&params.id)
            .await?;
        Ok(())
    }
}

impl Clone for InMemoryTaskStorage {
    fn clone(&self) -> Self {
        Self {
            tasks: self.tasks.clone(),
            versions: self.versions.clone(),
            conversations: self.conversations.clone(),
            digests: self.digests.clone(),
            context_owners: self.context_owners.clone(),
            next_seq: self.next_seq.clone(),
            push_notification_registry: self.push_notification_registry.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ContextId;

    fn tid(s: &str) -> TaskId {
        s.parse().unwrap()
    }
    fn cid(s: &str) -> ContextId {
        s.parse().unwrap()
    }

    fn said(text: &str) -> Message {
        use crate::domain::{Part, Role};
        Message::builder()
            .role(Role::User)
            .parts(vec![Part::text(text.to_string())])
            .message_id(uuid::Uuid::new_v4().to_string())
            .build()
    }

    fn texts(conversation: &Conversation) -> Vec<String> {
        use crate::domain::part;
        conversation
            .tail
            .iter()
            .flat_map(|entry| {
                entry.message.parts.iter().filter_map(|p| match &p.content {
                    Some(part::Content::Text(text)) => Some(text.clone()),
                    _ => None,
                })
            })
            .collect()
    }

    /// The conversation is the messages of every task in a context, in the order
    /// they were recorded. Two tasks, because that is what a multi-turn
    /// conversation actually looks like: one task per turn, sharing a context.
    #[tokio::test]
    async fn a_context_reads_back_as_one_ordered_conversation() {
        let store = InMemoryTaskStorage::new();
        store.create(&tid("t1"), &cid("c1")).await.unwrap();
        store
            .update_status(&tid("t1"), TaskState::Working, Some(said("what is it")))
            .await
            .unwrap();
        store
            .update_status(&tid("t1"), TaskState::Completed, Some(said("Oslo")))
            .await
            .unwrap();

        store.create(&tid("t2"), &cid("c1")).await.unwrap();
        store
            .update_status(
                &tid("t2"),
                TaskState::Completed,
                Some(said("and the population")),
            )
            .await
            .unwrap();

        let conversation = store.load(&cid("c1"), None, None).await.unwrap();
        assert_eq!(
            texts(&conversation),
            vec!["what is it", "Oslo", "and the population"]
        );
    }

    /// A status transition with no message has nothing to record. Storing a
    /// placeholder would put an empty turn in the model's prompt.
    #[tokio::test]
    async fn a_transition_without_a_message_records_nothing() {
        let store = InMemoryTaskStorage::new();
        store.create(&tid("t1"), &cid("c1")).await.unwrap();
        store
            .update_status(&tid("t1"), TaskState::Working, None)
            .await
            .unwrap();

        assert!(store.load(&cid("c1"), None, None).await.unwrap().is_empty());
    }

    /// Contexts do not leak into one another. This is the whole reason the log
    /// is keyed by context rather than kept per handler.
    #[tokio::test]
    async fn conversations_are_separate_per_context() {
        let store = InMemoryTaskStorage::new();
        store.create(&tid("t1"), &cid("c1")).await.unwrap();
        store.create(&tid("t2"), &cid("c2")).await.unwrap();
        store
            .update_status(&tid("t1"), TaskState::Completed, Some(said("in one")))
            .await
            .unwrap();
        store
            .update_status(&tid("t2"), TaskState::Completed, Some(said("in two")))
            .await
            .unwrap();

        let one = store.load(&cid("c1"), None, None).await.unwrap();
        assert_eq!(texts(&one), vec!["in one"]);
    }

    /// A digest hides everything at or below its watermark, and the tail picks
    /// up after it. Loading the summarized part again would double the tokens
    /// compaction was meant to save.
    #[tokio::test]
    async fn a_digest_replaces_the_messages_it_covers() {
        let store = InMemoryTaskStorage::new();
        store.create(&tid("t1"), &cid("c1")).await.unwrap();
        for text in ["one", "two", "three"] {
            store
                .update_status(&tid("t1"), TaskState::Working, Some(said(text)))
                .await
                .unwrap();
        }

        let before = store.load(&cid("c1"), None, None).await.unwrap();
        let watermark = before.tail[1].seq;
        store
            .compact(
                &cid("c1"),
                None,
                Digest {
                    covers_through: watermark,
                    summary: "they said one and two".to_string(),
                    replaced_messages: 2,
                    model: "test".to_string(),
                },
            )
            .await
            .unwrap();

        let after = store.load(&cid("c1"), None, None).await.unwrap();
        assert_eq!(after.summary(), Some("they said one and two"));
        assert_eq!(texts(&after), vec!["three"]);
    }

    /// Two turns of one conversation can compact at the same time. Both digests
    /// land, and the one covering more wins — the reason digests append with a
    /// watermark instead of updating in place.
    #[tokio::test]
    async fn concurrent_compaction_keeps_the_widest_digest() {
        let store = InMemoryTaskStorage::new();
        store.create(&tid("t1"), &cid("c1")).await.unwrap();
        for text in ["one", "two", "three"] {
            store
                .update_status(&tid("t1"), TaskState::Working, Some(said(text)))
                .await
                .unwrap();
        }
        let loaded = store.load(&cid("c1"), None, None).await.unwrap();

        // The wider digest is written first, so "newest row wins" would pick the
        // narrow one and re-feed a message the summary already covers.
        for (seq, summary) in [
            (loaded.tail[2].seq, "covers all three"),
            (loaded.tail[0].seq, "covers only the first"),
        ] {
            store
                .compact(
                    &cid("c1"),
                    None,
                    Digest {
                        covers_through: seq,
                        summary: summary.to_string(),
                        replaced_messages: 1,
                        model: "test".to_string(),
                    },
                )
                .await
                .unwrap();
        }

        let after = store.load(&cid("c1"), None, None).await.unwrap();
        assert_eq!(after.summary(), Some("covers all three"));
        assert!(after.tail.is_empty(), "{:?}", texts(&after));
    }

    /// Limiting keeps the newest. The older end is what a summary stands in for,
    /// so truncating there would leave the model the least relevant half.
    #[tokio::test]
    async fn limiting_a_conversation_keeps_the_most_recent_messages() {
        let store = InMemoryTaskStorage::new();
        store.create(&tid("t1"), &cid("c1")).await.unwrap();
        for text in ["one", "two", "three", "four"] {
            store
                .update_status(&tid("t1"), TaskState::Working, Some(said(text)))
                .await
                .unwrap();
        }

        let conversation = store.load(&cid("c1"), None, Some(2)).await.unwrap();
        assert_eq!(texts(&conversation), vec!["three", "four"]);
    }

    /// Reading a conversation back turns `context_id` into a capability: whoever
    /// holds one would otherwise read what was said in it.
    #[tokio::test]
    async fn a_context_belongs_to_whoever_started_it() {
        let store = InMemoryTaskStorage::new();
        store.create(&tid("t1"), &cid("c1")).await.unwrap();
        store
            .update_status(&tid("t1"), TaskState::Completed, Some(said("private")))
            .await
            .unwrap();

        // First read claims it.
        store.load(&cid("c1"), Some("alice"), None).await.unwrap();
        assert_eq!(
            texts(&store.load(&cid("c1"), Some("alice"), None).await.unwrap()),
            vec!["private"]
        );

        let err = store
            .load(&cid("c1"), Some("mallory"), None)
            .await
            .unwrap_err();
        assert!(
            matches!(err, A2AError::ContextAccessDenied { .. }),
            "{err:?}"
        );

        // And compacting someone else's conversation is refused the same way.
        let err = store
            .compact(
                &cid("c1"),
                Some("mallory"),
                Digest {
                    covers_through: Seq::new(1),
                    summary: "mine now".to_string(),
                    replaced_messages: 1,
                    model: "test".to_string(),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, A2AError::ContextAccessDenied { .. }));
    }

    /// An agent running without an authenticator has no principal to claim with,
    /// and its conversations stay readable. Refusing here would break every
    /// unauthenticated deployment.
    #[tokio::test]
    async fn an_unowned_context_stays_open() {
        let store = InMemoryTaskStorage::new();
        store.create(&tid("t1"), &cid("c1")).await.unwrap();
        store
            .update_status(&tid("t1"), TaskState::Completed, Some(said("open")))
            .await
            .unwrap();

        store.load(&cid("c1"), None, None).await.unwrap();
        assert_eq!(
            texts(&store.load(&cid("c1"), Some("anyone"), None).await.unwrap()),
            vec!["open"]
        );
    }

    #[tokio::test]
    async fn an_unknown_context_is_empty_rather_than_an_error() {
        let store = InMemoryTaskStorage::new();
        assert!(
            store
                .load(&cid("never-seen"), None, None)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn versioning_tracks_and_guards_mutations() {
        let store = InMemoryTaskStorage::new();
        store.create(&tid("t1"), &cid("c1")).await.unwrap();
        assert_eq!(store.version(&tid("t1")).await.unwrap(), 1);

        // Unversioned mutations bump the version, keeping the two views in sync.
        store
            .update_status(&tid("t1"), TaskState::Working, None)
            .await
            .unwrap();
        let snap = store.get_versioned(&tid("t1"), None).await.unwrap();
        assert_eq!(snap.version, 2);

        // Stale conditional update is rejected and leaves the task unchanged.
        let err = store
            .update_status_checked(&tid("t1"), 1, TaskState::Completed, None)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            A2AError::VersionConflict {
                expected: 1,
                actual: 2,
                ..
            }
        ));
        assert_eq!(
            store.get(&tid("t1"), None).await.unwrap().status.state,
            TaskState::Working
        );

        // Current-version conditional update succeeds and bumps.
        let ok = store
            .update_status_checked(&tid("t1"), 2, TaskState::Completed, None)
            .await
            .unwrap();
        assert_eq!(ok.version, 3);
        assert_eq!(ok.task.status.state, TaskState::Completed);
    }
}
