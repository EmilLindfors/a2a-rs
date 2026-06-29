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
//! Settings are expressed with this crate's own [`LlmSettings`] type rather than
//! a host's config struct so the helper takes no dependency on `a2a-agents`
//! (which would be circular).

use std::sync::Arc;

use tracing::{info, warn};

use super::{
    LlmProvider,
    gemini::{GeminiConfig, GeminiProvider},
    openai::{OpenAiConfig, OpenAiProvider},
};

/// Provider-agnostic LLM settings sourced from a host's configuration
/// (TOML, CLI flags, etc.). Mirrors the fields a host typically exposes.
#[derive(Debug, Clone, Default)]
pub struct LlmSettings {
    /// Provider selector: `"openrouter"`, `"openai"`, or `"gemini"`.
    pub provider: String,
    /// API key. May be omitted for `"openrouter"`/`"openai"` if the matching
    /// env var is set instead.
    pub api_key: Option<String>,
    /// Model identifier. Provider-specific default applied when `None`.
    pub model: Option<String>,
    /// Base URL override. Provider-specific default applied when `None`.
    pub base_url: Option<String>,
    /// OpenRouter `HTTP-Referer` attribution header (ignored by other providers).
    pub http_referer: Option<String>,
    /// OpenRouter `X-Title` attribution header (ignored by other providers).
    pub x_title: Option<String>,
}

fn env_set(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

/// Select a provider from the environment.
///
/// Preference order, each gated on a *present* key so an unconfigured agent
/// degrades to its non-LLM fallback instead of silently picking a dead provider:
///
/// 1. **OpenRouter** when `OPENROUTER_API_KEY` is set.
/// 2. **Gemini** when `GEMINI_API_KEY` is set.
/// 3. **OpenAI-compatible** when any of `OPENAI_API_KEY`, `AI_API_KEY`,
///    `OPENAI_API_BASE_URL`, or `AI_API_BASE_URL` is set (covers local Ollama).
///
/// Returns `None` when nothing is configured.
pub fn provider_from_env() -> Option<Arc<dyn LlmProvider>> {
    if env_set("OPENROUTER_API_KEY") {
        match OpenAiConfig::openrouter_from_env() {
            Ok(config) => {
                info!(model = %config.model, "🤖 LLM: OpenRouter (tool-calling enabled)");
                return Some(Arc::new(OpenAiProvider::new(config)));
            }
            Err(e) => warn!("OPENROUTER_API_KEY set but config failed: {e}"),
        }
    }

    if env_set("GEMINI_API_KEY") {
        match GeminiProvider::from_env() {
            Ok(gemini) => {
                info!("🤖 LLM: Gemini (tool-calling enabled)");
                return Some(Arc::new(gemini));
            }
            Err(e) => warn!("GEMINI_API_KEY set but config failed: {e}"),
        }
    }

    if env_set("OPENAI_API_KEY")
        || env_set("AI_API_KEY")
        || env_set("OPENAI_API_BASE_URL")
        || env_set("AI_API_BASE_URL")
    {
        match OpenAiProvider::from_env() {
            Ok(openai) => {
                info!("🤖 LLM: OpenAI-compatible (tool-calling enabled)");
                return Some(Arc::new(openai));
            }
            Err(e) => warn!("OpenAI config failed: {e}"),
        }
    }

    info!("🤖 LLM: none configured — host should use its non-LLM fallback");
    None
}

/// Build a provider from explicit [`LlmSettings`].
///
/// Errors on an unknown provider string or a missing required API key (the
/// `"openrouter"` key may instead come from `OPENROUTER_API_KEY`).
pub fn provider_from_settings(settings: &LlmSettings) -> Result<Arc<dyn LlmProvider>, String> {
    match settings.provider.as_str() {
        "openrouter" => {
            let api_key = settings
                .api_key
                .clone()
                .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
                .ok_or_else(|| {
                    "openrouter provider requires api_key (config) or OPENROUTER_API_KEY (env)"
                        .to_string()
                })?;
            let model = settings
                .model
                .clone()
                .unwrap_or_else(|| "z-ai/glm-4.6".to_string());
            let config = OpenAiConfig::openrouter(
                api_key,
                model,
                settings.base_url.clone(),
                settings.http_referer.clone(),
                settings.x_title.clone(),
            );
            Ok(Arc::new(OpenAiProvider::new(config)))
        }
        "openai" => {
            let config = OpenAiConfig {
                base_url: settings
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
                model: settings
                    .model
                    .clone()
                    .unwrap_or_else(|| "gpt-4o-mini".to_string()),
                api_key: settings.api_key.clone(),
                extra_headers: Vec::new(),
                supports_reasoning: false,
            };
            Ok(Arc::new(OpenAiProvider::new(config)))
        }
        "gemini" => {
            let config = GeminiConfig {
                base_url: settings.base_url.clone().unwrap_or_else(|| {
                    "https://generativelanguage.googleapis.com/v1beta/models".to_string()
                }),
                api_key: settings.api_key.clone().unwrap_or_default(),
                model: settings
                    .model
                    .clone()
                    .unwrap_or_else(|| "gemini-1.5-pro".to_string()),
            };
            Ok(Arc::new(GeminiProvider::new(config)))
        }
        other => Err(format!("unsupported LLM provider: {other}")),
    }
}
