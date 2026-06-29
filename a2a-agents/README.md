# A2A Agents

Example agent implementations for the A2A Protocol with production-ready patterns and **declarative configuration**.

## 🚀 Quick Start (New Builder API)

Create a production-ready agent in just **~30 lines of code** instead of ~300!

### 1. Define your agent (`agent.toml`)

```toml
[agent]
name = "My Agent"
description = "A helpful agent"

[[skills]]
id = "my_skill"
name = "My Skill"
description = "What this skill does"
```

### 2. Implement your handler

```rust
use a2a_rs::port::AsyncMessageHandler;
use async_trait::async_trait;

#[derive(Clone)]
struct MyHandler;

#[async_trait]
impl AsyncMessageHandler for MyHandler {
    async fn process_message(/* ... */) -> Result<Task, A2AError> {
        // Your business logic here
    }
}
```

### 3. Build and run!

```rust
use a2a_agents::AgentBuilder;
use a2a_rs::InMemoryTaskStorage;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    AgentBuilder::from_file("agent.toml")?
        .with_handler(MyHandler)
        .with_storage(InMemoryTaskStorage::new())
        .build()?
        .run()
        .await?;
    Ok(())
}
```

**That's it!** The framework handles servers, agent cards, authentication, and more.

📚 **[See complete Builder API documentation →](BUILDER_API.md)**

## Overview

This crate provides two approaches for building agents:

### ✨ New: Declarative Builder API (Recommended)

- **90% less boilerplate** - ~30 lines vs ~300 lines
- **TOML configuration** - Define agents declaratively
- **Environment-aware** - Built-in env var interpolation
- **Type-safe** - Configuration validated at load time
- **Production-ready** - Batteries included

**Examples:**
- [`examples/minimal_agent.rs`](examples/minimal_agent.rs) - Echo agent (~50 lines)
- [`examples/reimbursement_builder.rs`](examples/reimbursement_builder.rs) - Full-featured agent

### Traditional Approach

The original hexagonal architecture approach with manual wiring:

1. **Hexagonal Architecture**: Clean separation between domain logic and adapters
2. **Framework Integration**: Uses `DefaultRequestProcessor` and storage backends
3. **Protocol Compliance**: Full A2A protocol support with HTTP transport
4. **Modern Patterns**: Async/await, builder patterns, and structured error handling

## 🔌 Model Context Protocol (MCP) Integration

You can expose any declarative A2A Agent as a Model Context Protocol (MCP) server over `stdio` (for local clients like Claude Desktop) or **Streamable HTTP** (for networked clients) transport. Either way, MCP-compatible clients can invoke the agent's skills as tools.

The bridge dispatches tool calls to the agent's message handler **in-process**, which means:
- No backing HTTP server is required (you can set `http_port = 0` for a pure-stdio server).
- Authentication checks are bypassed for local stdio calls (secure by design as it is run locally by the client), while HTTP endpoints can still use standard Bearer/OAuth2 token auth.

### 1. Enable the MCP Server in `agent.toml`

Add the `[features.mcp_server]` section to your config:

```toml
[agent]
name = "My MCP Agent"
version = "1.0.0"

[server]
host = "127.0.0.1"
http_port = 0 # Can be 0 for pure-stdio mode

[features.mcp_server]
enabled = true
stdio = true
name = "Custom MCP Service Name"     # Optional override
version = "2.0.0"                    # Optional override
```

### 2. Run the MCP Agent

Compile and run your agent with the `mcp-server` Cargo feature enabled:

```bash
cargo run -p a2a-agents --features mcp-server --example mcp_server_agent
```

### 3. Claude Desktop Configuration

To connect Claude Desktop to your agent, add the following to your Claude Desktop configuration file (usually located at `%APPDATA%\Claude\claude_desktop_config.json` on Windows):

```json
{
  "mcpServers": {
    "a2a-echo-agent": {
      "command": "cargo",
      "args": [
        "run",
        "--release",
        "-p",
        "a2a-agents",
        "--features",
        "mcp-server",
        "--example",
        "mcp_server_agent"
      ]
    }
  }
}
```

### 4. Streamable HTTP transport

For networked MCP clients, serve the agent over MCP's Streamable HTTP transport
instead of stdio. Add a `[features.mcp_server.http]` section — when `enabled`,
it takes precedence over stdio:

```toml
[features.mcp_server]
enabled = true
stdio = false

[features.mcp_server.http]
enabled = true
host = "127.0.0.1"   # default
port = 8000          # default
path = "/mcp"        # default mount path
```

```bash
cargo run -p a2a-agents --features mcp-server --example mcp_http_agent
```

The server then accepts MCP requests at `http://127.0.0.1:8000/mcp`.

**DNS-rebinding protection.** By default the transport only accepts inbound
`Host` headers for loopback (`localhost`, `127.0.0.1`, `::1`). For a public
bind, list the hostnames you serve under — and optionally restrict browser
origins:

