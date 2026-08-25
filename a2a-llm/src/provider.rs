//! Centralized [`LlmProvider`] selection.
//!
//! This is the single place agents pick a concrete LLM provider, replacing the
//! ad-hoc `GeminiProvider::from_env()` / `OpenAiProvider::from_env()` cascades
//! that used to be copy-pasted across handlers, examples, and the CLI.
//!
//! Two entry points:
//! - [`provider_from_env`] — env-driven selection (OpenRouter → Gemini → OpenAI).
//! - [`provider_from_settings`] — config-driven selection from [`LlmSettings`].
//!
//! Both separate "nothing is configured" (`Ok(None)`) from "what is configured
//! does not work" ([`LlmConfigError`]). These used to both return `None`, so a
//! mistyped key started the agent anyway and it answered from its non-LLM
//! fallback.
//!
//! Selection performs no I/O, so `korps doctor` can run the same code as startup
//! to decide what startup will do. The only output is a warning when a
//! configured `reasoning` has no field to go in on the selected provider — the
//! cases the model decides are the provider's to report, and only at run time.
//!
//! Settings are expressed with this crate's own [`LlmSettings`] type rather than
//! a host's config struct so the helper takes no dependency on `korps`
//! (which would be circular).

use std::sync::Arc;

use super::{
    Env, LlmProvider, Reasoning,
    gemini::{GEMINI_BASE_URL, GeminiConfig, GeminiProvider},
    openai::{
        OPENAI_BASE_URL, OPENROUTER_DEFAULT_MODEL, OpenAiConfig, OpenAiProvider, ReasoningDialect,
        reasoning_effort,
    },
};

/// Provider-agnostic LLM settings sourced from a host's configuration
/// (TOML, CLI flags, etc.). Mirrors the fields a host typically exposes.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct LlmSettings {
    /// Provider selector: `"openrouter"`, `"openai"`, or `"gemini"`.
    pub provider: String,
    /// API key. When `None`, the provider's own environment variable is read
    /// instead.
    pub api_key: Option<String>,
    /// Model identifier. Environment, then a provider-specific default, applied
    /// when `None`.
    pub model: Option<String>,
    /// Base URL override. Environment, then a provider-specific default,
    /// applied when `None`.
    pub base_url: Option<String>,
    /// OpenRouter `HTTP-Referer` attribution header (ignored by other providers).
    pub http_referer: Option<String>,
    /// OpenRouter `X-Title` attribution header (ignored by other providers).
    pub x_title: Option<String>,
    /// What to ask this model to do with its thinking, for every request that
    /// doesn't ask for its own. `None` leaves the model's default alone.
    ///
    /// Every provider carries it now, in its own dialect. What differs is when
    /// the answer is known: OpenRouter takes any of it
    /// ([`ReasoningPlan::Sent`]), OpenAI has no field for a token budget
    /// ([`ReasoningPlan::Unsupported`]), and elsewhere it is the model that
    /// accepts or refuses, which only the first call finds out
    /// ([`ReasoningPlan::Attempted`]).
    pub reasoning: Option<Reasoning>,
    /// Whether the endpoint accepts `stream_options.include_usage`, which is
    /// what makes a *streaming* response report what it cost.
    ///
    /// `None` leaves it to the endpoint: on for OpenRouter and OpenAI's own
    /// URL, off elsewhere, because a local OpenAI-compatible server that
    /// rejects unknown parameters fails the whole call. Set it for a server
    /// this crate has no way to recognize — a proxy in front of OpenAI, or a
    /// self-hosted vLLM that does support it.
    pub stream_usage: Option<bool>,
}

impl std::fmt::Debug for LlmSettings {
    /// Hand-written to keep `api_key` out of the output. These settings travel
    /// inside a `doctor` requirement, so anything that prints one would print
    /// the key with it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmSettings")
            .field("provider", &self.provider)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .field("http_referer", &self.http_referer)
            .field("x_title", &self.x_title)
            .field("reasoning", &self.reasoning)
            .field("stream_usage", &self.stream_usage)
            .finish()
    }
}

