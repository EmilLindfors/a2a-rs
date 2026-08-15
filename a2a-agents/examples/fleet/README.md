# A fleet, end to end

Three agents, taken from configs on disk to a supervised deployment you can list,
read logs from, and stop — and, at the end, given a model each.

| Agent | Port | Handler | Skill |
|---|---|---|---|
| Greeter | 8081 | `echo` | `greet` |
| Analyst | 8082 | `llm` | `data-analysis` |
| Orchestrator | 8090 | `llm` | `route` — delegates to the other two by skill |

Nothing here needs an API key to *start*: the `llm` agents fall back to a
deterministic reply when no model is configured, so every command below runs as
written. Give them models in the last section.

Build the CLI once:

```bash
cargo build -p a2a-agents --bin a2a
export PATH="$PWD/target/debug:$PATH"      # or use the full path to a2a
```

Commands are written from the repository root.

## 1. Check the configs, then check the machine

Two different questions, one command each. `validate` asks whether the configs
are well-formed and whether they can coexist — no ports are bound, no network is
touched:

```console
$ a2a validate --fleet a2a-agents/examples/fleet/fleet.toml
fleet "Fleet Walkthrough" — 3 agent(s)
ok      a2a-agents/examples/fleet/greeter.toml
        agent "Greeter", handler echo, port 8081, 1 skill(s)
ok      a2a-agents/examples/fleet/analyst.toml
        agent "Analyst", handler llm, port 8082, 1 skill(s)
ok      a2a-agents/examples/fleet/orchestrator.toml
        agent "Orchestrator", handler llm, port 8090, 1 skill(s)
```

`doctor` asks whether *this machine* can run them — ports free, MCP commands
installed, model provider usable, container engine available:

```console
$ a2a doctor --fleet a2a-agents/examples/fleet/fleet.toml
environment
  ok      container engine: docker (/usr/bin/docker)

a2a-agents/examples/fleet/greeter.toml
  ok      config is valid — "Greeter", handler echo
  ok      127.0.0.1:8081 is free

a2a-agents/examples/fleet/analyst.toml
  ok      config is valid — "Analyst", handler llm
  ok      127.0.0.1:8082 is free
  warn    llm handler with no provider (OPENROUTER_API_KEY, GEMINI_API_KEY, ... unset)
          — it will answer with a deterministic fallback that lists its tools

...

together
  ok      these agents can run together

0 problem(s), 2 warning(s)
```

That warning is the keyless case being reported rather than discovered at
runtime. With a key exported it becomes a line naming what will actually be
called:

```
  ok      llm handler will use openrouter (z-ai/glm-4.6, via OPENROUTER_API_KEY)
```

Both commands exit non-zero when something is *wrong* (a warning is not), so
either is usable as a CI gate. `doctor` is the one worth running before a deploy:
a port that is already taken is invisible to `validate`.

## 2. Run the fleet locally

```bash
a2a up -f a2a-agents/examples/fleet/fleet.toml
```

The fleet is checked before anything binds — including the invariants that only
exist *between* agents, like two members claiming one port or two names that
slugify to the same registry id — and then all three start in one process,
sharing one agent registry:

```
2026-08-14T09:01:30.561216Z  INFO a2a: registered agent 'Greeter' as 'greeter'
2026-08-14T09:01:30.561628Z  INFO a2a: registered agent 'Analyst' as 'analyst'
2026-08-14T09:01:30.562070Z  INFO a2a: registered agent 'Orchestrator' as 'orchestrator'

  Greeter
    http://127.0.0.1:8081
    card: http://127.0.0.1:8081/.well-known/agent-card.json
  ...
```

That shared registry is what lets the orchestrator name its peers by skill
instead of by URL. It says which tool each one became:

```
INFO a2a: exposing remote agent 'Analyst' (skill 'data-analysis') as tool 'ask_analyst'
```

Talk to a member directly:

```console
$ a2acli send --url http://127.0.0.1:8081 'hello there'
task 72163aee-0ac9-4c0f-b8cf-ce4bd083d38c
  state:   completed

echo: hello there
```

…or let the orchestrator pick one for you (this needs a model — see the last
section). A slow model outlives the blocking send, which says so rather than
pretending the task is done:

```console
$ a2acli send --url http://127.0.0.1:8090 'Summarize these Q3 numbers: 12, 40, 38, 91'
  state:   working

the agent is still working; follow it with:
  a2acli --url http://127.0.0.1:8090 stream 3993ab43-...
  a2acli --url http://127.0.0.1:8090 get 3993ab43-...

$ a2acli --url http://127.0.0.1:8090 get 3993ab43-...
  state:   completed

Here's a summary of your Q3 numbers:
- **Total:** 181
- **Average:** 45.25
...
```

Ctrl-C stops the fleet. This is the dev loop; everything from here is deployment.

## 3. Start a control plane

The control plane runs agents on your behalf and exposes deploy/list/logs/stop
over HTTP. Deploying to it is remote code execution, so it refuses to start
without a bearer token:

```console
$ a2a control-plane
Error: control-plane requires a bearer token: pass --token <secret>, set
A2A_CONTROL_PLANE_TOKEN, or opt out explicitly with --no-auth
```

In a terminal of its own:

```bash
export A2A_CONTROL_PLANE_TOKEN=dev-token
a2a control-plane                    # binds 127.0.0.1:9090
```

