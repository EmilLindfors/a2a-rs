# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added
- **Declarative multi-agent platform (`a2a-agents`)**: `a2a-agents` grew from a single-agent builder into a platform for running and orchestrating *many* agents, with every outward capability defined as a **port** so the core `a2a-rs` protocol crate stays infrastructure-free.
  - **Agent-as-tool delegation**: a config-driven `LlmHandler` (behind the `llm` feature) can call peer A2A agents and MCP servers as tools — an orchestrator delegates work by listing peers under `[[handler.llm.agents]]` and tool sources under `[[handler.llm.mcp]]`. The handler no longer depends on `mcp-server`; LLM provider selection routes through the shared provider helper.
  - **Registry / discovery**: the `AgentRegistry` port + `InMemoryAgentRegistry` let agents find peers by **skill** instead of hard-coded URLs. Config refs resolve by `skill`/`agent_id`; the `a2a` binary self-registers in a two-phase startup.
  - **Runtime / isolation**: one `AgentRuntime` port supervises agents as local processes or OCI containers — `LocalProcessRuntime`, `ContainerRuntime` (CLI shell-out backend), and `InMemoryAgentRuntime` for tests. Sandboxing lives in the platform, not in `a2a-rs`.
  - **Control plane**: a `ControlPlane` service composes runtime + registry behind an HTTP API; `a2a control-plane` deploys, lists, and tears down agents (deploy is async).
  - **`terraform-provider-a2aagent`** (WIP): a Terraform provider for declaring agents as infrastructure.
- **OpenRouter LLM provider + reasoning (`a2a-agents-common`)**: added an OpenRouter provider with centralized provider selection and reasoning-token support; the `complex_agent` example streams GLM reasoning and answer tokens live over SSE.
- **`a2acli` command-line client**: a new CLI driving the `a2a-rs` `Transport` port — `card`, `send`, `get`, `cancel`, `stream`. Endpoint from `A2A_URL` (`--url`/`-u`); `--transport auto|connectrpc|jsonrpc`; `--json` for machine-readable output. Auto mode negotiates the transport from the agent card with a direct-client fallback, doubling as a manual cross-SDK interop harness. `auto_connect` was promoted into `a2a-rs` and is now shared by the CLI and web client.

### Changed
- **BREAKING — runtime type renamed (`a2a-agents`)**: the old single-agent server type `core::AgentRuntime<H, S>` is now `AgentServer`; `AgentRuntime` is the new platform runtime **port**. In-workspace call sites are updated in the same change (pre-1.0, no shim).

### Fixed
- **AgentId lookups are canonicalized (`a2a-agents`)** and environment-variable expansion in TOML config was widened.

## [0.4.0] - 2026-06-05

### Added
- **Client-side `Transport` port + JSON-RPC 2.0 client + card-driven negotiation (`a2a-rs`)**: The client gained a hexagonal transport abstraction mirroring the server side, plus a wire-compatible JSON-RPC 2.0 client so it can talk to any standard A2A agent.
  - `port::client::Transport` (re-exported as `a2a_rs::Transport`) is the outbound client port — the renamed, relocated `AsyncA2AClient` with an added `protocol()` discriminator. `HttpClient` (ConnectRPC) reports `"CONNECTRPC"`.
  - `JsonRpcClient` (new `jsonrpc-client` feature) implements `Transport` over the spec JSON-RPC 2.0 wire format (single `POST`, SSE for streaming), reusing the generated ProtoJSON request/response types. Its method names, error codes, and envelopes come from a shared `adapter::transport::jsonrpc_wire` module extracted from the server adapter, so the two directions are byte-compatible (proven by `tests/jsonrpc_client_interop_test.rs`, an in-process client↔server round-trip over a real socket: send/get/list/cancel, push-config CRUD, SSE subscribe, typed error mapping).
  - `TransportFactory` + `TransportNegotiator` + `connect(base_url, &negotiator)` select a transport from an agent card's `supported_interfaces`, ranked by client preference (factory registration order). `default_registry()` prefers CONNECTRPC then JSON-RPC. Unit tests in `tests/transport_negotiation_test.rs`.
  - `a2a-web-client`'s `WebA2AClient` now holds a `Box<dyn Transport>` (field `transport`, was `http`); `auto_connect` performs real card-driven negotiation, falling back to a direct ConnectRPC client.
