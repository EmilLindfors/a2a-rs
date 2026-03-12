//! Core plugin trait for A2A agents.
//!
//! The `AgentPlugin` trait defines the interface that all agents should implement
//! to integrate with the framework. It provides metadata, skill definitions, and
//! lifecycle hooks.

use a2a_rs::domain::A2AError;
use a2a_rs::port::AsyncMessageHandler;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Skill definition for agent capabilities.
///
/// Skills describe what an agent can do, including keywords for intent matching,
/// examples for documentation, and supported input/output formats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDefinition {
    /// Unique identifier for the skill
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description of what this skill does
    pub description: String,
    /// Keywords for intent classification
    pub keywords: Vec<String>,
    /// Example queries that trigger this skill
    pub examples: Vec<String>,
    /// Supported input formats (e.g., "text", "file", "data")
    pub input_formats: Vec<String>,
    /// Supported output formats (e.g., "text", "file", "data")
    pub output_formats: Vec<String>,
}

/// Plugin trait that all agents should implement.
///
/// This trait extends `AsyncMessageHandler` with metadata and capability discovery.
/// Agents implementing this trait can be automatically configured and discovered
/// by the framework.
///
/// # Example
///
/// ```rust
/// use a2a_agents::traits::{AgentPlugin, SkillDefinition};
/// use a2a_rs::port::AsyncMessageHandler;
/// use a2a_rs::domain::{A2AError, Message, Task};
/// use async_trait::async_trait;
///
/// #[derive(Clone)]
/// struct MyAgent;
///
/// impl AgentPlugin for MyAgent {
///     fn name(&self) -> &str {
///         "My Agent"
///     }
///
///     fn description(&self) -> &str {
///         "A simple example agent"
///     }
///
///     fn skills(&self) -> Vec<SkillDefinition> {
///         vec![
///             SkillDefinition {
///                 id: "hello".to_string(),
///                 name: "Say Hello".to_string(),
///                 description: "Greets the user".to_string(),
///                 keywords: vec!["hello".into(), "hi".into()],
///                 examples: vec!["Hello!".into()],
///                 input_formats: vec!["text".into()],
///                 output_formats: vec!["text".into()],
///             }
///         ]
///     }
/// }
///
/// #[async_trait]
/// impl AsyncMessageHandler for MyAgent {
///     async fn process_message(
///         &self,
///         task_id: &str,
///         message: &Message,
///         session_id: Option<&str>,
///     ) -> Result<Task, A2AError> {
///         // Implementation
///         todo!()
///     }
///
///     async fn validate_message(&self, message: &Message) -> Result<(), A2AError> {
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait AgentPlugin: AsyncMessageHandler + Clone + Send + Sync + 'static {
    /// Agent name (displayed to users)
    fn name(&self) -> &str;

    /// Agent description
    fn description(&self) -> &str;

    /// Version of the agent
    fn version(&self) -> &str {
        "1.0.0"
    }

    /// Skills provided by this agent
    fn skills(&self) -> Vec<SkillDefinition>;

    /// Optional: Initialize the agent (load models, connect to services, etc.)
    async fn initialize(&mut self) -> Result<(), A2AError> {
        Ok(())
    }

    /// Optional: Cleanup resources
    async fn shutdown(&mut self) -> Result<(), A2AError> {
        Ok(())
    }

    /// Optional: Health check
    async fn health_check(&self) -> Result<(), A2AError> {
        Ok(())
    }
}
