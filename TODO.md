# TODO

Open work across the workspace, roughly in the order it is worth doing.

Companion docs: `CHANGELOG.md` records what shipped, `NOTES.md` the decisions
behind it and the hazards worth not rediscovering.

Sections 1–6 are actionable now. Sections 7–9 are deferred themes: real work,
but not scheduled — each needs its own pass rather than a slot in the current
one.

The pre-release CLI audit of 2026-07-26 is closed; what it found shipped on
2026-07-27 (see `CHANGELOG.md`).

---

## 1. Context management for long-running agents

The core shipped on 2026-08-15: `[handler.llm.context]`, the
`AsyncConversationStore` port over `task_history`, token budgeting, trimming and
compaction. `CHANGELOG.md` has what; `NOTES.md` has why the conversation lives
in the protocol rather than in the handler, and read that before reopening any
of it.

The design follows Google ADK, which is A2A's reference companion, so the
mapping is protocol-native rather than invented here: ADK `Session` is A2A's
`context_id`, ADK's event log is `task_history`, ADK `Session.state` is the
per-context scratchpad still open below, and `MemoryService` is the deferred
retrieval tier.

What is left, roughly in the order it is worth doing.

The principal now reaches the handler (`port::RequestContext`, 2026-08-15), so
`mode = "context"` works alongside `[server.auth]` and a context belongs to
whoever started it. That was the blocker; what follows is not.

**Correctness and operability:**

- [ ] **`a2a doctor` cannot check `max_input_tokens` against the model.** A
      ceiling far above what the configured model actually has is the remaining
      context misconfiguration nothing catches; it needs a model-name → window
      table, which goes stale with every release — the same cost that keeps
      `[llm] reasoning` off OpenAI and Gemini. (The storage half shipped
      2026-08-16: `mode = "context"` over in-memory storage is now a warning.)
- [ ] **A failed history load makes an agent quietly amnesiac.**
      `LlmHandler::load_conversation` logs and returns an empty conversation for
      anything that is not `ContextAccessDenied`, on the grounds that answering
      without context beats not answering. For a database that is down that is
      probably right; it also means a persistent storage fault shows up as an
      agent that has forgotten the conversation rather than as an error. Revisit
      once there is somewhere to surface degraded-but-serving.
- [ ] **`mode = "task"` re-reads the whole task every turn** through
      `AsyncTaskLifecycle::get`, which returns artifacts and status to use only
      the history. Fine at current sizes.
- [ ] **Both reads on one turn ask the same question about ownership.** The
      claim and the check are one statement now (`claim_or_check_context`, see
      `CHANGELOG.md`), so a turn with `mode = "context"` *and* `remember = true`
      asks twice rather than four times — but it is the same question, and
      every `remember` asks it again. A context's owner never changes once the
      row exists, so the answer is cacheable; what stops that being free is that
      the cache is unbounded and becomes a second source of truth for an
      authorization decision. Not urgent — these are indexed primary-key
      lookups — but it is the per-turn floor, and it grows with each thing a
      turn remembers.

**Next tiers of memory.** The design is settled — see `NOTES.md`, and read it
before reopening any of it. The state bag shipped 2026-08-17
(`[handler.llm.context] remember`, in `CHANGELOG.md`); what is left of it, and
the tiers after it:

- [ ] **A remembered value is replaced in place, leaving no trace.** An agent
      that overwrites `user:name` with the wrong thing loses what it held, and
      nothing tells the caller it happened — `context_state.updated_at` is
      written and read by nobody. A history table nobody reads is not the fix;
      surfacing the change to whoever is talking to the agent is, and that needs
      somewhere to surface it.
- [ ] **Retention.** The conversation log is durable now, so storage growth is
      the real cost — and `InMemoryTaskStorage` never evicts its tasks,
      conversations, digests or state at all, which matters for a long-lived
      process. Contexts idle beyond N days. Separate policy and opt-in, since
      `tasks/get` with history is a protocol feature and deleting history breaks
      it. `user:`-scoped state is the awkward part: it belongs to a principal
      rather than to a context, so no context going idle says it is stale.
- [ ] **Retrieval memory is deferred, not forgotten.** The tier-3 shape (embed,
      index, search — ADK `MemoryService`, LangGraph `BaseStore`, Letta
      archival) needs a vector index and is its own pass. Define the config key
      before then so enabling it later is not a breaking config change.

## 2. Platform — runtime and delegation

