# Declarative Agents Platform

This workspace contains the building blocks for a **declarative-agents platform**
built on the A2A protocol: define agents in TOML/HCL, provision them with
Terraform, run them with the `a2a` binary — zero custom Rust required for the
common case.

## Pieces

- `a2a-agents/` — the declarative framework: TOML config (`AgentConfig`), the
  `AgentBuilder`, the `a2a` binary, and the generic config-driven `LlmHandler`
  (`src/handlers/llm.rs`).
- `a2a-agents-common/` — LLM providers, NLP, formatting.
- `terraform-provider-a2aagent/` — a Terraform provider that is the source of
  truth for agent definitions and renders the TOML files the `a2a` binary
  consumes.

## Config → Schema → Provider loop

```text
  AgentConfig (Rust)            terraform-provider-a2aagent (Go)
        │                              │
        │ schemars derive               │ JSON Schema fixture
        ▼                              ▼
  a2a print-schema ──────────► internal/schema/agent_config.json
        │                              │
        │ a2a validate                  │ a2aagent_agent resource
        ▼                              ▼
  <name>.toml  ◄──────────────── renders + validates
        │
        ▼
  a2a run --config <name>.toml
```

The Rust `AgentConfig` type is the single source of truth: the provider never
re-implements validation. It either shells out to `a2a validate` (preferred,
when an `a2a` binary is configured) or validates against the JSON Schema
exported by `a2a print-schema`.

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

## Smoke tests

```sh
# Validate a TOML config without starting a server:
a2a validate --config a2a-agents/examples/llm_agent.toml

# Print the JSON Schema for AgentConfig:
a2a print-schema > schema.json

# Run a TOML-only agent (set an LLM key for natural-language answers):
a2a run --config a2a-agents/examples/llm_agent.toml
```
