//! Bridge that exposes A2A agents as MCP tools

use crate::{
    converters::{SkillToolConverter, TaskResultConverter},
    error::{A2aMcpError, Result},
};
use a2a_rs::{
    adapter::transport::http::HttpClient,
    domain::{AgentCard, Message, Part, Role},
    services::client::AsyncA2AClient,
};
use async_trait::async_trait;
use rmcp::{model::*, service::RequestContext, ErrorData as McpError, RoleServer, ServerHandler};
use std::sync::Arc;
use tracing::{debug, error, info};

/// Bridge that exposes A2A agent skills as MCP tools
///
/// This allows MCP clients to invoke A2A agent capabilities through the MCP protocol.
/// Each skill from the A2A agent becomes a callable MCP tool.
#[derive(Clone)]
pub struct AgentToMcpBridge {
    /// A2A client for communicating with the agent
    client: Arc<HttpClient>,
    /// Agent card containing skills and metadata
    agent_card: Arc<AgentCard>,
    /// Cached list of MCP tools generated from agent skills
    tools: Arc<Vec<Tool>>,
    /// Agent URL for namespacing tool names
    agent_url: String,
}

impl AgentToMcpBridge {
    /// Create a new bridge from an A2A agent
    ///
    /// # Arguments
    ///
    /// * `client` - A2A client configured to communicate with the agent
    /// * `agent_card` - The agent's capabilities card
    /// * `agent_url` - Base URL of the A2A agent (used for tool namespacing)
    pub fn new(client: HttpClient, agent_card: AgentCard, agent_url: String) -> Self {
        // Convert all agent skills to MCP tools
        let tools: Vec<Tool> = agent_card
            .skills
            .iter()
            .map(|skill| SkillToolConverter::skill_to_tool(skill, &agent_url))
            .collect();

        info!(
            "Created AgentToMcpBridge for agent '{}' with {} tools",
            agent_card.name,
            tools.len()
        );

        Self {
            client: Arc::new(client),
            agent_card: Arc::new(agent_card),
            tools: Arc::new(tools),
            agent_url,
        }
    }

    /// Helper to call an A2A agent skill
    async fn call_skill(&self, skill_id: &str, message_text: &str) -> Result<CallToolResult> {
        debug!(
            "Calling A2A skill '{}' with message: {}",
            skill_id, message_text
        );

        // Create an A2A message
        let message = Message::builder()
            .role(Role::User)
            .parts(vec![Part::Text {
                text: message_text.to_string(),
                metadata: None,
            }])
            .message_id(uuid::Uuid::new_v4().to_string())
            .build();

        // Send the message to the A2A agent via message/send
        let task = self
            .client
            .send_task_message(skill_id, &message, None, None)
            .await
            .map_err(|e| A2aMcpError::AgentCommunication(e.to_string()))?;

        debug!("A2A agent returned task: {}", task.id);

        // Convert task to MCP result
        let result = TaskResultConverter::task_to_result(&task)?;

        info!(
            "A2A skill '{}' completed with state: {:?}",
            skill_id, task.status.state
        );

        Ok(result)
    }
}

#[async_trait]
impl ServerHandler for AgentToMcpBridge {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: format!("a2a-mcp-bridge:{}", self.agent_card.name),
                title: Some(format!("A2A Agent: {}", self.agent_card.name)),
                version: "0.1.0".to_string(),
                icons: None,
                website_url: Some(self.agent_card.url.clone()),
            },
            instructions: Some(format!(
                "A2A Agent '{}' exposed as MCP tools. Available tools: {}",
                self.agent_card.name,
                self.tools
                    .iter()
                    .map(|t| t.name.as_ref())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<ListToolsResult, McpError>> + Send + '_
    {
        async move {
            debug!("MCP client requested tool list");

            Ok(ListToolsResult {
                tools: (*self.tools).clone(),
                next_cursor: None,
                meta: None,
            })
        }
    }

    fn call_tool(
        &self,
        CallToolRequestParam { name, arguments }: CallToolRequestParam,
        _ctx: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<CallToolResult, McpError>> + Send + '_
    {
        async move {
            info!("MCP client calling tool: {}", name);

            // Parse the tool name to extract skill ID
            let (_agent_id, skill_id) = match SkillToolConverter::parse_tool_name(&name) {
                Ok(result) => result,
                Err(e) => return Err(e.to_mcp_error()),
            };

            // Verify the skill exists
            if !self.agent_card.skills.iter().any(|s| s.id == skill_id) {
                error!("Skill not found: {}", skill_id);
                return Err(McpError::internal_error(
                    format!("Skill '{}' not found", skill_id),
                    None,
                ));
            }

            // Extract the message parameter from arguments
            let message_text = arguments
                .as_ref()
                .and_then(|args| args.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if message_text.is_empty() {
                return Err(McpError::invalid_params(
                    "Missing required parameter 'message'",
                    None,
                ));
            }

            // Call the A2A agent skill
            match self.call_skill(&skill_id, &message_text).await {
                Ok(result) => Ok(result),
                Err(e) => Err(e.to_mcp_error()),
            }
        }
    }

    fn initialize(
        &self,
        _request: InitializeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = std::result::Result<InitializeResult, McpError>> + Send + '_
    {
        async move {
            info!(
                "MCP client initializing with agent '{}'",
                self.agent_card.name
            );
            Ok(self.get_info())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_rs::domain::core::agent::AgentSkill;

    #[test]
    fn test_bridge_creation() {
        let agent_card = AgentCard::builder()
            .name("Test Agent".to_string())
            .description("A test agent".to_string())
            .url("https://example.com".to_string())
            .version("1.0.0".to_string())
            .capabilities(Default::default())
            .default_input_modes(vec!["text".to_string()])
            .default_output_modes(vec!["text".to_string()])
            .skills(vec![AgentSkill::new(
                "test_skill".to_string(),
                "Test Skill".to_string(),
                "A test skill".to_string(),
                vec![],
            )])
            .build();

        let client = HttpClient::new("https://example.com".to_string());
        let bridge = AgentToMcpBridge::new(client, agent_card, "https://example.com".to_string());

        assert_eq!(bridge.tools.len(), 1);
        assert!(bridge.tools[0].name.contains("test_skill"));
    }
}
