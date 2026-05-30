# A2A-RS Follow-Ups and Future Work

## Agent Payments Protocol (AP2) Integration
- Expand `a2a-ap2` crate to fully support AP2 primitives (Payment Request, Payment Receipt).
- Bridge AP2 features with native LLM tool calling (allow LLMs to request and verify payments).
- Add robust tests and error handling for AP2 flows.

## Complex Agent Example
- Create a comprehensive "kitchen-sink" example showcasing all components:
  - LLM Provider integration (OpenAI/Gemini).
  - MCP tool bridging (`AgentToMcpBridge` & `McpToA2ABridge`).
  - Streaming interactions to a Web Client (`a2a-client`).
  - Declarative TOML configuration.
  - A2A native tasks and progress tracking.

## Streaming Improvements
- Add support for partial/incremental tool call streaming (instead of waiting for the full JSON string to parse) to allow UIs to show function call progress in real time.
- Implement robust retry mechanisms and exponential backoff for SSE stream interruptions.
- Expand streaming integrations natively into the `a2a-client` framework.

## General
- Refine existing Rustdoc examples and ensure they are all compile-checked.
- Resolve any remaining compilation warnings across the workspace.

---

# 0.3.1 follow-ups

Items deferred from the 0.3.0 release. Ordered roughly by impact.

## Technical debt left after 0.3.0

### 1. MCP HTTP transport
`a2a-agents/src/core/mcp.rs:71` currently logs *"Only stdio transport is currently supported for MCP server"*. `rmcp` 1.7 already ships `streamable_http_server`, `sse`, and `ws` transports (see `rmcp-1.7.0/src/transport/`).

Scope:
- Extend `McpServerConfig` (`a2a-agents/src/core/config.rs`) with an `http` (or `streamable_http`) section: `enabled`, `host`, `port`.
- Branch on it in `a2a-agents/src/core/mcp.rs`; enable the matching `rmcp` feature in `a2a-agents/Cargo.toml`.
- Add an `mcp_http_agent.toml` example next to `mcp_server_agent.toml`.

### 2. Finish `mcp-client` framework integration
`a2a-agents/Cargo.toml:76-80` documents the feature as a work-in-progress: only the low-level `McpClientManager` is usable, framework-level integration is incomplete. Either finish it or mark the feature as preview/unstable in the README so downstream users don't trip on it.

### 3. Bump `a2a-mcp` to `edition = "2024"`
Everything else in the workspace is on 2024. The bump is blocked by a `ref` binding pattern in `a2a-mcp/src/bridge/agent_to_mcp.rs:1014`:
```rust
if let Some(a2a_rs::domain::generated::o_auth_flows::Flow::ClientCredentials(
    ref cc,
)) = &flows.flow
```
Drop the `ref` — match ergonomics handles it. After that, edition 2024 builds clean.

### 4. Proto drift between `spec/` and `a2a-rs/proto/`
For 0.3.0 we vendored the protos that `a2a-rs/build.rs` reads into `a2a-rs/proto/` so `cargo publish` would package them. That now duplicates `spec/a2a.proto` and the relevant `spec/google/api/*.proto` files, and they can drift silently. Pick one:
- **Option A:** delete `spec/` from the repo and treat `a2a-rs/proto/` as the source of truth.
- **Option B:** keep both, add a CI step that fails if the vendored files diverge from `spec/`.

## Release-pipeline ergonomics

### 5. Add a `release-plz.toml`
For 0.3.0 release-plz auto-generated CHANGELOG compare-links that pointed at the wrong version (`...v0.2.0...v0.2.1` for the 0.3.0 release; I hand-fixed each). A repo-level `release-plz.toml` would let us:
- Pin the changelog template / compare-link convention so future releases come out right.
- Filter commit types so noise like `fmt,clippy`, `fixed CI`, `Fix clippy warnings` doesn't end up in the user-facing changelog.
- Decide whether per-crate tags (`a2a-rs-v0.3.0` etc.) should be created alongside the umbrella `v0.3.0` tag, or one or the other.

### 6. Real fix for the aws-lc-sys + cross panic
Today we sidestep it by using `cross` only for `aarch64-unknown-linux-gnu` and native cargo for everything else (`.github/workflows/release-binaries.yml`). Any new target that needs cross (e.g. `aarch64-unknown-linux-musl`) hits the same `aws-lc-sys 0.41.0` "compiler bug detected" panic.

Root cause: `rustls 0.23` is pulled in by `connectrpc`, `hyper-rustls`, `reqwest` with default features, which re-enables the `aws_lc_rs` provider even though `a2a-rs` itself only asks for `ring`. Proper fix is to force `ring`-only via `[patch.crates-io]` or by chasing feature flags upstream.

### 7. Bump GitHub Actions off Node.js 20
Every CI run emits the deprecation annotation for `actions/checkout@v4`. Either bump to a newer checkout action, or set `FORCE_JAVASCRIPT_ACTIONS_TO_NODE24=true` at the job env.

## Workspace cleanups

### 8. Remove the workspace cycle in `a2a-agents-common`
`a2a-agents-common/Cargo.toml:50`:
```toml
[dev-dependencies]
a2a-agents = { path = "../a2a-agents" }
```
This is a cycle (`a2a-agents → a2a-agents-common → a2a-agents`). It works because dev-deps don't participate in the normal resolver, but it makes local `cargo publish --dry-run` misleading and is unnecessary — check whether any of `a2a-agents-common`'s tests actually use `a2a-agents`; if not, delete the dev-dep.

### 9. Delete the stale `ws_port` field
The 0.3.0 a2a-rs CHANGELOG says: *"Deleted legacy WebSocket transport infrastructure across the workspace."* But `a2a-agents/src/core/config.rs:157` still defines `ws_port` with a default. It surfaces in example TOMLs (`mcp_server_agent.toml:13` sets `ws_port = 0`). Dead config — remove the field, drop it from examples, document the removal in the next release's breaking-changes section.

### 10. Introduce `[workspace.dependencies]`
`tokio`, `serde`, `serde_json`, `thiserror`, `chrono`, `uuid`, `tracing`, `async-trait`, `futures`, `reqwest`, `bon` are duplicated across 6 `Cargo.toml`s with drifting version requirements (e.g. `thiserror = "1.0"` in most crates, `thiserror = "2"` in `a2a-mcp`). Consolidate the common set into the workspace root and use `dep.workspace = true` in members.

---

## Recommended 0.3.1 slice

If picking just three: **1 (MCP HTTP transport)**, **5 (`release-plz.toml`)**, **4 (proto-drift fix)** — they have the highest payoff per unit of effort and unblock the release pipeline before another version goes out.
