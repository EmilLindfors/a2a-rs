//! MCP client integration for A2A agents.
//!
//! Lets an A2A agent act as an MCP *client*: connect to one or more MCP servers
//! (spawned as child processes), discover their tools, and invoke them while
//! serving A2A requests.
//!
//! [`McpClientManager`] is the adapter that owns those connections. Build one
//! from the agent's `[features.mcp_client]` config with [`McpClientManager::connect`]
//! and hand it to your [`AsyncMessageHandler`](a2a_rs::port::AsyncMessageHandler);
//! the handler then reaches its tools through the
//! [`McpToolsExt`](crate::traits::McpToolsExt) convenience trait:
//!
//! ```rust,ignore
//! let config = AgentConfig::from_file("agent.toml")?;
//! let mcp = McpClientManager::connect(&config.features.mcp_client).await?;
//! let handler = MyHandler::new(mcp); // impls McpToolsExt by returning &self.mcp
//! AgentBuilder::new(config)
//!     .with_handler(handler)
//!     .build_with_auto_storage()
//!     .await?
//!     .run()
//!     .await?;
//! ```

#![cfg(feature = "mcp-client")]

use crate::core::config::{McpClientConfig, McpServerConnection};
use rmcp::{
    Peer, RoleClient, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResult, ClientCapabilities, ClientInfo, Implementation,
        ProtocolVersion, Tool,
    },
    service::RunningService,
    transport::TokioChildProcess,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::process::Command;
use tracing::{debug, error, info};

/// Errors raised while connecting to or calling out to MCP servers.
#[derive(Debug, thiserror::Error)]
pub enum McpClientError {
    /// The child process backing an MCP server could not be spawned.
    #[error("failed to spawn MCP server '{server}': {source}")]
    Spawn {
        server: String,
        #[source]
        source: std::io::Error,
    },

    /// The MCP handshake or initial tool listing failed.
    #[error("failed to connect to MCP server '{server}': {message}")]
    Connect { server: String, message: String },

    /// A tool was requested on a server that isn't connected.
    #[error("MCP server '{server}' is not connected")]
    NotConnected { server: String },

    /// The remote tool invocation failed.
    #[error("tool '{tool}' on MCP server '{server}' failed: {message}")]
    ToolCall {
        server: String,
        tool: String,
        message: String,
    },
}

/// Manages connections to external MCP servers and exposes their tools.
///
/// Cheap to clone — the connection registry lives behind an [`Arc`], so a
/// handler can hold one and the framework can share it freely.
#[derive(Clone)]
pub struct McpClientManager {
    /// Connected MCP servers and their peers.
    servers: Arc<tokio::sync::RwLock<HashMap<String, McpServerInfo>>>,
}

struct McpServerInfo {
    /// The live service handle. Dropping it tears down the transport (and the
    /// child process), so it's held for as long as the server is registered —
    /// the [`Peer`] below is only usable while this is alive.
    _service: RunningService<RoleClient, ClientInfo>,
    peer: Peer<RoleClient>,
    tools: Vec<Tool>,
}

impl Default for McpClientManager {
    fn default() -> Self {
        Self::new()
    }
}

