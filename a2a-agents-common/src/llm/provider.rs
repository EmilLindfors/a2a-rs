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
//! Selection performs no I/O, so `a2a doctor` can run the same code as startup
//! to decide what startup will do. The only output is a warning when a
//! configured `reasoning` cannot reach the provider's wire.
//!
//! Settings are expressed with this crate's own [`LlmSettings`] type rather than
//! a host's config struct so the helper takes no dependency on `a2a-agents`
//! (which would be circular).

use std::sync::Arc;

use super::{
    Env, LlmProvider, Reasoning,
    gemini::{GEMINI_BASE_URL, GEMINI_DEFAULT_MODEL, GeminiConfig, GeminiProvider},
    openai::{OPENROUTER_DEFAULT_MODEL, OpenAiConfig, OpenAiProvider},
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
    /// OpenRouter only today; elsewhere it is dropped, and the resulting
    /// [`SelectedLlm::reasoning`] says so as [`ReasoningPlan::Unsupported`].
    pub reasoning: Option<Reasoning>,
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
/// the same `None` on [`SelectedLlm`], which left `a2a doctor` unable to report
/// a setting the run would quietly discard.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReasoningPlan {
    /// Nothing was configured; the model's own default stands.
    #[default]
    Unset,
    /// Configured, and this provider puts it on the wire.
    Sent(Reasoning),
    /// Configured, and this provider has no field that carries it. Dropped.
    Unsupported(Reasoning),
}

impl ReasoningPlan {
    /// The plan for a provider that can carry whatever was configured.
    pub fn carried(reasoning: Option<Reasoning>) -> Self {
        reasoning.map_or(Self::Unset, Self::Sent)
    }

    /// The plan for a provider with no reasoning field.
    pub fn dropped(reasoning: Option<Reasoning>) -> Self {
        reasoning.map_or(Self::Unset, Self::Unsupported)
    }

    /// What reaches the wire, if anything. A dropped setting reads as nothing
    /// here, because nothing is what gets sent.
    pub fn sent(self) -> Option<Reasoning> {
        match self {
            Self::Sent(reasoning) => Some(reasoning),
            Self::Unset | Self::Unsupported(_) => None,
        }
    }

    /// What was configured, whether or not it will be sent.
    pub fn requested(self) -> Option<Reasoning> {
        match self {
            Self::Unset => None,
            Self::Sent(reasoning) | Self::Unsupported(reasoning) => Some(reasoning),
        }
    }

    /// What this provider was asked for and will not send.
    pub fn unsupported(self) -> Option<Reasoning> {
        match self {
            Self::Unsupported(reasoning) => Some(reasoning),
            Self::Unset | Self::Sent(_) => None,
        }
    }
}

impl std::fmt::Display for ReasoningPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unset => f.write_str("(model default)"),
            Self::Sent(reasoning) => write!(f, "{reasoning}"),
            Self::Unsupported(reasoning) => write!(f, "{reasoning} (dropped)"),
        }
    }
}

/// A resolved provider plus what a startup line or `a2a doctor` needs to
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

