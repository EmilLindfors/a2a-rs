# a2a-rs

[![Crates.io](https://img.shields.io/crates/v/a2a-rs.svg)](https://crates.io/crates/a2a-rs)
[![Documentation](https://docs.rs/a2a-rs/badge.svg)](https://docs.rs/a2a-rs)
[![CI](https://github.com/emillindfors/a2a-rs/actions/workflows/rust.yml/badge.svg)](https://github.com/emillindfors/a2a-rs/actions/workflows/rust.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A Rust implementation of the [Agent-to-Agent (A2A) Protocol](https://google.github.io/A2A/) v1.0.0. Provides a modular framework for building agents that communicate over ConnectRPC, following hexagonal architecture principles.

## Overview

The workspace is organized into several crates:

| Crate | Description |
|-------|-------------|
| [a2a-rs](./a2a-rs/) | Core protocol library — types, traits, transports, storage |
| [a2a-ap2](./a2a-ap2/) | Agent Payments Protocol (AP2) extension |
| [a2a-agents](./a2a-agents/) | Declarative TOML agent framework + multi-agent platform (registry, runtime, control-plane) |
| [a2a-llm](./a2a-llm/) | Provider-neutral LLM vocabulary and providers (OpenAI-compatible, Gemini) |
| [a2a-client](./a2a-client/) | Web client library for building agent frontends |
| [a2a-mcp](./a2a-mcp/) | Bidirectional A2A ↔ MCP bridge (Model Context Protocol) |

## Quick start

Define an agent in TOML and run it — no Rust required. The `echo` template needs
no API keys and no external services:

```bash
cargo install a2a-agents        # installs the `a2a` CLI

a2a new "Weather Agent"                  # writes weather-agent.toml
a2a validate --config weather-agent.toml # is the config well-formed?
a2a doctor   --config weather-agent.toml # will it run on this machine?
a2a run      --config weather-agent.toml # prints the endpoint and how to poke it
```

To work on the crates themselves:

```bash
git clone https://github.com/emillindfors/a2a-rs.git
cd a2a-rs
cargo build --workspace

# The reimbursement reference agent (agent + web UI on http://localhost:3000).
# Opt-in: it is a sample, so it is not in the default build.
cargo run -p a2a-agents --features reimbursement-agent --bin reimbursement_demo
```

### Add to your project

```toml
[dependencies]
# Server with default features (in-memory storage, tracing)
a2a-rs = "0.4"

# HTTP client
a2a-rs = { version = "0.4", features = ["http-client"] }

# HTTP server with Axum
a2a-rs = { version = "0.4", features = ["http-server"] }

# All transports, auth, SQLite + PostgreSQL storage
a2a-rs = { version = "0.4", features = ["full"] }
```

## Features

The core library uses Cargo feature flags so you only compile what you need:

| Feature | Description |
|---------|-------------|
| `server` (default) | Async server traits and in-memory storage |
| `tracing` (default) | Structured logging via `tracing` |
| `http-server` | Axum-based HTTP server |
| `http-client` | HTTP client via reqwest |
| `auth` | JWT, OAuth2, OpenID Connect authentication |
| `sqlite` | SQLite storage via SQLx |
| `postgres` | PostgreSQL storage via SQLx |
| `full` | All of the above |

## Usage

### Client

```rust
use a2a_rs::{HttpClient, Message};
use a2a_rs::Transport;
use a2a_rs::domain::SendCompletion;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = HttpClient::new("http://localhost:3030".to_string());

    let message = Message::user_text(
        "I need to submit a $50 lunch expense".to_string(),
        "msg-123".to_string(),
    );
    // `WhenSettled` is the A2A default: the server holds the response until the
    // task finishes, so `task.status.state` is the agent's answer, not `WORKING`.
    let task = client
        .send_task_message("task-123", &message, None, None, SendCompletion::WhenSettled)
        .await?;

    println!("Task state: {:?}", task.status.state);
    Ok(())
}
```

### Server

```rust
use a2a_rs::{HttpServer, SimpleAgentInfo, DefaultRequestProcessor};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = HttpServer::new(
        DefaultRequestProcessor::new(),
        SimpleAgentInfo::new("my-agent".to_string(), "1.0.0".to_string()),
        "127.0.0.1:3030".to_string(),
    );

    server.start().await?;
    Ok(())
}
```


### Declarative agent (TOML-based)

The `a2a-agents` framework lets you define agents with minimal boilerplate:

```rust
use a2a_agents::AgentBuilder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    AgentBuilder::from_file("agent.toml")?
        .with_handler(MyHandler)
        .build_with_auto_storage()
        .await?
        .run()
        .await?;
    Ok(())
}
```

See [a2a-agents/examples/](./a2a-agents/examples/) for complete examples.

### Multi-agent platform

`a2a-agents` is more than a single-agent builder — it provides the platform
capabilities for running and orchestrating *many* agents, defined as **ports**
in the platform layer (the pure `a2a-rs` protocol crate stays infrastructure-free):

- **Agent-as-tool delegation** — a config-driven `LlmHandler` (enable the `llm`
  feature) can call peer A2A agents as tools, so an orchestrator delegates work
  by listing them in `[[handler.llm.agents]]`.
- **Registry / discovery** — find peers by **skill** instead of hard-coded URLs
  (`AgentRegistry` port + `InMemoryAgentRegistry`).
- **Runtime / isolation** — supervise agents as local processes or OCI
  containers behind one `AgentRuntime` port (`LocalProcessRuntime`,
  `ContainerRuntime`).
- **Control plane** — a service composing runtime + registry with an HTTP API;
  `a2a control-plane` deploys, lists, and tears down agents.

```bash
# The `a2a` binary needs the llm, mcp-server, and schema features.
# Run one agent from a TOML config
cargo run -p a2a-agents --bin a2a -- run --config agent.toml

# Serve the control plane over local processes
cargo run -p a2a-agents --bin a2a -- \
  control-plane --bind 127.0.0.1:9090 --config-dir ./deployed --runtime local
```

See [a2a-agents/README.md](./a2a-agents/README.md) for the full platform
documentation — fleets, pre-flight checks, the control plane, and the `a2a` CLI.

## Architecture

The core library follows hexagonal architecture with clear layer separation:

```
                        Application Layer
            ┌──────────────────┬─────────────────────┐
            │  ConnectRPC      │  HTTP Transport     │
            │  Handlers        │                     │
            └────────┬─────────┴──────────┬──────────┘
                     │                    │
                     v                    v
                         Port Layer
            ┌──────────────────┬─────────────────────┐
            │  MessageHandler  │  StreamingHandler    │
            │  TaskManager     │  NotificationManager │
            │  Authenticator   │  RequestProcessor    │
            └────────┬─────────┴──────────┬──────────┘
                     │                    │
                     v                    v
                        Domain Layer
            ┌──────────────────┬─────────────────────┐
            │  Message, Part   │  AgentCard           │
            │  Task, Artifact  │  Capabilities        │
            │  TaskStatus      │  SecurityScheme      │
            └──────────────────┴─────────────────────┘
```

Port traits define the contracts between layers. Implement `AsyncMessageHandler` to handle incoming messages; implement `AsyncTaskManager` for task persistence. The framework provides default implementations (in-memory storage, SQLx backends) that can be swapped without changing business logic.

## Protocol coverage

Implements the A2A v1.0.0 protocol surface — wire-compatible with the spec, with
a couple of small, documented and backward-compatible divergences (see
[`a2a-rs` → Spec compliance](a2a-rs/README.md#spec-compliance)):

- `message/send` and `message/stream` (blocking and streaming message exchange)
- `tasks/get`, `tasks/list`, `tasks/cancel`, `tasks/resubscribe`
- Push notification CRUD (set, get, list, delete)
- `agent/getAuthenticatedExtendedCard`
- Security schemes: HTTP bearer, API key, OAuth2, OpenID Connect, mTLS
- Task states: submitted, working, input-required, completed, canceled, failed, rejected, auth-required

Notable enhancements beyond the spec (both opt-in / backward-compatible):

- **ConnectRPC transport.** The spec names `JSONRPC`, `GRPC`, and `HTTP+JSON`;
  a2a-rs adds **ConnectRPC** as the in-tree default (advertised under the
  non-spec `CONNECTRPC` binding) alongside a spec-compliant JSON-RPC 2.0
  transport. Use the JSON-RPC transport for third-party interop.
- **Gap-free SSE stream resumption via `Last-Event-ID`** (W3C SSE standard, not
  an A2A spec feature). Interoperable — spec clients fall back to standard
  reconnect-from-current-state — but gap-free resume only applies a2a-rs ↔ a2a-rs.

## Testing

```bash
# Full workspace
cargo test --workspace

# Core library with all features
cd a2a-rs && cargo test --all-features

# Agent framework
cd a2a-agents && cargo test
```

The test suite includes unit tests, integration tests, property-based tests, and spec compliance tests.

## Contributing

Contributions are welcome. To get started:

```bash
git clone https://github.com/emillindfors/a2a-rs.git
cd a2a-rs
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

See [TODO.md](./TODO.md) for open work and areas where help is appreciated, and
[NOTES.md](./NOTES.md) for the reasoning behind the current design.

## License

MIT
