//! Tool sources for the generic LLM handler.
//!
//! A [`ToolSource`] is the unifying abstraction behind the LLM tool-calling
//! loop: it advertises a set of [`ToolDefinition`]s to the model and executes a
//! [`ToolCall`] the model emits, returning a stringified result. The handler no
//! longer knows whether a tool is backed by an MCP server or by another A2A
//! agent — both are just sources.
//!
//! Two implementations ship today:
//!
//! * [`McpToolSource`] — exposes the tools of one connected MCP server (one per
//!   `[[features.mcp_client.servers]]`).
//! * [`A2aAgentToolSource`] — exposes **another A2A agent as a single tool**, so
//!   an LLM agent can delegate to peer agents (the multi-agent keystone). The
//!   remote agent is reached through the [`Transport`] port, so any wire
//!   protocol (ConnectRPC, JSON-RPC) works.

use std::sync::Arc;
use std::time::Duration;

use a2a_agents_common::llm::{ToolCall, ToolDefinition};
use a2a_rs::domain::{A2AError, Message, Part, Role, Task, TaskStateExt};
use a2a_rs::port::Transport;
use async_trait::async_trait;
use tokio::time::Instant;

/// A provider of LLM-callable tools, independent of what backs them.
#[async_trait]
pub trait ToolSource: Send + Sync {
    /// The LLM-facing tool definitions this source advertises.
    fn tool_defs(&self) -> Vec<ToolDefinition>;

    /// Whether this source owns (and can execute) the named tool.
    fn has_tool(&self, name: &str) -> bool;

    /// Execute a single tool call, returning the stringified result.
    async fn invoke(&self, task_id: &str, call: &ToolCall) -> Result<String, A2AError>;
}

/// Find the source that owns `name`, if any. First match wins, so callers should
/// keep tool names unique across sources.
pub fn resolve<'a>(sources: &'a [Arc<dyn ToolSource>], name: &str) -> Option<&'a dyn ToolSource> {
    sources
        .iter()
        .find(|s| s.has_tool(name))
        .map(|s| s.as_ref())
}

/// Flatten the tool definitions of every source into one list for the LLM.
pub fn collect_tool_defs(sources: &[Arc<dyn ToolSource>]) -> Vec<ToolDefinition> {
    sources.iter().flat_map(|s| s.tool_defs()).collect()
}

// --- A2A agent as a tool -----------------------------------------------------

/// Exposes a remote A2A agent as a single LLM tool named `ask_<slug>`.
///
/// On invocation it sends the model-supplied `message` to the remote agent as an
/// A2A task, waits for the task to reach a terminal state (A2A tasks are
/// asynchronous), and returns the agent's reply text.
pub struct A2aAgentToolSource {
    tool_name: String,
    description: String,
    transport: Arc<dyn Transport>,
    poll_interval: Duration,
    deadline: Duration,
}

impl A2aAgentToolSource {
    /// Build a tool source for a remote agent. `name` is the agent's friendly
    /// name (used to derive the tool name); `description` steers the model on
    /// when to delegate (typically the agent card's description + skills).
    pub fn new(name: &str, description: String, transport: Arc<dyn Transport>) -> Self {
        Self {
            tool_name: tool_name_for(name),
            description,
            transport,
            poll_interval: Duration::from_millis(250),
            deadline: Duration::from_secs(60),
        }
    }

    /// Override how long to wait for the remote task to finish (default 60s).
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    /// The tool name this source advertises (`ask_<slug>`).
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }
}

#[async_trait]
impl ToolSource for A2aAgentToolSource {
    fn tool_defs(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: self.tool_name.clone(),
            description: self.description.clone(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The natural-language request to send to the agent."
                    }
                },
                "required": ["message"]
            }),
        }]
    }

    fn has_tool(&self, name: &str) -> bool {
        name == self.tool_name
    }

    async fn invoke(&self, _task_id: &str, call: &ToolCall) -> Result<String, A2AError> {
        let args: serde_json::Value = serde_json::from_str(&call.arguments)
            .map_err(|e| A2AError::InvalidParams(format!("tool arguments must be JSON: {e}")))?;
        let text = args
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| A2AError::InvalidParams("missing `message` string argument".into()))?;

        // Each delegation is its own remote task; let the remote assign context.
        let remote_task_id = uuid::Uuid::new_v4().to_string();
        let msg = Message::builder()
            .role(Role::User)
            .parts(vec![Part::text(text.to_string())])
            .message_id(uuid::Uuid::new_v4().to_string())
            .build();

        let mut task = self
            .transport
            .send_task_message(&remote_task_id, &msg, None, Some(1))
            .await?;

        let start = Instant::now();
        while !task_terminal(&task) {
            if start.elapsed() >= self.deadline {
                return Err(A2AError::Internal(format!(
                    "remote agent tool '{}' did not finish within {:?}",
                    self.tool_name, self.deadline
                )));
            }
            tokio::time::sleep(self.poll_interval).await;
            task = self.transport.get_task(&remote_task_id, Some(1)).await?;
        }
        Ok(task_reply(&task))
    }
}

