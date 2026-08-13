# TODO

Open work across the workspace, roughly in the order it is worth doing.

Companion docs: `CHANGELOG.md` records what shipped, `NOTES.md` the decisions
behind it and the hazards worth not rediscovering.

Sections 1–5 are actionable now. Sections 6–8 are deferred themes: real work,
but not scheduled — each needs its own pass rather than a slot in the current
one.

The pre-release CLI audit of 2026-07-26 is closed; what it found shipped on
2026-07-27 (see `CHANGELOG.md`).

---

## 1. Platform — runtime and delegation

- [ ] **Per-agent images** (`image` on `AgentSpec` or a `[runtime]` config
      block). The escape hatch that keeps the declarative layer from being a toy:
      a custom Rust handler becomes just a different image and the platform stops
      caring. TOML-only covers the common case; image + config covers 100%. Also
      retires the `HandlerType::Custom(_) → echo` fallback — with images
      available, an unknown handler type should be a hard error. (`a2a doctor`
      reports it as a problem today, so it is no longer *silent*; it is still
      wrong at run time.)
- [ ] **Scrub the child environment in `LocalProcessRuntime`.** The allowlist
      bounds what a *config* may name, not what a spawned child can read. Needs
      `Command::env_clear()` plus an explicit carry-over set, which is
      platform-fiddly (`PATH`, `SystemRoot`, temp dirs) — hence deferred, with
      the adapter documented as dev-only meanwhile. See `NOTES.md`.
- [ ] **`provider_from_env` cannot tell "nothing configured" from "configured
      and broken".** It returns `Option`, so a present-but-unusable setup — a
      malformed key, now also a typo'd `OPENROUTER_REASONING` — warns once and
      falls through to the non-LLM fallback, and the agent answers with the echo
      stub. That is the "looks configured, behaves like a stub" failure this repo
      treats as a bug elsewhere (`HandlerType::Custom`, which `a2a doctor`
      reports as a problem). Returning `Result` and letting the binary refuse to
      start is the honest shape; `a2a doctor` should read the same path so it
      catches it before the agent runs.
- [ ] **Reasoning for non-OpenRouter providers.** `[llm] reasoning` reaches the
      wire on `openrouter` only; `openai` and `gemini` log that they are
      dropping it. OpenAI takes `reasoning_effort` and Gemini a thinking budget,
      so both are expressible — each needs its own request field and a mapping
      from `Reasoning`, including what `Off` means where reasoning cannot be
      turned off. Until then `a2a doctor` could report a `reasoning` its provider
      will drop, which is cheaper than either mapping and catches the same
      mistake before it is billed.
- [ ] **Feed non-text parts to the model.** `extract_text` (`handlers/llm.rs`)
      joins text parts and drops the rest, so a file or data part reaches a
      multimodal model as silence — `examples/multi-model/` points MiniMax M3 at
      text only for exactly this reason. Needs a mapping from `Part` to whatever
      the provider's content array wants, and a decision for providers that have
      no such array.
- [ ] **`LlmHandler::new` takes seven positional arguments**, five of them
      collaborators the call site has to keep in the right order. The types
      differ, so nothing is silently swappable today — the cost is that a reader
      cannot tell what `2` or the third `Arc` is without the signature, and every
      new knob has to argue against making it eight. That pressure is what sent
      `[llm] reasoning` to the provider instead, which was the better home
      anyway; the next one may not be so lucky. `bon` is already a dependency;
      a `#[builder]` here costs three call sites.
- [ ] **Stream a delegated agent's tokens through** instead of polling to a
      terminal state: prefer `subscribe_to_task`, fall back to the current
      bounded `get_task` poll (`A2aAgentToolSource::invoke`).
- [ ] **Resolve peers at call time** (a dynamic registry-backed `ToolSource`) so
      late joiners are reachable. A startup-only resolution pass goes stale by
      design once agents come and go under a control plane.
- [ ] **Card-fetch refresh loop** — re-poll `/.well-known/agent-card.json` for
      liveness.
- [ ] **Persistent `AgentRegistry` adapter**, for what recovery-by-derivation
      cannot cover: agents registered by something other than this runtime, and
      discovery shared across control-plane processes. Both speculative today —
      hence a port and not a database (see `NOTES.md`).
- [ ] **Resolve the axum 0.7 (frontend) vs 0.8 (`a2a-rs`) split.** Tests use an
      `axum8` dev-dep alias as a stopgap; bump the frontend when `askama_axum`
      allows.

