# A2A-RS Follow-Ups and Future Work

> **Status (2026-06-01).** The `0.4` branch **completed the full port-layer
> refactor**: capability-split ports (`AsyncTaskLifecycle`/`AsyncTaskQuery`,
> push-config reconciled into `AsyncNotificationManager`), `Arc<dyn …>` dispatch
> (no viral generics), newtype IDs (`TaskId`/`ContextId`/`PushConfigId`), `*Ext`
> validation traits, the `TaskStatusBroadcast` cross-port mixin, the
> `TaskService`/transport split, and the **storage/streaming/push struct-split**
> (storage is persistence-only; streaming lives in
> `adapter::streaming::InMemoryStreamingHandler`; webhook delivery sits behind the
> `AsyncPushNotifier` port). It also landed the wire-interop transport arc: the
> JSON-RPC 2.0 + HTTP+JSON **server** adapter (`JsonRpcAdapter`), the client-side
> **`Transport` port** (`port::client`), a wire-compatible **`JsonRpcClient`**
> (`jsonrpc-client` feature), and **card-driven transport negotiation**
> (`TransportFactory` / `TransportNegotiator` / `connect`).
>
> The refactor is done bar one idiomatic-modernization straggler (the `Result<T>`
> alias, item 4 below). The roadmap sections track what's left for 0.4 and what's
> deferred to 0.5; the older backlog further down is unchanged and still open.
>
> **Update (0.4, cherry-picked from the 0.5 backlog).** Three of the four 0.5
> "cheap wins" from `OFFICIAL_SDK_COMPARISON.md` §4.4 landed in 0.4 and are green
> under `clippy --workspace --all-features --all-targets -D warnings`,
> `--no-default-features`, and the full test suite:
> **typed error details** (`ErrorInfo`/`FieldViolation`/`BadRequest` in the
> JSON-RPC `error.data` array, round-tripped by the client), **`TaskStore`
> optimistic-concurrency versioning** (the `AsyncTaskVersioning` port + `u64`
> versions in both the in-memory and sqlx adapters, with a `VersionConflict`
> error), and the **`CallInterceptor` before/after chain** on both the JSON-RPC
> client and server (plus a built-in `LoggingInterceptor`). Multi-tenancy stays
> deferred — see the 0.5 section.
>
> **Update (2026-06-02).** The **Complex Agent Example** ("kitchen-sink") landed:
> `a2a-agents/examples/complex_agent.rs` (TOML + optional LLM tool-calling +
> `McpToA2ABridge` + live SSE streaming + native tasks). Building it surfaced and
> fixed a real framework gap — the `AgentBuilder`/`AgentRuntime` path defaulted
> the transport to a **no-op streaming handler**, so handler broadcasts never
> reached `tasks/subscribe` SSE clients. New `with_streaming` builder/runtime
> hooks (backed by a blanket `impl AsyncStreamingHandler for Arc<dyn …>`) close
> it. See the Complex Agent Example section below.

## 0.4 — remaining to finish the release

Round out the transport/interop story the rest of 0.4 already tells.

1. **CLI (`a2acli`-equivalent).** The single most natural follow-on — `OFFICIAL_SDK_COMPARISON.md` #1 ("verify interop empirically") + #4. A small bin/crate that drives the new client `Transport`: `card`, `send`, `stream`, `get`, `cancel`. Self-contained, zero blast radius; doubles as the manual interop harness.
2. **Empirical cross-SDK interop check.** Point the official `a2aproject/a2acli` at our `examples/jsonrpc_server.rs` agent, and/or our `JsonRpcClient` at a stock A2A agent. (`tests/jsonrpc_client_interop_test.rs` already proves *our*-client ↔ *our*-server byte-compat; this validates against the canonical SDKs.)
3. **Runnable `jsonrpc_client` / `auto_connect` example** mirroring `examples/jsonrpc_server.rs`. Also satisfies the General item on compile-checked rustdoc examples.
4. **`pub type Result<T> = std::result::Result<T, A2AError>`** in the domain module (re-exported from the crate root) — the last idiomatic-modernization item from the now-completed port-layer refactor (the standard `std::io`/`serde_json`/`sqlx` one-liner). Add the alias, then trim `Result<X, A2AError>` signatures down to `Result<X>` while touching them. Natural to land before tagging 0.4.

## 0.5 — deferred (by weight)

Landed early in 0.4 (struck through; see the status note at the top):

- ~~**`TaskStore` versioning (`u64` optimistic concurrency)**~~ — done: `AsyncTaskVersioning` port (`version` / `get_versioned` / `update_status_checked`), `VersionedTask` domain type, `A2AError::VersionConflict`, implemented in both `InMemoryTaskStorage` and `SqlxTaskStorage` (migration `003_task_version.sql`). Every mutation — versioned or not — bumps the counter so the views never drift.
- ~~**`CallInterceptor` before/after middleware (client + server)**~~ — done: `port::interceptor` (`CallInterceptor`/`CallContext`/`CallSide` + `run_before`/`run_after`), wired into `JsonRpcClient` and `JsonRpcAdapter` (covers JSON-RPC unary, REST, and the streaming open), plus a built-in `LoggingInterceptor`.
- ~~**Richer typed error details**~~ — done: `domain::error_details` (`ErrorDetail`/`ErrorInfo`/`FieldViolation`), `A2AError::error_details()` + `reason_code()`, surfaced in the JSON-RPC `error.data` array (Google-RPC `BadRequest` for validation, an `ErrorInfo` reason on every error) and reconstructed by `jsonrpc_to_a2a`.

