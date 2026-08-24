# a2a-llm

Provider-neutral vocabulary for chat completions, plus the providers that speak
it.

```toml
[dependencies]
a2a-llm = "0.1"
```

## What it is

`LlmProvider` is the port — `chat_completion` and `chat_completion_stream` over
`LlmRequest` / `LlmResponse`:

- **`openai`** covers OpenAI and every OpenAI-compatible endpoint (OpenRouter,
  vLLM, llama.cpp).
- **`gemini`** covers Google's API.
- **`provider_from_env`** picks one from the environment;
  `provider_from_settings` picks one from config a host already parsed.

`SUPPORTED_PROVIDERS` is `["openrouter", "openai", "gemini"]`, and
`PROVIDER_ENV_VARS` is the selection order — public so a host can print it in a
diagnostic instead of keeping its own copy that drifts.

```rust
use a2a_llm::{LlmProvider, LlmRequest, ChatMessage, provider_from_env};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let selected = provider_from_env()?.expect("no provider key in the environment");
let response = selected
    .provider
    .chat_completion(LlmRequest::new(vec![ChatMessage::user("hello")]))
    .await?;
# Ok(())
# }
```

## Why it is its own crate

The types are deliberately not tied to A2A. `ToolCall` and `ToolDefinition` are
the tool-calling vocabulary shared with the MCP bridge, and `a2a-mcp` needed
exactly those two out of what used to be a 5.3k-line agent-framework crate.
Splitting them out means the bridge does not depend on an agent framework to
name a tool call.

It is also the reason this half is MIT and commodity: LLM provider plumbing is
not where the value is.

## Selection is I/O-free, and says what it dropped

`provider_from_env` and `provider_from_settings` perform no network calls, so a
pre-flight check can run the same code that startup will run and report the same
answer. The one thing they report is what a configured `reasoning` will do:
`SelectedLlm` carries a `ReasoningPlan`, and `ReasoningPlan::Unsupported` names a
setting dropped before any request rather than letting it be discovered on the
bill. There is one of those — a token budget on OpenAI, whose Chat Completions
API has no field for one.

## Reasoning is sent, and a refusal is recovered from

Every provider carries `reasoning`, each in its own dialect: OpenRouter's
`reasoning` object, OpenAI's `reasoning_effort`, Gemini's
`generationConfig.thinkingConfig`. Whether a given *model* accepts it is another
matter — `reasoning_effort` is a 400 on `gpt-4o-mini` and mandatory on
`gpt-5-pro` — and a table of model names is wrong about every model released
after it was written. So the parameter is sent, a refusal is recognized from the
400 that names the field, and the request is retried once without it. A 400
generated nothing, so that costs a round trip and no tokens, and the answer is
remembered for the life of the provider. `ReasoningPlan::Attempted` is what
selection reports for that: sent, with the model getting the last word.

A variable set to whitespace reads as unset — `.env` files leave those behind,
and an empty `OPENROUTER_API_KEY` would otherwise select a provider that cannot
authenticate.

## License

MIT. See [LICENSE](./LICENSE).
