# TODO

Open work in this repo, roughly in the order it is worth doing.

Companion docs: `CHANGELOG.md` records what shipped, `NOTES.md` the decisions
behind it and the hazards worth not rediscovering.

Sections 1-3 are actionable now. Sections 4-5 are deferred themes: real work, but
not scheduled — each needs its own pass rather than a slot in the current one.

The platform's open work moved to
[korps](https://github.com/EmilLindfors/korps) on 2026-08-18, along with the code
— the CLI, fleets, the control plane, runtimes, the registry, delegation, and the
Terraform provider are all tracked in that repo's `TODO.md` now. Items are routed
by where the fix lands: a context-memory bug fixed in a storage adapter is here,
one fixed in `LlmHandler` is there.

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
per-context scratchpad, and `MemoryService` is the deferred retrieval tier.

What is left on this side of the seam — the handler-side remainder is in korps'
`TODO.md`.

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

## 2. Shared with korps

Work whose two halves land on opposite sides of the seam. Also listed in korps'
`TODO.md`; whoever picks one up should check the other copy.

- [ ] **Reasoning for non-OpenRouter providers.** The provider mappings are
      `a2a-llm`'s; the config key and the `doctor` warning are korps'.
      `[llm] reasoning` reaches the wire on `openrouter` only. The drop is now
      reported — `SelectedLlm` carries a `ReasoningPlan` and `korps doctor` warns on
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

## 3. Interop and CI

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
- [ ] **Downstream canary for korps.** Nothing here depends on korps, so the
      compiler never sees a change that breaks it — the shared workspace used to
      catch that and no longer can. A job that checks out korps' HEAD and builds
      it against the PR is the replacement. Blocked until korps can build against
      a published `a2a-rs`; today it needs the unreleased context-state API.

---

## 4. Protocol and core (`a2a-rs`) — deferred themes

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

## 5. Blocked on upstream

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