- **Wire-compatible JSON-RPC 2.0 + HTTP+JSON transport (`a2a-rs`)**: Added `JsonRpcAdapter`, a sibling of `ConnectRpcAdapter` that speaks the spec-mandated JSON-RPC 2.0 and HTTP+JSON (REST) bindings for interop with the canonical `a2aproject` SDK (and the Go/C#/Python SDKs). Behind the new `jsonrpc-server` feature.
  - Wraps the same inner `TaskService`; mounted at the composition edge via the `jsonrpc_router` / `rest_router` free functions (see `examples/jsonrpc_server.rs`).
  - JSON-RPC: single `POST /` with all 11 methods (`SendMessage`, `GetTask`, `ListTasks`, `CancelTask`, push-config CRUD, `GetExtendedAgentCard`), `A2AError` → spec error codes (`-32001`…, `-32700`/`-32601`/`-32602`), and SSE for the two streaming methods.
  - REST: official-SDK paths (no `/v1` prefix) — `POST /message:send`, `GET /tasks/{id}`, `GET /extendedAgentCard`, push-config routes — with HTTP status mapped from `A2AError`. Task custom-verbs use slash-form aliases (`/tasks/{id}/cancel`) since axum's matchit router rejects a path-param + `:`-suffix in one segment.
  - The wire body reuses the `buffa`-generated proto request/response types directly: verified ProtoJSON-clean (camelCase, SCREAMING_SNAKE enums, RFC3339 timestamps, base64 `bytes`, bare `Struct` metadata, tag-free field-presence unions), so no hand-written wire DTOs are needed. Golden + behavioral tests in `tests/jsonrpc_wire_test.rs` and `tests/jsonrpc_dispatch_test.rs`.
  - End-to-end router tests in `tests/jsonrpc_router_test.rs` drive the real `jsonrpc_router`/`rest_router` via `tower::ServiceExt::oneshot`: REST round-trip, the `/tasks/{id}/cancel` slash alias, 404/error-status mapping, list-via-query, the JSON-RPC envelope + version rejection, and both SSE framings (JSON-RPC wraps each event in a response envelope; REST emits the bare ProtoJSON `StreamResponse`).
- **Agent-card transport negotiation (`a2a-rs`)**: `SimpleAgentInfo` gained `with_preferred_transport` and `add_interface` so a card can advertise multiple `supportedInterfaces` (e.g. `JSONRPC` + `HTTP+JSON`) — the metadata an off-the-shelf A2A client reads to negotiate a transport. `examples/jsonrpc_server.rs` advertises both bindings it mounts.
- **Native LLM Tool Calling**: Added `LlmProvider` primitives (`ToolDefinition`, `ToolCall`) to `a2a-agents-common` for standardizing function calling across models (OpenAI, Gemini).
- **LLM Streaming Support (SSE)**: Added `chat_completion_stream` to stream content and fully formed tool calls in real time.
- **AI/LLM Integration (Phase 3)**: Integrated `McpClientManager` into `AgentBuilder` via the `mcp-client` feature.
- **a2a-mcp Integration Examples**:
  - `a2a_as_mcp_server.rs`: A2A agent exposed as MCP tools via `AgentToMcpBridge`.
  - `a2a_with_mcp_tools.rs`: A2A handler augmented with MCP tools via `McpToA2ABridge`.
  - `bidirectional_demo.rs`: Both bridges running in one process.
- **a2a-mcp Feature Completions**:
  - Bridged A2A `message/stream` ↔ MCP progress/sampling.
  - Bridged A2A artifacts ↔ MCP resources.
  - Bridged A2A skills ↔ MCP prompts.
  - Bridged A2A `SecurityScheme` ↔ MCP auth.
  - Added support for task cancellation propagation across bridges.
  - Added task resubscription on `McpToA2ABridge` (handled natively by A2A task storage).
  - Added in-process A2A handler backend for `AgentToMcpBridge` (`AgentToMcpBridge::with_handler` / `with_handler_and_namespace`).
- **Interactive Forms & Follow-ups over MCP**: Addressed via the Sampling API in `AgentToMcpBridge` (managing the `InputRequired` state).
- **Authentication Flows over MCP**: `AuthRequired` state mapped to `InputRequired` in MCP, correctly passing auth flows to LLMs.
- **Long-Running Tasks & Progress Streaming**: Implemented streaming bridge mapped to `notifications/progress` and A2A artifacts.
- **Testing & Coverage**: Added unit tests to `a2a-client`, `a2a-agents-common` and a comprehensive e2e framework lifecycle test.
- **Documentation**:
  - Documented the metadata tool-call envelope used by `McpToA2ABridge` in `lib.rs` crate-level rustdoc and the bridge's struct docs.
  - Added an architecture diagram to the `a2a-mcp` README.
  - Converted rustdoc examples in `a2a-mcp`'s `lib.rs` from `rust,ignore` to real compile-checked `no_run` doctests.
- **Kitchen-sink complex agent example (`a2a-agents`)**: `examples/complex_agent.rs` (+ `complex_agent.toml`, behind `--features mcp-server`) — a "Research Assistant" wiring every major building block in one binary: declarative TOML config, optional LLM tool-calling via `LlmProvider` (with a keyless rule-based fallback), MCP tool consumption through `McpToA2ABridge` against an in-process tool server, live SSE streaming of progress artifacts, and native A2A task lifecycle via the `TaskStatusBroadcast` mixin.
- **Builder-level streaming wiring (`a2a-agents` + `a2a-rs`)**: `AgentBuilder::with_streaming` / `AgentRuntime::with_streaming` attach a shared streaming backend that the runtime injects into the transport (`ConnectRpcAdapter::with_streaming_handler`), so `tasks/subscribe` SSE streams finally observe the broadcasts a handler emits. Backed by a new forwarding blanket `impl AsyncStreamingHandler for Arc<dyn AsyncStreamingHandler>` in `a2a-rs`. **Fixes** a gap where the builder path defaulted to a no-op streaming handler and silently dropped handler broadcasts before they reached SSE clients.

### Changed
- **`a2a-rs` transport**: Extracted the ConnectRPC adapter's request-decoding helpers (`decode_send_config`, `list_request_to_params`, `map_update_event`) to `pub(super)` so the new JSON-RPC adapter reuses them — both transports now share a single decode/encode path against the generated proto types.
- **BREAKING — client port renamed and relocated (`a2a-rs`)**: The client trait `services::client::AsyncA2AClient` is now `port::client::Transport` (re-exported as `a2a_rs::Transport`), with a new required `fn protocol(&self) -> &str` method. `StreamItem` moved alongside it (`a2a_rs::StreamItem`). The `services::client` module and the `services::{AsyncA2AClient, StreamItem}` re-exports are gone. Call sites import `a2a_rs::Transport` / `a2a_rs::StreamItem`; method names are unchanged. `a2a-web-client`'s `WebA2AClient.http: HttpClient` field became `transport: Box<dyn Transport>`.

### Removed
- Removed the printf-only `examples/minimal_example.rs` in `a2a-mcp`.
