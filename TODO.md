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
`TODO.md` §3, and the retention item below has its other half in korps' §2.

- [x] **A turn asks who owns a context once.** Done 2026-08-25: `SqlxTaskStorage`
      keeps the settled claim for a few seconds (`SqlxStorageBuilder::claim_cache`,
      5s by default), so a turn with `mode = "context"` and two `remember` calls
      asks once rather than four times. What made it safe is narrow enough to be
      worth repeating: `contexts.owner` is written by the claim and never
      reassigned, so the cached value cannot disagree with the row — only be
      absent from it after a sweep. See `NOTES.md` for the three bounds that
      close that (settled answers only, evict what this store's own sweep
      deletes, and a TTL for a sweep run by another replica).
- [x] **A remembered value says what it replaced.** The `a2a-rs` half landed on
      2026-08-25: `AsyncContextStateStore::remember` returns `Remembered`
      (`Stored` | `Unchanged` | `Replaced { previous }` | `NotStored`) instead of
      `()`, so the value overwritten in place is reported rather than lost. See
      `CHANGELOG.md`; `NOTES.md` has why it is a `SELECT` and an upsert in a
      transaction rather than one statement. Surfacing it is korps' half — see §2.
- [ ] **Retention has a sweep and no schedule.** The `a2a-rs` half landed on
      2026-08-21: `RetentionPolicy` (two knobs, both off by default) and the
      `AsyncRetention` port, implemented by both storage adapters. See
      `CHANGELOG.md`; `NOTES.md` has why idleness is measured from writes only
      and why `now` is a parameter. Nothing calls it — deliberately, since there
      is no timer in the library — so the remainder is korps': a config key for
      the two windows and a supervisor that sweeps on a schedule and logs what
      `Swept` reports. Until then a store still grows without bound, which is
      what the default policy asks for.
      - [x] **A fact that is only ever read expires.** Fixed 2026-08-25 by
        `ReadRefresh`, which lets a read refresh a principal's `user:` bag once
        the bag is already older than a window — bounding the extra writes at one
        per principal per window rather than one per turn, which is the cost the
        write-only rule exists to refuse. Off by default; `halfway_through` takes
        the window from the `RetentionPolicy` a sweep will run under. The rule
        lives on the domain so both adapters cannot mean different things by it.
- [ ] **Retrieval memory is deferred, not forgotten.** The tier-3 shape (embed,
      index, search — ADK `MemoryService`, LangGraph `BaseStore`, Letta
      archival) needs a vector index and is its own pass. Define the config key
      before then so enabling it later is not a breaking config change.
- [ ] **A `MAX_TOKENS` cut that ate Gemini's whole answer loses its reason.**
      Deliberately left out of the 0.4.0 finish-reason work (its commit says
      so): a candidate cut before any content arrived carries
      `finishReason: "MAX_TOKENS"` and no `content`, and `chat_completion`
      fails it as `ProviderError("No content in response")` *before* the
      reason is read — the one empty response whose emptiness is explained
      raises the same opaque error as a truly empty one. Read the reason
      first and carry it in the error (or return an empty response with
      `finish` set), so a caller can name the output-cap knob instead of
      reporting a provider fault.

## 2. Shared with korps

Work whose two halves land on opposite sides of the seam. Also listed in korps'
`TODO.md` §2; whoever picks one up should check the other copy.

- [x] **`ContextLengthExceeded` throws away the numbers the provider gave.**
      The upstream half landed 2026-08-31, as specified: a struct variant
      `{ detail, prompt_tokens: Option<u32>, context_window: Option<u32> }`,
      read from the raw body — llama.cpp's JSON fields first, then the two
      prose shapes the fixtures pin (OpenAI's window, Gemini's count), nothing
      speculative. What it did not predict: `classify_api_error` taking the
      raw body meant it had to take over the `{label} ({status}): {body}`
      formatting too, which deleted the same format string from all four call
      sites — the numbers must be read before the JSON is flattened, so the
      flattening moved inside. korps' half (the `CeilingWatch` report naming
      the window, `DriftWatch` sampling refusals) lands with the 0.3.0
      release; its `TODO.md` §2 tracks it.
      The original item, for the reasoning:
      It carries a `String` — the formatted error body — so the two facts a
      caller most wants are readable only by re-parsing prose it was handed as
      an opaque message. llama.cpp returns them as *fields*
      (`"n_ctx":32768,"n_prompt_tokens":40089`) and OpenAI's prose names the
      window; both are discarded. korps' half is already written against this:
      its new `CeilingWatch` reports the estimated size of the refused request,
      because that is all it can know, and tells the operator to set
      `max_input_tokens` below it — which is a bound, not an answer. Measured
      against llama.cpp at `n_ctx = 32768`, a request the estimator put at 51483
      tokens was 40089 by the server's count, so the advice leaves everything
      from 33k to 51k still failing. Wants optional `window` and `prompt_tokens`
      on the variant, parsed where the provider gives them structurally and left
      `None` otherwise. **Breaking** — it is a tuple variant today — so it wants
      a release boundary rather than a slot in the marker fix.
      korps' `TODO.md` §2 has the other half, plus a second use for the same
      fields: `DriftWatch` only ever samples *successful* requests, since it
      reconciles against `usage.prompt_tokens`, so the requests where the
      estimate being wrong actually costs something contribute nothing to it.