/// Warn when a configured [`LlmSettings::reasoning`] cannot reach the wire.
///
/// Reasoning tokens are billed, so a provider that drops the setting says so at
/// startup rather than leaving the caller to infer it from the bill. `a2a run`
/// has only this log line; a report reads [`SelectedLlm::reasoning`] instead.
fn warn_unsupported_reasoning(settings: &LlmSettings) {
    if let Some(reasoning) = settings.reasoning {
        tracing::warn!(
            provider = %settings.provider,
            %reasoning,
            "`reasoning` is only sent to the openrouter provider today; ignoring it"
        );
    }
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
            let config = OpenAiConfig {
                reasoning: settings.reasoning,
                ..OpenAiConfig::openrouter(
                    api_key,
                    model.clone(),
                    or_env(&settings.base_url, env, &["OPENROUTER_API_BASE_URL"]),
                    or_env(&settings.http_referer, env, &["OPENROUTER_HTTP_REFERER"]),
                    or_env(&settings.x_title, env, &["OPENROUTER_X_TITLE"]),
                )
            };
            Ok(SelectedLlm {
                kind: "openrouter",
                model,
                selected_by: SELECTED_BY_CONFIG,
                reasoning: ReasoningPlan::carried(settings.reasoning),
                provider: Arc::new(OpenAiProvider::new(config)),
            })
        }
        "openai" => {
            warn_unsupported_reasoning(settings);
            // No key is a valid OpenAI-compatible setup (a local Ollama), so
            // there is nothing to require here — and nothing to report either.
            let model = or_env(&settings.model, env, &["OPENAI_MODEL", "AI_MODEL"])
                .unwrap_or_else(|| "gpt-4o-mini".to_string());
            let config = OpenAiConfig {
                base_url: or_env(
                    &settings.base_url,
                    env,
                    &["OPENAI_API_BASE_URL", "AI_API_BASE_URL"],
                )
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
                model: model.clone(),
                api_key: or_env(&settings.api_key, env, &["OPENAI_API_KEY", "AI_API_KEY"]),
                extra_headers: Vec::new(),
                supports_reasoning: false,
                reasoning: None,
            };
            Ok(SelectedLlm {
                kind: "openai",
                model,
                selected_by: SELECTED_BY_CONFIG,
                reasoning: ReasoningPlan::dropped(settings.reasoning),
                provider: Arc::new(OpenAiProvider::new(config)),
            })
        }
        "gemini" => {
            warn_unsupported_reasoning(settings);
            let model = or_env(&settings.model, env, &["GEMINI_MODEL"])
                .unwrap_or_else(|| GEMINI_DEFAULT_MODEL.to_string());
            let config = GeminiConfig {
                base_url: or_env(&settings.base_url, env, &["GEMINI_API_BASE_URL"])
                    .unwrap_or_else(|| GEMINI_BASE_URL.to_string()),
                api_key: or_env(&settings.api_key, env, &["GEMINI_API_KEY"]).ok_or_else(|| {
                    LlmConfigError::unusable(
                        "gemini",
                        SELECTED_BY_CONFIG,
                        "no `api_key` in the config and GEMINI_API_KEY is not set",
                    )
                })?,
                model: model.clone(),
            };
            Ok(SelectedLlm {
                kind: "gemini",
                model,
                selected_by: SELECTED_BY_CONFIG,
                reasoning: ReasoningPlan::dropped(settings.reasoning),
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
    use crate::llm::ReasoningEffort;

    /// A fake environment. Mutating the real one would race the other tests in
    /// this binary (`set_var` is `unsafe` in edition 2024 for that reason).
    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |key| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
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

    /// A provider with no reasoning field reports what it discarded. This used
    /// to resolve to the same `None` as "nobody asked", so `a2a doctor` could
    /// only repeat which variables were set and the setting was found out about
    /// on the bill.
    #[test]
    fn a_provider_that_cannot_send_reasoning_reports_what_it_dropped() {
        for provider in ["openai", "gemini"] {
            let settings = LlmSettings {
                provider: provider.to_string(),
                api_key: Some("key".to_string()),
                reasoning: Some(Reasoning::Effort(ReasoningEffort::High)),
                ..Default::default()
            };
            let selected = build_from_settings(&settings, Env::new(&env_of(&[]))).unwrap();
            assert_eq!(
                selected.reasoning,
                ReasoningPlan::Unsupported(Reasoning::Effort(ReasoningEffort::High)),
                "for {provider}"
            );
            assert_eq!(selected.reasoning.sent(), None, "for {provider}");
            assert_eq!(
                selected.reasoning.requested(),
                Some(Reasoning::Effort(ReasoningEffort::High)),
                "for {provider}"
            );
        }
    }

    /// Nothing configured stays nothing. A plan that reported a drop here would
    /// have `doctor` warning about a setting no config contains.
    #[test]
    fn no_reasoning_configured_is_not_a_drop() {
        for provider in SUPPORTED_PROVIDERS {
            let settings = LlmSettings {
                provider: provider.to_string(),
                api_key: Some("key".to_string()),
                ..Default::default()
            };
            let selected = build_from_settings(&settings, Env::new(&env_of(&[]))).unwrap();
            assert_eq!(selected.reasoning, ReasoningPlan::Unset, "for {provider}");
            assert_eq!(selected.reasoning.unsupported(), None, "for {provider}");
        }
    }

    /// The error lists the valid providers, since the usual cause is a typo.
    /// These settings ride inside an `a2a doctor` requirement, and a report or a
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
