# An agent that is its own image

The declarative layer covers a lot — echo, LLM handlers, MCP tools, delegation —
and then some agent needs code. This one prices a parcel from a rate table:
deterministic arithmetic that must be the same every time and wrong in a way
someone can fix, which is exactly what a prompt is not.

`[runtime] image` is how that agent reaches the platform. It ships as a container
image, the config names it, and everything downstream — health, `a2a ps`, logs,
skill-based discovery — works as it does for a TOML agent.

Files here:

| | |
|---|---|
| `main.rs` | the handler, and a `main` that reads `A2A_CONFIG` |
| `agent.toml` | a normal agent config, plus `[runtime] image` |
| `Dockerfile` | builds this example, with its own `ENTRYPOINT` |

Commands are written from the repository root.

## 1. Run it without any of that

It is an ordinary agent, so it runs like one:

```console
$ cargo run -p a2a-agents --example image-agent
shipping-quote agent, config: a2a-agents/examples/image-agent/agent.toml

$ a2acli send --url http://127.0.0.1:8300 'quote 2.5kg to eu'
2.5kg to eu: 15.00 EUR

$ a2acli send --url http://127.0.0.1:8300 'ship this to eu'
  state:   input-required

I need a weight, e.g. `quote 2.5kg to eu`.

the agent is waiting for you; answer on the same task with:
  a2acli --url http://127.0.0.1:8300 send --task-id e596bfb4-... "your reply"
```

The second answer comes back `input-required`, not `completed` — a request this
agent cannot price is a question to ask back, so the caller (a person here, an
orchestrating model in a fleet) can answer it instead of relaying a guess.

## 2. Ask the tooling about it

```console
$ a2a validate --config a2a-agents/examples/image-agent/agent.toml
ok      a2a-agents/examples/image-agent/agent.toml
        agent "Shipping Quotes", handler shipping-quote, port 8300, 1 skill(s)
        image a2a-image-agent:latest, which supplies that handler

$ a2a doctor --config a2a-agents/examples/image-agent/agent.toml
...
  ok      runs from image "a2a-image-agent:latest" — its handler, model provider
          and tools are inside the image, so they are not checked here

all clear
```

Delete the `[runtime]` block and both change their minds: `doctor` reports
`handler "shipping-quote" is not built into this binary`, and `a2a run` refuses
to start. That refusal is the reason this feature exists — the alternative, which
is what the platform used to do, was an echo agent that bound the port, served a
card, answered every request, and did none of what the config said.

## 3. Build the image

```bash
docker build -t a2a-image-agent:latest -f a2a-agents/examples/image-agent/Dockerfile .
```

The tag has to match `[runtime] image` in `agent.toml`. Nothing pulls it: the
control plane hands the reference to the engine, so anything the engine can
resolve works — a local tag, a registry path, a digest.

## 4. Deploy it

```bash
export A2A_CONTROL_PLANE_TOKEN=$(openssl rand -hex 32)
a2a control-plane --runtime container --config-dir ./deployed
```

Then, in another terminal:

```console
$ a2a deploy --config a2a-agents/examples/image-agent/agent.toml
deployed shipping-quotes  http://127.0.0.1:8300  healthy

$ a2a ps
ID                HEALTH        ENDPOINT
shipping-quotes   healthy       http://127.0.0.1:8300

$ a2acli send --url http://127.0.0.1:8300 'quote 800g domestic'
0.8kg to domestic: 5.46 EUR

$ a2a logs shipping-quotes --tail 5
$ a2a stop shipping-quotes
```

`--runtime container` is not optional here. The local runtime has no image to
pull and refuses the deploy outright:

```console
$ a2a deploy --config a2a-agents/examples/image-agent/agent.toml
Error: custom image is not available on this runtime: agent 'shipping-quotes'
runs from image 'a2a-image-agent:latest', which this runtime cannot pull —
deploy it on a container runtime (`a2a control-plane --runtime container`)
```

Running the config under `a2a` instead would be worse than an error: for an agent
whose handler *is* built in, it starts, answers, and passes its health probe
while being a different agent than the one deployed.

## What the image has to do

Two things, both visible in `main.rs` and the `Dockerfile`:

- **Read `A2A_CONFIG`.** The platform bind-mounts the deployed config
  read-only and points that variable at it. The path is the runtime's to choose,
  so hard-coding one works today and breaks quietly later.
- **Serve on the config's `http_port`, bound to `HOST`.** The port is published
  from the container; `HOST=0.0.0.0` is set for you, and a config that hard-codes
  `127.0.0.1` binds where the published port cannot reach it.

Everything else it gets for free, and gets the same as every other agent: the
`${VAR}` references its config makes are passed through by name (subject to
`--allow-env`), so secrets never enter the mounted TOML or the `docker create`
argv; capabilities are dropped, privilege escalation is blocked, processes are
capped, and — because this config stores nothing — the root filesystem is
read-only with a `/tmp` tmpfs.

The one asymmetry worth knowing: the base image is started with `a2a run --config
…`, and your image with **no command override**. Its `ENTRYPOINT` is the whole
story, which is why the `Dockerfile` here has no `CMD`.

## Turning this into your own agent

Nothing in `main.rs` depends on living in this workspace. In your own crate:

```toml
[dependencies]
a2a-agents = "0.6"
a2a-rs = "0.4"
```

…write a handler, point `AgentBuilder::from_file(env::var("A2A_CONFIG")?)` at it,
build an image, and name that image in the config. The platform stops caring what
is inside it.
