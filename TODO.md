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
- [ ] **Retention has a sweep and no schedule.** The `a2a-rs` half landed on
      2026-08-21: `RetentionPolicy` (two knobs, both off by default) and the
      `AsyncRetention` port, implemented by both storage adapters. See
      `CHANGELOG.md`; `NOTES.md` has why idleness is measured from writes only
      and why `now` is a parameter. Nothing calls it — deliberately, since there
      is no timer in the library — so the remainder is korps': a config key for
      the two windows and a supervisor that sweeps on a schedule and logs what
      `Swept` reports. Until then a store still grows without bound, which is
      what the default policy asks for.
      - **A fact that is only ever read expires.** Idleness is measured from
        writes, because a read that recorded itself would be a write per turn —
        which is the cost the ownership item above is trying to remove. So a
        `user:` key the model reads on every turn and never rewrites is deleted
        after the window, and the agent forgets something it was using. Refreshing
        only when a key is close to its cutoff would bound the extra writes; so
        would recording reads per principal rather than per key. Neither is free,
        and nothing is scheduled to sweep yet, so this bites nobody today.
- [ ] **Retrieval memory is deferred, not forgotten.** The tier-3 shape (embed,
      index, search — ADK `MemoryService`, LangGraph `BaseStore`, Letta
      archival) needs a vector index and is its own pass. Define the config key
      before then so enabling it later is not a breaking config change.

## 2. Shared with korps

Work whose two halves land on opposite sides of the seam. Also listed in korps'
`TODO.md`; whoever picks one up should check the other copy.

- [ ] **A skill with no keywords serves an undecodable card.** The `a2a-rs` half
      landed on 2026-08-21: `SimpleAgentInfo::add_skill` and
      `add_comprehensive_skill` now require `tags`, because the spec marks the
      field REQUIRED and ProtoJSON drops an empty list, which makes the official
      client refuse the whole card. korps' `core/server.rs` still passes `None`
      when a skill's `[[skills]] keywords` is empty — a config `korps validate`
      accepts and no conformant client can talk to. Two halves: take the
      signature change (it does not compile otherwise), and decide whether an
      empty `keywords` is a config error or gets a default.
- [ ] **Reasoning for non-OpenRouter providers.** The provider mappings are
      `a2a-llm`'s; the config key and the `doctor` warning are korps'.
      `[llm] reasoning` reaches the wire on `openrouter` only. The drop is
      reported — `SelectedLlm` carries a `ReasoningPlan` and `korps doctor`
      warns on `ReasoningPlan::Unsupported` — so the mistake is caught before it
      is billed. What is left is actually sending it.

      Both mappings turn on a question about the *model*, not the provider,
      which is what makes them more than a field each. Provider facts below were
      checked on 2026-08-23; an earlier version of this entry had gone stale, so
      re-check before building against it.
      - **OpenAI** takes `reasoning_effort` on Chat Completions. Values have
        grown to `none`, `minimal`, `low`, `medium`, `high`, `xhigh` and `max`,
        and each model accepts a different subset (`none` starts at gpt-5.1;
        gpt-5-pro only accepts `high`). A model that does not reason rejects the
        parameter with a 400 `Unsupported parameter`, and our default model is
        still `gpt-4o-mini`, so sending it on provider kind alone breaks the
        common case. `Reasoning::Budget` has no field on this API at all.
      - **Gemini** now documents `generationConfig.thinkingLevel` (`minimal` /
        `low` / `medium` / `high`) as the control, and Google's table lists
        2.5-generation models under it as well as 3.x — which is new, and
        contradicts third-party reports that 2.5 rejects `thinkingLevel` and
        takes only `thinkingBudget`. `thinkingBudget` still exists and is still
        accepted on 3.x for compatibility; the two are mutually exclusive and
        sending both is an error. Supported levels differ per model
        (`gemini-3-pro-preview` takes only `low` and `high`), and turning
        thinking off is not offered on most of them. Verify against a live
        endpoint before choosing a mapping; the docs and the field reports do
        not agree.

      Two shapes are worth weighing, and neither is obviously right:
      - **A model-name table.** Explicit and reportable, and it goes stale with
        every model release — which is exactly what happened to this entry.
      - **Send it and recover.** OpenAI names the rejection precisely enough to
        catch the 400 and retry once without the field. Costs a round trip on
        the first call against an unknown model, needs no list, and cannot be
        wrong about a model nobody has told us about. Whether Gemini's rejection
        is as identifiable is unchecked.

      Whichever it is, `ReasoningPlan` is where it lands: a mapping that covers
      some models and not others has to keep saying which is which. A recovery
      shape means the plan is only known after the first call, so it would need
      a state `korps doctor` can report as "not known until it runs".
