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

/// Run agent as an MCP server over the configured transport.
///
/// Bridges the in-process [`AsyncMessageHandler`] into an MCP server. Two
/// transports are supported, selected by [`McpServerConfig`]:
///
/// * **stdio** (default) — stdin/stdout, the standard way to integrate with
///   Claude Desktop and other local MCP clients. No socket is bound, so tool
///   calls don't pay a round-trip cost and there is no auth-config caveat.
/// * **Streamable HTTP** ([`McpHttpConfig::enabled`]) — serves the bridge over
///   `rmcp`'s `StreamableHttpService` on `host:port`, for networked clients.
///   Takes precedence over stdio when enabled.
///
/// [`McpHttpConfig::enabled`]: crate::core::config::McpHttpConfig::enabled
#[cfg(feature = "mcp-server")]
pub async fn run_mcp_server<H>(
    config: &McpServerConfig,
    agent_card: AgentCard,
    handler: H,
) -> Result<(), Box<dyn std::error::Error>>
where
    H: AsyncMessageHandler + Clone + Send + Sync + 'static,
{
    if !config.enabled {
        return Ok(());
    }

    info!("Starting MCP server for agent: {}", agent_card.name);

    if config.http.enabled {
        return run_streamable_http(config, agent_card, handler).await;
    }

    if !config.stdio {
        info!(
            "No MCP transport enabled (set features.mcp_server.stdio or features.mcp_server.http.enabled)"
        );
        return Ok(());
    }

    info!("Starting stdio transport for MCP server");

    // Bridge the A2A agent into an MCP server handler. The bridge calls the
    // handler in-process; tool-name namespace is derived from agent_card.url.
    let bridge = AgentToMcpBridge::with_handler(handler, agent_card)
        .with_mcp_metadata(config.name.clone(), config.version.clone());

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
}

/// Serve the agent bridge over MCP's Streamable HTTP transport.
///
/// Mounts a fresh [`AgentToMcpBridge`] per session (via the service factory) on
/// an `axum` router at [`McpHttpConfig::path`], backed by an in-memory
/// [`LocalSessionManager`]. Runs until the process is terminated.
///
/// [`McpHttpConfig::path`]: crate::core::config::McpHttpConfig::path
#[cfg(feature = "mcp-server")]
async fn run_streamable_http<H>(
    config: &McpServerConfig,
    agent_card: AgentCard,
    handler: H,
) -> Result<(), Box<dyn std::error::Error>>
where
    H: AsyncMessageHandler + Clone + Send + Sync + 'static,
{
    use std::sync::Arc;

    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    };

    let http = &config.http;
    let addr = format!("{}:{}", http.host, http.port);

    // Start from the secure defaults (loopback-only Host, no Origin check) and
    // override only what the TOML specifies. An empty `allowed_hosts` disables
    // Host validation entirely (allow any host).
    let mut server_config = StreamableHttpServerConfig::default();
    if let Some(hosts) = &http.allowed_hosts {
        server_config = server_config.with_allowed_hosts(hosts.clone());
    }
    if let Some(origins) = &http.allowed_origins {
        server_config = server_config.with_allowed_origins(origins.clone());
    }

    // The factory is invoked once per MCP session; each gets its own bridge
    // wrapping clones of the shared handler and agent card.
    let name = config.name.clone();
    let version = config.version.clone();
    let service = StreamableHttpService::new(
        move || {
            Ok(
                AgentToMcpBridge::with_handler(handler.clone(), agent_card.clone())
                    .with_mcp_metadata(name.clone(), version.clone()),
            )
        },
        Arc::new(LocalSessionManager::default()),
        server_config,
    );

    let router = axum::Router::new().nest_service(&http.path, service);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(
        "MCP Streamable HTTP server listening on http://{}{}",
        addr, http.path
    );

    axum::serve(listener, router).await?;
    Ok(())
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
    H: a2a_rs::port::AsyncMessageHandler + Clone + Send + Sync + 'static,
{
    tracing::warn!("MCP server feature not enabled. Compile with --features mcp-server");
    Ok(())
}

#[cfg(not(feature = "mcp-server"))]
pub fn is_mcp_server_enabled(_config: &crate::core::config::McpServerConfig) -> bool {
    false
}