- [ ] **Nothing exercises an agent booting from its *own* image.**
      `examples/image-agent/` now walks the path (Dockerfile → `[runtime] image`
      → `a2a deploy`), and the argv is unit-tested, but the contract that
      someone else's `ENTRYPOINT` starts and serves is checked by hand only: the
      container tests skip when an image is absent, and CI builds no image at
      all. Wants a docker-gated test that builds the example image and deploys
      it.
- [ ] **Containerised peers cannot reach each other through a published port.**
      The address on the card is now a separate fact from the bind address
      (`[server] advertised_url`, 2026-08-16), so an agent no longer publishes
      `http://0.0.0.0:8080` — but what `ContainerRuntime` injects is
      `http://127.0.0.1:{port}`, which is the *host's* loopback. That is right
      for the control plane, `a2acli`, and any peer running as a local process,
      and wrong for a peer in another container, which has its own loopback.
      `a2a control-plane --advertise-host` is the escape hatch (the bridge
      gateway, `host.docker.internal`); the real answer is a shared container
      network with the agents addressed by container name, which is a change to
      how `ContainerRuntime` creates them.
- [ ] **Nothing exercises the Docker Sandboxes path.** Agents, a whole fleet, and
      `--runtime container` all work inside a sandbox unmodified on `sbx` v0.38 —
      verified by hand end to end, including reaching every agent from the host
      over a published port. None of it is tested, and the pieces it depends on
      are exactly the ones that drift: the published-port contract, and an agent
      binding an address the forward can reach. A `sbx`-gated test in the shape
      of `container_runtime_test.rs` would cover it. Setup documented in
      `a2a-agents/README.md`; note the bundled `docker sandbox` Desktop plugin
      (v0.12.0) is far behind the standalone CLI and supports none of this.
- [ ] **Scrub the child environment in `LocalProcessRuntime`.** The allowlist
      bounds what a *config* may name, not what a spawned child can read. Needs
      `Command::env_clear()` plus an explicit carry-over set, which is
      platform-fiddly (`PATH`, `SystemRoot`, temp dirs) — hence deferred, with
      the adapter documented as dev-only meanwhile. See `NOTES.md`.
