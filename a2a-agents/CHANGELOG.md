# Changelog

All notable changes to this crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0](https://github.com/EmilLindfors/a2a-rs/compare/a2a-agents-v0.4.0...a2a-agents-v0.5.0) - 2026-06-29

### Added

- *(agents)* Registry, runtime port, control-plane, and container runtime
- *(agents)* Declarative LLM handler with MCP + A2A-agent tool sources
- *(example)* Stream GLM reasoning and answer tokens live in complex_agent

### Changed

- *(agents)* Decouple llm from mcp-server; async control-plane deploy
- *(agents)* Route LLM selection through the shared provider helper

### Documentation

- *(changelog)* Close out 0.4.0; record the multi-agent platform under Unreleased
- Document the multi-agent platform; refresh stale READMEs

### Fixed

- *(agents)* Canonicalize AgentId lookups + widen env-var expansion

### Other

- Drop premature a2a-agents version bump (let release-plz drive it)

## [0.4.0](https://github.com/EmilLindfors/a2a-rs/compare/a2a-agents-v0.3.0...a2a-agents-v0.4.0) - 2026-06-05

### Added

- *(0.4)* Finish mcp-client framework integration + a2a-mcp edition 2024
- *(a2a-agents)* MCP server over Streamable HTTP transport
- *(0.4)* Typed error details, task versioning, call interceptors, streaming wiring + doc audit

### Changed

- *(a2a-agents)* Drop the stale ws_port config field

### Documentation

- Doc-comment audit, add ROADMAP, retire stale planning docs

### Feat

- *(a2a-rs)* Client Transport port + JSON-RPC 2.0 client + card negotiation

### Refactor

- *(a2a-rs)* Split streaming & push out of storage adapters (Phase 4 final)

### Added

- **MCP server over Streamable HTTP** — `run_mcp_server` can now serve a
  TOML-configured agent over MCP's Streamable HTTP transport (rmcp's
  `StreamableHttpService` on an `axum` router) in addition to stdio. Configure it
  via a new `[features.mcp_server.http]` section (`McpHttpConfig`: `enabled`,
  `host`, `port`, `path`); when `http.enabled` it takes precedence over stdio.
  DNS-rebinding protection defaults to loopback-only and is tunable via
  `allowed_hosts` / `allowed_origins` (empty `allowed_hosts` disables `Host`
  validation for proxy-fronted public binds). Enables the
  `transport-streamable-http-server` rmcp feature. New `mcp_http_agent` example
  (`examples/mcp_http_agent.{rs,toml}`) plus an end-to-end `initialize`-handshake
  and `Host`-allow-list integration test (`tests/mcp_http_test.rs`).
- **`AgentBuilder::with_streaming` / `AgentRuntime::with_streaming`** — attach a
  shared streaming backend so `tasks/subscribe` SSE streams observe the
  broadcasts a handler emits (e.g. via the `TaskStatusBroadcast` mixin). Pass the
  *same* `InMemoryStreamingHandler` your handler broadcasts to (clones share
  their subscriber registry); the runtime injects it into the transport via
  `ConnectRpcAdapter::with_streaming_handler` and logs "📡 Streaming backend
  wired into transport" when active.
- **`complex_agent` example** (`examples/complex_agent.rs` +
  `examples/complex_agent.toml`, behind `--features mcp-server`) — a kitchen-sink
  "Research Assistant" that wires declarative TOML config, optional LLM
  tool-calling (with a keyless, deterministic rule-based fallback), MCP tool
  consumption via `McpToA2ABridge` (against an in-process tool server over
  `tokio::io::duplex`), live SSE streaming of progress artifacts, and native A2A
  task lifecycle through the broadcast mixin.

### Fixed

- **Streaming through the builder reached a no-op.** `AgentRuntime::start_http`
  built its transport with `ConnectRpcAdapter::new(...)`, which defaults to a
  `NoopStreamingHandler` — so broadcasts from a builder-constructed handler never
  reached `tasks/subscribe` SSE clients. They now do when the streaming backend
  is supplied via `with_streaming` (see Added).

## [0.3.0](https://github.com/EmilLindfors/a2a-rs/compare/a2a-agents-v0.2.0...a2a-agents-v0.3.0) - 2026-05-27

### Fixed

- fixed CI
- fixed release bin

### Other

- fix formatting and doc warnings
- fmt,clippy
- Fix clippy warnings and failing tests
- migrate to Connect-Rust, refactor project structure, update protobuf specs, and clean up temporary scripts
- docs

### Added

- **`mcp-server` feature is now functional.** Previously declared as a stub
  (`mcp-server = []` with no dependency), this feature now wires `a2a-mcp`
  and `rmcp` 1.7 through `AgentBuilder` / `AgentRuntime`. Setting

  ```toml
  [features.mcp_server]
  enabled = true
  stdio = true
  ```

  in an agent's TOML and running the binary with `--features mcp-server`
  exposes every configured agent skill as an MCP tool over stdio (Claude
  Desktop and other MCP clients can call them directly).

  - `mcp-server` now resolves to `["dep:a2a-mcp", "dep:rmcp"]`.
  - `a2a-mcp = { path = "../a2a-mcp", version = "0.1", optional = true }`
  - `rmcp = { version = "1.7", features = ["server", "client",
    "transport-io", "transport-child-process"], optional = true }`

- **`mcp-client` feature is now functional.** Previously also a stub
  (`mcp-client = []`), it now pulls in `rmcp` with the matching client
  features so `core::mcp_client::McpClientManager` and
  `traits::mcp_tools::McpToolsExt` actually compile. The framework-level
  wiring (auto-routing tool calls from inside an `AsyncMessageHandler`) is
  still pending — see `a2a-mcp/TODO.md` — but the low-level API is usable
  directly today.

- New example `a2a-agents/examples/mcp_server_agent.{rs,toml}` demonstrating
  the feature end-to-end. Run with:

  ```sh
  cargo run --example mcp_server_agent -p a2a-agents --features mcp-server
  ```

### Changed

- **`AgentRuntime::run_as_mcp_server` no longer spawns a loopback HTTP
  server.** The MCP bridge now calls the configured `AsyncMessageHandler`
  in-process via `AgentToMcpBridge::with_handler` (new in `a2a-mcp`).
  This removes the 200 ms bind delay, the background HTTP task and abort
  dance, and the previous "auth ignored in MCP mode" caveat — that warning
  is gone because there is no HTTP surface to authenticate.
- `core::mcp::run_mcp_server` signature changed from
  `run_mcp_server(config, card, agent_url: String)` to
  `run_mcp_server(config, card, handler: H)` where
  `H: AsyncMessageHandler + Send + Sync + 'static`. Only the internal
  runtime called this function, so external impact should be minimal.
- `core::mcp_client::McpClientManager` updated for rmcp 1.7's
  `#[non_exhaustive]` types: `ClientInfo`/`Implementation` struct literals
  replaced with `ClientInfo::new(...).with_protocol_version(...)` and
  `Implementation::new(name, version)`; `CallToolRequestParam { ... }`
  replaced with `CallToolRequestParams::new(name).with_arguments(map)`.
- Updated callers for the breaking `AgentToMcpBridge::new(client, card)`
  signature change in `a2a-mcp` (was `new(client, card, agent_url)`).

## [0.2.0]

Earlier history precedes this changelog; see `git log` for details.
