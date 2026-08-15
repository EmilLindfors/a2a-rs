# A2A Agents

Agent implementations for the A2A Protocol with production-ready patterns and
**declarative configuration**.

## 🚀 Quick Start (no Rust)

Install the `a2a` binary:

```bash
cargo install a2a-agents               # from crates.io
cargo install --path a2a-agents        # from a checkout of this repo
```

The CLI's features are the crate's defaults, so that is all it takes. (The
reimbursement sample agent is *not* in that build — add
`--features reimbursement-agent` if you want it.)

Then scaffold, check, and run an agent. The `echo` template needs no API keys
and no external services:

```bash
a2a new "Weather Agent"                  # writes weather-agent.toml
a2a validate --config weather-agent.toml
a2a run --config weather-agent.toml      # prints the endpoint and how to poke it
```

Templates: `echo`, `llm` (natural-language answers), `mcp` (LLM + MCP tools),
`orchestrator` (delegates to peer A2A agents). Pick one with `--template`, and
`--port` / `--output` to place it.

For more than one agent, `--fleet` adds each new agent to a fleet file (creating
it the first time) so they can be run — and checked against each other — as a set:

```bash
a2a new "Weather Agent" --fleet demo.toml
a2a new "Router" --template orchestrator --fleet demo.toml
a2a up -f demo.toml                      # runs both, checked together first
```

Generated configs are commented — they double as the schema documentation.
`a2a print-schema` emits the full JSON Schema, and unknown keys are rejected, so
a mistyped setting is an error rather than a silently ignored line.

## Quick Start (custom Rust handler)

When the built-in handlers are not enough, keep the TOML and supply your own
`AsyncMessageHandler`.

### 1. Define your agent (`agent.toml`)

```toml
[agent]
name = "My Agent"
description = "A helpful agent"

[[skills]]
id = "my_skill"
name = "My Skill"
description = "What this skill does"
```

### 2. Implement your handler

```rust
use a2a_rs::port::AsyncMessageHandler;
use async_trait::async_trait;

#[derive(Clone)]
struct MyHandler;

#[async_trait]
impl AsyncMessageHandler for MyHandler {
    async fn process_message(/* ... */) -> Result<Task, A2AError> {
        // Your business logic here
    }
}
```

### 3. Build and run!

```rust
use a2a_agents::AgentBuilder;
use a2a_rs::InMemoryTaskStorage;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    AgentBuilder::from_file("agent.toml")?
        .with_handler(MyHandler)
        .with_storage(InMemoryTaskStorage::new())
        .build()?
        .run()
        .await?;
    Ok(())
}
```

**That's it!** The framework handles servers, agent cards, authentication, and more.

📚 **[See complete Builder API documentation →](docs/builder-api.md)**

### 4. Deploy it like any other agent

The platform runs configs, not handlers, so a custom handler reaches it as an
image. Read the config path from `A2A_CONFIG` (the platform mounts the agent's
TOML and points that at it), build an image, and name it in the config:

```rust
let config = std::env::var("A2A_CONFIG").unwrap_or_else(|_| "agent.toml".to_string());
AgentBuilder::from_file(config)?
```

```toml
[runtime]
image = "ghcr.io/acme/my-agent:1.0"
```

