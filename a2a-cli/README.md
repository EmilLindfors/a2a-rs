# a2acli

A small command-line client for the **Agent-to-Agent (A2A) protocol**. It drives
the client `Transport` port from [`a2a-rs`](../a2a-rs) directly — `card`, `send`,
`get`, `cancel`, `stream` — and doubles as a manual cross-SDK interop harness.

## Install / run

```sh
cargo run -p a2acli -- <global options> <command>
# or build the binary:
cargo build -p a2acli   # target/debug/a2acli
```

## Endpoint

The agent base URL comes from `--url`/`-u` (alias `--base-url`), falling back to
the `A2A_URL` environment variable:

```sh
export A2A_URL=http://localhost:8137
a2acli card
# or per-invocation:
a2acli --url http://localhost:8137 card
```

## Global options

| Flag | Description |
|---|---|
| `-u, --url <URL>` (`--base-url`) | Agent base URL. Env: `A2A_URL`. |
| `--transport <auto\|connectrpc\|jsonrpc>` | Wire transport. Default `auto` (negotiate from the agent card, ConnectRPC preferred, JSON-RPC 2.0 as interop fallback). |
| `--auth <TOKEN>` | Bearer token. Env: `A2A_AUTH_TOKEN`. |
| `--timeout <SECS>` | Timeout for a single request (not the whole wait for a reply — that is `send --wait-timeout`). |
| `--json` | Emit raw JSON instead of human-readable output. |

`--auth` and `--timeout` apply in every transport mode, including the default
`auto`, and including the agent-card fetch that drives negotiation — an agent
that guards its RPC endpoints usually guards its card too.

## Commands

```sh
a2acli card                                   # fetch & print the agent card
a2acli send "hello"                           # send to a fresh (uuid) task id
a2acli send "hello" --task-id t1              # send to a specific task
a2acli send "and then?" --context-id c1       # continue a conversation (a new task in context c1)
a2acli send - < prompt.txt                    # read the message from stdin
a2acli send "hello" --no-wait                 # don't wait for the reply
a2acli send "hello" --wait-timeout 120        # wait longer than the 30s default
a2acli get t1                                 # fetch a task by id
a2acli list                                   # list the agent's tasks
a2acli list --state input-required            # …only ones waiting on you
a2acli list --limit 10 --context-id c1        # …paged, or scoped to one conversation
a2acli cancel t1                              # cancel a task
a2acli stream t1                              # subscribe to task updates
a2acli stream t1 --resilient                  # reconnect with backoff on disconnect
a2acli stream t1 --resilient --last-event-id 42   # resume from an event id
```

`send` waits for the agent's reply, and asks the *server* to wait too — A2A's
default (`return_immediately = false`) obliges it to hold the response until the
task finishes or stops for input, so a conformant agent answers with the reply
already attached. Against an agent that ignores the flag and returns `working`,
`send` waits on the task's event stream, falling back to polling if that agent
has no streaming backend either. Either way it prints what the agent said, and
if the budget runs out it prints the task as it stands plus how to follow it.
`--no-wait` opts out of both halves — it tells the server not to block and skips
the client-side wait — returning the acknowledgement immediately.

When the agent stops and hands the conversation back — `input-required` or
`auth-required` — `send` and `get` print what it asked and the command that
answers it, on the same task:

```
task t1
  state:   input-required

Which currency should I use?

the agent is waiting for you; answer on the same task with:
  a2acli --url http://localhost:8137 send --task-id t1 "your reply"
```

Add `--json` to any command for machine-readable output (JSON object for
`card`/`get`/`send`/`cancel`/`list`; one JSON envelope per line for `stream`).

## Exit codes

| Code | Meaning |
|---|---|
| `0` | The command worked. A task still running, or waiting on you, counts — the output says what to do next. |
| `1` | The command failed: bad arguments, agent unreachable, transport error. |
| `2` | The command worked and the **agent** failed or rejected the task. |

`2` is separate from `1` on purpose: a timeout is worth retrying and a refusal
is not, and one exit code cannot ask for both. `cancel` is the exception — a
canceled task is that command succeeding, so it exits `0`.

```sh
a2acli send "ship it" && ./deploy.sh    # only deploys if the agent finished the task
```

## Interop harness

Validate wire-compat against the canonical SDKs by crossing clients and servers:

```sh
# Terminal 1: our JSON-RPC server
cargo run -p a2a-rs --example jsonrpc_server --features jsonrpc-server   # binds :8137

# Terminal 2: our CLI against our server
A2A_URL=http://localhost:8137 cargo run -p a2acli -- card
A2A_URL=http://localhost:8137 cargo run -p a2acli -- --transport jsonrpc send "hello"
```

`a2a-rs/tests/jsonrpc_client_interop_test.rs` proves our-client ↔ our-server
byte-compat in process. Crossing the SDKs is what validates against *other*
implementations, and it is the part that has found real bugs.

### What has been crossed

Run on 2026-08-21 against `a2aproject/a2a-rs` at `7676ec9` (`a2acli` 0.1.5,
`helloworld-server`). Clone it next to this repo — the path `a2aproject/` is
gitignored for exactly this.

| Direction | Transport | Result |
|---|---|---|
| official `a2acli` → our `examples/jsonrpc_server` | JSONRPC | `card`, `send` (with and without a client-supplied task id), `get-task`, `list-tasks`, `stream` pass |
| official `a2acli` → our `examples/jsonrpc_server` | HTTP+JSON | same set passes |
| our `a2acli` → upstream `helloworld-server` | JSONRPC, negotiated from the card | `card`, `send`, `get`, `list`, `stream` pass |
| our `a2acli` → upstream `helloworld-server` | JSONRPC, `--transport jsonrpc` | `send`, `list` pass |
| either direction | GRPC | not crossed — upstream serves gRPC on `:50051`, we do not speak it |

`subscribe` and `cancel` are absent from the table because neither echo agent
leaves a task in a state where they apply: both answer synchronously, so a task
is finished by the time either call reaches it, and both correctly return the
spec's refusals (`-32004` unsupported-operation for subscribe, `-32002`
not-cancelable). Exercise them against an agent that takes its time.

### Known upstream bug

The official `a2acli` drops a JSON-RPC error on the *streaming* path. Such an
error is HTTP 200 with a JSON body, and its SSE reader finds no frames, so the
command prints nothing and exits 0. Visible two ways against our server: a
`subscribe` on a finished task reports the refusal over HTTP+JSON (where the
status is 501) and says nothing over JSON-RPC, and `stream` against a server
with no streaming backend says nothing on either. Our client had the same bug
until 2026-08-21 — see `CHANGELOG.md`.

```sh
# Terminal 1: the upstream agent (JSON-RPC on :3000/jsonrpc, REST on /rest)
cd a2aproject/a2a-rs && cargo run -p examples --bin helloworld-server

# Terminal 2: our CLI, negotiating the transport from its card
cargo run -p a2acli -- --url http://localhost:3000 card
cargo run -p a2acli -- --url http://localhost:3000 send "hello"

# ...and the official CLI against ours
cd a2aproject/a2a-rs && cargo run -p a2a-cli -- --base-url http://localhost:8137 card
```

Note the upstream package is `a2a-cli` and its binary is `a2acli`; ours is the
other way round.
