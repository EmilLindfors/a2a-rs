//! MCP server integration for A2A agents
//!
//! This module provides functionality to expose A2A agents as MCP (Model Context Protocol) servers,
//! allowing them to be used by MCP clients like Claude Desktop.

#[cfg(feature = "mcp-server")]
use a2a_mcp::bridge::agent_to_mcp::AgentToMcpBridge;
#[cfg(feature = "mcp-server")]
use a2a_rs::{adapter::transport::http::HttpClient, domain::AgentCard};
#[cfg(feature = "mcp-server")]
use rmcp::{RoleServer, transport::stdio};
#[cfg(feature = "mcp-server")]
use tracing::{error, info};

#[cfg(feature = "mcp-server")]
use crate::core::config::McpServerConfig;

/// Run agent as MCP server via stdio transport
///
/// This function starts the agent as an MCP server using stdin/stdout for communication.
/// This is the standard way to integrate with Claude Desktop and other MCP clients.
#[cfg(feature = "mcp-server")]
pub async fn run_mcp_server(
    config: &McpServerConfig,
    agent_card: AgentCard,
    agent_url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    if !config.enabled {
        return Ok(());
    }

    info!("Starting MCP server for agent: {}", agent_card.name);

    // Create HTTP client for the agent
    let http_client = HttpClient::new(agent_url.clone());

    // Create the bridge that exposes A2A agent as MCP tools
    let bridge = AgentToMcpBridge::new(http_client, agent_card.clone(), agent_url);

    if config.stdio {
        info!("Starting stdio transport for MCP server");

        // Get stdio transport
        let (read, write) = stdio();

        // Create and run the MCP service
        // serve_directly runs the service in a background task
        let _running =
            rmcp::service::serve_directly::<RoleServer, _, _, _, _>(bridge, (read, write), None);

        // Keep the service running - wait for Ctrl+C or stdio to close
        // The stdio transport will handle shutdown when the connection closes
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Received Ctrl+C, shutting down");
            }
            _ = tokio::time::sleep(tokio::time::Duration::MAX) => {
                // Never completes normally - only via Ctrl+C or process termination
            }
        }

        info!("MCP server shutdown gracefully");
        Ok(())
    } else {
        // Future: could support other transports (HTTP SSE, WebSocket)
        info!("Only stdio transport is currently supported for MCP server");
        Ok(())
    }
}

/// Check if MCP server mode is enabled
#[cfg(feature = "mcp-server")]
pub fn is_mcp_server_enabled(config: &McpServerConfig) -> bool {
    config.enabled
}

#[cfg(not(feature = "mcp-server"))]
pub async fn run_mcp_server(
    _config: &crate::core::config::McpServerConfig,
    _agent_card: a2a_rs::domain::AgentCard,
    _agent_url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::warn!("MCP server feature not enabled. Compile with --features mcp-server");
    Ok(())
}

#[cfg(not(feature = "mcp-server"))]
pub fn is_mcp_server_enabled(_config: &crate::core::config::McpServerConfig) -> bool {
    false
}
