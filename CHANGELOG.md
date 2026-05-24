# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased] - 2026-05-24

### Added
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

### Removed
- Removed the printf-only `examples/minimal_example.rs` in `a2a-mcp`.