impl McpClientManager {
    /// Create an empty manager with no connections.
    ///
    /// Prefer [`connect`](Self::connect) to build and wire up a manager from
    /// configuration in one step.
    pub fn new() -> Self {
        Self {
            servers: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Build a manager and connect to every server in `config`.
    ///
    /// Connection is lenient: a server that fails to start is logged and
    /// skipped so one bad entry doesn't take down the agent. The call only
    /// fails if servers were configured but *none* could be reached — a clear
    /// startup error rather than a tool call that mysteriously fails later.
    /// When [`config.enabled`](McpClientConfig::enabled) is false this returns
    /// an empty manager.
    pub async fn connect(config: &McpClientConfig) -> Result<Self, McpClientError> {
        let manager = Self::new();
        manager.initialize(config).await?;
        Ok(manager)
    }

    /// Connect to the servers in `config`, adding them to this manager.
    ///
    /// See [`connect`](Self::connect) for the leniency contract.
    pub async fn initialize(&self, config: &McpClientConfig) -> Result<(), McpClientError> {
        if !config.enabled {
            info!("MCP client is disabled");
            return Ok(());
        }

        info!(
            "Initializing MCP client with {} server(s)",
            config.servers.len()
        );

        let mut connected = 0usize;
        let mut last_err = None;
        for server_config in &config.servers {
            match self.connect_to_server(server_config).await {
                Ok(()) => {
                    connected += 1;
                    info!("Connected to MCP server '{}'", server_config.name);
                }
                Err(e) => {
                    error!(
                        "Failed to connect to MCP server '{}': {e}",
                        server_config.name
                    );
                    last_err = Some(e);
                }
            }
        }

        if connected == 0 && !config.servers.is_empty() {
            return Err(last_err.expect("a non-empty server list reports a failure"));
        }

        Ok(())
    }

    /// Connect to a single MCP server and register its tools.
    async fn connect_to_server(&self, config: &McpServerConnection) -> Result<(), McpClientError> {
        debug!("Connecting to MCP server '{}'", config.name);
        debug!("Command: {} {:?}", config.command, config.args);

        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args);
        for (key, value) in &config.env {
            cmd.env(key, value);
        }
        if let Some(ref cwd) = config.cwd {
            cmd.current_dir(cwd);
        }

        // Spawn the server as a child process and talk to it over its stdio.
        let (transport, _stderr) =
            TokioChildProcess::builder(cmd)
                .spawn()
                .map_err(|source| McpClientError::Spawn {
                    server: config.name.clone(),
                    source,
                })?;

        // `ClientInfo` and `Implementation` are `#[non_exhaustive]` in rmcp —
        // use the typed builders rather than struct literals.
        let implementation = Implementation::new(format!("a2a-agent-{}", config.name), "0.1.0");
        let client_info = ClientInfo::new(ClientCapabilities::default(), implementation)
            .with_protocol_version(ProtocolVersion::V_2024_11_05);

        let service = client_info
            .serve(transport)
            .await
            .map_err(|e| McpClientError::Connect {
                server: config.name.clone(),
                message: e.to_string(),
            })?;
        let peer = service.peer().clone();

        debug!("Listing tools from MCP server '{}'", config.name);
        let tools_result = peer
            .list_tools(None)
            .await
            .map_err(|e| McpClientError::Connect {
                server: config.name.clone(),
                message: format!("failed to list tools: {e}"),
            })?;

        info!(
            "MCP server '{}' exposes {} tool(s)",
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

        let server_info = McpServerInfo {
            _service: service,
            peer,
            tools: tools_result.tools,
        };
        self.servers
            .write()
            .await
            .insert(config.name.clone(), server_info);

        Ok(())
    }

    /// Call a tool on a connected MCP server.
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult, McpClientError> {
        let servers = self.servers.read().await;
        let server = servers
            .get(server_name)
            .ok_or_else(|| McpClientError::NotConnected {
                server: server_name.to_string(),
            })?;

        debug!("Calling tool '{tool_name}' on MCP server '{server_name}'");

        let args_map = arguments.and_then(|v| v.as_object().cloned());
        let mut params = CallToolRequestParams::new(tool_name.to_string());
        if let Some(map) = args_map {
            params = params.with_arguments(map);
        }

        server
            .peer
            .call_tool(params)
            .await
            .map_err(|e| McpClientError::ToolCall {
                server: server_name.to_string(),
                tool: tool_name.to_string(),
                message: e.to_string(),
            })
    }

    /// List every tool across all connected servers as `(server, tool)` pairs.
    pub async fn list_all_tools(&self) -> Vec<(String, Tool)> {
        let servers = self.servers.read().await;
        servers
            .iter()
            .flat_map(|(name, info)| info.tools.iter().map(move |t| (name.clone(), t.clone())))
            .collect()
    }

    /// Get the tools exposed by a specific server, if connected.
    pub async fn list_server_tools(&self, server_name: &str) -> Option<Vec<Tool>> {
        let servers = self.servers.read().await;
        servers.get(server_name).map(|s| s.tools.clone())
    }

    /// Whether a server with the given name is connected.
    pub async fn is_connected(&self, server_name: &str) -> bool {
        self.servers.read().await.contains_key(server_name)
    }

    /// Names of all connected servers.
    pub async fn connected_servers(&self) -> Vec<String> {
        self.servers.read().await.keys().cloned().collect()
    }
}
