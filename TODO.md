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

What is left on this side of the seam. The handler-side remainder is in korps'
`TODO.md` §2. The retention item below waits on a timer korps does not track
yet.

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
- [x] **A `MAX_TOKENS` cut that ate Gemini's whole answer loses its reason.**
      Fixed 2026-09-03: a candidate with a `finishReason` and no `content` is
      an empty `LlmResponse` with `finish` set — the shape the OpenAI path
      already returned for a choice with no content — and only a candidate
      with neither is still `ProviderError("No content in response")`. See
      `CHANGELOG.md`; `NOTES.md` has why the order of reading matters.
      `a2a-llm/tests/wire_test.rs` pins both over a socket.

## 2. Shared with korps

Work whose two halves land on opposite sides of the seam. korps' copies are in
its `TODO.md` §3, §5 and §7; whoever picks one up should check the other copy.
Every half korps owed from before 2026-09-05 shipped there (`CeilingWatch`, a
skill with no `keywords`, the `axum` and `reqwest` deletions, `Remembered`, a
Gemini agent without a model, the status route reading `reasoning_refused()`);
see its `CHANGELOG.md`.

The two below are the protocol half of a fleet building a strata project
end to end (korps' `TODO.md` §7, strata's `TODO.md` § *Agents build a
project end to end*), set 2026-09-05.

- [ ] **Nothing in the fleet asks the bridge for a task yet.**
      Both halves of the tasks extension are in: the client half shipped
      2026-09-07 (`McpToA2ABridge` watches a task a server makes of a tool
      call) and the server half the same day (`AgentToMcpBridge` answers
      `tools/call` with a task id past a grace period, drives it detached,
      and takes the answer through `tasks/update`). See `CHANGELOG.md`;
      `NOTES.md` has why the switch is a grace period and why only a
      task-mode call is spawned. What is left is korps': its `mcp-client`
      does not declare `ClientCapabilities::enable_tasks()`, so every call
      it makes still blocks and neither half is exercised by a fleet. Its
      §3 has that half.
- [ ] **A model can be sent bytes, and korps still withholds them.**
      The `a2a-llm` half landed on 2026-09-07: `ChatMessage::content` is
      `MessageContent`, a string or a list of `ContentPart`s, rendered per
      provider. See `CHANGELOG.md`; `NOTES.md` has why it is one content
      channel rather than a second field, and which URIs OpenAI will not
      fetch. korps' half is left: `part_text` in `handlers/context.rs` still
      turns a `FilePart` into `[attachment: … — not sent to the model]`, and
      that maps to `ContentPart::blob(media_type, bytes)` and
      `ContentPart::uri` now. Its §3 has that half; its §7 *Handoffs carry
      files* rides on it.
      - The upgrade breaks 52 call sites in korps, all of them reading
        `content` as a string. `ChatMessage::text()` is the replacement for
        `content.as_deref()`.
      - korps' `TokenEstimate` counts a message's text and nothing else, so a
        two-megabyte image estimates as zero tokens and the ceiling that
        guards a context window stops guarding it. A blob's token cost is the
        provider's business (Gemini bills an image by tiles), so the estimate
        needs a per-media rule rather than a byte count.

---

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
- [x] **`a2acli send` cannot continue a conversation.** Done 2026-09-03. The
      flag existed under the wrong name: `--session-id` was already the
      message's `context_id` on the wire. It is `--context-id` now, matching
      `list`, and a finished task's next-step hint points at its context.
      `cli_e2e_test.rs` sends two tasks into one context and reads the hint.
      The port's `session_id` parameter keeps its name for now — renaming it
      touches every transport and both clients, and is not what was missing.
- [x] **What does `auto_connect` dial when the card disagrees with the
      endpoint it was given?** Answered 2026-09-03: the card's interface URL,
      always, when the card can be fetched and negotiated; `base_url` is only
      where the card comes from, and the direct-client fallback fires only
      when it cannot be. Decided that the caller's address neither wins nor
      is the fallback — `NOTES.md` has the two legitimate setups that rules
      out — and `connect_with` now logs both URLs when their origins differ,
      at the one moment both are known. The fix for the case korps hit is
      the server's `advertised_url`, and the warning names it.

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
      **Checked upstream on 2026-09-03: `connectrpc 0.9.0` is half the fix.**
      Its `client-tls` now takes `hyper-rustls` with `default-features =
      false` (so hyper-rustls no longer drags aws-lc-rs in), but its `rustls`
      dependency still has default features, and rustls' default is
      `aws_lc_rs` — so the provider is still on. Getting there is not a bump:
      0.9 wants `buffa` 0.9 (we generate against 0.3, and
      `domain/generated.rs` is checked in, not built), the generated code's
      `__buffa` shim is gone, and `connectrpc::Context` — which the server
      adapter uses at 23 sites — no longer exists. A trial bump produced 143
      errors before the generated domain was touched. That is a regeneration
      of the domain plus a rewrite of the server adapter: its own pass, and
      the release after this one at the earliest.
      A feature-only "ring-only" fix is **blocked by `connectrpc 0.3.3`**: it
      exposes no TLS feature flags and depends on `hyper-rustls`/`tokio-rustls`
      with their default `aws-lc-rs` provider, so no combination of our flags
      removes `aws-lc-sys`. (`sqlx` offers `tls-rustls-ring` and `reqwest`
      offers `rustls-tls-*-no-provider`, but fixing only those leaves connectrpc
      still pulling `aws-lc-rs`.) A `[patch.crates-io]` swaps the *source*, not
      features, so it cannot flip connectrpc's `hyper-rustls` default either.
      Paths, none cheap:
      - **(a)** upstream a `ring` feature into `connectrpc`, then set ring on
        `connectrpc` + `reqwest` `rustls-no-provider` (0.13's spelling; its
        `rustls` feature brings aws-lc-rs itself, so this is now the one
        place the provider is chosen for reqwest) + `sqlx` `tls-rustls-ring`;
      - **(b)** fork or vendor `connectrpc` with
        `hyper-rustls = { default-features = false, features = ["ring", …] }`;
      - **(c)** keep `aws-lc-rs` and make it cross-build — a `Cross.toml` whose
        image carries clang and cmake (plus `AWS_LC_SYS_PREBUILT_NASM=1` on x86)
        — sidestepping the provider question. Needs a reproducible `cross`
        environment to validate.