- [ ] **Reasoning for non-OpenRouter providers.** `[llm] reasoning` reaches the
      wire on `openrouter` only. The drop is now reported — `SelectedLlm`
      carries a `ReasoningPlan` and `a2a doctor` warns on
      `ReasoningPlan::Unsupported` — so the mistake is caught before it is
      billed; what is left is actually sending it. Both mappings turn on a
      question about the *model*, not the provider, which is why neither is a
      small change:
      - **OpenAI** takes `reasoning_effort`, and models that do not reason
        reject the parameter outright. The default model here is `gpt-4o-mini`,
        so sending it on provider kind alone breaks the common case; it needs to
        know whether the configured model reasons, i.e. a model-name list that
        goes stale with every release. `Budget` has no field at all.
      - **Gemini** takes a thinking budget under `generationConfig.thinkingConfig`,
        but the spelling differs by model generation (2.5's `thinkingBudget`
        against 3's `thinkingLevel`, which are mutually exclusive) and the
        minimum is model-dependent — `Off` is expressible on Flash and not on
        Pro. `Effort` has no direct field.
      Whatever shape this takes, `ReasoningPlan` is where it lands: a mapping
      that covers some models and not others has to keep saying which is which.
- [ ] **Feed non-text parts to the model.** `extract_text` (`handlers/llm.rs`)
      joins text parts and drops the rest, so a file or data part reaches a
      multimodal model as silence — which is why no shipped example points a
      multimodal model at anything but text. Needs a mapping from `Part` to
      whatever the provider's content array wants, and a decision for providers
      that have no such array. Note there are two copies of this function to fix:
      `handlers::llm::extract_text` reads the incoming message and the `pub`
      `handlers::context::text_of` reads stored history, and they are the same
      function. Whichever one grows the mapping leaves the other dropping parts
      silently, on the half of the prompt it happens to own.
- [ ] **Relay a delegated agent's tokens to the orchestrator's own stream.**
      The wait itself is a subscription now, so the peer's status updates
      already arrive as they happen — but `ToolSource::invoke` returns
      `Result<String>`, so they are drained and discarded, and a caller watching
      the orchestrator sees nothing until the whole delegation is done. Needs a
      way for a source to emit progress mid-invoke (the handler holds the
      streaming port; the source does not).
- [ ] **Refresh a delegation tool's description when its peer arrives.** The
      *endpoint* is resolved per call now, and the registry's card is kept
      current by `CardRefresher` (2026-08-16), but the description shown to the
      model is still fixed at startup — so a peer that registers later is
      described by the config's skill name rather than by its card.
      `ToolSource::tool_defs` is synchronous, which is what stops it asking;
      either it becomes async or a `DiscoveredPeer` caches the card it fetched
      while dialing and the source reads that.
- [ ] **A renamed agent has no way back into the registry.**
      `AgentRegistry::update_card` refuses a card whose name derives a different
      id (`RegistryError::Renamed`, 2026-08-16) rather than duplicating the
      entry, and `CardRefresher` reports it every pass. What it does not have is
      a resolution: re-registering files the agent under the new id and orphans
      the old entry, which every config referring to it by `agent_id` still
      points at. Wants a rename operation that moves the entry and says what
      broke, or a decision that ids are immutable and the name on the card is
      just a label.
- [ ] **Persistent `AgentRegistry` adapter**, for what recovery-by-derivation
      cannot cover: agents registered by something other than this runtime, and
      discovery shared across control-plane processes. Both speculative today —
      hence a port and not a database (see `NOTES.md`).
- [ ] **The duplicate-tool-name check only runs on the `a2a run` LLM path.**
      `report_tool_collisions` lives in `bin/a2a.rs`, so a Rust agent that
      assembles its own `ToolSource`s and drives `LlmHandler::builder()` —
      `complex_agent`, and any embedder — gets no warning at all, on exactly the
      path where the sources were written by hand rather than derived from a
      config. Same shape as the composition edge that `build_wired` closed, but
      it does not fit there: `AgentPorts` holds storage, streaming and push, and
      tool sources are assembled before any of them exist. The check belongs
      wherever the sources are assembled, and there is more than one such place.
      The handler is the tempting home, since it holds `tools` and knows whether
      the bag is on, but it assembles the list per turn and the report must not
      be.
- [ ] **`AutoStorage` is 16 hand-written delegation arms and grows with every
      port method.** Two ports were added to it for the state bag, and each new
      method on any storage port means another
      `match self { InMemory(s) => …, Sqlx(s) => … }`. The enum exists to keep
      dispatch static across a choice made once at startup, which is the right
      call in itself — and nothing here is silent, since a port gaining a method
      stops this file compiling. The cost is that every port method is paid for
      twice, forever — and the branch that skipped `AutoStorage` entirely was the
      one that needed a port it did not implement. Either a macro over the port
      traits, or `Arc<dyn …>` per port at the composition edge, accepting the
      vtable on calls that already do I/O.
- [ ] **Resolve the axum 0.7 (`a2a-agents`) vs 0.8 (`a2a-rs`) split.** Tests use
      an `axum8` dev-dep alias as a stopgap. Gating `askama_axum` behind
      `reimbursement-agent` (§3 Phase 0) did *not* close this: `axum` stays
      non-optional because `control_plane/http.rs` is built on it, and enabling
      `reimbursement-agent` on an axum-0.8 `a2a-agents` would put `askama_axum`
      0.4's own axum 0.7 types in the same file as an 0.8 `Router`. Closing it
      means moving `bin/reimbursement_demo.rs` (plus `templates/`, `static/`) out
      into its own sample crate — which is what `CLAUDE.md` already asks of every
      other agent, and which naturally happens on the korps side.

## 3. Platform extraction → `korps`

`a2a-agents` moves to https://github.com/EmilLindfors/korps (private, created
2026-08-17), depending only on **published** `a2a-rs` / `a2a-llm` / `a2a-mcp` /
`a2a-ap2` — no path deps back, so the protocol crates stay clean and stay MIT.
The split line is protocol vs. platform. `MIGRATION.md` in that repo holds the
full plan; the Terraform provider does **not** move yet (see §4).

### Phase 0 — cross-seam cleanup, in this repo ✅ done 2026-08-17

- [x] Extract `a2a-llm` from `a2a-agents-common/src/llm/`. `a2a-mcp` depended on
      all 5.3k lines of `a2a-agents-common` for two types (`ToolCall`,
      `ToolDefinition`); it now takes a 2.8k-line crate that is MIT-side by
      design.
- [x] Move `a2a-agents-common/src/context/` into `a2a-agents/src/context/`.
- [x] Delete `nlp/`, `formatting/`, `caching/`, `testing/` (1,157 lines) and
      `CommonError`. Zero consumers anywhere in the workspace. `a2a-agents-common`
      is gone; `moka` and its `async` feature go with it.
