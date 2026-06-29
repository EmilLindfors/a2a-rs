# Declarative Agents — Next Steps

Roadmap for turning the A2A crates into a usable multi-agent **platform**. The
guiding principle: don't build infrastructure into `a2a-rs` (the pure hexagonal
protocol crate) — define **capabilities as ports** and add **adapters** in the
platform layer. Four pillars; pillar 1 is done.

Status legend: ✅ done · 🔜 next · ⬜ planned

---

## Pillar 1 — Agent-as-tool delegation ✅

Shipped in `feat/declarative-agents` (commit `9decb30`).

- ✅ `ToolSource` trait unifying MCP servers and remote A2A agents
  (`a2a-agents/src/handlers/tools.rs`).
- ✅ `McpToolSource` (all configured MCP servers) + `A2aAgentToolSource`
  (a remote agent as one `ask_<slug>` tool via the `Transport` port).
- ✅ `[[handler.llm.agents]]` config + `a2a` binary wiring.
- ✅ Live integration test over a real socket + `orchestrator_agent.toml`.

**Follow-ups (small, do opportunistically):**
- ⬜ Stream the delegated agent's tokens through instead of poll-to-terminal:
  prefer `subscribe_to_task`, fall back to the current bounded `get_task`
  poll. (`A2aAgentToolSource::invoke`.)
- ⬜ Decouple the `tools` module from the `mcp-server` feature so agent-as-tool
  works without pulling in `rmcp` (today the only consumer, `LlmHandler`, is
  `mcp-server`-gated). Add an `agent-tools` feature.
- ⬜ Resolve the axum 0.7 (frontend) vs 0.8 (`a2a-rs`) split — the test uses an
  `axum8` dev-dep alias as a stopgap. Bump the frontend to 0.8 when
  `askama_axum` allows.

---

## Pillar 2 — Agent registry / discovery ✅ (first cut)

So an orchestrator finds peers by **skill** instead of hard-coded URLs.

- ✅ `AgentRegistry` port (capability, not technology):
  `register(card, endpoint)`, `deregister(id)`, `get(id)`, `find_by_skill(skill)`,
  `list()`, in the platform layer (`a2a-agents/src/registry/mod.rs`).
- ✅ `InMemoryAgentRegistry` adapter (first-class type, per hex rule 6) —
  `RwLock<HashMap>`, runs without external infra. Unit-tested.
