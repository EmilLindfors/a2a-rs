# a2a-rs

[![Crates.io](https://img.shields.io/crates/v/a2a-rs.svg)](https://crates.io/crates/a2a-rs)
[![Documentation](https://docs.rs/a2a-rs/badge.svg)](https://docs.rs/a2a-rs)
[![CI](https://github.com/EmilLindfors/a2a-rs/actions/workflows/rust.yml/badge.svg)](https://github.com/EmilLindfors/a2a-rs/actions/workflows/rust.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

An MIT-licensed Rust framework for building multi-agent systems on the
[Agent2Agent (A2A) protocol](https://a2a-protocol.org/).

- **Up to spec with A2A 1.0:** the types, a client, a server, and the storage
  behind them.
- **The latest MCP:** a bridge in both directions on MCP 2026-07-28, and
  2025-11-25 for peers that still initialize.
- **Three transports:** the spec's JSON-RPC 2.0 and HTTP+JSON bindings for
  interop with the other A2A SDKs, and ConnectRPC as the default between two
  a2a-rs peers.
- **LLM providers and agent payments:** OpenAI-compatible endpoints, Gemini,
  decision models, and the Agent Payments Protocol (AP2).

The core is hexagonal: storage, streaming, push delivery and authentication are
ports, and every adapter behind them can be swapped without touching the
agent's own logic. Every crate is MIT licensed, so you can build a product or
your own agent platform on top.

Project site: <https://emillindfors.github.io/a2a-rs/>

## Crates

| Crate | Directory | What it is |
|---|---|---|
| [![a2a-rs](https://img.shields.io/crates/v/a2a-rs.svg?label=a2a-rs)](https://crates.io/crates/a2a-rs) | [a2a-rs](./a2a-rs/) | The protocol: domain types, ports, transports (ConnectRPC, JSON-RPC 2.0, HTTP+JSON), auth, in-memory and SQLx storage |
| [![a2acli](https://img.shields.io/crates/v/a2acli.svg?label=a2acli)](https://crates.io/crates/a2acli) | [a2a-cli](./a2a-cli/) | Command-line client: `card`, `send`, `get`, `list`, `cancel`, `stream` |
| [![a2a-mcp](https://img.shields.io/crates/v/a2a-mcp.svg?label=a2a-mcp)](https://crates.io/crates/a2a-mcp) | [a2a-mcp](./a2a-mcp/) | A2A ↔ MCP bridge in both directions on MCP 2026-07-28 and 2025-11-25, built on `rmcp` |
| [![a2a-llm](https://img.shields.io/crates/v/a2a-llm.svg?label=a2a-llm)](https://crates.io/crates/a2a-llm) | [a2a-llm](./a2a-llm/) | Provider-neutral chat completions (OpenAI-compatible, Gemini) and decision models |
| [![a2a-ap2](https://img.shields.io/crates/v/a2a-ap2.svg?label=a2a-ap2)](https://crates.io/crates/a2a-ap2) | [a2a-ap2](./a2a-ap2/) | Agent Payments Protocol (AP2) types and A2A extension helpers |
| [![a2a-web-client](https://img.shields.io/crates/v/a2a-web-client.svg?label=a2a-web-client)](https://crates.io/crates/a2a-web-client) | [a2a-client](./a2a-client/) | Building blocks for web frontends to A2A agents (SSE with polling fallback, view models, Axum components) |

Two directories are named differently from the crate they hold; the crate name
is what goes in a `[dependencies]` table.

## Quick start

### Talk to an agent

```bash
cargo install a2acli    # or take a prebuilt binary from the a2acli GitHub release

export A2A_URL=http://127.0.0.1:8080
a2acli card             # what can this agent do?
a2acli send "hello"     # send a message, wait for the reply
a2acli list --state input-required
```

`a2acli` negotiates the transport from the agent card (ConnectRPC first, JSON-RPC
as the interop fallback), so it talks to agents written with other SDKs too. For
an agent to point it at, run the quick start below.

### Build an agent

```toml
[dependencies]
a2a-rs = { version = "0.10", features = ["http-server", "http-client"] }
```

An agent's business logic can be one `Responder`: given the message and the
task, return a reply and the state the task ends in.
`ResponderMessageHandler` does the rest — creating the task, appending history,
announcing every transition to streaming subscribers and push webhooks.

```rust
use a2a_rs::adapter::business::{Responder, ResponderMessageHandler};
use a2a_rs::adapter::{
    ConnectRpcAdapter, HttpServer, InMemoryStreamingHandler, InMemoryTaskStorage,
    SimpleAgentInfo,
};
use a2a_rs::domain::{A2AError, Message, Part, Role, Task, TaskState};

struct Shout;

#[async_trait::async_trait]
impl Responder for Shout {
    async fn respond(
        &self,
        message: &Message,
        task: &Task,
    ) -> Result<(Message, TaskState), A2AError> {
        let text: Vec<&str> = message.parts.iter().filter_map(|p| p.get_text()).collect();
        let reply = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(text.join(" ").to_uppercase())])
            .message_id(uuid::Uuid::new_v4().to_string())
            .task_id(task.id.clone())
            .context_id(message.context_id.clone())
            .build();
        Ok((reply, TaskState::Completed))
    }
}

// Inside an async fn:
let storage = InMemoryTaskStorage::new();
let streaming = InMemoryStreamingHandler::new();
let handler = ResponderMessageHandler::new(
    storage.clone(),
    streaming.clone(),
    storage.push_notifier(),
    Shout,
);

let card = SimpleAgentInfo::new("shout".to_string(), "http://127.0.0.1:8080".to_string())
    .with_description("Repeats what you say, louder".to_string())
    .with_streaming();
let processor = ConnectRpcAdapter::new(handler, storage.clone(), storage, card.clone())
    .with_streaming_handler(streaming);

HttpServer::new(processor, card, "127.0.0.1:8080".to_string())
    .start()
    .await?;
```

And a client:

```rust
use a2a_rs::{HttpClient, Message, Transport};
use a2a_rs::domain::SendCompletion;

let client = HttpClient::new("http://127.0.0.1:8080".to_string());
let message = Message::user_text("hello, agent".to_string(), uuid::Uuid::new_v4().to_string());

// `WhenSettled` is the A2A default: the server answers once the task has
// finished, so the reply is already in `task.history`. `None` lets the server
// name the task.
let task = client
    .send_task_message(None, &message, None, None, SendCompletion::WhenSettled)
    .await?;
```

Both halves run together in
[`examples/quickstart.rs`](a2a-rs/examples/quickstart.rs), which CI compiles:

```bash
cargo run -p a2a-rs --example quickstart --features http-server,http-client
```

An agent that acknowledges now and finishes later implements
`AsyncMessageHandler` directly instead of `Responder`. The other examples in
[`a2a-rs/examples`](a2a-rs/examples) cover bearer auth, the JSON-RPC server
and client, and SQLx storage.

## Feature flags (`a2a-rs`)

Compile only what you use. The default is `server` + `tracing`.

| Feature | What it adds |
|---|---|
| `server` (default) | Server ports, `TaskService`, in-memory storage and streaming |
| `tracing` (default) | Structured logging via `tracing` |
| `client` | Client ports, without a transport |
| `http-client` | ConnectRPC client over reqwest (`HttpClient`) |
| `jsonrpc-client` | Spec JSON-RPC 2.0 + SSE client (`JsonRpcClient`), for third-party agents |
| `http-server` | ConnectRPC server on Axum (`HttpServer`) |
| `jsonrpc-server` | Spec JSON-RPC 2.0 and HTTP+JSON routers (`jsonrpc_router`, `rest_router`) |
| `auth` | JWT, OAuth2 and OpenID Connect authenticators |
| `sqlite` / `postgres` | SQLx task storage; enable both and the backend is picked from the URL at runtime |
| `full` | All of the above |

Each feature builds on its own; CI checks every one of them in isolation.

## Protocol coverage

The A2A v1.0.0 surface, wire-compatible with the spec:

- `SendMessage` and `SendStreamingMessage`, blocking or returning immediately
- `GetTask`, `ListTasks` (filtered and paged), `CancelTask`, `SubscribeToTask`
- Push notification config: create, get, list, delete, with webhook delivery
- `GetExtendedAgentCard`, agent card signatures, and the public card readable
  without credentials on a guarded server
- Security schemes: HTTP bearer, API key, OAuth2, OpenID Connect, mTLS
- Every task state, including `input-required` and `auth-required`, and
  artifacts that stay on the task

Beyond the spec, both opt-in and backward-compatible:

- **ConnectRPC transport.** The spec names `JSONRPC`, `GRPC` and `HTTP+JSON`.
  a2a-rs adds ConnectRPC as the default between a2a-rs peers, advertised under
  the non-spec `CONNECTRPC` binding. Use the JSON-RPC transport for third-party
  interop.
- **Gap-free stream resumption via `Last-Event-ID`** (W3C SSE, not an A2A
  feature). Spec clients ignore it and get the standard
  reconnect-from-current-state behaviour; gap-free resume only applies
  a2a-rs ↔ a2a-rs.

The full list of divergences is in
[`a2a-rs` → Spec compliance](a2a-rs/README.md#spec-compliance).

## Architecture

```
  Transports    ConnectRPC  ·  JSON-RPC 2.0  ·  HTTP+JSON      (behind an auth guard)
                                      │
                                      v
  Application                    TaskService
                                      │
                                      v
  Ports         AsyncMessageHandler       AsyncStreamingHandler   Authenticator
                AsyncTaskLifecycle        AsyncEventLog           AsyncPushNotifier
                AsyncTaskQuery            AsyncNotificationManager
                AsyncConversationStore    AsyncContextStateStore
                                      │
                                      v
  Adapters      in-memory · SQLite · Postgres · streaming fan-out · JWT · OAuth2 · OIDC

  Domain        Message · Part · Task · Artifact · AgentCard · TaskState
```

Transports decode a request and hand it to `TaskService`, which talks only to
ports. The agent implements `AsyncMessageHandler` (or a `Responder`); everything
else has an adapter in-tree that can be replaced.

The ports are the surface a platform built on a2a-rs depends on, so a change
to any of them is a breaking change downstream, even when nothing in this repo
fails.

## Development

The toolchain is Rust 1.96 (`rust-version` in the workspace manifest; CI has an
MSRV job that holds it to that).

```bash
cargo build --workspace
cargo test --workspace --all-targets
cargo clippy -p a2a-rs --all-targets --all-features -- -D warnings
cargo fmt --all -- --check

# The PostgreSQL backend needs a server; without A2A_TEST_POSTGRES_URL its
# tests skip. CI runs them against a postgres:17 service.
A2A_TEST_POSTGRES_URL=postgres://postgres:a2a@localhost:5432/a2a \
  cargo test -p a2a-rs --features full --test postgres_storage_test
```

The suite has unit, integration, property-based and spec-compliance tests, and
`a2acli` doubles as a cross-SDK interop harness (see its
[README](a2a-cli/README.md#interop-harness)).

Releases are cut by [release-plz](https://release-plz.dev/) from
[Conventional Commits](https://www.conventionalcommits.org/): `feat` and `fix`
land in the changelog, and `!` marks a breaking change.

- [TODO.md](./TODO.md) — open work, and where help is welcome
- [NOTES.md](./NOTES.md) — the reasoning behind the current design
- [CHANGELOG.md](./CHANGELOG.md) — what changed, per crate

## License

MIT
