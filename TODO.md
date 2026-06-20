# TODO — `a2acli` follow-ups

Working branch: `feat/a2acli`. Tracks the next steps after landing the `a2acli`
crate + the `auto_connect` promotion. Companion to `ROADMAP.md` (this is the
near-term, actionable slice).

## 1. Ship the current branch

- [ ] Commit the staged work on `feat/a2acli` (new `a2acli` crate, `a2a_rs::auto_connect`,
      `WebA2AClient` delegation, unused-`reqwest` drop from `a2a-client`).
- [ ] Open the PR. In the description, call out:
  - the `--auth`/`--timeout` caveat in `auto` mode (item 3 below), and
  - the agent-card transport-mislabel finding (item 4 below).
- [ ] Add `a2acli` to the workspace table in `CLAUDE.md` (currently lists the six
      library crates but not the new bin crate).

## 2. Bugs found while testing the CLI (not CLI bugs)

- [ ] **Agent card mislabels its transport.** `a2a-agents` `AgentBuilder`/runtime
      (`a2a-agents/src/core/runtime.rs`) mounts a **ConnectRPC** server
      (`ConnectRpcAdapter` + `HttpServer`) but the published card advertises the
      interface as **`JSONRPC`** (the `SimpleAgentInfo` default `protocol_binding`).
      Client auto-negotiation then picks the JSON-RPC client and fails
      (`invalid JSON-RPC response: error decoding response body`); `--transport
      connectrpc` works. Fix card generation so `protocol_binding` matches the
      mounted adapter. *Observed against `complex_agent` on :8080.*
- [ ] **ConnectRPC SSE subscription never closes on terminal state.** `a2acli stream`
      (and any subscriber) stays open after the task reaches `FAILED`/`COMPLETED`;
      had to cap each run with `timeout`. The stream should end when the task hits a
      terminal state. (Distinct from the `Last-Event-ID` gap in `ROADMAP.md`.)

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

- [ ] `complex_agent`'s rule-based path is unreachable env-only:
      `OpenAiProvider::from_env()` always returns `Ok` (it defaults base-url/model),
      so `load_llm()` never returns `None`. Add an opt-out (e.g. `A2A_NO_LLM=1` or a
      `--no-llm` flag) so the deterministic, no-network streaming path can be
      exercised in demos/tests without standing up an LLM endpoint.