- ✅ `A2aAgentToolSource` resolvable from the registry: a `[[handler.llm.agents]]`
  entry names exactly one of `url` / `skill` / `agent_id`
  (`RemoteAgentConfig::target()`, parse-don't-validate). The runner two-phase
  starts: phase 1 self-registers every agent's card from config (race-free),
  phase 2 resolves skill/agent-id refs at startup (`bin/a2a.rs`).
- ✅ Live discovery-by-skill integration test
  (`tests/registry_discovery_test.rs`) + `examples/registry_{worker,orchestrator}.toml`.

**Follow-ups (do opportunistically):**
- ⬜ Resolve at **call time** (dynamic registry-backed `ToolSource`) for late
  joiners, not just at startup.
- ⬜ Card-fetch refresh loop (re-poll `/.well-known/agent-card.json` for
  liveness).
- ⬜ Decide registry persistence (in-memory vs sqlx vs the control-plane service
  in pillar 3) — keep it a port so the choice is an adapter swap.

---

## Pillar 3 — Runtime / isolation 🔜 (runtime + control-plane + containers landed)

A secure place to run an agent. Chosen first substrate: **OCI containers**.

- ✅ `AgentRuntime` port: `provision(spec)`, `start(id)`, `stop(id)`,
  `health(id)`, `list()` — capability port in the platform layer
  (`a2a-agents/src/runtime/mod.rs`), sharing `registry::AgentId` so runtime and
  registry compose. `AgentSpec`, `RuntimeHealth`, `RuntimeStatus`, `RuntimeError`.
  (The existing per-agent `core::AgentRuntime<H,S>` was renamed to `AgentServer`
  to free the name — that type *serves one agent*; the port *supervises many*.)
- ✅ `LocalProcessRuntime` adapter (`a2a-agents/src/runtime/local.rs`) —
  supervises each agent as a child `a2a run --config <path>` OS process; health =
  process alive **+** agent-card probe (`a2a_rs::fetch_agent_card`). First-class
  type (hex rule 6). Live end-to-end test spawns a real `a2a` child via
  `CARGO_BIN_EXE_a2a` and drives provision→start→Healthy→stop→Stopped
  (`tests/local_runtime_test.rs`).
- ✅ `ContainerRuntime` adapter (`a2a-agents/src/runtime/container.rs`) — CLI
  shell-out to a configurable `docker`/`podman` (no new dep). One base image
  (`a2a-agents/Dockerfile`) with the TOML bind-mounted at `/etc/agent.toml` and
  `a2a run` as the command; one container per agent (`a2a-agent-<id>`). Health =
  `inspect` status + agent-card probe through the published port. Pure
  `create_args`/`container_name` are unit-tested without Docker; a Docker-gated
  e2e (`tests/container_runtime_test.rs`) skips when the engine/image is absent.
  `a2a control-plane --runtime container [--engine --image]` runs the control
  plane over it (the payoff of the shared port). Adds `RuntimeError::Backend`.
  Constraint: container agent configs must omit `host` (so `HOST=0.0.0.0`
  applies) to be reachable. *Per-agent images for custom Rust handlers still ⬜.*
- ✅ Thin **control-plane service** owning `AgentRuntime` + `AgentRegistry`
  (`a2a-agents/src/control_plane/`): `ControlPlane::{deploy,undeploy,status,list}`
  (service, hex rule 9a) — deploy provisions+starts via the runtime **and**
  registers the card so peers discover it (runtime/registry ids coincide). HTTP
  adapter `control_plane_router` (`POST/GET/DELETE /agents`, axum 0.7) is the
  surface the Terraform provider will target. `a2a control-plane --bind
  --config-dir` subcommand serves it over `LocalProcessRuntime`. Added
  `InMemoryAgentRuntime` (process-free fake, hex rule 6) for fast service tests;
  HTTP round-trip test drives it with `reqwest` (`tests/control_plane_test.rs`).
- ⬜ Move secrets out of on-disk TOML: `${ENV}` refs → injected container env.
- ⬜ Future adapters behind the same port: microVM (Firecracker) / gVisor for
  untrusted third-party agents.

---

## Pillar 4 — Terraform provider rework ⬜

Make `terraform-provider-a2aagent` a real provider, not a file writer.

- ⬜ `a2aagent_agent` targets the control-plane API: Create = provision + start
  container + register card; Read = health/inspect; Update = re-provision;
  Delete = stop + deregister.
- ⬜ Fill the stubbed validators (currently `return nil`):
  `validateWithBinary` (shell `a2a validate`) and `validateWithJSONSchema`
  (real JSON Schema check) in
  `terraform-provider-a2aagent/internal/provider/agent_resource.go`.
- ⬜ Fix `renderTOML` drift: it emits ~8 of many fields and the legacy
  `implementation =` instead of `[handler].type`. Either render the full schema
  or generate from the JSON Schema.
- ⬜ Commit the provider on its own (currently untracked); decide if it moves to
  the extracted platform repo.

---

## Platform extraction ⬜

Per `DECLARATIVE_AGENTS.md`: move `a2a-agents`, `a2a-agents-common`, and the
Terraform provider into `a2a-agents-platform`, depending only on **published**
`a2a-rs` / `a2a-mcp` / `a2a-ap2` (no path deps back). Keeps the protocol crates
clean. Runtime + registry land in the new repo.

---

## Finish-work bugs (from review)

- ✅ `expand_env_vars` now honours `${VAR:-default}` (widened regex; missing var
  uses the default, else a hard error) (`a2a-agents/src/core/config.rs`).
- ✅ `handler.type` is a typed `HandlerType { Echo, Llm, Reimbursement,
  Custom(String) }` enum (parse, don't validate), replacing the `!= "echo"`
  check; `bin/a2a.rs` matches on it with a `Custom`/unsupported→echo fallback.
- ✅ `reasoning_enabled` is driven by `LlmProvider::supports_reasoning()` (true
  for the OpenRouter provider via `OpenAiConfig`), not env-sniffing
  (`handlers/llm.rs`).
