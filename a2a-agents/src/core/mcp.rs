//! MCP server integration for A2A agents
//!
//! This module provides functionality to expose A2A agents as MCP (Model Context Protocol) servers,
//! allowing them to be used by MCP clients like Claude Desktop.

#[cfg(feature = "mcp-server")]
use a2a_mcp::bridge::agent_to_mcp::AgentToMcpBridge;
#[cfg(feature = "mcp-server")]
use a2a_rs::{domain::AgentCard, port::AsyncMessageHandler};
#[cfg(feature = "mcp-server")]
use rmcp::{RoleServer, transport::stdio};
#[cfg(feature = "mcp-server")]
use tracing::info;

#[cfg(feature = "mcp-server")]
use crate::core::config::McpServerConfig;

/// Run agent as MCP server via stdio transport.
///
/// Bridges the in-process [`AsyncMessageHandler`] into an MCP server using
/// stdin/stdout — no loopback HTTP server is involved, so there is no
/// auth-config-ignored caveat and tool calls don't pay the round-trip cost.
/// This is the standard way to integrate with Claude Desktop and other MCP
/// stdio clients.
#[cfg(feature = "mcp-server")]
pub async fn run_mcp_server<H>(
    config: &McpServerConfig,
    agent_card: AgentCard,
    handler: H,
) -> Result<(), Box<dyn std::error::Error>>
where
    H: AsyncMessageHandler + Send + Sync + 'static,
{
    if !config.enabled {
        return Ok(());
    }

    info!("Starting MCP server for agent: {}", agent_card.name);

    // Bridge the A2A agent into an MCP server handler. The bridge calls the
    // handler in-process; tool-name namespace is derived from agent_card.url.
    let bridge = AgentToMcpBridge::with_handler(handler, agent_card.clone())
        .with_mcp_metadata(config.name.clone(), config.version.clone());

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
pub async fn run_mcp_server<H>(
    _config: &crate::core::config::McpServerConfig,
    _agent_card: a2a_rs::domain::AgentCard,
    _handler: H,
) -> Result<(), Box<dyn std::error::Error>>
where
    H: a2a_rs::port::AsyncMessageHandler + Send + Sync + 'static,
{
    tracing::warn!("MCP server feature not enabled. Compile with --features mcp-server");
    Ok(())
}

#[cfg(not(feature = "mcp-server"))]
pub fn is_mcp_server_enabled(_config: &crate::core::config::McpServerConfig) -> bool {
    false
}