Prefer the environment variable over `--token`: an argv token is visible to
anyone who can run `ps`.

Two things it says at startup, both worth reading:

- **`no --allow-env vars: configs referencing ${VAR} will be rejected`** — the
  secure default. Deployed configs are sent *raw* and the control plane expands
  `${VAR}` against its own environment, but only for variables you explicitly
  permit with `--allow-env NAME`. The machine running `a2a deploy` never needs
  the secrets the agent runs with.
- **`this runtime cannot survive a restart`** — `--runtime local` supervises
  plain child processes and forgets them if it is bounced. It is for dev loops.
  Use `--runtime container`, where `docker ps` is the durable record, for
  anything you expect to restart.

## 4. Deploy, inspect, stop

In a second terminal:

```bash
export A2A_CONTROL_PLANE_TOKEN=dev-token
export A2A_CONTROL_PLANE_URL=http://127.0.0.1:9090   # this is also the default
```

Deploy the whole fleet. Cross-agent invariants are checked before anything is
sent, so a port clash cannot leave a half-rolled-out fleet behind:

```console
$ a2a deploy --fleet a2a-agents/examples/fleet/fleet.toml
deploying 3 agent(s) to http://127.0.0.1:9090
ok      greeter                 healthy       http://127.0.0.1:8081
ok      analyst                 healthy       http://127.0.0.1:8082
ok      orchestrator            healthy       http://127.0.0.1:8090

$ a2a ps
ID                      HEALTH        ENDPOINT
greeter                 healthy       http://127.0.0.1:8081
analyst                 healthy       http://127.0.0.1:8082
orchestrator            healthy       http://127.0.0.1:8090
```

The agents are real and reachable:

```console
$ curl -s http://127.0.0.1:8081/.well-known/agent-card.json
{"name":"Greeter","description":"Greets whoever writes to it.",...}
```

`ps` reports health, which is a card probe — it says an agent is not answering,
never why. That is what logs are for:

```console
$ a2a logs greeter --tail 3
2026-07-26T14:15:20.595136Z  INFO a2a_agents::core::server:    - Greet (greet)
2026-07-26T14:15:20.596876Z  INFO ...: Starting HTTP server
2026-07-26T14:15:20.608061Z  INFO ...: HTTP server listening on 127.0.0.1:8081
```

Then stop them:

```console
$ a2a stop greeter analyst orchestrator
stopped greeter
stopped analyst
stopped orchestrator
```

Stopped agents are removed from discovery — peers resolving by skill or agent id
no longer find them — and drop out of `a2a ps`. They are not forgotten:
`a2a ps --all` shows them with health `stopped`, and `a2a logs` still answers for
them, which is when the log matters most.

## 5. Give them models

Both `llm` members ship without an `[llm]` block, so they take whatever the
environment provides. Export one key and both start on the same model:

```
INFO a2a: LLM provider provider="openrouter" model=z-ai/glm-4.6 selected_by="OPENROUTER_API_KEY" reasoning=(model default)
INFO a2a: LLM provider provider="openrouter" model=z-ai/glm-4.6 selected_by="OPENROUTER_API_KEY" reasoning=(model default)
```

Two identical lines is the thing to notice. A fleet shares one process
environment, so `OPENROUTER_MODEL` moves *every* unpinned member at once — which
is rarely what you want, because routing and analysis are not worth the same
money. Pin the model per agent instead (uncomment the block at the bottom of
`analyst.toml`):

```toml
[llm]
provider = "openrouter"
model = "z-ai/glm-5.2"
reasoning = "high"
```

The API key stays out of the config: with `api_key` absent, the provider reads
`OPENROUTER_API_KEY` from the environment. One key, a model per agent.

`reasoning` belongs beside `model` because it is a property of the model you
pointed at, and it is billed. Say what each workload is worth: `high` for
analysis, `low` for a router whose whole job is picking a tool, `off` for a
one-line lookup — or a token budget (`reasoning = 1000`). Omit it and nothing is
sent, so the model's own default stands, which for a reasoning model still means
it reasons. Two traps worth knowing:

- A small model asked to think hard can spend its entire response budget
  reasoning and return **nothing**. That fails the task naming the cause rather
  than completing it empty.
- `reasoning` reaches the wire on `openrouter` today. Elsewhere `a2a doctor`
  warns that it will be dropped, so the mistake is caught before it is billed.

Deploying a model-driven agent needs one more thing than step 4 did — permission
for the key to cross into it:

```bash
a2a control-plane --allow-env OPENROUTER_API_KEY
```

Without the flag, a config referencing `${OPENROUTER_API_KEY}` is rejected by
name at deploy time. With it, the variable is passed through to the agent
process; the config itself never carries the value.

## What this example does not cover

- **Containers.** Swap `--runtime container` into step 3 for isolation, resource
  ceilings (`--memory`, `--cpus`), and a control plane that survives a restart.
  It needs the `a2a-agents:latest` image built from `a2a-agents/Dockerfile`.
- **Custom handlers.** A handler no TOML can express ships as its own image and
  deploys like anything else — see `examples/image-agent/`.
- **Model ids and prices.** `z-ai/glm-5.2` and the rest move; check
  `curl https://openrouter.ai/api/v1/models` rather than trusting a README.
