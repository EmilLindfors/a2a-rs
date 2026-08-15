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

- [ ] **Only `jwt` gives a principal id that outlives the credential.**
      `JwtAuthenticator` uses the token's `sub`, so a refresh keeps the same
      identity. `BearerTokenAuthenticator` and `ApiKeyAuthenticator` use the
      credential itself, and `OAuth2Authenticator` uses `oauth2:{access_token}` —
      which rotates on every refresh, so a conversation owned under OAuth2 stops
      being readable by the same user. It is documented (`a2a-agents/README.md`)
      and it is still wrong: OAuth2 should carry the subject from introspection
      or userinfo, not the bearer string. Bearer and API key have nothing else to
      use and are honestly credential-scoped.
- [ ] **`a2a doctor` says nothing about context configuration.** An agent with
      `mode = "context"` and `[server.storage] type = "memory"` loses every
      conversation on restart, and the control plane restarts agents on purpose.
      That is exactly the class of mistake `doctor` exists to catch before it is
      discovered in production, and it is checkable from the config alone. Same
      for `max_input_tokens` far above what the configured model actually has,
      once there is anywhere to look that up.
- [ ] **A summary has no length bound.** `compact_conversation` sends its request
      with no `max_tokens`, so a verbose model can return a "summary" about as
      long as the transcript it replaces — which costs the tokens compaction
      exists to save and does so on every later turn. Wants a cap derived from
      the budget, and a check that what came back is meaningfully shorter than
      what it stands in for.
- [ ] **A failed history load makes an agent quietly amnesiac.**
      `LlmHandler::load_conversation` logs and returns an empty conversation for
      anything that is not `ContextAccessDenied`, on the grounds that answering
      without context beats not answering. For a database that is down that is
      probably right; it also means a persistent storage fault shows up as an
      agent that has forgotten the conversation rather than as an error. Revisit
      once there is somewhere to surface degraded-but-serving.
- [ ] **Compaction runs at most once per turn.** The `compacted` flag stops a
      second pass, so a conversation still over budget after one summary
      proceeds and may be refused by the provider — recoverable, since
      `ContextLengthExceeded` retries smaller, but it burns a round trip. The
      alternative is summarizing a summary mid-turn, which is worse; what is
      actually wanted is noticing the case and saying so.
- [ ] **Nothing reconciles the token estimator against reported usage.**
      `CharEstimate` divides characters by 3.5 and the handler logs the estimate
      beside what the provider charged, but nothing closes the loop: a deployment
      that measures its own ratio sets it by hand, and there is no signal when
      the configured model drifts far enough from 3.5 to matter.
      `CharEstimate::with_chars_per_token` is the seam, and `TokenUsage` is
      already on the stream.
- [ ] **`mode = "task"` re-reads the whole task every turn** through
      `AsyncTaskLifecycle::get`, which returns artifacts and status to use only
      the history. Fine at current sizes.
- [ ] **`stream_options.include_usage` is decided by string-comparing the base
      URL** against `OPENAI_BASE_URL`, so a config naming the same endpoint with
      a trailing slash silently loses streaming usage. It is a log line either
      way, not a failure — but the rule should be a config field once anyone
      needs it on a server we do not know about.
- [ ] **Migrations 003–005 have no PostgreSQL siblings** where 001 and 002 do.
      Nothing runs them — `SqlxTaskStorage` is `SqlitePool`-only — so the
      existing `_postgres.sql` files imply support that does not exist. Either
      finish the Postgres adapter or delete the files; leaving three of five is
      the worst of both.

**Next tiers of memory** (the design is settled — see `NOTES.md` — only the work
is open):

- [ ] **State bag.** `contexts.state` already exists as a column and nothing
      reads or writes it. Fill it: JSON injected into the system prompt, written
      by a built-in `remember(key, value)` tool. Letta's core memory blocks and
      Anthropic's memory tool in miniature, at the cost of one column and no
      embeddings. Steal ADK's key prefixes so scope is visible in the key
      (`user:*` outlives the context, bare keys are per-context, `temp:*` never
      persists).
