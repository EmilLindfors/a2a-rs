# A2A-RS Roadmap

Deferred themes and not-yet-scheduled work. Pre-1.0 with only in-workspace
consumers: break cleanly and fix call sites in one PR — no deprecation shims.

## 0.5

### CLI (`a2acli`) + empirical cross-SDK interop

A small `a2acli` bin crate driving the client `Transport` port: `card`, `send`,
`stream`, `get`, `cancel`.

- Depend on **`a2a-rs` directly** (the `client` / `http-client` /
  `jsonrpc-client` features) — *not* on `a2a-client`, which drags in
  axum/askama for zero CLI benefit. The reusable client core (`Transport`,
  `JsonRpcClient`/`HttpClient`, transport negotiation, `subscribe_resilient`)
  already lives in `a2a-rs`; `a2a-client`'s `WebA2AClient` is a thin wrapper
  over it. The CLI and the web client are siblings on `a2a-rs`, not a stack.
- Promote the one ergonomic bit currently trapped in the web crate —
  `auto_connect` (URL-validate → `connect` → ConnectRPC fallback) — down into
  `a2a-rs` behind the `client` feature so both consumers share it.

Doubles as the manual interop harness: point the official `a2aproject/a2acli`
at our `examples/jsonrpc_server.rs`, and/or our `JsonRpcClient` at a stock A2A
agent, to validate wire-compat against the canonical SDKs.
(`tests/jsonrpc_client_interop_test.rs` already proves our-client ↔ our-server
byte-compat; this validates against *other* SDKs.)

### AP2 (Agent Payments Protocol) expansion

- Expand `a2a-ap2` to fully support AP2 primitives (Payment Request, Receipt).
- Bridge AP2 with native LLM tool calling (let LLMs request and verify payments).
- Add robust tests and error handling for AP2 flows.

### Multi-tenancy

Thread a `tenant` through requests/storage. Today only placeholder fields exist
(`TaskPushNotificationConfig.tenant`, the proto `/{tenant}/…` routes). It
reshapes the storage/port/transport surface, so it warrants its own pass. Two
viable shapes:

- **(a) edge tenant-routing** — a `TenantRouter` holding per-tenant storage,
  resolving the tenant from the `/{tenant}/` path at the transport edge, keeping
  domain/ports tenant-free (smallest blast radius, most hexagonal).
- **(b) per-request `tenant` param** threaded through every port method +
  transport extraction + storage scoping, matching the official SDK exactly
  (largest diff, touches every call site across all crates).

### Durable streaming resumption

The replay buffer is in-memory and bounded (256 events/task); beyond it, resume
falls back to the initial snapshot. A sqlx-backed event log would make
resumption survive restarts.

### ConnectRPC SSE `Last-Event-ID`

ConnectRPC transport has no SSE `Last-Event-ID`, so `RetryingTransport` over it
reconnects from scratch rather than resuming gap-free.

## Release pipeline

### aws-lc-sys + `cross` (blocked on upstream)

`cross` is used only for `aarch64-unknown-linux-gnu` today (native cargo
elsewhere), and that works. Any *new* cross target (e.g.
`aarch64-unknown-linux-musl`) hits the `aws-lc-sys 0.41.0` "compiler bug
detected" panic. Root cause: `rustls 0.23` (pulled in by `connectrpc`,
`hyper-rustls`, `reqwest` defaults) re-enables the `aws_lc_rs` provider even
though `a2a-rs` only asks for `ring`.

A feature-only "ring-only" fix is **blocked by `connectrpc 0.3.3`**: it exposes
no TLS feature flags and depends on `hyper-rustls`/`tokio-rustls` with their
default `aws-lc-rs` provider, so no combination of our flags removes
`aws-lc-sys`. (`sqlx` offers `tls-rustls-ring` and `reqwest` offers
`rustls-tls-*-no-provider`, but fixing only those leaves connectrpc still pulling
`aws-lc-rs`.) Cargo `[patch.crates-io]` swaps the *source*, not features, so it
can't flip connectrpc's `hyper-rustls` default either. Viable paths:

- **(a)** upstream a `ring` feature into `connectrpc`, then set ring on
  `connectrpc` + `reqwest` `rustls-tls-no-provider` + `sqlx` `tls-rustls-ring`;
- **(b)** fork/vendor `connectrpc` with
  `hyper-rustls = { default-features = false, features = ["ring", …] }`;
- **(c)** leave `aws-lc-rs` in and make it cross-build — a `Cross.toml` whose
  image has clang+cmake (and `AWS_LC_SYS_PREBUILT_NASM=1` on x86) — sidestepping
  the provider question. Needs a reproducible `cross` env to validate.

## Optional / nice-to-have

- **Single bidirectional showcase** — fold `AgentToMcpBridge` (re-expose the
  agent *as* MCP tools) into `complex_agent`. Already covered standalone by
  `a2a-mcp/examples/bidirectional_demo.rs`; only worth it for one end-to-end demo.
- **MCP-native progress** — wire `McpToA2ABridge::with_streaming` +
  `ProgressClientHandler` so downstream tool progress streams (the tool server
  would need to emit `notify_progress`). Progress is handler-driven today.
