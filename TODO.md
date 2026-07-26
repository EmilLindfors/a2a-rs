# TODO — `a2acli` follow-ups

Tracks the next steps after landing the `a2acli` crate + the `auto_connect`
promotion. Companion to `ROADMAP.md` (this is the near-term, actionable slice).
The platform/CLI roadmap lives in `DECLARATIVE_AGENTS_TODO.md`.

## 1. Ship the current branch ✅

The `feat/a2acli` branch merged and shipped (`a2acli` 0.4.0, released in #29).

- [x] Commit the staged work (new `a2acli` crate, `a2a_rs::auto_connect`,
      `WebA2AClient` delegation, unused-`reqwest` drop from `a2a-client`).
- [x] Open the PR, calling out the `--auth`/`--timeout` caveat in `auto` mode
      (item 3 below) and the agent-card transport-mislabel finding (item 2).
- [x] Add `a2acli` to the workspace table in `CLAUDE.md` — done, with a note
      distinguishing it from the `a2a` binary in `a2a-agents`, since "the CLI" is
      now ambiguous between a client (`a2acli`) and a runner (`a2a`).

## 2. Bugs found while testing the CLI (not CLI bugs)

- [x] **Agent card mislabels its transport.** `a2a-agents`' `AgentServer` mounts a
      **ConnectRPC** server (`ConnectRpcAdapter` + `HttpServer`) but the published
      card advertised the interface as **`JSONRPC`** (the `SimpleAgentInfo`
      default `protocol_binding`), so client auto-negotiation picked the JSON-RPC
      client and failed (`invalid JSON-RPC response: error decoding response
      body`). Not specific to `a2a-agents` — `HttpServer` mounts *only* the
      ConnectRPC router, so every one of its users published a card that lied.
      Fixed at the root: `HttpServer` stamps `CONNECTRPC` on the card it serves,
      `agent_info_from_config` sets it at the source for the off-HTTP readers
      (registry self-registration, MCP mode), and the binding strings are now
      shared `PROTOCOL_BINDING_*` consts in `domain` so card and client cannot
      drift. Regression: `a2a-rs/tests/agent_card_transport_test.rs`.
- [ ] **ConnectRPC SSE subscription never closes on terminal state.** `a2acli stream`
      (and any subscriber) stays open after the task reaches `FAILED`/`COMPLETED`;
      had to cap each run with `timeout`. The stream should end when the task hits a
      terminal state. (Distinct from the `Last-Event-ID` gap in `ROADMAP.md`.)

## 2b. Decide what MSRV we actually claim

- [ ] **`rust-version = "1.85"` may no longer be true.** `a2a-rs` and
      `a2a-agents` both declare it, but the current dependency tree needs
      **1.87+** — `icu_*`/`idna_adapter` want 1.86, `process-wrap` wants 1.87 —
      which is how the agent Dockerfile's pinned `rust:1.85` builder came to fail
      outright (fixed there by bumping to 1.96). Nothing catches this locally
      because a developer's toolchain is newer; cargo only errors when the
      *toolchain* is below what a dep requires, never when `rust-version` is.
      Three options, and it is a publishing decision rather than a code one:
      raise the declared MSRV to something true, pin the offending deps back to
      versions that build on 1.85, or drop the claim. Whichever it is, an MSRV
      that is asserted and untested is worth less than no claim at all — if we
      keep one, it wants a CI job on that exact toolchain.

## 3. CLI follow-ups

- [ ] **Thread `--auth`/`--timeout` through `auto` mode.** Today the negotiation
      factories (`TransportFactory` in `a2a-rs/src/adapter/transport/negotiation.rs`)
      build unauthenticated, default-timeout clients, so credentials only apply with
      an explicit `--transport`. Options: add a `ClientConfig` (token + timeout) to
      `TransportFactory::create`, or a `connect_with`/`auto_connect_with` variant.
- [ ] **Add an `a2acli` integration test.** Spin up `examples/jsonrpc_server` and
      drive the built binary through `card`/`send`/`get`/`cancel` (mirrors the manual
      e2e). Complements `a2a-rs/tests/jsonrpc_client_interop_test.rs`.
- [ ] **(Optional) `list` command** — the `Transport` port already has `list_tasks`;
      expose it (`a2acli list [--state …] [--limit …]`). Push-notification-config
      commands (`set`/`get`/`list`/`delete`) are also available on the port but are
      out of the roadmap's `card/send/stream/get/cancel` scope.

## 4. Cross-SDK interop validation (ROADMAP §0.5)

- [ ] Point the **official** `a2aproject/a2acli` at our
      `examples/jsonrpc_server` (`:8137`) — validates our *server* against the
      canonical client.
- [ ] Point **our** `JsonRpcClient`/`a2acli` at a stock upstream A2A agent —
      validates our *client* against other SDKs.
- [ ] Once both pass, capture the matrix (which transports/SDKs interoperate) in the
      `a2acli` README or `ROADMAP.md`.

## 5. Example/test ergonomics (minor)

- [x] `complex_agent`'s rule-based path is now reachable. Two separate causes:
      the original one (`OpenAiProvider::from_env()` always returns `Ok`, so
      `load_llm()` never returned `None`) went away when provider selection was
      centralized — `provider_from_env()` gates each branch on a *set* key and
      returns `None` otherwise. What remained is that the example loads a `.env`
      at startup, so on a machine set up for the LLM path the deterministic
      branch was still unreachable without unsetting things. `A2A_NO_LLM=1` is
      the explicit opt-out (`load_llm`).