- [ ] **The Gemini default model is one Google no longer lists.**
      `GEMINI_DEFAULT_MODEL` is `gemini-1.5-pro`, which is absent from the
      current models page (checked 2026-08-23) and absent from the deprecation
      schedule too, so nothing has been announced about it either way. Every
      korps config that names `provider = "gemini"` without a `model` gets it.
      The fix is `a2a-llm`'s alone, but it changes which model an existing
      config runs on, so it is worth deciding together with korps whether the
      default moves or the model becomes required. Found while re-checking the
      entry above; separable from it.

## 3. Interop and CI

- [x] Point the **official** `a2aproject/a2acli` at our `examples/jsonrpc_server`
      (`:8137`) — done 2026-08-21 against `a2acli` 0.1.5, and it found three
      bugs on our side (missing `tags`, the `:verb` task paths, a stream that
      never ended). `card`, `send` with and without a client-supplied task id,
      `get-task`, `list-tasks`, `subscribe` and `stream` now all pass over both
      the `jsonrpc` and `http-json` bindings. See `CHANGELOG.md`.
      - One upstream bug to report: `a2acli stream` against a server with no
        streaming backend prints nothing and exits 0, swallowing the JSON-RPC
        error (`-32004`, HTTP 200) that says why.
- [x] Point **our** `JsonRpcClient` / `a2acli` at a stock upstream A2A agent —
      done 2026-08-21 against upstream's `helloworld-server`. `card`, `send`,
      `get`, `list` and `stream` pass, both negotiated from the card (its
      JSON-RPC interface is a sub-path, `:3000/jsonrpc`) and with
      `--transport jsonrpc` forced. It found one bug on our side: the client
      turned a JSON-RPC error on the *streaming* path into an empty stream.
- [x] Capture the matrix (which transports and SDKs interoperate) in the
      `a2acli` README — done, with the upstream commit it was run against.
      gRPC is the gap: upstream serves it on `:50051`, we do not speak it.
- [x] **`SubscribeToTask` on a terminal task is an error.** Done 2026-08-21:
      `a2a.proto:75` specifies `UnsupportedOperationError` and we answered with
      an empty stream. Resumption (`Last-Event-ID`) still opens. See
      `NOTES.md`. Upstream errors here too but with `-32001 TASK_NOT_FOUND`
      rather than the spec's code, so crossing this case still shows a
      difference — theirs, now.
- [x] **Pin an MSRV CI job.** Done 2026-08-21, on the condition this item was
      waiting for: stable moved to 1.98, so `dtolnay/rust-toolchain@stable` no
      longer builds the declared 1.96 and the number went unproven. `rust.yml`
      now has a job pinned to 1.96 running `cargo check --workspace
      --all-features --locked`. The workspace builds on it as declared —
      nothing had to move. See `NOTES.md`.
- [x] **ConnectRPC streams resume too.** Done 2026-08-23: the transport ignored
      `last_event_id` and tagged every event `None`, so `RetryingTransport` over
      it reconnected from current state and dropped the gap. `Last-Event-ID`
      goes in as an ordinary request header; the id comes back in the update's
      `metadata`, since ConnectRPC has no SSE `id:` field — and only for a
      client that asked with `a2a-rs-event-ids`, because that is a change to the
      payload rather than an inert protocol field. See `NOTES.md`.
- [ ] **Make the downstream canary blocking.**
      `.github/workflows/downstream-korps.yml` builds korps against each PR by
      checking both repos out as siblings, so korps' `[patch.crates-io]`
      resolves to the PR's source and it needs no release. It is
      `continue-on-error` until two things hold: a `KORPS_CANARY_TOKEN` secret
      with `repo` scope exists (korps is private, this repo is public, and
      without the secret the job skips), and korps' master is current (while it
      lags, the canary builds an old korps and can fail for reasons unrelated
      to the PR).

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