/// Every provider [`provider_from_settings`] can build, for the error message
/// that lists them.
pub const SUPPORTED_PROVIDERS: [&str; 3] = ["openrouter", "openai", "gemini"];

/// Every environment variable that can select a provider, in the order
/// [`provider_from_env`] prefers them. Public so a host can list them in a
/// report instead of keeping its own copy.
pub const PROVIDER_ENV_VARS: [&str; 6] = [
    "OPENROUTER_API_KEY",
    "GEMINI_API_KEY",
    "OPENAI_API_KEY",
    "AI_API_KEY",
    "OPENAI_API_BASE_URL",
    "AI_API_BASE_URL",
];

/// How the config path names itself in an error, where the env path names the
/// variable it read.
const SELECTED_BY_CONFIG: &str = "`[llm] provider`";

/// Whether to ask a streaming request to report what it cost.
///
/// The config decides when it says anything. Otherwise the endpoint does, and
/// the only endpoint whose support is *known* rather than guessed is OpenAI's
/// own — everything else on this branch is a local OpenAI-compatible server,
/// which may reject the parameter and fail the whole call.
///
/// The URL is compared with a trailing slash trimmed: `https://api.openai.com/v1/`
/// names the same endpoint, and a plain string comparison quietly decides it is
/// some other server.
fn stream_usage_for(settings: &LlmSettings, base_url: &str) -> bool {
    settings
        .stream_usage
        .unwrap_or_else(|| base_url.trim_end_matches('/') == OPENAI_BASE_URL)
}

/// A provider is named but cannot be built.
///
/// Raised before any request is made, so a host can refuse to start instead of
/// failing on the first message. Errors from calling a provider are
/// [`LlmError`](super::LlmError).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LlmConfigError {
    /// A provider is named and its settings cannot be read: a missing key, a
    /// malformed `OPENROUTER_REASONING`.
    #[error("the {provider} provider is selected by {selected_by}, but is not usable: {detail}")]
    Unusable {
        /// Which provider was selected.
        provider: &'static str,
        /// What selected it — an environment variable, or `[llm] provider`.
        selected_by: &'static str,
        /// What is wrong with its settings.
        detail: String,
    },
    /// [`LlmSettings::provider`] names something no adapter implements —
    /// usually a typo.
    #[error("unsupported LLM provider {name:?}; expected one of: {}", SUPPORTED_PROVIDERS.join(", "))]
    Unsupported {
        /// The provider string as configured.
        name: String,
    },
}

impl LlmConfigError {
    fn unusable(
        provider: &'static str,
        selected_by: &'static str,
        detail: impl Into<String>,
    ) -> Self {
        Self::Unusable {
            provider,
            selected_by,
            detail: detail.into(),
        }
    }
}

/// What a configured [`Reasoning`] will do on the provider that was selected.
///
/// Reasoning tokens are billed, so "asked for, and this provider has nowhere to
/// put it" has to be tellable from "never asked for". Both used to resolve to
/// the same `None` on [`SelectedLlm`], which left `korps doctor` unable to report
/// a setting the run would quietly discard.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReasoningPlan {
    /// Nothing was configured; the model's own default stands.
    #[default]
    Unset,
    /// Configured, and this provider puts it on the wire.
    Sent(Reasoning),
    /// Configured, and this provider sends it — but whether the *model* accepts
    /// it is not known until the first call.
    ///
    /// OpenAI's `reasoning_effort` and Gemini's `thinkingConfig` are refused by
    /// the models that do not reason, and which models those are changes with
    /// every release. So the parameter is sent, a refusal is recognized from the
    /// 400, and the request is retried without it — meaning this setting may
    /// still end up dropped, and only the run knows.
    Attempted(Reasoning),
    /// Configured, and this provider has no field that carries it. Dropped
    /// before any request is made.
    Unsupported(Reasoning),
}

impl ReasoningPlan {
    /// The plan for a provider that can carry whatever was configured.
    pub fn carried(reasoning: Option<Reasoning>) -> Self {
        reasoning.map_or(Self::Unset, Self::Sent)
    }

    /// The plan for a provider that sends it and finds out from the model.
    pub fn attempted(reasoning: Option<Reasoning>) -> Self {
        reasoning.map_or(Self::Unset, Self::Attempted)
    }