## 2. Platform extraction — before the provider work

Move `a2a-agents`, `a2a-agents-common`, and the Terraform provider into
`a2a-agents-platform`, depending only on **published** `a2a-rs` / `a2a-mcp` /
`a2a-ap2` (no path deps back), keeping the protocol crates clean.

Extract *before* the provider rework (§3), not after: a Terraform provider with
its own Go toolchain, Go CI, and TF acceptance tests does not belong in the
protocol repo, and the provider work is where the Go surface gets serious —
extracting afterwards means moving a much larger, freshly-churning surface. The
extraction only needs published `a2a-rs`; use a local `[patch.crates-io]` path
override if co-development is needed during the transition.

One PR, pre-1.0 "break cleanly" posture:

- [ ] Create `a2a-agents-platform`; copy `a2a-agents/`, `a2a-agents-common/`,
      and `terraform-provider-a2aagent/`.
- [ ] Flip path deps to crates.io versions (`a2a-rs = "0.4"`, etc.).
- [ ] Split the generic handler into its own crate if wanted — it is co-located
      in `a2a-agents/src/handlers/` today to avoid a circular dep with `a2a-mcp`.
- [ ] In this repo: drop `a2a-agents` / `a2a-agents-common` from the workspace
      `Cargo.toml`; point `README.md` / `CLAUDE.md` at the new repo. Keep
      `a2a-rs`, `a2a-ap2`, `a2a-client`, `a2a-mcp`, `a2acli` here.

## 3. Terraform provider ⏸ (parked)

Parked behind the standalone track and the extraction — see `NOTES.md` for why.
The design below still holds for when it resumes.

Present state, precisely: `renderTOML`
(`internal/provider/agent_resource.go:223`) emits `implementation = "llm"`, a key
`HandlerConfig` no longer reads, so the provider's output is *silently wrong* —
`terraform apply` succeeds and the agent falls back to echo. Both validators
(`:274`, `:286`) `return nil`, so nothing validates anything. And the
control-plane HTTP API built for the provider to target is not targeted; the
provider still writes files to a directory.

The fix is structural. Hand-maintaining a TOML serializer in Go against a Rust
struct is a permanent drift source, and the typed HCL attributes cover ~8 of ~40
config fields, so the provider cannot express most agents even when correct.

- [ ] **Passthrough config.** `POST /agents` already takes `config_toml: String`;
      lean into it. `AgentConfig` derives `Deserialize`, so accepting a JSON body
      variant is nearly free and lets HCL do `jsonencode(...)`. Go never learns
      the schema, so it cannot drift, and Rust becomes the sole validator for
      real.
- [ ] **Delete `validateWithJSONSchema` rather than implementing it.** One
      working validation path beats two stubs — either shell to `a2a validate`
      (needs a stdin mode; it is paths-only today) or let the control plane
      reject on deploy and surface it as a TF diagnostic. With passthrough
      config the bundled `internal/schema/agent_config.json` fixture and the
      `a2a print-schema` regeneration loop become unnecessary, not unimplemented.
- [ ] **Real lifecycle against the control-plane API:** Create = provision +
      start + register card; Read = health/inspect; Update = re-provision;
      Delete = stop + deregister. Its blocker is gone — restart-recovery landed,
      so `Read` no longer lies after a bounce (on `--runtime container`, the only
      backend a TF-driven control plane should use).
- [ ] **End-to-end `terraform apply` smoke test:** a live `a2a control-plane`,
      real HCL, assert an agent answers. Every layer is tested in isolation and
      the seams are exactly where the failures have been.

## 4. Interop and CI

- [ ] **Retire the legacy `MessageSendConfiguration.blocking`**
      (`domain/core/task.rs`) — the v0.x spelling of `return_immediately`, still
      read nowhere. The whole hand-written `MessageSendParams` family is
      re-exported but unused by the v1.0 path; decide whether it is deleted or
      documented as legacy-only.
- [ ] Point the **official** `a2aproject/a2acli` at our `examples/jsonrpc_server`
      (`:8137`) — validates our *server* against the canonical client.
- [ ] Point **our** `JsonRpcClient` / `a2acli` at a stock upstream A2A agent —
      validates our *client* against other SDKs.
- [ ] Once both pass, capture the matrix (which transports and SDKs interoperate)
      in the `a2acli` README.