```toml
[features.mcp_server.http]
enabled = true
host = "0.0.0.0"
port = 8000
allowed_hosts = ["mcp.example.com", "mcp.example.com:8000"]
allowed_origins = ["https://app.example.com"]   # omit to disable Origin checks
```

Setting `allowed_hosts = []` disables `Host` validation entirely (accepts any
host) — only do this behind a trusted reverse proxy.

### 5. MCP client (consume external MCP tools)

The other direction: let your agent **call out** to MCP servers and use their
tools while it serves A2A requests. Enable the `mcp-client` Cargo feature and
declare the servers to connect to under `[features.mcp_client]`. Each server is
spawned as a child process:

```toml
[features.mcp_client]
enabled = true

[[features.mcp_client.servers]]
name = "echo"
command = "cargo"
args = ["run", "-q", "-p", "a2a-agents", "--features", "mcp-client", "--bin", "mcp_echo_server"]
# `env = { KEY = "value" }` and `cwd = "…"` are also supported.
```

In code, connect the config-declared servers into an `McpClientManager` and
hand it to the handler that will use the tools. The handler owns the manager and
reaches tools through the `McpToolsExt` trait:

```rust
use a2a_agents::core::{AgentBuilder, AgentConfig, McpClientManager};
use a2a_agents::traits::{McpToolsExt, extract_tool_result_text};

#[derive(Clone)]
struct MyHandler { mcp: McpClientManager }

impl McpToolsExt for MyHandler {
    fn mcp_client(&self) -> &McpClientManager { &self.mcp }
}

// inside process_message:
//   let result = self.call_mcp_tool("echo", "echo", Some(json!({ "text": text }))).await?;
//   let reply  = extract_tool_result_text(&result);

let config = AgentConfig::from_file("agent.toml")?;
let mcp = McpClientManager::connect(&config.features.mcp_client).await?; // connects + discovers tools
AgentBuilder::new(config)
    .with_handler(MyHandler { mcp })
    .with_storage(a2a_rs::InMemoryTaskStorage::new())
    .build()?
    .run()
    .await?;
```

Connection is lenient — a server that fails to start is logged and skipped, and
`connect` only errors if servers were configured but none could be reached.

```bash
cargo run -p a2a-agents --features mcp-client --example mcp_client_agent
```

The example connects to the bundled `mcp_echo_server`, so it runs with no
external setup; point `command`/`args` at any MCP stdio server to talk to
something real.

## 🤖 LLM agents & multi-agent platform

Beyond single agents, `a2a-agents` ships the building blocks for an
**LLM-driven, multi-agent platform** — defined as **ports** in the platform
layer so the pure `a2a-rs` protocol crate stays infrastructure-free. The
`a2a` binary wires them together; it requires the `llm`, `mcp-server`, and
`schema` features:

```bash
cargo run -p a2a-agents --features llm,mcp-server,schema --bin a2a -- <subcommand>
```

| Subcommand | What it does |
|---|---|
| `run --config <toml>…` | Run one or more agents from TOML configs |
| `validate --config <toml>…` | Load + validate configs without serving |
| `control-plane --bind … --config-dir … --runtime local\|container` | Serve the deploy/list/status/undeploy HTTP API |
| `print-schema` | Print the `AgentConfig` JSON Schema to stdout |

### Config-driven LLM handler (`llm` feature)

Set `type = "llm"` and the framework drives a generic tool-calling LLM handler —
**no Rust code**. The model provider is picked up from the environment
(`OPENAI_API_KEY` / `GEMINI_API_KEY` / `OPENROUTER_API_KEY`):

```toml
[handler]
type = "llm"

[handler.llm]
system_prompt = "You are a helpful assistant."
max_tool_rounds = 4
```

The `llm` feature is independent of MCP: an LLM agent that only delegates to peer
A2A agents builds without pulling in `rmcp`. Add `mcp-server` as well to also
feed it the tools of connected MCP servers.

### Agent-as-tool delegation

List peer agents under `[[handler.llm.agents]]` and each is exposed to the model
as one `ask_<slug>` tool, so an orchestrator delegates work to specialists. Name
a peer by **exactly one** of `url`, `skill`, or `agent_id`:

```toml
[[handler.llm.agents]]
name = "Weather Agent"
url = "http://127.0.0.1:8081"     # dial directly, or…

[[handler.llm.agents]]
name = "Billing"
skill = "invoice-lookup"          # …resolve by advertised skill, or…

[[handler.llm.agents]]
name = "Scheduler"
agent_id = "scheduler-agent"      # …by registry id (slug of the name)
```

`skill` / `agent_id` are resolved against the **agent registry** at startup, so
peers are found by capability instead of a hard-coded URL. See
[`examples/orchestrator_agent.toml`](examples/orchestrator_agent.toml) and
[`examples/registry_orchestrator.toml`](examples/registry_orchestrator.toml).

### Control plane