    /// The plan for a provider with no reasoning field.
    pub fn dropped(reasoning: Option<Reasoning>) -> Self {
        reasoning.map_or(Self::Unset, Self::Unsupported)
    }

    /// What reaches the wire, if anything. A setting dropped before the request
    /// reads as nothing here, because nothing is what gets sent; an attempted
    /// one reads as sent, because it is on the first request.
    pub fn sent(self) -> Option<Reasoning> {
        match self {
            Self::Sent(reasoning) | Self::Attempted(reasoning) => Some(reasoning),
            Self::Unset | Self::Unsupported(_) => None,
        }
    }

    /// What was configured, whether or not it will be sent.
    pub fn requested(self) -> Option<Reasoning> {
        match self {
            Self::Unset => None,
            Self::Sent(reasoning) | Self::Attempted(reasoning) | Self::Unsupported(reasoning) => {
                Some(reasoning)
            }
        }
    }

    /// What this provider was asked for and will not send. Only the settings
    /// answered before a request is made — see [`Self::attempted`] for the ones
    /// the model gets the last word on.
    pub fn unsupported(self) -> Option<Reasoning> {
        match self {
            Self::Unsupported(reasoning) => Some(reasoning),
            Self::Unset | Self::Sent(_) | Self::Attempted(_) => None,
        }
    }

    /// What this provider will send and may still have refused. `None` when the
    /// answer is already known either way.
    pub fn attempting(self) -> Option<Reasoning> {
        match self {
            Self::Attempted(reasoning) => Some(reasoning),
            Self::Unset | Self::Sent(_) | Self::Unsupported(_) => None,
        }
    }
}

impl std::fmt::Display for ReasoningPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unset => f.write_str("(model default)"),
            Self::Sent(reasoning) => write!(f, "{reasoning}"),
            Self::Attempted(reasoning) => write!(f, "{reasoning} (if the model takes it)"),
            Self::Unsupported(reasoning) => write!(f, "{reasoning} (dropped)"),
        }
    }
}

/// A resolved provider plus what a startup line or `korps doctor` needs to
/// describe it.
///
/// [`model`](Self::model) is resolved here because a config often leaves it to
/// the environment, and `OPENROUTER_MODEL` is per process: a fleet that omits
/// it runs every agent on the same model.
#[derive(Clone)]
pub struct SelectedLlm {
    /// The provider adapter, ready to use.
    pub provider: Arc<dyn LlmProvider>,
    /// Which adapter it is — one of [`SUPPORTED_PROVIDERS`].
    pub kind: &'static str,
    /// The model it will call, after config and environment defaults.
    pub model: String,
    /// What selected it: an environment variable name, or `[llm] provider`.
    pub selected_by: &'static str,
    /// What will be asked of the model's thinking on requests that don't ask
    /// for their own, and whether this provider can ask it at all.
    pub reasoning: ReasoningPlan,
}

impl std::fmt::Debug for SelectedLlm {
    /// Hand-written because `dyn LlmProvider` is not `Debug`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectedLlm")
            .field("kind", &self.kind)
            .field("model", &self.model)
            .field("selected_by", &self.selected_by)
            .field("reasoning", &self.reasoning)
            .finish_non_exhaustive()
    }
}

/// Select a provider from the environment.
///
/// Preference order, each gated on a *present* key:
///
/// 1. **OpenRouter** when `OPENROUTER_API_KEY` is set.
/// 2. **Gemini** when `GEMINI_API_KEY` is set.
/// 3. **OpenAI-compatible** when any of `OPENAI_API_KEY`, `AI_API_KEY`,
///    `OPENAI_API_BASE_URL`, or `AI_API_BASE_URL` is set (covers local Ollama).
///
/// `Ok(None)` means no variable names a provider; the host should use its
/// non-LLM fallback. `Err` means one does and could not be built — a failing
/// provider does not fall through to the next, since substituting a different
/// model (and a different bill) for the one that was asked for is worse than
/// failing.
pub fn provider_from_env() -> Result<Option<SelectedLlm>, LlmConfigError> {
    select_from_env(Env::os())
}