/// Derive an LLM tool name (`ask_<slug>`) from a free-form agent name.
pub fn tool_name_for(agent: &str) -> String {
    format!("ask_{}", crate::utils::slugify(agent, '_'))
}

/// True once the task has reached a terminal A2A state.
fn task_terminal(task: &Task) -> bool {
    task.status
        .as_option()
        .map(|s| s.state.is_terminal())
        .unwrap_or(false)
}

/// Extract the agent's reply text from a finished task's status message.
fn task_reply(task: &Task) -> String {
    task.status
        .as_option()
        .and_then(|s| s.message.as_option())
        .map(|m| {
            m.parts
                .iter()
                .filter_map(|p| p.get_text())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(the agent returned no text)".to_string())
}

// --- MCP server as a tool source --------------------------------------------

#[cfg(feature = "mcp-server")]
pub use mcp::{McpToolSource, UnusedInner};

#[cfg(feature = "mcp-server")]
mod mcp {
    use super::*;
    use a2a_mcp::McpToA2ABridge;
    use a2a_rs::domain::Task;
    use a2a_rs::port::AsyncMessageHandler;

    /// Filler inner handler for [`McpToA2ABridge`], which is generic over an
    /// `AsyncMessageHandler` it would delegate non-tool messages to. The LLM
    /// tool path never delegates, so this never runs.
    #[derive(Clone)]
    pub struct UnusedInner;

    #[async_trait]
    impl AsyncMessageHandler for UnusedInner {
        async fn process_message(
            &self,
            _task_id: &str,
            _message: &Message,
            _session_id: Option<&str>,
        ) -> Result<Task, A2AError> {
            Err(A2AError::UnsupportedOperation(
                "the generic LLM handler does not delegate to the MCP bridge".to_string(),
            ))
        }
    }

    /// Exposes one connected MCP server's tools to the LLM loop.
    pub struct McpToolSource {
        bridge: Arc<McpToA2ABridge<UnusedInner>>,
    }

    impl McpToolSource {
        pub fn new(bridge: Arc<McpToA2ABridge<UnusedInner>>) -> Self {
            Self { bridge }
        }
    }

    #[async_trait]
    impl ToolSource for McpToolSource {
        fn tool_defs(&self) -> Vec<ToolDefinition> {
            self.bridge.get_llm_tools()
        }

        fn has_tool(&self, name: &str) -> bool {
            self.bridge.tools().iter().any(|t| t.name.as_ref() == name)
        }

        async fn invoke(&self, task_id: &str, call: &ToolCall) -> Result<String, A2AError> {
            self.bridge
                .execute_llm_tool_call(task_id, call)
                .await
                .map_err(|e| e.to_a2a_error())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_name_is_slugified_and_prefixed() {
        assert_eq!(tool_name_for("Weather Agent"), "ask_weather_agent");
        assert_eq!(tool_name_for("billing-v2"), "ask_billing_v2");
        assert_eq!(tool_name_for("  Spaces  "), "ask_spaces");
    }

    #[derive(Clone)]
    struct FakeSource {
        name: String,
        result: String,
    }

    #[async_trait]
    impl ToolSource for FakeSource {
        fn tool_defs(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: self.name.clone(),
                description: "fake".into(),
                parameters: serde_json::json!({"type": "object"}),
            }]
        }
        fn has_tool(&self, name: &str) -> bool {
            name == self.name
        }
        async fn invoke(&self, _task_id: &str, _call: &ToolCall) -> Result<String, A2AError> {
            Ok(self.result.clone())
        }
    }

    #[test]
    fn resolve_picks_owning_source_and_collects_defs() {
        let sources: Vec<Arc<dyn ToolSource>> = vec![
            Arc::new(FakeSource {
                name: "alpha".into(),
                result: "a".into(),
            }),
            Arc::new(FakeSource {
                name: "beta".into(),
                result: "b".into(),
            }),
        ];

        assert!(resolve(&sources, "beta").is_some());
        assert!(resolve(&sources, "missing").is_none());
        assert_eq!(collect_tool_defs(&sources).len(), 2);
    }

    #[tokio::test]
    async fn resolved_source_executes() {
        let sources: Vec<Arc<dyn ToolSource>> = vec![Arc::new(FakeSource {
            name: "alpha".into(),
            result: "hello".into(),
        })];
        let src = resolve(&sources, "alpha").unwrap();
        let call = ToolCall {
            id: "1".into(),
            name: "alpha".into(),
            arguments: "{}".into(),
        };
        assert_eq!(src.invoke("t1", &call).await.unwrap(), "hello");
    }
}
