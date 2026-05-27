# Changelog

All notable changes to this crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.1](https://github.com/EmilLindfors/a2a-rs/compare/a2a-agents-v0.2.0...a2a-agents-v0.2.1) - 2026-05-27

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