fn select_from_env(env: Env<'_>) -> Result<Option<SelectedLlm>, LlmConfigError> {
    let [openrouter_key, gemini_key, openai_vars @ ..] = PROVIDER_ENV_VARS;

    if env.get(openrouter_key).is_some() {
        let config = OpenAiConfig::openrouter_from_lookup(env)
            .map_err(|detail| LlmConfigError::unusable("openrouter", openrouter_key, detail))?;
        return Ok(Some(SelectedLlm {
            kind: "openrouter",
            model: config.model.clone(),
            selected_by: openrouter_key,
            reasoning: ReasoningPlan::carried(config.reasoning),
            provider: Arc::new(OpenAiProvider::new(config)),
        }));
    }

    if env.get(gemini_key).is_some() {
        let config = GeminiConfig::from_lookup(env)
            .map_err(|detail| LlmConfigError::unusable("gemini", gemini_key, detail))?;
        return Ok(Some(SelectedLlm {
            kind: "gemini",
            model: config.model.clone(),
            selected_by: gemini_key,
            reasoning: ReasoningPlan::Unset,
            provider: Arc::new(GeminiProvider::new(config)),
        }));
    }

    if let Some(var) = openai_vars.into_iter().find(|var| env.get(var).is_some()) {
        let config = OpenAiConfig::from_lookup(env);
        return Ok(Some(SelectedLlm {
            kind: "openai",
            model: config.model.clone(),
            selected_by: var,
            reasoning: ReasoningPlan::Unset,
            provider: Arc::new(OpenAiProvider::new(config)),
        }));
    }

    Ok(None)
}

/// Warn that a configured token budget has nowhere to go on OpenAI.
///
/// The one reasoning drop selection can still report: every other case is the
/// model's answer, given at run time. Reasoning tokens are billed, so this says
/// it at startup rather than leaving the caller to infer it from the bill.
/// `korps run` has only this log line; a report reads
/// [`SelectedLlm::reasoning`] instead.
fn warn_budget_has_no_openai_field(reasoning: Reasoning) {
    tracing::warn!(
        provider = "openai",
        %reasoning,
        "the OpenAI chat-completions API has no field for a reasoning token budget; ignoring it"
    );
}

/// Build a provider from explicit [`LlmSettings`].
///
/// Resolution order for every value: the config, then the provider's own
/// environment variables, then a built-in default. So a `[llm]` block with no
/// `api_key` reads the key from the environment, which is how the shipped
/// examples are written. (Previously only `openrouter` did this; `gemini` used
/// an empty key and `openai` used none, and both failed at the endpoint.)
pub fn provider_from_settings(settings: &LlmSettings) -> Result<SelectedLlm, LlmConfigError> {
    build_from_settings(settings, Env::os())
}

