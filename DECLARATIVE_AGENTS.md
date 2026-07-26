# Declarative Agents Platform

This workspace contains the building blocks for a **declarative-agents
platform** built on the A2A protocol: define agents in TOML, run them with the
`a2a` binary — zero custom Rust required for the common case.

> **Direction (2026-07-25):** the `a2a` CLI is the front door. Terraform is
> **deferred** — `terraform-provider-a2aagent/` is parked WIP and is not part of
> the supported path today. See `DECLARATIVE_AGENTS_TODO.md` for the reasoning:
> HCL is a front-end onto the same control plane and config schema, so it is
> cheaper to settle config strictness, fleet composition, and lifecycle
> commands standalone first than to design them through a Go provider.

## Pieces

- `a2a-agents/` — the declarative framework: TOML config (`AgentConfig`), the
  `AgentBuilder`, the `a2a` binary, and the generic config-driven `LlmHandler`
  (`src/handlers/llm.rs`).
- `a2a-agents-common/` — LLM providers, NLP, formatting.
- `terraform-provider-a2aagent/` — **parked.** A Terraform provider that
  currently renders TOML files. When it resumes it becomes a thin client over
  the control-plane API rather than a file writer.

## Getting started

```sh
# Scaffold a commented, immediately-runnable config.
a2a new "Weather Agent"                       # echo template, no keys needed
a2a new "Router" --template orchestrator      # delegates to peer agents

# Check it (unset ${VAR} refs are reported, not fatal; --strict-env to require).
a2a validate --config weather-agent.toml

# Check the *machine*: port free, MCP command installed, model key set.
a2a doctor --config weather-agent.toml

# Run it. The banner prints the endpoint and a command to poke it.
a2a run --config weather-agent.toml
```

Templates: `echo` (no API keys, no external services), `llm`, `mcp`,
`orchestrator`.

## Config is the source of truth

The Rust `AgentConfig` type defines what a valid agent is, and nothing
re-implements that validation:

```text
  AgentConfig (Rust)  ──schemars──►  a2a print-schema  ──►  JSON Schema
        │
        │  a2a new       renders a starter config
        │  a2a validate  checks shape (deny_unknown_fields: typos are errors)
        ▼
  <name>.toml  ──►  a2a run --config <name>.toml
                    a2a up -f fleet.toml  (a set of configs, checked together)
                    a2a deploy            (to a control plane; ps/logs/stop drive it)
```

Unknown keys are rejected, so a mistyped key is an error rather than a silently
dropped setting. Any future front-end (a Terraform provider, a UI) is expected
to pass configs through to this validator rather than duplicate it.

`a2a validate` answers *is this config well-formed*; `a2a doctor` answers *will
it work on this machine* — port free, MCP command installed, model key set,
`${VAR}`s resolvable, and (for more than one config) whether they can run
together. The split is why validate stays usable in CI without secrets while
doctor treats a missing secret as fatal.

## The generic LLM handler (`handler.type = "llm"`)

Generalizes `examples/complex_agent.rs`: a system prompt + tool-routing loop +
MCP tool bindings, all from config. Select it with:

```toml
[handler]
type = "llm"

[handler.llm]
system_prompt = "You are a concise, helpful assistant."
max_tool_rounds = 4
```

With an LLM key set (`OPENAI_API_KEY` / `GEMINI_API_KEY` / `OPENROUTER_API_KEY`),
the agent answers in natural language and picks MCP tools itself. With no key,
it falls back to a deterministic response that lists available tools (so the
agent still answers in secret-free CI).

## Agents calling agents (`[[handler.llm.agents]]`)

The `llm` handler's tools come from a unified `ToolSource` abstraction, so MCP
servers **and other A2A agents** are both just tools to the model. Declare peer
agents and the orchestrator delegates to them:

```toml
[[handler.llm.agents]]
name = "Weather Agent"
url  = "http://127.0.0.1:8080"
# description is optional — derived from the peer's agent card when omitted
```