`control-plane` serves an HTTP API that composes the runtime and registry:
`POST /agents` deploys an agent from rendered TOML (provision + start + register
its card so peers discover it), `GET /agents` lists, `GET /agents/{id}` reports
health, `DELETE /agents/{id}` tears down. Pick the backend with `--runtime`:
`local` supervises child `a2a run` processes, `container` runs each agent in a
`docker`/`podman` container (`--engine`, `--image`).

```bash
cargo run -p a2a-agents --features llm,mcp-server,schema --bin a2a -- \
  control-plane --bind 127.0.0.1:9090 --config-dir ./deployed --runtime local
```

See the workspace [`DECLARATIVE_AGENTS.md`](../DECLARATIVE_AGENTS.md) for the
platform design and roadmap.

## Architecture

### ReimbursementMessageHandler

The core business logic implementing `AsyncMessageHandler`:

- Processes reimbursement requests using the A2A message format
- Generates interactive forms for expense submissions
- Validates and approves reimbursement requests
- Returns structured responses with proper task states

### ModernReimbursementServer

The server implementation using framework components:

- Integrates with `DefaultBusinessHandler` for request processing
- Uses `InMemoryTaskStorage` for task persistence
- Configures `SimpleAgentInfo` with agent capabilities
- Supports both HTTP transport

## Usage

### Quick Start - Unified Demo (Recommended)

Run the complete demo with both agent backend and web frontend in a single command:

```bash
# Run everything (agent backend + web UI)
cargo run --bin reimbursement_demo

# Open your browser to http://localhost:3000
```

This starts:
- **Agent Backend** on `http://localhost:8080` (HTTP)
- **Web Frontend** on `http://localhost:3000`

The frontend automatically connects to the local agent and provides an interactive interface for submitting expenses and viewing tasks.

### Advanced Usage

Run specific components:

```bash
# Run only the agent backend (HTTP)
cargo run --bin reimbursement_demo -- --mode agent

# Run only the web frontend (point it to an existing agent)
AGENT_HTTP_URL=http://localhost:8080 cargo run --bin reimbursement_demo -- --mode frontend

# Customize ports
cargo run --bin reimbursement_demo -- \
  --agent-http-port 8080 \
  --frontend-port 3000

# Run only HTTP transport for agent
cargo run --bin reimbursement_demo -- --transport http

```

### Available Endpoints

**Agent Backend:**
- HTTP API: `http://localhost:8080` (ConnectRPC)
- Agent Card: `http://localhost:8080/agent-card`

**Web Frontend:**
- Main UI: `http://localhost:3000`
- Task List: `http://localhost:3000/tasks`
- Expense Form: `http://localhost:3000/expense/new`

## Example Conversation

Here's an example conversation with the reimbursement agent:

1. User: "Can you reimburse me $50 for the team lunch yesterday?"

2. Agent: *Returns a form*
   ```json
   {
     "type": "form",
     "form": {
       "type": "object",
       "properties": {
         "date": {
           "type": "string",
           "format": "date",
           "description": "Date of expense",
           "title": "Date"
         },
         "amount": {
           "type": "string",
           "format": "number",
           "description": "Amount of expense",
           "title": "Amount"
         },
         "purpose": {
           "type": "string",
           "description": "Purpose of expense",
           "title": "Purpose"
         },
         "request_id": {
           "type": "string",
           "description": "Request id",
           "title": "Request ID"
         }
       },
       "required": ["request_id", "date", "amount", "purpose"]
     },
     "form_data": {
       "request_id": "request_id_1234567",
       "date": "<transaction date>",
       "amount": "50",
       "purpose": " the team lunch yesterday"
     }
   }
   ```

3. User: *Submits the filled form*
   ```json
   {
     "request_id": "request_id_1234567",
     "date": "2023-10-15",
     "amount": "50",
     "purpose": "team lunch with product team"
   }
   ```

4. Agent: "Your reimbursement request has been approved. Request ID: request_id_1234567"

## Current Limitations

The **reimbursement** reference agent deliberately keeps its business logic
simple to showcase the framework architecture (the generic `llm` handler above
is the path to real model-driven agents):

- **Message Processing**: Basic pattern matching (use `type = "llm"` for LLM-driven agents)
- **Storage**: In-memory storage (framework supports SQLx for production)
- **Authentication**: Not implemented (framework supports Bearer/OAuth2)
- **Form Processing**: Simple JSON forms without complex validation

## Future Enhancements

See the workspace [ROADMAP.md](../ROADMAP.md) for deferred themes and planned
work.

## Framework Features Demonstrated

- ✅ **AsyncMessageHandler** trait implementation
- ✅ **DefaultBusinessHandler** integration  
- ✅ **InMemoryTaskStorage** for task persistence
- ✅ **SimpleAgentInfo** for agent metadata
- ✅ **HTTP** transport support
- ✅ **Structured error handling** with A2AError
- ✅ **Modern async/await** patterns
- ✅ **Builder patterns** for complex objects