- [x] Move `e2e_framework_lifecycle_test.rs` and `sse_streaming_test.rs` from
      `a2a-client/tests/` to `a2a-agents/tests/`, killing the
      `a2a-agents → a2a-web-client → a2a-agents` dev cycle.
- [x] Gate `a2a-client`, `askama`, `askama_axum`, and `tower-http` behind
      `reimbursement-agent` — `bin/reimbursement_demo.rs` is their only consumer.
      Drop `base64`, unused anywhere.
- [x] Add LICENSE files. Seven crates declared `license = "MIT"` with no license
      text in the repo.

### Phase 1 — the move

- [ ] `git filter-repo` `a2a-agents/` into `korps` so its history comes along
- [ ] Rename the crate and the `a2a` binary to `korps`
- [ ] Flip path deps to crates.io versions; add `[patch.crates-io]` for
      co-development
- [ ] Split the generic handler into its own crate if wanted — it is co-located
      in `a2a-agents/src/handlers/` today to avoid a circular dep with `a2a-mcp`.
- [ ] In this repo: drop `a2a-agents` from the workspace `Cargo.toml`; point
      `README.md` / `CLAUDE.md` at the new repo. Keep `a2a-rs`, `a2a-ap2`,
      `a2a-client`, `a2a-llm`, `a2a-mcp`, `a2acli` here.
- [ ] Downstream-canary CI job here that builds `korps` HEAD against each PR,
      replacing what the shared workspace used to catch
- [ ] Split `NOTES.md` and `CHANGELOG.md` along the same seam

## 4. Terraform provider ⏸ (parked)

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

## 5. Interop and CI

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
- [ ] **`authenticated_principal_test` flakes under a full workspace run.**
      `the_connectrpc_path_carries_the_caller_over_a_socket` binds a hard-coded
      `127.0.0.1:8199` and waits a flat 200ms for the server to come up. Seen
      failing once on 2026-08-17 under `cargo test --workspace --all-features`
      and passing on its own and on a re-run, which is the shape of both
      possible causes — the sleep being short under load, and another test
      binary holding the port. Bind port 0 and read the address back, or poll
      until the socket answers.
- [ ] **Pin an MSRV CI job the moment the declared version drops below stable.**
      Not needed today: every workflow uses `dtolnay/rust-toolchain@stable` and
      1.96 is current stable, so CI already builds on exactly the declared
      version. That stops being true the day stable moves on — from then the
      number is unproven again, and a `dtolnay/rust-toolchain@1.96` job is what
      makes it real.

## 6. Docs

- [ ] `terraform-provider-a2aagent/README.md` still describes the provider as the
      source of truth for agent definitions. Fix when the provider resumes — or
      sooner, with a parked-WIP banner, if it starts misleading anyone.

---

## 7. Protocol and core (`a2a-rs`) — deferred themes

Real work, unscheduled. Each reshapes a surface and warrants its own pass.

- [ ] **Multi-tenancy.** Thread a `tenant` through requests and storage. Only
      placeholder fields exist today (`TaskPushNotificationConfig.tenant`, the
      proto `/{tenant}/…` routes). It is also what would make one database
      serve several agents: nothing in the schema names the agent, so today a
      database belongs to exactly one (`FleetConflict::Storage` reports the
      mistake — see `NOTES.md`). Two viable shapes, and the choice is the work:
      - **(a) edge tenant-routing** — a `TenantRouter` holding per-tenant
        storage, resolving the tenant from the `/{tenant}/` path at the transport
        edge, keeping domain and ports tenant-free. Smallest blast radius, most
        hexagonal.
      - **(b) per-request `tenant` parameter** threaded through every port
        method, plus transport extraction and storage scoping. Matches the
        official SDK exactly; largest diff, touches every call site in every
        crate. Cheaper than it was on the message path: `RequestContext` already
        travels from the transport to the message handler, and a `tenant` field
        on it costs no new parameter. The task, notification and storage ports
        still take none.
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

## 8. Blocked on upstream

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

## 9. Nice to have

- [ ] **Single bidirectional showcase** — fold `AgentToMcpBridge` (re-expose the
      agent *as* MCP tools) into `complex_agent`. Already covered standalone by
      `a2a-mcp/examples/bidirectional_demo.rs`; only worth it for one end-to-end
      demo.
- [ ] **MCP-native progress** — wire `McpToA2ABridge::with_streaming` +
      `ProgressClientHandler` so downstream tool progress streams (the tool
      server would need to emit `notify_progress`). Progress is handler-driven
      today.
