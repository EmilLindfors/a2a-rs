# Changelog

All notable changes to this crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed - Breaking

- **`McpToA2ABridge` tool-call wire format replaced.** The bridge no longer
  detects tool calls by scanning `Message.parts` for a `Text` part starting
  with `TOOL_CALL: <name>`. It now reads a typed [`McpToolCall`] envelope
  from `Message.metadata` under the key `a2a_rs_tool_call` (exported as
  `MCP_TOOL_CALL_METADATA_KEY`). The `parts` vector is no longer inspected
  for routing and is free for any display/logging payload.

  Migration: replace ad-hoc message construction with
  `create_tool_call_message(name, args)` or `attach_tool_call(&mut msg, name, args)`.
  Messages built with the previous `TOOL_CALL:` text prefix are now treated as
  ordinary text and forwarded to the inner `AsyncMessageHandler` unchanged.

  ```rust
  // Before
  let msg = Message::builder()
      .role(Role::User)
      .parts(vec![
          Part::Text { text: "TOOL_CALL: add".into(), metadata: None },
          Part::Data { data: data_map, metadata: None },
      ])
      .message_id(id)
      .build();

  // After
  let msg = a2a_mcp::create_tool_call_message("add", json!({"a": 5, "b": 7}));
  ```

- **`AgentToMcpBridge::new` signature simplified.** The redundant
  `agent_url: String` parameter has been removed; the bridge now derives its
  MCP tool-name namespace from `agent_card.url`. For the rare cases where the
  namespace should differ from the advertised URL (e.g. tunnels, reverse
  proxies), use the new `AgentToMcpBridge::with_namespace(client, card, namespace)`
  constructor.

  ```rust
  // Before
  let bridge = AgentToMcpBridge::new(client, card, "https://...".to_string());

  // After
  let bridge = AgentToMcpBridge::new(client, card);
  // Or, with explicit namespace:
  let bridge = AgentToMcpBridge::with_namespace(client, card, "internal-alias".into());
  ```

### Added

- **MCP prompts ↔ A2A skills mapping for `McpToA2ABridge`**:
  - Automatically query downstream MCP prompts via `list_prompts` and expose them as A2A skills.
  - Route prompt calls using a typed `MCP_PROMPT_CALL_METADATA_KEY` (`"a2a_rs_prompt_call"`) envelope in `Message.metadata` to `self.mcp_peer.get_prompt`.
  - Convert `PromptMessage`s back to A2A messages.
- **Dynamic Auth Bridging**:
  - Parse `self.agent_card.security_schemes` in `AgentToMcpBridge::get_info` and advertise `"io.modelcontextprotocol/oauth-client-credentials"` extension capability if OAuth2 client credentials scheme is present.
- **Bidirectional Task Cancellation**:
  - Added `cancel_task` to the `BridgeBackend` trait and implemented it for HTTP and WebSocket backends.
  - Implemented `cancel_task` on `AgentToMcpBridge` to support upstream cancellation.
  - Implemented `RequestCancelGuard` in `McpToA2ABridge` to cancel downstream MCP tool/prompt requests if the wrapping A2A task is canceled/dropped.
- **`McpToA2ABridge` Progress Streaming & Token Matching**:
  - Dynamic matching of auto-generated client progress tokens returned by `rmcp` to correctly route downstream progress updates to A2A clients via `streaming_handler`.
- **Task streaming, progress reporting, and client sampling support for `AgentToMcpBridge`**:
  - Support streaming task status, progress updates, and artifacts from the agent back to MCP clients.
  - Implement MCP progress notification support using `progress_token` (retrieved from `RequestContext::meta`).
  - Implement client-driven LLM sampling support: if the streaming task or polling loop transitions to `InputRequired`, the bridge suspends execution, converts the task history and current prompt into sampling messages, requests input from the client peer via `peer.create_message`, and resumes the task with the response.
  - Added polling fallback loop for backends that do not support status streams (e.g. HTTP backends).
- **Public `BridgeBackend` trait & WebSocket Backend**:
  - Re-exported `BridgeBackend` as a public trait to allow custom backend implementations.
  - Implemented `WebSocketBackend` for communication with WebSocket-based agents (enabled via the `"ws-client"` feature).
  - Added `AgentToMcpBridge::with_websocket` and `with_websocket_and_namespace` constructors.
- **In-process backend for `AgentToMcpBridge`.** Two new constructors —
  `AgentToMcpBridge::with_handler(handler, card)` and
  `with_handler_and_namespace(handler, card, namespace)` — call an
  in-process `AsyncMessageHandler` directly instead of dialing the agent
  over HTTP. When the bridge and the wrapped agent live in the same
  process, this skips the loopback HTTP server entirely.

  ```rust
  // HTTP-backed (unchanged) — for agents in another process/host:
  let bridge = AgentToMcpBridge::new(HttpClient::new(url), card);

  // In-process — for agents living in the same process:
  let bridge = AgentToMcpBridge::with_handler(my_handler, card);
  ```

  Covered by `tests/agent_to_mcp_integration.rs::test_in_process_backend_dispatches_to_handler`.
- `McpToolCall` public struct (`{ name, arguments }`) representing the
  tool-call envelope carried in `Message.metadata`.
- `MCP_TOOL_CALL_METADATA_KEY` constant (`"a2a_rs_tool_call"`) — the metadata
  key the bridge looks at.
- `attach_tool_call(&mut Message, name, arguments)` helper for adding a
  tool-call envelope to an existing message without losing its `parts`.
- `AgentToMcpBridge::with_namespace(...)` constructor for explicit
  tool-name namespacing.
- Examples: `a2a_as_mcp_server.rs` (A2A agent exposed as MCP tools),
  `a2a_with_mcp_tools.rs` (A2A handler augmented with MCP tools), and
  `bidirectional_demo.rs` (both bridges in one process: upstream MCP
  client → `AgentToMcpBridge` → A2A HTTP server → `McpToA2ABridge` →
  downstream calculator MCP server).

### Documentation

- The metadata tool-call envelope is now documented in the crate-level
  rustdoc (`lib.rs`) and in `McpToA2ABridge`'s struct docs, including a
  worked example of the on-wire JSON shape.
- Crate-level rustdoc examples in `lib.rs` were promoted from
  `rust,ignore` to compile-checked `no_run` doctests, rewritten against
  the real public API (the previous snippets referenced a non-existent
  `A2AClient`). Doctests are now part of `cargo test --doc -p a2a-mcp`.

## [0.1.0]

### Added

- Initial release: bidirectional bridge between A2A and MCP via `rmcp` 1.7.
- `AgentToMcpBridge` — implements `rmcp::ServerHandler` to expose A2A agent
  skills as MCP tools.
- `McpToA2ABridge` — wraps an `AsyncMessageHandler` and routes designated
  messages to MCP tools on an underlying MCP server.
- Converters: `MessageConverter`, `SkillToolConverter`, `TaskResultConverter`.
- Error type `A2aMcpError` with bidirectional conversion to/from A2A and MCP
  error families.
