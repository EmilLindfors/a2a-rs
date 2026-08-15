# Running Agents in Docker Sandboxes

[Docker Sandboxes](https://docs.docker.com/ai/sandboxes/) run a workload in a
microVM behind a governed egress proxy. Agents run there unmodified — a single
agent, a whole fleet under `a2a up`, or the control plane with
`--runtime container`. This guide covers the setup, the one config change a
published port needs, and what is and is not isolated.

Everything here was verified against **`sbx` v0.38.0** on Windows with Docker
Desktop 4.64.

## Why bother

`--runtime container` already gives each agent capability dropping, a process
cap, and a read-only root filesystem where its storage allows. What it does not
give you is a kernel boundary or any control over where an agent connects *out*
to — and an LLM agent's whole job is to make outbound calls decided by a model.

A sandbox adds both: a microVM rather than a shared kernel, and a proxy that
enforces a per-sandbox egress policy with an audit log. That is the difference
between an agent being *contained* and being *isolated*.

The two compose. Running `--runtime container` inside a sandbox keeps the
per-agent hardening and puts the whole fleet behind the VM boundary.

## Version matters

Use the standalone `sbx` CLI. The `docker sandbox` plugin bundled with Docker
Desktop is a much older build — v0.12.0 against the standalone v0.38.0 — with no
port publishing and a daemon that rejects bind mounts. None of this guide works
there.

```console
$ sbx version
sbx version: v0.38.0 ...
```

Install it per the [Docker docs](https://docs.docker.com/ai/sandboxes/)
(`winget install Docker.sbx` on Windows, Homebrew on macOS, apt on Linux). The
bundled plugin can stay installed; the two coexist.

## One-time setup

```console
$ sbx login
$ sbx policy init balanced
```

The global network policy must be initialized before any sandbox starts. It is
one-time and machine-wide (`sbx policy reset` to start over):

| Policy | Effect |
|---|---|
| `allow-all` | No egress filtering. |
| `balanced` | Typical development traffic — AI services, package registries. Docker's recommendation. |
| `deny-all` | Everything blocked; add hosts with `sbx policy allow`. |

`balanced` is the right default for agents: LLM provider APIs and registries
reach out, nothing else does. Narrow it per sandbox rather than globally — see
[Governing egress](#governing-egress).

## Running a fleet

Publish each agent's port at creation and mount the directory holding your
configs as the workspace:

```console
$ sbx create --name my-fleet \
    -p 8081:8081 -p 8082:8082 -p 8090:8090 \
    shell /path/to/configs

$ sbx exec my-fleet a2a up -f /path/to/configs/fleet.toml
```

Every agent is then reachable from the host on its published port — agent cards,
`SendMessage`, streaming, `a2acli` — exactly as if it ran locally:

```console
$ curl http://127.0.0.1:8081/agent-card
{"name":"Greeter", ...}
```

To add a forward to a sandbox that already exists:

```console
$ sbx ports my-fleet --publish 8083        # or --publish 8083 for an ephemeral host port
$ sbx ports my-fleet                       # list what is published
```

`--publish` is **ignored** when re-attaching with `sbx run`; use `sbx ports`.

### Bind a reachable address

A published port forwards to the sandbox's `0.0.0.0`, so an agent bound to
loopback is not reachable through it. This is the one change a fleet needs, and
it is easy to miss because the failure looks like the forward is broken rather
than the agent:

```toml
[server]
host = "0.0.0.0"     # not "127.0.0.1"
http_port = 8081
```

Omitting `host` works too — it falls back to the `HOST` environment variable, and
to `127.0.0.1` when that is unset.

Worth knowing: `a2a new` scaffolds `host = "127.0.0.1"`, and the
`examples/fleet/` configs carry it as well, so both need this edit before their
ports can be forwarded.

**Caveat.** An agent advertises the address it *bound*, so binding `0.0.0.0`
publishes `http://0.0.0.0:PORT` as the URL on its agent card — which no peer can
dial. Inside one sandbox that is harmless (`a2a up` shares a registry and members
resolve each other by their configured URLs), but it does mean a card fetched
from outside is not directly usable. Tracked in `TODO.md`; the bind address and
the advertised address need to be two fields.

### Getting `a2a` into the sandbox

The `shell` template has no `a2a` binary. Options, cheapest first:

- **`sbx cp`** a Linux build in, or drop it in the mounted workspace. Extract one
  from the agent image if you have no Linux toolchain:
  ```console
  $ cid=$(docker create a2a-agents:latest)
  $ docker cp "$cid:/usr/local/bin/a2a" ./a2a && docker rm -f "$cid"
  ```
- **Install it inside** the sandbox (`cargo install a2a-agents`, or a release
  binary).
- **A custom template.** `sbx template load` takes a locally built image tar with
  no registry involved:
  ```console
  $ docker save a2a-agents:latest -o a2a.tar
  $ sbx template load a2a.tar
  $ sbx create -t a2a-agents:latest shell /path/to/configs
  ```
  This does **not** work with the image from `a2a-agents/Dockerfile` as-is: a
  sandbox template has to satisfy the sandbox base contract, and that image is a
  non-root `ENTRYPOINT` with no shell tooling. Creating from it fails with
  `failed to run sandbox container`. Building a sandbox-flavoured image is a
  separate piece of work.

### Workspace paths

The workspace mounts at the **translated host path**, not at a fixed location:
`C:\Users\me\configs` becomes `/c/Users/me/configs` inside. Pass configs by that
in-sandbox path, not by the host path and not by a guessed `/workspace`.

```console
$ sbx exec my-fleet a2a validate --fleet /c/Users/me/configs/fleet.toml
```

## Using `--runtime container` inside a sandbox

The VM runs its own Docker engine, and the control plane's provisioning works
there unchanged — bind-mounted config at `/etc/agent.toml`, published port, and
the full `ContainerHardening` policy:

```console
$ sbx exec my-fleet a2a control-plane --runtime container --allow-env OPENAI_API_KEY
```

The engine inside the sandbox is separate from the host's, so it has none of your
local images. Load what you need:

```console
$ docker save a2a-agents:latest -o a2a.tar          # on the host, into the workspace
$ sbx exec my-fleet docker load -i /c/path/to/a2a.tar
```

Restart-recovery still works within the sandbox's life — `docker ps --filter
label=a2a-agent` is the durable store, and it is the sandbox's engine answering.
Stopping the sandbox takes the VM and every container in it, so recovery has
nothing to adopt after that.

## Governing egress

This is the reason to use a sandbox. Narrow what a fleet may reach, per sandbox:

```console
$ sbx create --name my-fleet --deny-network evil.example.com shell /path/to/configs
$ sbx policy allow network --sandbox my-fleet "api.openai.com:443,*.npmjs.org"
$ sbx policy ls my-fleet                 # rules in effect for this sandbox
$ sbx policy log my-fleet                # what was allowed, what was blocked, and by which rule
```

Resources are a comma-separated list and take wildcards (`*.example.com`) and
optional ports. Without `--sandbox` a rule applies globally, which is rarely what
you want for one fleet's model provider.

A per-sandbox deny can only narrow the global policy, never widen it, so this is
safe under centralized governance.

### TLS interception

The sandbox installs its own CA and **can** re-sign outbound TLS. Whether it does
depends on policy: under `balanced` it tunnels allowed hosts, so agents see real
certificates.

Either way our agents work — the workspace enables reqwest's
`rustls-tls-native-roots` alongside `rustls-tls`, so the OS trust store is
honoured in addition to the built-in roots.

This is worth knowing because it is not reqwest's default, and the failure it
prevents is opaque. With webpki roots alone, rustls rejects the proxy's
certificate and every LLM call, peer delegation and MCP-over-HTTP request dies as
`Network error: error sending request for url (…)` with the certificate error
dropped from the message — while `curl` in the same shell works, because curl
reads the OS store. rustls ignores `SSL_CERT_FILE`, so nothing outside the binary
can fix it. See `NOTES.md`.

The same applies to any corporate MITM gateway, which is the more common case.

## Secrets

`sbx secret` stores credentials outside the sandbox, and the proxy authenticates
outbound API requests on the agent's behalf. Every sandbox starts with
placeholder values in its environment — `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`,
`GOOGLE_API_KEY`, `OPENROUTER_API_KEY` and more, all set to `proxy-managed` —
which the proxy swaps for the real key on the way out. An agent reaches its model
without ever holding the credential.

Our `[llm]` handler reads these variables when `api_key` is absent from the
config, so an OpenAI or OpenRouter agent needs no credential wiring at all inside
a sandbox — `OPENAI_API_KEY` and `OPENROUTER_API_KEY` line up by name. Store a key
with `sbx secret set`, or `sbx setup` to import ones already in your host
environment.

**Gemini is the exception.** The sandbox injects `GOOGLE_API_KEY`; we read
`GEMINI_API_KEY` (see `PROVIDER_ENV_VARS`). A Gemini agent will not pick up the
proxy-managed key, and falls back to whatever key selection finds next. Set
`GEMINI_API_KEY` explicitly, or name the key in the config.

That composes with how the control plane already handles secrets: configs are
deployed raw, `${VAR}` is expanded by the runtime under `--allow-env`, and secret
values never appear in the TOML or in the `docker create` argv.

If no key is configured for a provider, the placeholder is passed through as-is
and the API rejects it — a `401` naming `proxy-ma*aged` means the injection is
not set up, not that your agent is broken.

## Limits

- **Agents die with the sandbox.** A running sandbox keeps a long-lived server up
  indefinitely, but `sbx stop` takes the VM and everything in it.
- **A sandbox is a development boundary.** It is per-machine and driven by a
  local CLI; it is not a deployment target. For production isolation the
  `AgentRuntime` port is where a microVM backend would go.
- **Nothing in the test suite covers this path.** Everything above was verified
  by hand. See `TODO.md`.

## Troubleshooting

| Symptom | Cause |
|---|---|
| `unknown flag: --publish` | You are on the bundled `docker sandbox` plugin, not `sbx`. |
| `global network policy has not been initialized` | Run `sbx policy init balanced`. |
| `ERROR: Not authenticated to Docker` | Run `sbx login`. |
| `Daemon — not reachable` | Run `sbx daemon start`; `sbx diagnose` checks the rest. |
| Port published, connection refused from host | The agent is bound to `127.0.0.1`. Bind `0.0.0.0`. |
| `Network error: error sending request for url (…)` | TLS interception with a build that lacks native roots. |
| `failed to run sandbox container` with `-t` | The image is not a valid sandbox template. |
