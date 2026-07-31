# The platform lifecycle, end to end

Two agents, taken from a config on disk to a supervised deployment you can list,
read logs from, and stop. Every command below is runnable as written and needs no
API key — both agents use the `echo` handler, because the subject here is the
*platform*, not what the agents say.

`examples/fleet.toml` covers running a fleet locally (`a2a up`). This walkthrough
picks up where that stops: handing the same fleet to a control plane.

Build the CLI once:

```bash
cargo build -p a2a-agents --bin a2a
export PATH="$PWD/target/debug:$PATH"      # or use the full path to a2a
```

Commands below are written from the repository root.

## 1. Check the config, then check the machine

The two questions are different, and each has its own command. `validate` asks
whether the configs are well-formed and whether they can coexist:

```console
$ a2a validate --fleet a2a-agents/examples/platform/fleet.toml
fleet "Platform Walkthrough" — 2 agent(s)
ok      .../greeter.toml
        agent "Greeter", handler echo, port 8081, 1 skill(s)
ok      .../notifier.toml
        agent "Notifier", handler echo, port 8082, 1 skill(s)
```

`doctor` asks whether *this machine* can actually run them — ports free, MCP
commands installed, model key present, container engine available:

```console
$ a2a doctor --fleet a2a-agents/examples/platform/fleet.toml
environment
  ok      model provider: OPENROUTER_API_KEY is set
  ok      container engine: docker (/usr/bin/docker)

.../greeter.toml
  ok      config is valid — "Greeter", handler echo
  ok      127.0.0.1:8081 is free

.../notifier.toml
  ok      config is valid — "Notifier", handler echo
  ok      127.0.0.1:8082 is free

together
  ok      these agents can run together

all clear
```

Both exit non-zero when something is wrong, so either is usable as a CI gate.
`doctor` is the one worth running before a deploy: a port that is already taken
is invisible to `validate`, which never touches the network.

## 2. Run the fleet locally

```bash
a2a up -f a2a-agents/examples/platform/fleet.toml
```

The fleet is checked before anything binds, then both agents start in one process
sharing an agent registry. Ctrl-C stops them.

This is the dev loop. Everything from here on is the deployment story.

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

Two things it tells you at startup, both worth reading:

- **`no --allow-env vars: configs referencing ${VAR} will be rejected`** — the
  secure default. Deployed configs are sent *raw*, and the control plane expands
  `${VAR}` against its own environment, but only for variables you have
  explicitly permitted with `--allow-env NAME`. The machine running `a2a deploy`
  never needs the secrets the agent runs with.
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
sent, so a port clash cannot leave you with a half-rolled-out fleet:

```console
$ a2a deploy --fleet a2a-agents/examples/platform/fleet.toml
deploying 2 agent(s) to http://127.0.0.1:9090
ok      greeter                 healthy       http://127.0.0.1:8081
ok      notifier                healthy       http://127.0.0.1:8082
```

```console
$ a2a ps
ID                      HEALTH        ENDPOINT
greeter                 healthy       http://127.0.0.1:8081
notifier                healthy       http://127.0.0.1:8082
```

The agents are real and reachable:

```console
$ curl -s http://127.0.0.1:8081/.well-known/agent-card.json
{"name":"Greeter","description":"Greets whoever writes to it.",...}

$ a2acli send --url http://127.0.0.1:8081 'hello' --json
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
$ a2a stop greeter notifier
stopped greeter
stopped notifier
```

Stopped agents are removed from discovery — peers resolving by skill or agent id
will no longer find them — and they drop out of `a2a ps`. They are not
forgotten, though: `a2a ps --all` shows them with health `stopped`, and
`a2a logs` still answers for them, which is when the log matters most.

## What this example does not cover

- **Containers.** Swap `--runtime container` into step 3 for isolation, resource
  ceilings (`--memory`, `--cpus`), and a control plane that survives a restart.
  It needs the `a2a-agents:latest` image built from `a2a-agents/Dockerfile`.
- **Secrets.** Add `--allow-env OPENAI_API_KEY` to step 3 and `${OPENAI_API_KEY}`
  to a config to see the allowlist in action; without the flag the deploy is
  rejected by name.
- **Real work.** Both agents echo. Point `[handler] type = "llm"` at a model, or
  see `examples/registry_orchestrator.toml` for delegation between agents.
