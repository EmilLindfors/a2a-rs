# A2A-RMCP Integration

A bridge between Agent-to-Agent (A2A) protocol and Rusty Model Context Protocol (RMCP)

## Overview

This crate provides integration between the A2A protocol and RMCP, enabling bidirectional communication between these protocols. It follows a bridge pattern with adapter layers for message conversion and protocol translation.

## Key Features

- Use A2A agents as RMCP tools
- Expose RMCP tools as A2A agents
- Bidirectional message conversion
- State management across protocols

## Examples

Both run end-to-end with no external setup — they wire up the A2A and MCP
sides in-process over an in-memory duplex transport.

- `cargo run --example a2a_as_mcp_server -p a2a-mcp` — spins up a tiny A2A
  HTTP agent, bridges it with `AgentToMcpBridge`, and demonstrates an MCP
  client listing and calling its tools.
- `cargo run --example a2a_with_mcp_tools -p a2a-mcp` — wraps an A2A handler
  with `McpToA2ABridge` so `TOOL_CALL: <name>` messages get routed to an
  in-process MCP server.

## Architecture

```mermaid
flowchart TD
    subgraph A2A[A2A Protocol]
        A2AAgent[A2A Agent]
        A2AClient[A2A Client]
    end

    subgraph Bridge[a2a-mcp Bridge]
        A2AMCPBridge[AgentToMcpBridge\n(A2A Agent as MCP Server)]
        MCPA2ABridge[McpToA2ABridge\n(MCP Server as A2A Agent)]
        
        A2AMCPBridge <--> |MessageConverter| Converters
        MCPA2ABridge <--> |MessageConverter| Converters
    end

    subgraph MCP[MCP Protocol]
        MCPClient[MCP Client]
        MCPServer[MCP Server]
    end

    A2AAgent <--> |A2A Messages| A2AMCPBridge
    A2AMCPBridge <--> |MCP JSON-RPC| MCPClient
    
    A2AClient <--> |A2A Messages| MCPA2ABridge
    MCPA2ABridge <--> |MCP JSON-RPC| MCPServer
```

## Development Status

See the workspace [ROADMAP.md](../ROADMAP.md) for deferred themes and next steps.