- [ ] **Pin an MSRV CI job the moment the declared version drops below stable.**
      Not needed today: every workflow uses `dtolnay/rust-toolchain@stable` and
      1.96 is current stable, so CI already builds on exactly the declared
      version. That stops being true the day stable moves on — from then the
      number is unproven again, and a `dtolnay/rust-toolchain@1.96` job is what
      makes it real.

## 5. Docs

- [ ] `terraform-provider-a2aagent/README.md` still describes the provider as the
      source of truth for agent definitions. Fix when the provider resumes — or
      sooner, with a parked-WIP banner, if it starts misleading anyone.

---

## 6. Protocol and core (`a2a-rs`) — deferred themes

Real work, unscheduled. Each reshapes a surface and warrants its own pass.

- [ ] **Multi-tenancy.** Thread a `tenant` through requests and storage. Only
      placeholder fields exist today (`TaskPushNotificationConfig.tenant`, the
      proto `/{tenant}/…` routes). Two viable shapes, and the choice is the work:
      - **(a) edge tenant-routing** — a `TenantRouter` holding per-tenant
        storage, resolving the tenant from the `/{tenant}/` path at the transport
        edge, keeping domain and ports tenant-free. Smallest blast radius, most
        hexagonal.
      - **(b) per-request `tenant` parameter** threaded through every port
        method, plus transport extraction and storage scoping. Matches the
        official SDK exactly; largest diff, touches every call site in every
        crate.
- [ ] **Durable streaming resumption.** The replay buffer is in-memory and
      bounded (256 events/task); past it, resume falls back to the initial
      snapshot. A sqlx-backed event log would make resumption survive restarts.
- [ ] **ConnectRPC SSE `Last-Event-ID`.** The ConnectRPC transport has none, so
      `RetryingTransport` over it reconnects from scratch rather than resuming
      gap-free.
- [ ] **AP2 expansion (`a2a-ap2`).** Full support for the AP2 primitives
      (Payment Request, Receipt); bridge AP2 with native LLM tool calling so a
      model can request and verify payments; tests and error handling for the
      flows.

## 7. Blocked on upstream

- [ ] **`aws-lc-sys` breaks any new `cross` target.** `cross` is used only for
      `aarch64-unknown-linux-gnu` today (native cargo elsewhere) and that works,
      but any *new* cross target (e.g. `aarch64-unknown-linux-musl`) hits the
      `aws-lc-sys 0.41.0` "compiler bug detected" panic. Root cause: `rustls
      0.23` — pulled in by `connectrpc`, `hyper-rustls`, and `reqwest` defaults —
      re-enables the `aws_lc_rs` provider even though `a2a-rs` only asks for
      `ring`.
      A feature-only "ring-only" fix is **blocked by `connectrpc 0.3.3`**: it
      exposes no TLS feature flags and depends on `hyper-rustls`/`tokio-rustls`
      with their default `aws-lc-rs` provider, so no combination of our flags
      removes `aws-lc-sys`. (`sqlx` offers `tls-rustls-ring` and `reqwest`
      offers `rustls-tls-*-no-provider`, but fixing only those leaves connectrpc
      still pulling `aws-lc-rs`.) A `[patch.crates-io]` swaps the *source*, not
      features, so it cannot flip connectrpc's `hyper-rustls` default either.
      Paths, none cheap:
      - **(a)** upstream a `ring` feature into `connectrpc`, then set ring on
        `connectrpc` + `reqwest` `rustls-tls-no-provider` + `sqlx`
        `tls-rustls-ring`;
      - **(b)** fork or vendor `connectrpc` with
        `hyper-rustls = { default-features = false, features = ["ring", …] }`;
      - **(c)** keep `aws-lc-rs` and make it cross-build — a `Cross.toml` whose
        image carries clang and cmake (plus `AWS_LC_SYS_PREBUILT_NASM=1` on x86)
        — sidestepping the provider question. Needs a reproducible `cross`
        environment to validate.

## 8. Nice to have

- [ ] **Single bidirectional showcase** — fold `AgentToMcpBridge` (re-expose the
      agent *as* MCP tools) into `complex_agent`. Already covered standalone by
      `a2a-mcp/examples/bidirectional_demo.rs`; only worth it for one end-to-end
      demo.
- [ ] **MCP-native progress** — wire `McpToA2ABridge::with_streaming` +
      `ProgressClientHandler` so downstream tool progress streams (the tool
      server would need to emit `notify_progress`). Progress is handler-driven
      today.
