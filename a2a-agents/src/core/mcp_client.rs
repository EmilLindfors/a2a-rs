//! MCP client integration for A2A agents
//!
//! This module provides functionality for A2A agents to connect to MCP servers
//! and use their tools as part of the agent's capabilities.

#[cfg(feature = "mcp-client")]
use crate::core::config::{McpClientConfig, McpServerConnection};
#[cfg(feature = "mcp-client")]
use rmcp::{
    model::{
        CallToolRequestParam, ClientCapabilities, ClientInfo, Implementation, ProtocolVersion,
        Tool,
    },
    transport::TokioChildProcess,
    Peer, RoleClient, ServiceExt,
};
#[cfg(feature = "mcp-client")]
use std::collections::HashMap;
#[cfg(feature = "mcp-client")]
use std::sync::Arc;
#[cfg(feature = "mcp-client")]
use tokio::process::Command;
#[cfg(feature = "mcp-client")]
use tracing::{debug, error, info};

/// Manager for MCP client connections
#[cfg(feature = "mcp-client")]
#[derive(Clone)]
pub struct McpClientManager {
    /// Connected MCP servers and their peers
    servers: Arc<tokio::sync::RwLock<HashMap<String, McpServerInfo>>>,
}

#[cfg(feature = "mcp-client")]
struct McpServerInfo {
    name: String,
    peer: Peer<RoleClient>,
    tools: Vec<Tool>,
}

#[cfg(feature = "mcp-client")]
impl McpClientManager {
    /// Create a new MCP client manager
    pub fn new() -> Self {
        Self {
            servers: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Initialize connections to MCP servers from configuration
    pub async fn initialize(
        &self,
        config: &McpClientConfig,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !config.enabled {
            info!("MCP client is disabled");
            return Ok(());
        }

        info!("Initializing MCP client with {} servers", config.servers.len());

        for server_config in &config.servers {
            match self.connect_to_server(server_config).await {
                Ok(_) => {
                    info!("Successfully connected to MCP server: {}", server_config.name);
                }
                Err(e) => {
                    error!(
                        "Failed to connect to MCP server '{}': {}",
                        server_config.name, e
                    );
                    // Continue with other servers even if one fails
                }
            }
        }

        Ok(())
    }

    /// Connect to a single MCP server
    async fn connect_to_server(
        &self,
        config: &McpServerConnection,
    ) -> Result<(), Box<dyn std::error::Error>> {
        debug!("Connecting to MCP server: {}", config.name);
        debug!("Command: {} {:?}", config.command, config.args);

        // Build the command
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args);

        // Set environment variables
        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        // Set working directory
        if let Some(ref cwd) = config.cwd {
            cmd.current_dir(cwd);
        }

        // Create transport from the child process
        let (transport, _stderr) = TokioChildProcess::builder(cmd).spawn()?;

        // Create MCP client with custom client info
        let client_info = ClientInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: format!("a2a-agent-{}", config.name),
                version: "0.1.0".to_string(),
                title: None,
                icons: None,
                website_url: None,
            },
        };

        // Start the client service
        let service = client_info.serve(transport).await?;
        let peer = service.peer().clone();

        // List available tools
        debug!("Listing tools from MCP server: {}", config.name);
        let tools_result = peer
            .list_tools(None)
            .await
            .map_err(|e| format!("Failed to list tools: {}", e))?;

        info!(
            "MCP server '{}' has {} tools",
            config.name,
            tools_result.tools.len()
        );

        for tool in &tools_result.tools {
            let desc = tool
                .description
                .as_ref()
                .map(|d| d.as_ref())
                .unwrap_or("no description");
            debug!("  - {} ({})", tool.name, desc);
        }

        // Store server info
        let server_info = McpServerInfo {
            name: config.name.clone(),
            peer,
            tools: tools_result.tools,
        };

        let mut servers = self.servers.write().await;
        servers.insert(config.name.clone(), server_info);

        Ok(())
    }

    /// Call an MCP tool
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<rmcp::model::CallToolResult, Box<dyn std::error::Error>> {
        let servers = self.servers.read().await;

        let server = servers
            .get(server_name)
            .ok_or_else(|| format!("MCP server '{}' not found", server_name))?;

        debug!(
            "Calling tool '{}' on MCP server '{}'",
            tool_name, server_name
        );

        // Convert arguments to Map if provided
        let args_map = arguments.and_then(|v| v.as_object().map(|o| o.clone()));

        let result = server
            .peer
            .call_tool(CallToolRequestParam {
                name: tool_name.to_string().into(),
                arguments: args_map,
            })
            .await
            .map_err(|e| format!("Tool call failed: {}", e))?;

        Ok(result)
    }

    /// List all available tools from all connected servers
    pub async fn list_all_tools(&self) -> Vec<(String, Tool)> {
        let servers = self.servers.read().await;
        let mut all_tools = Vec::new();

        for (server_name, server_info) in servers.iter() {
            for tool in &server_info.tools {
                all_tools.push((server_name.clone(), tool.clone()));
            }
        }

        all_tools
    }

    /// Get tools from a specific server
    pub async fn list_server_tools(&self, server_name: &str) -> Option<Vec<Tool>> {
        let servers = self.servers.read().await;
        servers.get(server_name).map(|s| s.tools.clone())
    }

    /// Check if a server is connected
    pub async fn is_connected(&self, server_name: &str) -> bool {
        let servers = self.servers.read().await;
        servers.contains_key(server_name)
    }

    /// Get names of all connected servers
    pub async fn connected_servers(&self) -> Vec<String> {
        let servers = self.servers.read().await;
        servers.keys().cloned().collect()
    }
}

#[cfg(not(feature = "mcp-client"))]
#[derive(Clone)]
pub struct McpClientManager;

#[cfg(not(feature = "mcp-client"))]
impl McpClientManager {
    pub fn new() -> Self {
        Self
    }

    pub async fn initialize(
        &self,
        _config: &crate::core::config::McpClientConfig,
    ) -> Result<(), Box<dyn std::error::Error>> {
        tracing::warn!("MCP client feature not enabled. Compile with --features mcp-client");
        Ok(())
    }

    pub async fn call_tool(
        &self,
        _server_name: &str,
        _tool_name: &str,
        _arguments: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        Err("MCP client feature not enabled".into())
    }

    pub async fn list_all_tools(&self) -> Vec<(String, serde_json::Value)> {
        Vec::new()
    }
}