Each entry becomes an `ask_<slug>` tool reached over the A2A `Transport` port
(auto-negotiated from the peer's card). The call sends an A2A task, waits for it
to reach a terminal state, and returns the agent's reply. See
`a2a-agents/examples/orchestrator_agent.toml`. This is the multi-agent keystone:
zero Rust to wire a fleet of agents that call each other.

## A fleet in one file

A multi-agent system is a set of agents, so it should be one artifact — not a
`--config` per agent retyped on every invocation. A fleet file names the set:

```toml
# fleet.toml
name = "Weather Demo"

[[agents]]
config = "weather.toml"      # paths are relative to this file

[[agents]]
config = "orchestrator.toml"
```

```sh
a2a up -f fleet.toml               # defaults to ./fleet.toml
a2a validate --fleet fleet.toml    # check without running
a2a doctor --fleet fleet.toml      # ...and check the machine it will run on
```

The fleet file redefines nothing about an agent — it is a list of configs plus
the invariants that only exist *between* them, which `a2a up` checks before
anything binds:

- two agents claiming the same **port** (otherwise one silently fails to bind
  inside a process that otherwise came up), and
- two agent names that slugify to the same **registry id** (otherwise
  registration upserts, and delegation by skill or `agent_id` reaches only
  whichever registered last).

Members run in one process sharing one registry, so peers resolve each other by
skill. See `a2a-agents/examples/fleet.toml`.

## Extraction to a standalone repo

The plan is to move the declarative-agent surface into its own repo
(`a2a-agents-platform`) so this repo stays focused on the protocol crates
(`a2a-rs`, `a2a-ap2`, `a2a-client`, `a2a-mcp`, `a2acli`). The boundary
contract: the new repo depends **only on published** `a2a-rs` / `a2a-mcp` /
`a2a-ap2` from crates.io — no path dependencies back here.

Migration steps (one PR, pre-1.0 "break cleanly" posture):

1. Create `a2a-agents-platform`; copy `a2a-agents/`, `a2a-agents-common/`,
   and `terraform-provider-a2aagent/`.
2. Flip path deps to crates.io versions (`a2a-rs = "0.4"`, etc.).
3. Add the generic handler crate if desired as a separate crate (currently
   co-located in `a2a-agents/src/handlers/` behind the `mcp-server` feature
   to avoid a circular dep with `a2a-mcp`).
4. In this repo: remove `a2a-agents`/`a2a-agents-common` from the workspace
   `Cargo.toml`; update `README.md`/`CLAUDE.md` to point at the new repo.
5. Keep `a2a-rs`, `a2a-ap2`, `a2a-client`, `a2a-mcp`, `a2acli` here.

## Deploying a fleet (control plane)

`a2a up` runs a fleet in one process, which is the right shape for a dev loop and
a small deployment. Supervising agents as separate, individually
restartable units goes through the control plane, which composes the
`AgentRuntime` and `AgentRegistry` ports:

```sh
# Deploying is remote code execution, so a token is required (--no-auth to opt
# out for a dev loop). `--allow-env` is deny-by-default: a deployed config may
# only reference the variables you name here.
A2A_CONTROL_PLANE_TOKEN=… a2a control-plane \
  --runtime container \
  --allow-env OPENROUTER_API_KEY
```

`POST/GET/DELETE /agents` deploys, lists, and tears down, and
`GET /agents/{id}/logs` replays an agent's output. The same binary drives it, so
nothing here needs Terraform or `curl`:

```sh
export A2A_CONTROL_PLANE_TOKEN=…   # --url defaults to where control-plane binds

a2a deploy --fleet fleet.toml      # or --config <toml>, repeatable
a2a ps                             # id, health, endpoint
a2a logs weather-agent --tail 50   # why "unhealthy", not just that it is
a2a stop weather-agent
```

Configs go over the wire **as written** — `${VAR}`s are the control plane's to
resolve, against its own environment and allowlist — so the deploying machine
never has to hold the secrets the agent runs with. The shape and cross-agent
conflict checks run before anything is sent, since a port clash discovered
halfway through a fleet leaves a partial rollout to unpick.

On startup it **recovers** the fleet it was already running, before serving
anything: `docker ps --filter label=a2a-agent` is the durable store (provisioning
stamps the id and published port as labels), and adopted agents are re-registered
for discovery by fetching their cards. Without that, a restart reports an empty
fleet while the agents are still serving. `--runtime local` cannot recover — its
children die with the supervisor — so it reports itself as *ephemeral* and warns;
use `--runtime container` for a control plane you expect to bounce.

## Smoke tests

```sh
# Scaffold and check a config (no API keys needed for the echo template):
a2a new "Smoke Agent" --output /tmp/smoke.toml
a2a validate --config /tmp/smoke.toml

# Validate a shipped example without holding its secrets:
a2a validate --config a2a-agents/examples/oauth2_auth.toml

# Print the JSON Schema for AgentConfig:
a2a print-schema > schema.json

# Run a TOML-only agent (set an LLM key for natural-language answers):
a2a run --config a2a-agents/examples/llm_agent.toml
```
