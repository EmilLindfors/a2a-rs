# A2A-RS Follow-Ups and Future Work

## 0.4 — remaining to finish the release

Round out the transport/interop story the rest of 0.4 already tells.

1. **CLI (`a2acli`-equivalent).** The single most natural follow-on — `OFFICIAL_SDK_COMPARISON.md` #1 ("verify interop empirically") + #4. A small bin/crate that drives the new client `Transport`: `card`, `send`, `stream`, `get`, `cancel`. Self-contained, zero blast radius; doubles as the manual interop harness.
2. **Empirical cross-SDK interop check.** Point the official `a2aproject/a2acli` at our `examples/jsonrpc_server.rs` agent, and/or our `JsonRpcClient` at a stock A2A agent. (`tests/jsonrpc_client_interop_test.rs` already proves *our*-client ↔ *our*-server byte-compat; this validates against the canonical SDKs.)
3. **Runnable `jsonrpc_client` / `auto_connect` example** mirroring `examples/jsonrpc_server.rs`. Also satisfies the General item on compile-checked rustdoc examples.
4. **`pub type Result<T> = std::result::Result<T, A2AError>`** in the domain module (re-exported from the crate root) — the last idiomatic-modernization item from the port-layer refactor (the standard `std::io`/`serde_json`/`sqlx` one-liner). Add the alias, then trim `Result<X, A2AError>` signatures down to `Result<X>` while touching them. Natural to land before tagging 0.4.

## 0.5 — deferred

- **Multi-tenancy** — thread a `tenant` through requests/storage (§4.4/§7). Currently only placeholder fields exist (`TaskPushNotificationConfig.tenant`, the proto `/{tenant}/…` routes). **Deferred to a focused 0.4.x** (decision 2026-06-01): it reshapes the storage/port/transport surface, so it warrants its own pass. Two viable shapes when picked up: **(a) edge tenant-routing** — a `TenantRouter` that holds per-tenant storage and resolves the tenant from the `/{tenant}/` path at the transport edge, keeping the domain/ports tenant-free (smallest blast radius, most hexagonal); **(b) per-request `tenant` param** threaded through every port method + transport extraction + storage scoping, matching the official SDK exactly (largest diff, touches every call site across all 6 crates).

## Agent Payments Protocol (AP2) Integration
- Expand `a2a-ap2` crate to fully support AP2 primitives (Payment Request, Payment Receipt).
- Bridge AP2 features with native LLM tool calling (allow LLMs to request and verify payments).
- Add robust tests and error handling for AP2 flows.

## Complex Agent Example

The kitchen-sink example (`a2a-agents/examples/complex_agent.rs`) has landed;
remaining / optional follow-ups:
  - `AgentToMcpBridge` (re-expose the agent *as* MCP tools) is **not** in this
    example — it's already covered by `a2a-mcp/examples/bidirectional_demo.rs`.
    Fold it in only if a single end-to-end bidirectional showcase is wanted.
  - Wire MCP-native progress (`McpToA2ABridge::with_streaming` +
    `ProgressClientHandler`) so downstream tool progress also streams; the tool
    server would need to emit `notify_progress`. Currently progress is
    handler-driven (analyze → call tool → done), which is enough for the demo.

## Streaming Improvements

The three streaming items (resilient retry/backoff, native `a2a-client`
streaming, incremental tool-call streaming) have landed; remaining / optional
follow-ups:
  - Durable (cross-restart) resumption: the replay buffer is in-memory and
    bounded (256 events/task); beyond it, resume falls back to the initial
    snapshot. A sqlx-backed event log would make resumption survive restarts.
  - A handler-level integration test asserting the tool-call `metadata` reaches
    SSE subscribers end-to-end (the accumulator and metadata builder are unit-
    tested; the broadcast wiring is covered only by compilation today).
  - ConnectRPC transport has no SSE `Last-Event-ID`, so `RetryingTransport` over
    it reconnects from scratch rather than resuming.