- [ ] **A skill with no keywords serves an undecodable card.** The `a2a-rs` half
      landed on 2026-08-21: `SimpleAgentInfo::add_skill` and
      `add_comprehensive_skill` now require `tags`, because the spec marks the
      field REQUIRED and ProtoJSON drops an empty list, which makes the official
      client refuse the whole card. korps' `core/server.rs` still passes `None`
      when a skill's `[[skills]] keywords` is empty — a config `korps validate`
      accepts and no conformant client can talk to. Two halves: take the
      signature change (it does not compile otherwise), and decide whether an
      empty `keywords` is a config error or gets a default.
- [x] **`a2a-web-client`'s `axum-components` feature did not turn off.** Done
      2026-08-25. `components/mod.rs` declared `pub mod streaming;` ungated
      while the axum it imports was behind the flag, so `default-features =
      false` failed to compile instead of dropping the module — the flag was
      advertised as optional and was not. Gated now; `axum` also moves 0.7 → 0.8
      (**breaking**), which was the reason anyone wanted the flag: this crate
      was the workspace's last 0.7, and `create_sse_stream` returning an `Sse`
      pushed that version onto every dependent. The dead `async-stream` dep went
      too. A `--no-default-features` CI job is what would have caught it and now
      does. See `CHANGELOG.md` and `NOTES.md`.
      - korps' half stands until this is released: it builds against published
        `a2a-web-client` 0.6.1, so its inlined SSE serializer and the `axum7`
        dev-dep alias stay for now. Both are deletions when the release lands —
        korps' `TODO.md` §2 tracks them, and this is one of the changes the
        release item in that repo's §1 covers.
- [ ] **Reasoning for non-OpenRouter providers — korps' half is left.** The
      `a2a-llm` half landed on 2026-08-24: `[llm] reasoning` now reaches
      OpenAI's `reasoning_effort` and Gemini's `generationConfig.thinkingConfig`,
      by the send-it-and-recover shape rather than a model-name table. See
      `CHANGELOG.md`; `NOTES.md` has why the endpoint is asked instead of a list
      consulted, and why the refusal is only remembered after the retry works.
      What is left is korps':
      - `korps doctor` reports `ReasoningPlan::unsupported()`, which now answers
        only for a token budget on OpenAI. The new `ReasoningPlan::Attempted`
        (`attempting()`) is the state a report should say "sent, and the model
        has the last word" about, and nothing says it yet.
      - Whether a refusal *happened* is only in the provider's `warn!` log.
        `Arc<dyn LlmProvider>` erases the concrete provider, so a report has no
        way to read it back after a run; giving it one means a channel that does
        not exist today.
      - `[llm] reasoning` on an `openai` provider with the default
        `gpt-4o-mini` now costs one wasted round trip on the first call of the
        process. Nothing is wrong with it, but a `doctor` line saying so would
        save the question.
- [ ] **An overwritten fact has nowhere to surface — korps' half is left.** The
      `a2a-rs` half landed on 2026-08-25 (§1): `remember` now returns
      `Remembered`, so `Replaced { previous }` carries the value that used to be
      lost. Two halves on korps' side: `AutoStorage` in `core/server.rs`'s
      builder delegates `remember` and must take the new return type (it does not
      compile otherwise), and `handlers/memory.rs` builds the `remember` tool
      result — which is the "somewhere to surface it" this item was waiting for.
      Whether the model is the right audience or the user is, is the open part:
      an agent told it overwrote something may just apologise, where a caller
      would want to know.
- [ ] **Gemini has no default model now — korps' half is left.** The `a2a-llm`
      half landed on 2026-08-25: `GEMINI_DEFAULT_MODEL` was `gemini-1.5-pro`,
      absent from Google's current models page and from the deprecation
      schedule both, so nothing had been announced either way and every config
      naming `provider = "gemini"` without a `model` ran on it regardless.
      Resolved by **removing the default** rather than moving it — a default
      model is a one-entry table of a vendor's product line and goes stale
      invisibly, where a missing one is an error at startup. `model` is now
      required for Gemini, from the config or `GEMINI_MODEL`. See
      `CHANGELOG.md` (**breaking**); `NOTES.md` has why this is the same
      conclusion the `reasoning` work reached from the other end.
      - korps' half is a config decision this cannot make for it: a `[llm]`
        block with `provider = "gemini"` and no `model` is a startup error now,
        so `korps validate` should catch it before a run does — the same shape
        as the empty-`keywords` question above. Whether korps supplies its own
        default instead is its call; nothing here stops it.
      - Like the `axum` item, korps only feels this when the release lands; it
        builds against published `a2a-llm` today.