Still deferred:

- **Multi-tenancy** — thread a `tenant` through requests/storage (§4.4/§7). Currently only placeholder fields exist (`TaskPushNotificationConfig.tenant`, the proto `/{tenant}/…` routes). **Deferred to a focused 0.4.x** (decision 2026-06-01): it reshapes the storage/port/transport surface, so it warrants its own pass. Two viable shapes when picked up: **(a) edge tenant-routing** — a `TenantRouter` that holds per-tenant storage and resolves the tenant from the `/{tenant}/` path at the transport edge, keeping the domain/ports tenant-free (smallest blast radius, most hexagonal); **(b) per-request `tenant` param** threaded through every port method + transport extraction + storage scoping, matching the official SDK exactly (largest diff, touches every call site across all 6 crates).

## Agent Payments Protocol (AP2) Integration
- Expand `a2a-ap2` crate to fully support AP2 primitives (Payment Request, Payment Receipt).
- Bridge AP2 features with native LLM tool calling (allow LLMs to request and verify payments).
- Add robust tests and error handling for AP2 flows.

## Complex Agent Example

**Landed (2026-06-02):** `a2a-agents/examples/complex_agent.rs` (+ `complex_agent.toml`),
a "Research Assistant" kitchen-sink behind `--features mcp-server`. Builds
clean, clippy-clean, boots and serves its agent card. It wires:
  - ~~LLM Provider integration (OpenAI/Gemini)~~ — optional via `LlmProvider`
    (`from_env`), with a deterministic rule-based fallback so it runs keyless.
  - ~~MCP tool bridging (`McpToA2ABridge`)~~ — an in-process MCP tool server
    (`add`/`multiply`/`word_count`) over `tokio::io::duplex`; the agent discovers
    tools (`get_llm_tools`) and executes them (`execute_llm_tool_call`), and the
    LLM path drives tool selection.
  - ~~Streaming to a web client~~ — the handler broadcasts progress artifacts
    through the `TaskStatusBroadcast` mixin and a shared `InMemoryStreamingHandler`.
    **This exposed and fixed a real framework gap:** `AgentRuntime`'s transport
    was built with `ConnectRpcAdapter::new(...)`, which defaults to a *no-op*
    streaming handler — so handler broadcasts never reached `tasks/subscribe`
    SSE clients. Fixed by adding `AgentBuilder::with_streaming` / `AgentRuntime::
    with_streaming` (threaded into `ConnectRpcAdapter::with_streaming_handler`)
    plus a blanket `impl AsyncStreamingHandler for Arc<dyn AsyncStreamingHandler>`
    so a shared backend can be injected type-erased. The builder log now prints
    "📡 Streaming backend wired into transport" when active.
  - ~~Declarative TOML configuration~~ — identity, skills, transport, storage,
    and the `streaming` flag all come from `complex_agent.toml`.
  - ~~A2A native tasks and progress tracking~~ — every request advances a task
    `Working` → `Completed`/`Failed` via the mixin.

Remaining / optional follow-ups:
  - `AgentToMcpBridge` (re-expose the agent *as* MCP tools) is **not** in this
    example — it's already covered by `a2a-mcp/examples/bidirectional_demo.rs`.
    Fold it in only if a single end-to-end bidirectional showcase is wanted.
  - Wire MCP-native progress (`McpToA2ABridge::with_streaming` +
    `ProgressClientHandler`) so downstream tool progress also streams; the tool
    server would need to emit `notify_progress`. Currently progress is
    handler-driven (analyze → call tool → done), which is enough for the demo.

## Streaming Improvements
- Add support for partial/incremental tool call streaming (instead of waiting for the full JSON string to parse) to allow UIs to show function call progress in real time.
- Implement robust retry mechanisms and exponential backoff for SSE stream interruptions.
- Expand streaming integrations natively into the `a2a-client` framework.

## General
- **Audit all doc comments for self-containment.** Sweep `///` / `//!` docs across the workspace so each one describes the *actual architecture and behavior* on its own terms — what the type/port/adapter does and how it fits the hexagonal layering — rather than referencing design rationale, migration history, or internal planning docs (`REFACTORING_PLAN.md`, `OFFICIAL_SDK_COMPARISON.md`, `JSONRPC_ADAPTER_PLAN.md`, "the 0.5 backlog", "Phase 4", etc.) that will be deleted. Docs must still read correctly once those files are gone.
- Refine existing Rustdoc examples and ensure they are all compile-checked.
- Resolve any remaining compilation warnings across the workspace. *(Clean under `clippy --workspace --all-features --all-targets -D warnings` **and** `cargo check -p a2a-rs --no-default-features` as of the 0.4 struct-split work — the earlier `--no-default-features` warnings in `adapter/storage/task_storage.rs` went away with the streaming/broadcast code removed from storage.)*

---

# Release-pipeline & workspace tech debt (open backlog)

Items deferred from the 0.3.0 release and still unresolved as of the 0.4 work
(verified 2026-06-01: ws_port, the `a2a-mcp` edition bump, `[workspace.dependencies]`,
and `release-plz.toml` are all still pending). Independent of the 0.4 transport
arc — fold into 0.4 or a 0.4.x as convenient. Ordered roughly by impact.

## Technical debt

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

## Recommended slice

If picking just three from this backlog: **1 (MCP HTTP transport)**, **5 (`release-plz.toml`)**, **4 (proto-drift fix)** — highest payoff per unit of effort, and the pipeline ones (5) make the 0.4 tag come out clean.