## General
- **Audit all doc comments for self-containment.** Sweep `///` / `//!` docs across the workspace so each one describes the *actual architecture and behavior* on its own terms — what the type/port/adapter does and how it fits the hexagonal layering — rather than referencing design rationale, migration history, or internal planning docs (`REFACTORING_PLAN.md`, `OFFICIAL_SDK_COMPARISON.md`, `JSONRPC_ADAPTER_PLAN.md`, "the 0.5 backlog", "Phase 4", etc.) that will be deleted. Docs must still read correctly once those files are gone.
- Refine existing Rustdoc examples and ensure they are all compile-checked.

---

# Release-pipeline & workspace tech debt (open backlog)

Items deferred from the 0.3.0 release and still unresolved as of the 0.4 work
(verified 2026-06-01: ws_port, the `a2a-mcp` edition bump, `[workspace.dependencies]`,
and `release-plz.toml` are all still pending). Independent of the 0.4 transport
arc — fold into 0.4 or a 0.4.x as convenient. Ordered roughly by impact.

## Technical debt

### 1. MCP HTTP transport ✅ (landed 2026-06-04)
`a2a-agents/src/core/mcp.rs` now serves the agent over MCP's Streamable HTTP
transport in addition to stdio.

Done:
- `McpServerConfig` gained an `http` section (`McpHttpConfig`: `enabled`, `host`,
  `port`, `path`, plus `allowed_hosts` / `allowed_origins` DNS-rebinding knobs)
  in `a2a-agents/src/core/config.rs`.
- `run_mcp_server` branches on `http.enabled` (takes precedence over stdio) and
  serves `rmcp`'s `StreamableHttpService` on an `axum` router via the new
  `run_streamable_http`; the `transport-streamable-http-server` `rmcp` feature
  is enabled in `a2a-agents/Cargo.toml`.
- `examples/mcp_http_agent.{toml,rs}` + integration tests (`tests/mcp_http_test.rs`):
  an end-to-end `initialize` handshake and `Host`-allow-list reject/allow checks,
  plus config-parse unit tests.

Not done (out of scope): the `sse` and `ws` rmcp transports — Streamable HTTP is
the current MCP-spec networked transport and supersedes the older SSE one.

### 2. Finish `mcp-client` framework integration ✅ (landed 2026-06-04)
The previous wiring was dead code: `build_with_auto_storage` connected to the
`[features.mcp_client]` servers and stashed the `McpClientManager` in the
`AgentRuntime`, but nothing ever read it — the handler (the actual tool
consumer) never saw it. Closed the loop with a handler-owns-the-client design:

- `McpClientManager::connect(&McpClientConfig)` is the one-call constructor
  (connect + tool discovery); lenient per-server, errors only on total failure.
  Returns a typed `McpClientError` instead of `Box<dyn Error>`.
- The handler owns the connected manager and implements `McpToolsExt`
  (`fn mcp_client(&self) -> &McpClientManager`); `McpToolsExt` now returns the
  typed error too.
- Removed the dead auto-init from the builder and the unused
  `mcp_client`/`with_mcp_client`/`mcp_client()` on `AgentRuntime`. The
  `McpClientManager`/`McpClientError` re-exports are now `mcp-client`-gated (no
  more no-feature stub).
- `bin/mcp_echo_server.rs` (fixture MCP stdio server) +
  `examples/mcp_client_agent.{rs,toml}` + `tests/mcp_client_test.rs` (spawns the
  fixture, asserts discovery + `echo`/`add` calls + `NotConnected`). README §5
  documents the flow.

### 3. Bump `a2a-mcp` to `edition = "2024"` ✅ (landed 2026-06-04)
The whole workspace is now on edition 2024. The blocker was two `ref` bindings
in a reference-matched `if let` in `a2a-mcp/src/bridge/agent_to_mcp.rs`
(`ref oauth2_scheme` and `ref cc`, matched against `&scheme.scheme` / `&flows.flow`)
— redundant under match ergonomics and an error in edition 2024. Dropped both;
builds, clippy, and tests are clean.

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

## Recommended slice

With **1 (MCP HTTP transport)** now landed, the next-highest payoff per unit of
effort are **5 (`release-plz.toml`)** and **4 (proto-drift fix)** — and the
pipeline one (5) makes the 0.4 tag come out clean.