## 3. Interop and CI

- [x] **The two storage backends returned different tasks for the same run.**
      Fixed 2026-08-26. A completed task from `InMemoryTaskStorage` carried the
      agent's reply in `status.message`; the same task from `SqlxTaskStorage`
      had none. The cause was one missing column in three statements:
      `update_status`, `update_status_checked` and `cancel` all wrote
      `status_state` and left `status_message` holding whatever `create` put
      there — normally `NULL`. The column, the read in `row_to_task`, and the
      `TASK_COLUMNS` list were all correct and had been all along; nothing ever
      wrote to it after insert.
      `Task::update_status` in the domain is the reference and it replaces the
      whole `TaskStatus`, so a transition carrying no message clears the last
      one rather than leaving it attributed to a state it was never about. The
      storage side does that now: `status_message_json(None)` writes `NULL`
      deliberately rather than skipping the column.
      **The test is the point of the fix.** New `tests/storage_parity_test.rs`
      holds the assertions once and takes the backend as a parameter, because
      each adapter having its own file asserting what *it* does is exactly what
      let these two drift while both suites stayed green. Verified against the
      unfixed code first: `in_memory` passes, `sqlx` fails on the completed-task
      assertion — so it catches the real difference rather than restating the
      new behaviour. A backend added later gets one `parity_suite!` line and
      inherits the lot.
      Also deduplicated on the way through: the `TaskState` → column-string
      match had five copies against a sixth that reads it back, and anything the
      writer spells differently from the reader returns as `Unknown`. One
      `state_str` now.
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
- [x] **A refused ConnectRPC subscription arrived as an empty stream** (#71).
      Fixed 2026-09-02: the refusal is in the END_STREAM envelope, which the
      client library parks in `ServerStream::error()` behind an `Ok(None)`, and
      the client's `unfold` never read it. It yields the refusal once now.
      `tests/connectrpc_error_test.rs` pins both pre-stream refusals over a
      socket; see `NOTES.md`.
- [x] **ConnectRPC lost the A2A error code** (#72). Fixed 2026-09-02: the
      server attaches the JSON-RPC error object as a Connect error detail and
      the client reads it back through the JSON-RPC table, so both bindings
      share one exhaustive map (`connect_wire`, `jsonrpc_wire`). The Connect
      code stays as a category for foreign clients. See `CHANGELOG.md` for the
      JSON-RPC client's typed-variant change that came with it, and `NOTES.md`
      for why the code does not go through the Connect code at all.
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
- [x] **Durable streaming resumption.** Done 2026-08-23: ids and retained events
      moved out of `InMemoryStreamingHandler` into the `AsyncEventLog` port, and
      `SqlxTaskStorage` implements it (migration 007, `task_events`), so a
      restart no longer starts every task's ids at 1. The fan-out is
      `StreamingFanout<L>`; `InMemoryStreamingHandler` is the in-memory pairing
      and is unchanged for callers. See `CHANGELOG.md`; `NOTES.md` has why the
      log is a port rather than a second streaming adapter, why the id is
      assigned inside the insert, and why a replay that cannot cover the gap is
      dropped instead of sent.
      - Nothing schedules `AsyncEventLog::discard`, the same gap the retention
        item in §1 describes: a sweep of the context takes its events, so this
        rides on korps growing a timer. Until then the per-task cap
        (`event_log_capacity`, 1024 by default) is what bounds the table.
- [ ] **`a2acli send` cannot continue a conversation.** No `--context` flag,
      so continuing from the CLI means reusing a task id — a different thing
      on the wire, and a settled task is not a conversation handle. Found
      driving the 2026-09-01 korps wake-up session by hand. Small: accept a
      context id on `send` the way a task id already is.
- [ ] **What does `auto_connect` dial when the card disagrees with the
      endpoint it was given?** Observed from korps' fleet wake e2e
      falsification (2026-09-01): `auto_connect` against a *reachable*
      endpoint whose card advertises an unreachable `url` fails the
      delegation — so a mis-set `advertised_url` takes down even callers
      holding the agent's real address. First check exactly which URL the
      negotiated transport connects to; then decide whether the endpoint the
      caller handed in should win (or be the fallback) over the card's, and
      make the connection error name both URLs either way.

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
