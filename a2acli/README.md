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
| `--auth <TOKEN>` | Bearer token. Env: `A2A_AUTH_TOKEN`. **See caveat below.** |
| `--timeout <SECS>` | Request timeout. **See caveat below.** |
| `--json` | Emit raw JSON instead of human-readable output. |

### `--auth` / `--timeout` caveat

The card-driven negotiation factories build *unauthenticated* clients, so
`--auth` and `--timeout` apply only when you pin a transport with
`--transport connectrpc` or `--transport jsonrpc`. In the default `auto` mode they
are ignored (a warning is logged to stderr). Threading credentials through
transport negotiation is out of scope for the CLI.

## Commands

```sh
a2acli card                                   # fetch & print the agent card
a2acli send "hello"                           # send to a fresh (uuid) task id
a2acli send "hello" --task-id t1              # send to a specific task
a2acli get t1                                 # fetch a task by id
a2acli cancel t1                              # cancel a task
a2acli stream t1                              # subscribe to task updates
a2acli stream t1 --resilient                  # reconnect with backoff on disconnect
a2acli stream t1 --resilient --last-event-id 42   # resume from an event id
```

Add `--json` to any command for machine-readable output (JSON object for
`card`/`get`/`send`/`cancel`; one JSON envelope per line for `stream`).

## Interop harness

Validate wire-compat against the canonical SDKs by crossing clients and servers:

```sh
# Terminal 1: our JSON-RPC server
cargo run -p a2a-rs --example jsonrpc_server --features jsonrpc-server   # binds :8137

# Terminal 2: our CLI against our server
A2A_URL=http://localhost:8137 cargo run -p a2acli -- card
A2A_URL=http://localhost:8137 cargo run -p a2acli -- --transport jsonrpc send "hello"
```

Then point the **official** `a2aproject/a2acli` at the same server, and/or point
this CLI at a stock A2A agent, to validate against other implementations.
(`a2a-rs/tests/jsonrpc_client_interop_test.rs` already proves our-client ↔
our-server byte-compat; this validates against *other* SDKs.)
