# Multi-Model OpenRouter Showcase

Three specialist A2A agents, each running a **different OpenRouter model**, routed
by an orchestrator that discovers them by **skill** — all in one process.

This is the *per-agent model* story of the platform: one `OPENROUTER_API_KEY` for
billing, an independent model per agent, so each workload runs on the cheapest
model that handles it.

## The agents

| Agent           | Port | Model                             | Skill                | Reasoning     |
|-----------------|------|-----------------------------------|----------------------|---------------|
| Flash Worker    | 8081 | `deepseek/deepseek-v4-flash-0731` | `weather-lookup`     | `off`         |
| GLM Analyst     | 8082 | `z-ai/glm-5.2`                    | `data-analysis`      | `high`        |
| MiniMax Extract | 8083 | `minimax/minimax-m3`              | `content-extraction` | 1000 tokens   |
| Orchestrator    | 8090 | `deepseek/deepseek-v4-flash-0731` | `route` (delegates)  | `low`         |

All three have a 1M-token context and support tool calling. Prices below are per
million tokens (in / out), read from OpenRouter's `GET /models` on **2026-08-13**:

- `deepseek/deepseek-v4-flash-0731` — $0.08 / $0.18. The cheapest of the three;
  good for high-volume lookups and for routing.
- `z-ai/glm-5.2` — $0.50 / $3.15. Stronger general-purpose model, and by far the
  most expensive per output token; good for analysis worth paying for.
- `minimax/minimax-m3` — $0.30 / $1.20. Accepts text, image and video input.

## How per-agent models work

Each agent pins its provider + model in a **top-level `[llm]` block**:

```toml
[llm]
provider = "openrouter"
model = "z-ai/glm-5.2"
```

The API key is **not** in the config — it comes from `OPENROUTER_API_KEY` in the
environment (`provider_from_settings` falls back to the env key when `[llm]
api_key` is absent). So one key, many models.

`a2a up` launches every fleet member in the same process, but each member loads
its own config and builds its own provider through `resolve_llm`, so a per-agent
`model` is honoured independently. Startup says so, one line per member:

```
Loading LLM configuration from TOML (provider: openrouter, model: deepseek/deepseek-v4-flash-0731, reasoning: off)
Loading LLM configuration from TOML (provider: openrouter, model: z-ai/glm-5.2, reasoning: high)
Loading LLM configuration from TOML (provider: openrouter, model: minimax/minimax-m3, reasoning: 1000)
```

## Reasoning per agent

How hard a model thinks is priced like the model itself, so it is configured
beside it — `[llm] reasoning`, not something the handler decides:

```toml
[llm]
provider = "openrouter"
model = "deepseek/deepseek-v4-flash-0731"
reasoning = "off"      # off | low | medium | high, or a token budget: 1000
```

Each agent here says what its work is worth: the flash worker answering weather
questions in one line turns thinking **off**, the GLM analyst gets **high**
effort, extraction takes a **1000-token** budget, and the orchestrator — whose
whole job is picking a tool — takes **low**. Omitted, nothing is sent and the
model's own default applies. A request can still override the config in code
(`LlmRequest::reasoning`), which is how `examples/complex_agent.rs` streams its
thinking regardless of how the model was configured.

## Run it

```bash
export OPENROUTER_API_KEY=...     # or put it in a .env file

# from the a2a-agents crate directory
cargo build -p a2a-agents --bin a2a

# will this machine run them? (ports free, key set, models reachable)
cargo run -p a2a-agents --bin a2a -- doctor --fleet examples/multi-model/fleet.toml

# run all four in one process
cargo run -p a2a-agents --bin a2a -- up -f examples/multi-model/fleet.toml
```

Do **not** set a process-wide `OPENROUTER_MODEL` — see gotcha 1 below.

Then talk to a specialist directly, or let the orchestrator pick one:

```bash
# straight to the cheap model
a2acli send --url http://127.0.0.1:8081 "What's the weather in Bergen tomorrow?"

# the orchestrator routes it — watch the fleet log for `ask_glm_analyst`
a2acli send --url http://127.0.0.1:8090 "Summarize these Q3 numbers: 12, 40, 38, 91"
```

## ⚠️ Gotchas to keep in mind

1. **`OPENROUTER_MODEL` is global.** `resolve_llm` prefers each agent's `[llm]`,
   but a member whose `[llm]` is missing or invalid falls back to
   `provider_from_env()`, which reads the *process-wide* `OPENROUTER_MODEL`.
   The whole fleet shares one process environment, so a single stray env var
   quietly moves every such member onto the same model. **Pin `model` in every
   agent's `[llm]`; do not drive a fleet from `OPENROUTER_MODEL`.**

2. **Reasoning is per model, and it costs money — set it.** All three models
   here support reasoning, and asking a flash model to think hard about a
   one-line weather answer is billed thinking you did not want. Worse, a small
   model can spend its *whole* response budget on reasoning and emit no answer
   at all; that settles the task as **failed** naming the cause, rather than
   completing with an empty reply (indistinguishable on the wire from a file
   part — proto3 omits the empty string, so a client renders
   `[non-text content]`).

   Omit `reasoning` and nothing is sent, so the model's own default stands —
   which for a reasoning model still means it reasons. Say what each workload is
   worth instead, as these four do (see "Reasoning per agent" above).

3. **Delegation is by skill, resolved at startup.** The orchestrator resolves
   its `skill` references against the shared in-memory registry once, at
   startup, so all three workers must be up before it connects — they are, in
   the same fleet process. See `TODO.md` (resolve peers at call time) for the
   known limitation.

4. **Multimodal is a capability here, not a demo.** MiniMax M3 accepts image and
   video input, but `LlmHandler` feeds it text parts only (`extract_text` in
   `handlers/llm.rs`). This example shows the skill/model split; wiring real
   media parts through is future work.

5. **Model IDs and prices move.** Both were read from OpenRouter on 2026-08-13.
   Re-check with `curl https://openrouter.ai/api/v1/models`.

## Files

- `flash_worker.toml` — DeepSeek Flash worker (`weather-lookup`)
- `glm_analyst.toml` — GLM 5.2 worker (`data-analysis`)
- `minimax_extract.toml` — MiniMax M3 worker (`content-extraction`)
- `orchestrator.toml` — skill-routing orchestrator
- `fleet.toml` — runs all four in one process