fn build_from_settings(
    settings: &LlmSettings,
    env: Env<'_>,
) -> Result<SelectedLlm, LlmConfigError> {
    /// Config first, then the environment. An empty or whitespace-only
    /// configured value counts as absent.
    fn or_env(configured: &Option<String>, env: Env<'_>, keys: &[&str]) -> Option<String> {
        configured
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| keys.iter().find_map(|key| env.get(key)))
    }

    match settings.provider.as_str() {
        "openrouter" => {
            let api_key =
                or_env(&settings.api_key, env, &["OPENROUTER_API_KEY"]).ok_or_else(|| {
                    LlmConfigError::unusable(
                        "openrouter",
                        SELECTED_BY_CONFIG,
                        "no `api_key` in the config and OPENROUTER_API_KEY is not set",
                    )
                })?;
            let model = or_env(&settings.model, env, &["OPENROUTER_MODEL"])
                .unwrap_or_else(|| OPENROUTER_DEFAULT_MODEL.to_string());
            let mut config = OpenAiConfig {
                reasoning: settings.reasoning,
                ..OpenAiConfig::openrouter(
                    api_key,
                    model.clone(),
                    or_env(&settings.base_url, env, &["OPENROUTER_API_BASE_URL"]),
                    or_env(&settings.http_referer, env, &["OPENROUTER_HTTP_REFERER"]),
                    or_env(&settings.x_title, env, &["OPENROUTER_X_TITLE"]),
                )
            };
            // OpenRouter takes `stream_options`, but a proxy in front of it may
            // not — the config gets the last word wherever it has one.
            if let Some(stream_usage) = settings.stream_usage {
                config.stream_usage = stream_usage;
            }
            Ok(SelectedLlm {
                kind: "openrouter",
                model,
                selected_by: SELECTED_BY_CONFIG,
                reasoning: ReasoningPlan::carried(settings.reasoning),
                provider: Arc::new(OpenAiProvider::new(config)),
            })
        }
        "openai" => {
            // No key is a valid OpenAI-compatible setup (a local Ollama), so
            // there is nothing to require here — and nothing to report either.
            let model = or_env(&settings.model, env, &["OPENAI_MODEL", "AI_MODEL"])
                .unwrap_or_else(|| "gpt-4o-mini".to_string());
            let base_url = or_env(
                &settings.base_url,
                env,
                &["OPENAI_API_BASE_URL", "AI_API_BASE_URL"],
            )
            .unwrap_or_else(|| OPENAI_BASE_URL.to_string());
            let config = OpenAiConfig {
                // `stream_options.include_usage` is known to work on OpenAI's own
                // endpoint. This branch also serves local OpenAI-compatible
                // servers, which vary on it and reject unknown parameters
                // outright, and nothing here can tell them apart beyond the URL —
                // so a config that knows better says so.
                stream_usage: stream_usage_for(settings, &base_url),
                base_url,
                model: model.clone(),
                api_key: or_env(&settings.api_key, env, &["OPENAI_API_KEY", "AI_API_KEY"]),
                extra_headers: Vec::new(),
                reasoning_dialect: ReasoningDialect::OpenAi,
                reasoning: settings.reasoning,
            };
            // A token budget has no `reasoning_effort` to go in, so that one is
            // answered here; a level is the model's to accept or refuse.
            let plan = match settings.reasoning {
                Some(reasoning) if reasoning_effort(reasoning).is_none() => {
                    warn_budget_has_no_openai_field(reasoning);
                    ReasoningPlan::Unsupported(reasoning)
                }
                reasoning => ReasoningPlan::attempted(reasoning),
            };
            Ok(SelectedLlm {
                kind: "openai",
                model,
                selected_by: SELECTED_BY_CONFIG,
                reasoning: plan,
                provider: Arc::new(OpenAiProvider::new(config)),
            })
        }
        "gemini" => {
            // The key first, so a config supplying neither reports the
            // credential rather than the model: without a key nothing about
            // this provider is reachable, and naming the model would send a
            // caller to fix the second thing wrong with it.
            let api_key = or_env(&settings.api_key, env, &["GEMINI_API_KEY"]).ok_or_else(|| {
                LlmConfigError::unusable(
                    "gemini",
                    SELECTED_BY_CONFIG,
                    "no `api_key` in the config and GEMINI_API_KEY is not set",
                )
            })?;
            // The one provider with no default model, for the reason
            // `gemini.rs` gives: the default it used to carry named a model
            // Google stopped listing, and a stale default is invisible in a way
            // a missing one is not. `openai` and `openrouter` keep theirs,
            // which still name models their vendors list.
            let model = or_env(&settings.model, env, &["GEMINI_MODEL"]).ok_or_else(|| {
                LlmConfigError::unusable(
                    "gemini",
                    SELECTED_BY_CONFIG,
                    "no `model` in the config and GEMINI_MODEL is not set",
                )
            })?;
            let config = GeminiConfig {
                base_url: or_env(&settings.base_url, env, &["GEMINI_API_BASE_URL"])
                    .unwrap_or_else(|| GEMINI_BASE_URL.to_string()),
                api_key,
                model: model.clone(),
                reasoning: settings.reasoning,
            };
            Ok(SelectedLlm {
                kind: "gemini",
                model,
                selected_by: SELECTED_BY_CONFIG,
                // Every `Reasoning` has a `thinkingConfig` spelling, so nothing
                // is dropped here — the model decides.
                reasoning: ReasoningPlan::attempted(settings.reasoning),
                provider: Arc::new(GeminiProvider::new(config)),
            })
        }
        other => Err(LlmConfigError::Unsupported {
            name: other.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReasoningEffort;

    /// A fake environment. Mutating the real one would race the other tests in
    /// this binary (`set_var` is `unsafe` in edition 2024 for that reason).
    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |key| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    /// Streaming usage is opt-in per endpoint because a server that rejects
    /// unknown parameters fails the whole call — but the same endpoint written
    /// with a trailing slash used to silently lose it.
    #[test]
    fn stream_usage_follows_the_endpoint_when_nothing_says_otherwise() {
        let settings = LlmSettings::default();
        assert!(stream_usage_for(&settings, OPENAI_BASE_URL));
        assert!(stream_usage_for(&settings, &format!("{OPENAI_BASE_URL}/")));
        assert!(!stream_usage_for(&settings, "http://localhost:11434/v1"));
    }

    /// The point of the field: a server this crate cannot recognize — a proxy
    /// in front of OpenAI, a self-hosted vLLM — is the config's to describe.
    #[test]
    fn a_configured_stream_usage_wins_in_both_directions() {
        let on = LlmSettings {
            stream_usage: Some(true),
            ..LlmSettings::default()
        };
        assert!(stream_usage_for(&on, "https://llm.internal/v1"));

        let off = LlmSettings {
            stream_usage: Some(false),
            ..LlmSettings::default()
        };
        assert!(!stream_usage_for(&off, OPENAI_BASE_URL));
    }

    #[test]
    fn nothing_configured_is_not_a_failure() {
        assert!(select_from_env(Env::new(&env_of(&[]))).unwrap().is_none());
    }

    #[test]
    fn a_key_selects_its_provider_and_reports_the_model() {
        let selected = select_from_env(Env::new(&env_of(&[
            ("OPENROUTER_API_KEY", "sk-or-test"),
            ("OPENROUTER_MODEL", "minimax/minimax-m2"),
        ])))
        .expect("a usable key is not an error")
        .expect("a key selects a provider");
        assert_eq!(selected.kind, "openrouter");
        assert_eq!(selected.selected_by, "OPENROUTER_API_KEY");
        assert_eq!(selected.model, "minimax/minimax-m2");
    }

    /// A key that is set but unusable must not report as "unconfigured": the
    /// host reads that as "use the non-LLM fallback" and the agent answers with
    /// a stub.
    #[test]
    fn a_broken_setting_is_an_error_not_an_absence() {
        let error = select_from_env(Env::new(&env_of(&[
            ("OPENROUTER_API_KEY", "sk-or-test"),
            ("OPENROUTER_REASONING", "verry-high"),
        ])))
        .expect_err("a malformed OPENROUTER_REASONING is a failure");
        assert!(
            matches!(&error, LlmConfigError::Unusable { provider, selected_by, .. }
                if *provider == "openrouter" && *selected_by == "OPENROUTER_API_KEY"),
            "{error}"
        );
        assert!(
            error.to_string().contains("OPENROUTER_REASONING"),
            "{error}"
        );
    }

    /// A broken first choice fails rather than falling through: the operator
    /// asked for OpenRouter, and billing Gemini instead is not a fix.
    #[test]
    fn a_broken_provider_does_not_fall_through_to_the_next() {
        let error = select_from_env(Env::new(&env_of(&[
            ("OPENROUTER_API_KEY", "sk-or-test"),
            ("OPENROUTER_REASONING", "verry-high"),
            ("GEMINI_API_KEY", "gemini-key"),
        ])))
        .expect_err("the broken first choice wins over the working second");
        assert!(error.to_string().contains("openrouter"), "{error}");
    }

    /// `.env` files leave whitespace-only values behind. Treating one as set
    /// selects a provider that cannot authenticate.
    #[test]
    fn a_blank_key_does_not_select_a_provider() {
        assert!(
            select_from_env(Env::new(&env_of(&[("OPENROUTER_API_KEY", "   ")])))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_base_url_alone_selects_the_openai_compatible_provider() {
        let selected = select_from_env(Env::new(&env_of(&[(
            "OPENAI_API_BASE_URL",
            "http://localhost:11434/v1",
        )])))
        .unwrap()
        .expect("a base URL is enough for a local server");
        assert_eq!(selected.kind, "openai");
        assert_eq!(selected.selected_by, "OPENAI_API_BASE_URL");
    }

    /// A config with no `api_key` reads the key from the environment. It used to
    /// build an empty key, which failed only on the first message.
    #[test]
    fn settings_without_a_key_read_the_environment() {
        let settings = LlmSettings {
            provider: "gemini".to_string(),
            ..Default::default()
        };
        let env = env_of(&[
            ("GEMINI_API_KEY", "gemini-key"),
            ("GEMINI_MODEL", "gemini-3-pro"),
        ]);
        let selected =
            build_from_settings(&settings, Env::new(&env)).expect("the env supplies the key");
        assert_eq!(selected.kind, "gemini");
        assert_eq!(selected.model, "gemini-3-pro");

        let error = build_from_settings(&settings, Env::new(&env_of(&[])))
            .expect_err("no key anywhere is a failure, not an empty key");
        assert!(error.to_string().contains("GEMINI_API_KEY"), "{error}");
    }

    /// Gemini is the one provider that names no default model. The default it
    /// used to carry was `gemini-1.5-pro`, which Google stopped listing while
    /// every config that omitted a model went on running it — so the absence
    /// has to be an error a caller sees, not a value this crate invents. Both
    /// entry points have to agree, since a config and a bare environment reach
    /// the provider by different code.
    #[test]
    fn gemini_names_no_default_model() {
        let settings = LlmSettings {
            provider: "gemini".to_string(),
            api_key: Some("gemini-key".to_string()),
            ..Default::default()
        };
        let error = build_from_settings(&settings, Env::new(&env_of(&[])))
            .expect_err("a key without a model cannot pick one");
        assert!(error.to_string().contains("GEMINI_MODEL"), "{error}");

        // The environment supplies it just as the config would.
        let selected = build_from_settings(
            &settings,
            Env::new(&env_of(&[("GEMINI_MODEL", "gemini-2.5-pro")])),
        )
        .expect("the env names the model");
        assert_eq!(selected.model, "gemini-2.5-pro");

        // Selection from a bare environment is the other path in, and a key on
        // its own is enough to *choose* gemini — so it must fail here too
        // rather than fall through to another provider or a made-up model.
        let error = select_from_env(Env::new(&env_of(&[("GEMINI_API_KEY", "gemini-key")])))
            .expect_err("a key selects gemini, which then has no model");
        assert!(error.to_string().contains("GEMINI_MODEL"), "{error}");

        let selected = select_from_env(Env::new(&env_of(&[
            ("GEMINI_API_KEY", "gemini-key"),
            ("GEMINI_MODEL", "gemini-2.5-pro"),
        ])))
        .unwrap()
        .expect("both halves present");
        assert_eq!(selected.kind, "gemini");
        assert_eq!(selected.model, "gemini-2.5-pro");
    }

    #[test]
    fn the_config_wins_over_the_environment() {
        let settings = LlmSettings {
            provider: "openrouter".to_string(),
            api_key: Some("sk-or-config".to_string()),
            model: Some("z-ai/glm-5.2".to_string()),
            reasoning: Some(Reasoning::Effort(ReasoningEffort::Low)),
            ..Default::default()
        };
        let selected = build_from_settings(
            &settings,
            Env::new(&env_of(&[("OPENROUTER_MODEL", "some/other-model")])),
        )
        .expect("a configured key needs no environment");
        assert_eq!(selected.model, "z-ai/glm-5.2");
        assert_eq!(selected.selected_by, SELECTED_BY_CONFIG);
        assert_eq!(
            selected.reasoning,
            ReasoningPlan::Sent(Reasoning::Effort(ReasoningEffort::Low))
        );
    }

    /// A level reaches every provider now, but only OpenRouter's answer is known
    /// before the call: the other two send it and find out from the model.
    #[test]
    fn a_level_is_carried_by_openrouter_and_attempted_elsewhere() {
        let plan_for = |provider: &str| {
            let settings = LlmSettings {
                provider: provider.to_string(),
                api_key: Some("key".to_string()),
                // Named because `gemini` has no default to fall back on; the
                // subject here is the plan, not where the model came from.
                model: Some("a-model".to_string()),
                reasoning: Some(Reasoning::Effort(ReasoningEffort::High)),
                ..Default::default()
            };
            build_from_settings(&settings, Env::new(&env_of(&[])))
                .unwrap()
                .reasoning
        };

        let high = Reasoning::Effort(ReasoningEffort::High);
        assert_eq!(plan_for("openrouter"), ReasoningPlan::Sent(high));
        for provider in ["openai", "gemini"] {
            let plan = plan_for(provider);
            assert_eq!(plan, ReasoningPlan::Attempted(high), "for {provider}");
            // On the wire on the first call, so `sent` says so; and not a drop,
            // so `korps doctor` does not warn about a setting that may well work.
            assert_eq!(plan.sent(), Some(high), "for {provider}");
            assert_eq!(plan.unsupported(), None, "for {provider}");
            assert_eq!(plan.attempting(), Some(high), "for {provider}");
        }
    }

    /// A token budget is the one setting answered without asking a model:
    /// `reasoning_effort` has no field for it, so `korps doctor` can still warn
    /// before a request is billed. Gemini has `thinkingBudget` and takes it.
    #[test]
    fn a_token_budget_is_a_drop_on_openai_and_carried_on_gemini() {
        let plan_for = |provider: &str| {
            let settings = LlmSettings {
                provider: provider.to_string(),
                api_key: Some("key".to_string()),
                model: Some("a-model".to_string()),
                reasoning: Some(Reasoning::Budget(2000)),
                ..Default::default()
            };
            build_from_settings(&settings, Env::new(&env_of(&[])))
                .unwrap()
                .reasoning
        };

        let openai = plan_for("openai");
        assert_eq!(openai, ReasoningPlan::Unsupported(Reasoning::Budget(2000)));
        assert_eq!(openai.sent(), None);
        assert_eq!(openai.requested(), Some(Reasoning::Budget(2000)));

        assert_eq!(
            plan_for("gemini"),
            ReasoningPlan::Attempted(Reasoning::Budget(2000))
        );
    }

    /// Nothing configured stays nothing. A plan that reported a drop here would
    /// have `doctor` warning about a setting no config contains.
    #[test]
    fn no_reasoning_configured_is_not_a_drop() {
        for provider in SUPPORTED_PROVIDERS {
            let settings = LlmSettings {
                provider: provider.to_string(),
                api_key: Some("key".to_string()),
                model: Some("a-model".to_string()),
                ..Default::default()
            };
            let selected = build_from_settings(&settings, Env::new(&env_of(&[]))).unwrap();
            assert_eq!(selected.reasoning, ReasoningPlan::Unset, "for {provider}");
            assert_eq!(selected.reasoning.unsupported(), None, "for {provider}");
        }
    }

    /// The error lists the valid providers, since the usual cause is a typo.
    /// These settings ride inside a `korps doctor` requirement, and a report or a
    /// test failure that prints one must not print the key.
    #[test]
    fn debug_output_redacts_the_api_key() {
        let settings = LlmSettings {
            provider: "openrouter".to_string(),
            api_key: Some("sk-or-supersecret".to_string()),
            ..Default::default()
        };
        let printed = format!("{settings:?}");
        assert!(!printed.contains("supersecret"), "{printed}");
        assert!(printed.contains("redacted"), "{printed}");
    }

    #[test]
    fn an_unknown_provider_names_the_ones_that_exist() {
        let settings = LlmSettings {
            provider: "opnrouter".to_string(),
            ..Default::default()
        };
        let error =
            build_from_settings(&settings, Env::new(&env_of(&[]))).expect_err("no such provider");
        assert!(matches!(&error, LlmConfigError::Unsupported { name } if name == "opnrouter"));
        for provider in SUPPORTED_PROVIDERS {
            assert!(error.to_string().contains(provider), "{error}");
        }
    }
}