- [ ] **Retention.** The conversation log is durable now, so storage growth is
      the real cost — and `InMemoryTaskStorage` never evicts its tasks,
      conversations or digests at all, which matters for a long-lived process.
      Contexts idle beyond N days. Separate policy and opt-in, since `tasks/get`
      with history is a protocol feature and deleting history breaks it.
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
- [ ] **An agent advertises the address it *bound*, not one a peer can dial.**
      `AgentConfig::agent_url` is `http://{server.host}:{port}`, and that string
      becomes the card's `supportedInterfaces[].url`. Binding all interfaces —
      which `ContainerRuntime` requires, and the base image sets with
      `HOST=0.0.0.0` — therefore publishes `http://0.0.0.0:8080` as the agent's
      address, and a peer resolving it by skill dials `0.0.0.0`. Confirmed
      against a running containerised agent. The bind address and the advertised
      address are two different facts and need two fields; the second should
      default to the first only when the first is dialable. Note the scaffolded
      templates set `host = "127.0.0.1"` explicitly, which produces a card that
      is *correct* and an agent that is unreachable once containerised — the two
      failures point opposite ways, which is why guessing from the bind address
      cannot work.
- [ ] **Wrapping a `reqwest::Error` drops the cause.** `Network error: error
      sending request for url (…)` is the whole message for a DNS failure, a
      refused connection and an untrusted certificate alike — `reqwest::Error`'s
      `Display` omits its source chain. Cost a full investigation to tell a
      proxy CA problem from the network being down (see `NOTES.md`). Every site
      that wraps one should walk `Error::source()` into the message.
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
      that have no such array.
- [ ] **`LlmHandler::new` takes seven positional arguments**, five of them
      collaborators the call site has to keep in the right order. The types
      differ, so nothing is silently swappable today — the cost is that a reader
      cannot tell what `2` or the third `Arc` is without the signature, and every
      new knob has to argue against making it eight. That pressure is what sent
      `[llm] reasoning` to the provider instead, which was the better home
      anyway; the next one may not be so lucky. `bon` is already a dependency;
      a `#[builder]` here costs three call sites.
- [ ] **Relay a delegated agent's tokens to the orchestrator's own stream.**
      The wait itself is a subscription now, so the peer's status updates
      already arrive as they happen — but `ToolSource::invoke` returns
      `Result<String>`, so they are drained and discarded, and a caller watching
      the orchestrator sees nothing until the whole delegation is done. Needs a
      way for a source to emit progress mid-invoke (the handler holds the
      streaming port; the source does not).
- [ ] **Refresh a delegation tool's description when its peer arrives.** The
      *endpoint* is resolved per call now, but the description shown to the
      model is fixed at startup, so a peer that registers later is described by
      the config's skill name rather than by its card. `ToolSource::tool_defs`
      is synchronous, which is what stops it asking; either it becomes async or
      a `DiscoveredPeer` caches the card it fetched while dialing and the source
      reads that.
- [ ] **Let the refresh loop adopt the card it fetched.** `CardRefresher` probes
      liveness and throws the card away, so a skill added to a running agent is
      invisible until something re-registers it. Blocked on the rename hazard:
      `register` derives the id from `card.name`, so adopting a renamed card
      creates a second entry and orphans the first. Needs an update-in-place on
      `AgentRegistry` that keeps the id and replaces the card — at which point
      "the name changed" becomes a case to decide rather than a silent
      duplicate.
- [ ] **Persistent `AgentRegistry` adapter**, for what recovery-by-derivation
      cannot cover: agents registered by something other than this runtime, and
      discovery shared across control-plane processes. Both speculative today —
      hence a port and not a database (see `NOTES.md`).
- [ ] **Resolve the axum 0.7 (frontend) vs 0.8 (`a2a-rs`) split.** Tests use an
      `axum8` dev-dep alias as a stopgap; bump the frontend when `askama_axum`
      allows.

## 3. Platform extraction — before the provider work

Move `a2a-agents`, `a2a-agents-common`, and the Terraform provider into
`a2a-agents-platform`, depending only on **published** `a2a-rs` / `a2a-mcp` /
`a2a-ap2` (no path deps back), keeping the protocol crates clean.

Extract *before* the provider rework (§4), not after: a Terraform provider with
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
      proto `/{tenant}/…` routes). Two viable shapes, and the choice is the work:
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