`a2a deploy --config agent.toml` then provisions, starts, health-checks and
registers it exactly as it would a TOML agent. See
[Agents that bring their own image](#agents-that-bring-their-own-image).

## Overview

This crate provides two approaches for building agents:

### ✨ New: Declarative Builder API (Recommended)

- **90% less boilerplate** - ~30 lines vs ~300 lines
- **TOML configuration** - Define agents declaratively
- **Environment-aware** - Built-in env var interpolation
- **Type-safe** - Configuration validated at load time
- **Production-ready** - Batteries included

**Examples:**
- [`examples/minimal_agent.rs`](examples/minimal_agent.rs) - Echo agent (~50 lines)
- [`examples/reimbursement_builder.rs`](examples/reimbursement_builder.rs) - Full-featured agent

### Traditional Approach

The original hexagonal architecture approach with manual wiring:

1. **Hexagonal Architecture**: Clean separation between domain logic and adapters
2. **Framework Integration**: Uses `DefaultRequestProcessor` and storage backends
3. **Protocol Compliance**: Full A2A protocol support with HTTP transport
4. **Modern Patterns**: Async/await, builder patterns, and structured error handling

## 🔌 Model Context Protocol (MCP) Integration

You can expose any declarative A2A Agent as a Model Context Protocol (MCP) server over `stdio` (for local clients like Claude Desktop) or **Streamable HTTP** (for networked clients) transport. Either way, MCP-compatible clients can invoke the agent's skills as tools.

The bridge dispatches tool calls to the agent's message handler **in-process**, which means:
- No backing HTTP server is required (you can set `http_port = 0` for a pure-stdio server).
- Authentication checks are bypassed for local stdio calls (secure by design as it is run locally by the client), while HTTP endpoints can still use standard Bearer/OAuth2 token auth.

### 1. Enable the MCP Server in `agent.toml`

Add the `[features.mcp_server]` section to your config:

```toml
[agent]
name = "My MCP Agent"
version = "1.0.0"

[server]
host = "127.0.0.1"
http_port = 0 # Can be 0 for pure-stdio mode

[features.mcp_server]
enabled = true
stdio = true
name = "Custom MCP Service Name"     # Optional override
version = "2.0.0"                    # Optional override
```

### 2. Run the MCP Agent

`[features.mcp_server]` is config, so the `a2a` binary serves it — no Rust
needed. Scaffold one with `a2a new Notes --template mcp`, then:

```bash
a2a run --config notes.toml
```

### 3. Claude Desktop Configuration

To connect Claude Desktop to your agent, add the following to your Claude Desktop configuration file (usually located at `%APPDATA%\Claude\claude_desktop_config.json` on Windows):

```json
{
  "mcpServers": {
    "a2a-echo-agent": {
      "command": "/usr/local/bin/a2a",
      "args": ["run", "--config", "/absolute/path/to/notes.toml"]
    }
  }
}
```

### 4. Streamable HTTP transport

For networked MCP clients, serve the agent over MCP's Streamable HTTP transport
instead of stdio. Add a `[features.mcp_server.http]` section — when `enabled`,
it takes precedence over stdio:

```toml
[features.mcp_server]
enabled = true
stdio = false

[features.mcp_server.http]
enabled = true
host = "127.0.0.1"   # default
port = 8000          # default
path = "/mcp"        # default mount path
```

```bash
a2a run --config notes.toml
```

The server then accepts MCP requests at `http://127.0.0.1:8000/mcp`.

**DNS-rebinding protection.** By default the transport only accepts inbound
`Host` headers for loopback (`localhost`, `127.0.0.1`, `::1`). For a public
bind, list the hostnames you serve under — and optionally restrict browser
origins:

```toml
[features.mcp_server.http]
enabled = true
host = "0.0.0.0"
port = 8000
allowed_hosts = ["mcp.example.com", "mcp.example.com:8000"]
allowed_origins = ["https://app.example.com"]   # omit to disable Origin checks
```

Setting `allowed_hosts = []` disables `Host` validation entirely (accepts any
host) — only do this behind a trusted reverse proxy.

### 5. MCP client (consume external MCP tools)

The other direction: let your agent **call out** to MCP servers and use their
tools while it serves A2A requests. Enable the `mcp-client` Cargo feature and
declare the servers to connect to under `[features.mcp_client]`. Each server is
spawned as a child process:

```toml
[features.mcp_client]
enabled = true

[[features.mcp_client.servers]]
name = "echo"
command = "cargo"
args = ["run", "-q", "-p", "a2a-agents", "--features", "mcp-client", "--bin", "mcp_echo_server"]
# `env = { KEY = "value" }` and `cwd = "…"` are also supported.
```

In code, connect the config-declared servers into an `McpClientManager` and
hand it to the handler that will use the tools. The handler owns the manager and
reaches tools through the `McpToolsExt` trait:

```rust
use a2a_agents::core::{AgentBuilder, AgentConfig, McpClientManager};
use a2a_agents::traits::{McpToolsExt, extract_tool_result_text};

#[derive(Clone)]
struct MyHandler { mcp: McpClientManager }

impl McpToolsExt for MyHandler {
    fn mcp_client(&self) -> &McpClientManager { &self.mcp }
}

// inside process_message:
//   let result = self.call_mcp_tool("echo", "echo", Some(json!({ "text": text }))).await?;
//   let reply  = extract_tool_result_text(&result);

let config = AgentConfig::from_file("agent.toml")?;
let mcp = McpClientManager::connect(&config.features.mcp_client).await?; // connects + discovers tools
AgentBuilder::new(config)
    .with_handler(MyHandler { mcp })
    .with_storage(a2a_rs::InMemoryTaskStorage::new())
    .build()?
    .run()
    .await?;
```

Connection is lenient — a server that fails to start is logged and skipped, and
`connect` only errors if servers were configured but none could be reached.

`bin/mcp_echo_server.rs` is a two-tool MCP stdio server (`echo`, `add`) bundled
for exactly this: point a `[[features.mcp_client.servers]]` entry at
`cargo run -p a2a-agents --features mcp-client --bin mcp_echo_server` and the
agent has real tools to call with no external setup. Swap in any MCP stdio
server to talk to something real.

## 🤖 LLM agents & multi-agent platform

Beyond single agents, `a2a-agents` ships the building blocks for an
**LLM-driven, multi-agent platform** — defined as **ports** in the platform
layer so the pure `a2a-rs` protocol crate stays infrastructure-free. The
`a2a` binary wires them together. Its features (`llm`, `mcp-server`, `schema`)
are the crate defaults, so from a checkout:

```bash
cargo run -p a2a-agents --bin a2a -- <subcommand>
```

| Subcommand | What it does |
|---|---|
| `new <name> [--template …] [--fleet <toml>]` | Scaffold a starter config, optionally adding it to a fleet |
| `run --config <toml>…` | Run one or more agents from TOML configs |
| `up -f <fleet.toml>` | Run every agent a fleet file names, checked together first |
| `validate --config <toml>… [--fleet <toml>]` | Load + validate configs without serving |
| `doctor [--config <toml>…] [--fleet <toml>]` | Pre-flight: port free, MCP command installed, model key set |
| `control-plane --bind … --config-dir … --runtime local\|container` | Serve the deploy/list/status/logs/undeploy HTTP API |
| `deploy --config <toml>… [--fleet <toml>]` | Deploy agents to a running control plane |
| `ps [--all]` | List what a control plane is running, with health (`--all` includes stopped) |
| `logs <id> [--tail N]` | Print a deployed agent's captured output |
| `stop <id>…` | Stop deployed agents and remove them from discovery |
| `print-schema [--fleet]` | Print the `AgentConfig` (or `FleetConfig`) JSON Schema to stdout |

### Config is the source of truth

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
dropped setting. Any future front-end (a Terraform provider, a UI) is expected to
pass configs through to this validator rather than duplicate it.

### Fleets (`a2a up`)

A multi-agent system is one artifact, not a `--config` per agent retyped every
run. A fleet file lists member configs by path (relative to itself, so it runs
from anywhere) and adds the checks that only exist *between* agents — a shared
port, or two names that slugify to the same registry id. Both are silent-wrong
at runtime, so `a2a up` catches them before anything binds:

```toml
# fleet.toml
name = "Fleet Walkthrough"

[[agents]]
config = "greeter.toml"

[[agents]]
config = "analyst.toml"
```

```bash
a2a up -f fleet.toml               # defaults to ./fleet.toml
a2a validate --fleet fleet.toml    # same checks, nothing started
```

Members share one process and one agent registry, so peers resolve each other by
skill. [`examples/fleet/`](examples/fleet/) walks the whole thing through.

### Pre-flight (`a2a doctor`)

`validate` asks whether a config is well-formed; `doctor` asks whether it will
work *here* — port free, MCP command on `PATH`, model key set, `${VAR}`s
resolvable, container engine present, and whether the configs named can run
together:

```bash
a2a doctor --config weather.toml     # one agent
a2a doctor --fleet fleet.toml        # the whole fleet, plus the environment
```

Only *problems* set the exit code; warnings (no model key, no container engine)
describe something that will run, differently than you may have meant. An unset
`${VAR}` is reported by `validate` and a problem here — `a2a run` refuses to
start until it resolves.

### Config-driven LLM handler (`llm` feature)

Set `type = "llm"` and the framework drives a generic tool-calling LLM handler —
**no Rust code**. The model provider is picked up from the environment
(`OPENAI_API_KEY` / `GEMINI_API_KEY` / `OPENROUTER_API_KEY`):

```toml
[handler]
type = "llm"

[handler.llm]
system_prompt = "You are a helpful assistant."
max_tool_rounds = 4
```

The `llm` feature is independent of MCP: an LLM agent that only delegates to peer
A2A agents builds without pulling in `rmcp`. Add `mcp-server` as well to also
feed it the tools of connected MCP servers.

#### Picking the model (`[llm]`)

A top-level `[llm]` block pins the provider and model for **this** agent, which
is what lets a fleet run each agent on a different one — the env vars are
process-wide, so a fleet driven from `OPENROUTER_MODEL` collapses onto a single
model:

```toml
[llm]
provider = "openrouter"           # openrouter | openai | gemini
model = "z-ai/glm-4.6"
reasoning = "off"                 # off | low | medium | high, or a token budget: 1000
# api_key = "${OPENROUTER_API_KEY}"   # optional; the env key is used when absent
```

`reasoning` says how hard the model should think, and it belongs here because
that is a property of the model you pointed at: a flash model answering in one
line and a frontier model doing analysis want different answers, and the handler
serving them cannot tell which it got. Omit it and nothing is sent — the model's
own default stands (a reasoning model still reasons). It reaches the wire on
`openrouter` today; other providers log that they are dropping it rather than
pretending. Code can override it per request with `LlmRequest::reasoning`.

Beware the trap it exists to close: a small model asked for high effort can spend
its whole response budget thinking and return **nothing**. That fails the task
with the reason rather than completing it empty — see
[`examples/fleet/`](examples/fleet/), "Give them models".

#### Remembering the conversation (`[handler.llm.context]`)

By default an agent answers each message on its own: every turn starts from the
system prompt and the incoming text, with no memory of the last one. Turn that
off with `mode`:

```toml
[handler.llm.context]
mode = "context"          # none (default) | task | context
max_input_tokens = 100000 # ceiling on the request; 0 disables trimming
reserve_for_output = 8000 # held back for the reply
compact_at_percent = 80   # summarize at this share of the usable budget
keep_recent_turns = 4     # turns kept verbatim
max_tool_result_chars = 8000
```

- `task` carries the current task's own messages. Useful for a task that goes
  back and forth through `input-required`.
- `context` carries the whole conversation — every task sharing the message's
  `contextId`. Pair it with `[server.storage] type = "sqlx"`; with in-memory
  storage the conversation dies with the process.

There is no separate conversation store to configure. A2A already records a
message per task status transition, and read back in `contextId` order that *is*
the conversation, so `mode = "context"` reads the same rows `tasks/get` does.

When a conversation outgrows the budget the handler gives things up in order:
first it trims tool results to their head and tail (one chatty MCP server is the
usual cause), then it drops whole tool call/result pairs while keeping the
assistant text that concluded from them, and only then does it ask the model to
summarize the older turns. The summary is stored against the context and stands
in for what it replaced on later turns. The system prompt and the message being
answered are never trimmed — if those alone exceed the budget the task fails
naming `max_input_tokens`, because silently dropping half of a question produces
a confident answer to something nobody asked.

**`mode = "context"` cannot yet be combined with `[server.auth]`.** A `contextId`
is supplied by the caller, so once conversations are read back it decides which
conversation you get — and no authenticated principal reaches the message
handler yet to tell two callers apart. The storage layer records and enforces an
owner per context, but with nothing to compare against it cannot separate them,
so the config is rejected at startup rather than served as if it were enforcing
something. Use `mode = "task"`, or run the agent without authentication.

### Agent-as-tool delegation

List peer agents under `[[handler.llm.agents]]` and each is exposed to the model
as one `ask_<slug>` tool, so an orchestrator delegates work to specialists. Name
a peer by **exactly one** of `url`, `skill`, or `agent_id`:

```toml
[[handler.llm.agents]]
name = "Weather Agent"
url = "http://127.0.0.1:8081"     # dial directly, or…

[[handler.llm.agents]]
name = "Billing"
skill = "invoice-lookup"          # …resolve by advertised skill, or…

[[handler.llm.agents]]
name = "Scheduler"
agent_id = "scheduler-agent"      # …by registry id (slug of the name)
```

`skill` / `agent_id` are resolved against the **agent registry** at startup, so
peers are found by capability instead of a hard-coded URL. See
[`examples/fleet/orchestrator.toml`](examples/fleet/orchestrator.toml).

### Control plane

`control-plane` serves an HTTP API that composes the runtime and registry:
`POST /agents` deploys an agent from rendered TOML (provision + start + register
its card so peers discover it), `GET /agents` lists, `GET /agents/{id}` reports
health, `GET /agents/{id}/logs` replays its output, `DELETE /agents/{id}` tears
down. Pick the backend with `--runtime`: `local` supervises child `a2a run`
processes, `container` runs each agent in a `docker`/`podman` container
(`--engine`, `--image`).

#### Agents that bring their own image

`--image` is the *base* image, for agents that are just a config. An agent that
needs a handler no TOML can express names its own:

```toml
[runtime]
image = "ghcr.io/acme/billing:1.4"
```

That is the escape hatch that keeps the declarative layer open: a custom Rust
handler is a different image, and the platform goes back to caring only about the
config, the port and the container. Deploy it exactly like any other agent
(`a2a deploy --config billing.toml`).

Every agent container gets the same contract, whoever built the image:

| | |
|---|---|
| `/etc/agent.toml` | the agent's config, bind-mounted read-only |
| `A2A_CONFIG` | that path, so the image need not hard-code it |
| `HOST=0.0.0.0` | bind all interfaces, or the published port reaches nothing |
| `-p <http_port>:<http_port>` | the port from `[server]` |
| `-e VAR` | each `${VAR}` the config references, if `--allow-env` permits it |

The one difference: the base image is started with `a2a run --config
/etc/agent.toml`, while your image is started with **no command override** — its
own `ENTRYPOINT` decides how to boot. Serve A2A on the configured `http_port` and
the platform's health probe, `a2a ps` and skill-based discovery all work as they
do for a TOML agent.

One thing to keep in step: the card the control plane **registers** is derived
from the config, not fetched from the image, so peers resolve by the `[[skills]]`
the TOML declares. Declare there whatever the image's own card advertises.

Two things say no rather than pretending:

- `a2a run --config` warns that it is serving the config from *this* binary, not
  from the image.
- `--runtime local` refuses to deploy an image agent at all (`501`, "custom image
  is not available on this runtime"). It has no image to pull, and running the
  config under `a2a` instead would start an agent that is healthy and wrong.

A handler name with **no** image behind it is now a hard error — `a2a run`
refuses to start rather than falling back to echo, which used to give an agent
that bound its port, answered requests, and did none of what its config said.

Deploying an agent is remote code execution, so the API **requires a bearer
token** — startup fails without `--token` / `A2A_CONTROL_PLANE_TOKEN` unless you
explicitly opt out with `--no-auth`. Secrets are deny-by-default: a deployed
config may only reference environment variables named with `--allow-env`.

```bash
export A2A_CONTROL_PLANE_TOKEN=$(openssl rand -hex 32)

cargo run -p a2a-agents --bin a2a -- \
  control-plane --bind 127.0.0.1:9090 --config-dir ./deployed --runtime local \
  --allow-env OPENROUTER_API_KEY
```

`--runtime local` children inherit the whole process environment, so it is a
dev-loop backend; deploy configs you do not control on `--runtime container`,
where only allow-listed variables cross the boundary.

#### Container hardening

Every container is created with capabilities dropped (`--cap-drop=ALL`),
privilege escalation blocked (`no-new-privileges`), a 512-process cap, and — for
agents whose storage writes nothing, i.e. `inmemory` — a read-only root
filesystem with a `/tmp` tmpfs. The base image runs as uid 10001. That last pair
is derived from the config rather than asked for, because guessing wrong is
invisible either way: a read-only `sqlx` agent crash-loops on a disk error that
names nothing useful, and a writable in-memory one gives up the protection for
free.

Resource ceilings are **not** defaulted — no memory limit is right for every
agent, and a guessed one shows up as an agent dying under load for no visible
reason:

```bash
a2a control-plane --runtime container --memory 512m --cpus 1.5
a2a control-plane --runtime container --no-hardening   # escape hatch; warns
```

Two consequences worth knowing: an agent cannot bind a container port below 1024
(publish it on whatever host port you like — `-p 80:8080`), and a handler that
writes outside `/tmp` needs `sqlx` storage or a relaxed policy
(`ContainerHardening`). This is the cheap 80% of isolation: it removes what an
HTTP server never needed and bounds what a misbehaving one can consume. It is
not a defence against code written to escape a container — call an agent run
this way **contained**, not *isolated*.

#### Driving it (`deploy` / `ps` / `logs` / `stop`)

The same binary is the client. `--url` defaults to where `control-plane` binds
and `--token` to `A2A_CONTROL_PLANE_TOKEN`, so a control plane in one terminal
and these in another need no configuration to find each other:

```bash
export A2A_CONTROL_PLANE_TOKEN=…          # prefer the env var: argv is public

a2a deploy --config weather.toml           # or --fleet fleet.toml
a2a ps
a2a logs weather-agent --tail 50
a2a stop weather-agent
```

Configs are sent **as written**: `${VAR}` references are resolved by the control
plane against its own environment and `--allow-env` allowlist, so the machine
deploying never needs the secrets the agent runs with. Shape and the cross-agent
conflict checks run locally first, before anything is sent — a port clash in a
fleet should not leave half of it deployed.

`logs` answers the question health cannot: `unhealthy` says the card probe is
failing, not why. The container runtime replays what the engine retained; the
local runtime serves the per-agent files it captured under `--log-dir`
(defaulting to `<config-dir>/logs`). A backend that keeps no logs says so
explicitly rather than reporting an empty log, since "printed nothing" and "not
recorded" send you to very different places.

#### Surviving a restart

On startup the control plane **recovers** the fleet it was already running,
before it serves a single request — otherwise a bounce leaves it reporting an
empty fleet while the agents are still up (`GET /agents` → `[]`, `DELETE` → 404,
and a Terraform `Read` concluding the agents were destroyed and redeploying on
top of them).

Only `--runtime container` can do this: `docker ps --filter label=a2a-agent` is
the durable store, since provisioning stamps the agent id and published port as
container labels. Recovered agents are re-registered for discovery by fetching
their cards, so peers resolve them by skill again. `--runtime local` reports
itself as *ephemeral* and warns loudly — its children die with the supervisor,
and nothing durable ties a stray process to an agent id. **Use `container` for
any control plane you expect to restart.**

See the workspace [`NOTES.md`](../NOTES.md) for the decisions behind this design
(and why Terraform is deferred), and [`TODO.md`](../TODO.md) for open work.

### Running a fleet inside a Docker Sandbox

[Docker Sandboxes](https://docs.docker.com/ai/sandboxes/) run a workload in a
microVM behind a governed egress proxy. That is a stronger boundary than
`--runtime container` gives you: `ContainerHardening` drops capabilities and caps
processes, but shares the host kernel and says nothing about where an agent
connects *out* to. Agents run there unmodified, and a fleet needs no changes
beyond binding a reachable address.

Publish each agent's port at creation, mount the directory holding your configs
as the workspace, and run the fleet inside it:

```bash
sbx create --name my-fleet -p 8081:8081 -p 8082:8082 -p 8090:8090 shell /path/to/configs
sbx exec my-fleet a2a up -f /path/to/configs/fleet.toml
```

Every agent is then reachable from the host on its published port — agent cards,
`SendMessage`, streaming, `a2acli` — exactly as if it ran locally.

Two things catch people out. A published port forwards to the sandbox's
`0.0.0.0`, so the `host = "127.0.0.1"` that `a2a new` scaffolds has to become
`host = "0.0.0.0"` before the forward reaches anything. And use the standalone
`sbx` CLI: the `docker sandbox` plugin bundled with Docker Desktop is far behind
it and supports none of this.

See [docs/docker-sandboxes.md](docs/docker-sandboxes.md) for the full setup —
egress policy, secrets, getting `a2a` into the sandbox, running
`--runtime container` inside one, and troubleshooting.

## Reference agent and further reading

- [docs/reimbursement-demo.md](docs/reimbursement-demo.md) — the reimbursement
  reference agent: a hand-written `AsyncMessageHandler` with an interactive form
  flow and a small web frontend. Opt-in behind the `reimbursement-agent` feature.
- [docs/builder-api.md](docs/builder-api.md) — the full `AgentBuilder` API.
- [docs/authentication.md](docs/authentication.md) — Bearer, JWT, and OAuth2.
- [docs/docker-sandboxes.md](docs/docker-sandboxes.md) — running agents and
  fleets in a microVM with governed egress.
- [examples/fleet/](examples/fleet/) — a worked walkthrough: three agents from
  configs on disk to a supervised deployment, then given models.
- [examples/image-agent/](examples/image-agent/) — a handler written in Rust,
  shipped as its own image and deployed like any other agent.
