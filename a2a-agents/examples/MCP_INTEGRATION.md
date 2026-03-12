# A2A Agent MCP Server Integration

This guide explains how to expose A2A agents as MCP (Model Context Protocol) servers, making them usable in Claude Desktop and other MCP clients.

## Overview

The a2a-agents framework now supports running agents as MCP servers via the `mcp-server` feature flag. This allows:

- **A2A agent skills** to be exposed as **MCP tools**
- **Claude Desktop** and other MCP clients to call your agents
- **Seamless integration** with the MCP ecosystem

## Quick Start

### 1. Enable the MCP Server Feature

Build with the `mcp-server` feature:

```bash
cargo build --features mcp-server
cargo run --example mcp_server_agent --features mcp-server
```

### 2. Configure Your Agent

Add MCP server configuration to your agent's TOML file:

```toml
[agent]
name = "My Agent"
description = "Description of what the agent does"

[[skills]]
id = "my_skill"
name = "My Skill"
description = "What this skill does"

[features.mcp_server]
enabled = true  # Enable MCP server mode
stdio = true    # Use stdio transport (required for Claude Desktop)
```

### 3. Integrate with Claude Desktop

Add your agent to Claude Desktop's configuration file (`claude_desktop_config.json`):

**macOS:** `~/Library/Application Support/Claude/claude_desktop_config.json`
**Windows:** `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "my-a2a-agent": {
      "command": "cargo",
      "args": ["run", "--example", "mcp_server_agent", "--features", "mcp-server"],
      "cwd": "/path/to/a2a-rs/a2a-agents"
    }
  }
}
```

Or for a production binary:

```json
{
  "mcpServers": {
    "my-a2a-agent": {
      "command": "/path/to/your-agent-binary",
      "args": []
    }
  }
}
```

### 4. Use in Claude Desktop

1. Restart Claude Desktop
2. Start a new chat
3. Click the 🔌 icon to see available MCP tools
4. Your A2A agent's skills will appear as callable tools!

## How It Works

### Architecture

```
┌─────────────────┐
│ Claude Desktop  │
│   (MCP Client)  │
└────────┬────────┘
         │ MCP Protocol (JSON-RPC)
         │ via stdio
┌────────▼────────┐
│ AgentToMcpBridge│
│  (a2a-mcp crate)│
└────────┬────────┘
         │ A2A Protocol
┌────────▼────────┐
│   A2A Agent     │
│  (your agent)   │
└─────────────────┘
```

### Conversion Flow

1. **Agent Skills → MCP Tools**
   - Each A2A agent skill becomes an MCP tool
   - Tool names are namespaced by agent URL
   - Skill descriptions, examples, and input/output modes are preserved

2. **MCP Tool Calls → A2A Messages**
   - Tool invocation → `message/send` to A2A agent
   - Tool parameters → A2A message parts
   - Response handling → Task result conversion

3. **Task Results → MCP Responses**
   - Completed tasks → Success responses
   - Failed tasks → Error responses
   - Agent messages → Tool output content

## Configuration Options

### MCP Server Config

```toml
[features.mcp_server]
enabled = true           # Enable/disable MCP server mode
stdio = true             # Use stdio transport (required for Claude Desktop)
name = "Custom Name"     # Optional: Override server name (defaults to agent name)
version = "1.0.0"        # Optional: Override version (defaults to agent version)
```

### Normal A2A Mode vs MCP Mode

When `mcp_server.enabled = true`:
- Agent runs as MCP server via stdio
- HTTP/WebSocket ports are NOT started
- Agent is controlled by MCP client lifecycle

When `mcp_server.enabled = false` (default):
- Agent runs as A2A server
- HTTP/WebSocket servers start normally
- Standard A2A protocol operations

## Example: Simple Calculator Agent

See `examples/mcp_server_agent.rs` for a complete example with:
- Greeting skill
- Calculator skill
- Echo skill

Run it:
```bash
cargo run --example mcp_server_agent --features mcp-server
```

The agent will start in MCP stdio mode, ready to receive JSON-RPC messages.

## Testing Without Claude Desktop

Use the MCP Inspector for development and testing:

```bash
# Install MCP Inspector
npm install -g @modelcontextprotocol/inspector

# Run your agent
cargo run --example mcp_server_agent --features mcp-server

# In another terminal, test with MCP Inspector
npx @modelcontextprotocol/inspector
```

## Building Production Binaries

For production deployment:

```bash
# Build release binary with MCP server support
cargo build --release --features mcp-server

# The binary can then be used in Claude Desktop config:
{
  "mcpServers": {
    "my-agent": {
      "command": "/usr/local/bin/my-agent"
    }
  }
}
```

## Troubleshooting

### Agent doesn't appear in Claude Desktop

1. Check `claude_desktop_config.json` syntax
2. Verify the `command` path is correct
3. Check Claude Desktop logs (Help → View Logs)
4. Ensure agent builds with `--features mcp-server`

### Tool calls fail

1. Check agent logs for errors
2. Verify skill IDs match between config and handler
3. Ensure handler returns valid Task objects
4. Check message format in handler implementation

### Performance issues

1. MCP uses stdio - ensure no logging to stdout/stderr
2. Use `RUST_LOG=error` or redirect logs to file
3. Optimize handler logic for low latency

## Advanced Topics

### Multiple Agents

You can expose multiple A2A agents as separate MCP servers:

```json
{
  "mcpServers": {
    "calculator-agent": {
      "command": "/path/to/calculator-agent"
    },
    "weather-agent": {
      "command": "/path/to/weather-agent"
    }
  }
}
```

### Custom Tool Names

Tool names are automatically generated as `{agent_url}_{skill_id}`. This prevents conflicts between multiple agents.

### Logging

When running as MCP server, avoid logging to stdout/stderr as this interferes with the JSON-RPC protocol. Use:

```rust
tracing_subscriber::fmt()
    .with_writer(std::fs::File::create("agent.log").unwrap())
    .init();
```

Or set `RUST_LOG=off` in Claude Desktop config:

```json
{
  "mcpServers": {
    "my-agent": {
      "command": "/path/to/agent",
      "env": {
        "RUST_LOG": "error"
      }
    }
  }
}
```

## Next Steps

- See `a2a-mcp/README.md` for bridge implementation details
- Check `spec/` for A2A Protocol v0.3.0 specifications
- Explore `examples/` for more agent implementations
- Read MCP documentation at https://modelcontextprotocol.io

## Support

For issues and questions:
- A2A Protocol: https://github.com/EmilLindfors/a2a-rs
- MCP Protocol: https://modelcontextprotocol.io
