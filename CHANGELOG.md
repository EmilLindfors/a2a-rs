# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased] - 2026-05-24

### Added
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

### Changed
- **`a2a-rs` transport**: Extracted the ConnectRPC adapter's request-decoding helpers (`decode_send_config`, `list_request_to_params`, `map_update_event`) to `pub(super)` so the new JSON-RPC adapter reuses them — both transports now share a single decode/encode path against the generated proto types.

### Removed
- Removed the printf-only `examples/minimal_example.rs` in `a2a-mcp`.
