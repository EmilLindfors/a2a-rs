//! Agent configuration with TOML support
//!
//! This module provides declarative configuration for A2A agents via TOML files.
//! It supports environment variable interpolation and sensible defaults.

use a2a_llm::{LlmSettings, Reasoning};
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[cfg(feature = "schema")]
use schemars::JsonSchema;

/// Which built-in handler drives an agent (parse, don't validate).
///
/// The typed replacement for the stringly-typed `[handler].type` /
/// `agent.implementation`. Known selectors map to their variant; anything else
/// is a [`Custom`](HandlerType::Custom) name resolved by the host binary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum HandlerType {
    /// Echoes the request back (the default).
    #[default]
    Echo,
    /// Generic config-driven LLM handler (`type = "llm"`).
    Llm,
    /// The reimbursement reference agent (`type = "reimbursement"`).
    Reimbursement,
    /// A host-resolved custom handler keyed by name.
    Custom(String),
}

impl HandlerType {
    /// The wire string for this selector (round-trips through `from`).
    pub fn as_str(&self) -> &str {
        match self {
            HandlerType::Echo => "echo",
            HandlerType::Llm => "llm",
            HandlerType::Reimbursement => "reimbursement",
            HandlerType::Custom(name) => name,
        }
    }
}

impl From<&str> for HandlerType {
    fn from(s: &str) -> Self {
        match s {
            "echo" => HandlerType::Echo,
            "llm" => HandlerType::Llm,
            "reimbursement" => HandlerType::Reimbursement,
            other => HandlerType::Custom(other.to_string()),
        }
    }
}

impl From<String> for HandlerType {
    fn from(s: String) -> Self {
        // Reuse the &str mapping; only `Custom` keeps the owned string.
        match HandlerType::from(s.as_str()) {
            HandlerType::Custom(_) => HandlerType::Custom(s),
            known => known,
        }
    }
}

impl From<HandlerType> for String {
    fn from(t: HandlerType) -> Self {
        match t {
            HandlerType::Custom(name) => name,
            other => other.as_str().to_string(),
        }
    }
}

impl std::str::FromStr for HandlerType {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(HandlerType::from(s))
    }
}

impl std::fmt::Display for HandlerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How a handler is selected for an agent.
///
/// The typed replacement for the stringly-typed `agent.implementation`. When
/// absent, the legacy `agent.implementation` field is honoured, so existing
/// configs keep working unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HandlerConfig {
    /// Built-in handler selector: `echo`, `llm`, `reimbursement`, or any custom
    /// name resolved by the host binary.
    #[serde(default)]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub r#type: HandlerType,

    /// Options for the generic LLM-driven handler (`type = "llm"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm: Option<LlmHandlerConfig>,
}

/// Options for the generic config-driven LLM handler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LlmHandlerConfig {
    /// System prompt steering the model's behaviour.
    #[serde(default = "default_llm_system_prompt")]
    pub system_prompt: String,

    /// Maximum LLM <-> tool round-trips before the handler gives up.
    ///
    /// Rounds spent only on `remember`/`forget` are not counted against this,
    /// up to a small allowance — bookkeeping is not the work this budget is
    /// for. See `handlers::llm::FREE_MEMORY_ROUNDS`.
    #[serde(default = "default_max_tool_rounds")]
    pub max_tool_rounds: u32,

    /// Remote A2A agents exposed to the model as tools (`[[handler.llm.agents]]`),
    /// enabling agent-to-agent delegation. Each becomes an `ask_<slug>` tool.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<RemoteAgentConfig>,

    /// How much of the conversation this agent carries between turns, and how it
    /// keeps that inside the model's window (`[handler.llm.context]`).
    #[serde(default)]
    pub context: ContextConfig,
}

/// What an agent remembers between messages, and what it does when that no
/// longer fits.
///
/// Off by default. Carrying history changes what an existing agent costs and how
/// it answers, so it is opted into rather than switched on under everyone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ContextConfig {
    /// Which messages a turn is given.
    #[serde(default)]
    pub mode: ContextMode,

    /// Ceiling on the request, in tokens, counted by an estimator rather than a
    /// real tokenizer — leave headroom. Zero means no ceiling and no trimming.
    #[serde(default = "default_max_input_tokens")]
    pub max_input_tokens: usize,

    /// Held back from `max_input_tokens` for the reply.
    #[serde(default = "default_reserve_for_output")]
    pub reserve_for_output: usize,

    /// Percentage of the usable budget at which the conversation is summarized,
    /// before anything has to be dropped. A percentage rather than a fraction so
    /// this config stays comparable by equality, which the surrounding structs
    /// derive.
    #[serde(default = "default_compact_at_percent")]
    pub compact_at_percent: u8,

    /// Turns kept verbatim at the end of the conversation; compaction may only
    /// fold what precedes them.
    #[serde(default = "default_keep_recent_turns")]
    pub keep_recent_turns: usize,

    /// Longest a single tool result may be, in characters, before it is trimmed
    /// to its head and tail. The first thing given up, and usually enough.
    #[serde(default = "default_max_tool_result_chars")]
    pub max_tool_result_chars: usize,

    /// Give the model `remember` and `forget`, and put what it remembered into
    /// every prompt.
    ///
    /// Independent of `mode`: an agent that carries no transcript can still be
    /// told the user's name, and one that carries the whole conversation need
    /// not keep anything apart from it. Off by default — it adds two tools and
    /// a block of prompt, which changes what an existing agent costs and how it
    /// answers.
    #[serde(default)]
    pub remember: bool,

    /// Ceiling on the state block, in characters.
    ///
    /// It is rendered as a system message, and `fit` never trims those — the
    /// same reason the system prompt is never trimmed. So the bound has to be
    /// applied where a value is written, and a `remember` that would cross it
    /// is refused rather than silently making every later request larger.
    #[serde(default = "default_max_state_chars")]
    pub max_state_chars: usize,

    /// Characters per token the estimator assumes when deciding what fits.
    ///
    /// The right value belongs to the model's tokenizer and the language it is
    /// reading; the default suits English prose mixed with JSON. The handler
    /// measures the estimate against what the provider charges and logs the
    /// value that would have matched, which is where a deployment gets its own.
    #[serde(default)]
    pub chars_per_token: CharsPerToken,
}

/// Characters per token, as a config value.
///
/// A config file writes a decimal (`chars_per_token = 3.2`); this stores
/// hundredths, so the config structs around it stay comparable by equality —
/// `f32` is not `Eq`, and neither is any struct holding one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "f64"))]
#[serde(try_from = "f64", into = "f64")]
pub struct CharsPerToken(u16);

/// The estimator's own default, repeated here because a config value has to
/// have one at parse time.
const DEFAULT_CHARS_PER_TOKEN: u16 = 350;

/// Below this a request estimates as more tokens than it has characters, which
/// only a byte-level tokenizer on non-Latin script approaches; above it, nothing
/// would ever be trimmed.
const CHARS_PER_TOKEN_RANGE: std::ops::RangeInclusive<f64> = 0.1..=20.0;

impl Default for CharsPerToken {
    fn default() -> Self {
        Self(DEFAULT_CHARS_PER_TOKEN)
    }
}

impl CharsPerToken {
    /// The ratio, for [`CharEstimate::with_chars_per_token`](crate::context::CharEstimate::with_chars_per_token).
    pub fn as_f32(self) -> f32 {
        f32::from(self.0) / 100.0
    }
}

impl TryFrom<f64> for CharsPerToken {
    type Error = String;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        if !CHARS_PER_TOKEN_RANGE.contains(&value) {
            return Err(format!(
                "chars_per_token must be between {} and {}, got {value}",
                CHARS_PER_TOKEN_RANGE.start(),
                CHARS_PER_TOKEN_RANGE.end()
            ));
        }
        Ok(Self((value * 100.0).round() as u16))
    }
}

impl From<CharsPerToken> for f64 {
    fn from(value: CharsPerToken) -> Self {
        f64::from(value.0) / 100.0
    }
}

/// Which messages a turn is given.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum ContextMode {
    /// Only the incoming message. Every turn starts fresh, which is how this
    /// handler has always behaved.
    #[default]
    None,
    /// The messages of the current task.
    Task,
    /// Every message in the context — the conversation this message belongs to,
    /// across all of its tasks. Needs storage that outlives the process to be
    /// worth anything, so pair it with `[server.storage] type = "sqlx"`.
    Context,
}

impl ContextMode {
    /// Whether this mode reads anything back from storage.
    pub fn reads_history(self) -> bool {
        !matches!(self, ContextMode::None)
    }
}

impl std::fmt::Display for ContextMode {
    /// The spelling a config uses, so a report names the key back.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ContextMode::None => "none",
            ContextMode::Task => "task",
            ContextMode::Context => "context",
        })
    }
}

fn default_max_input_tokens() -> usize {
    100_000
}

/// Room for a handful of short facts and no more. The state bag is meant to
/// hold what a later turn needs to know, and anything that outgrows this is a
/// document, which belongs in a tool result rather than in every request.
fn default_max_state_chars() -> usize {
    2_000
}

fn default_reserve_for_output() -> usize {
    8_000
}

fn default_compact_at_percent() -> u8 {
    80
}

fn default_keep_recent_turns() -> usize {
    4
}

fn default_max_tool_result_chars() -> usize {
    8_000
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            mode: ContextMode::default(),
            max_input_tokens: default_max_input_tokens(),
            reserve_for_output: default_reserve_for_output(),
            compact_at_percent: default_compact_at_percent(),
            keep_recent_turns: default_keep_recent_turns(),
            max_tool_result_chars: default_max_tool_result_chars(),
            remember: false,
            max_state_chars: default_max_state_chars(),
            chars_per_token: CharsPerToken::default(),
        }
    }
}

/// A remote A2A agent the LLM handler can delegate to, exposed as one tool.
///
/// The peer is named by **exactly one** of `url`, `skill`, or `agent_id`:
/// a raw `url` dials directly, while `skill`/`agent_id` are resolved against the
/// [`AgentRegistry`](crate::registry::AgentRegistry) at startup. Use
/// [`target`](Self::target) to read the resolved one-of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RemoteAgentConfig {
    /// Friendly name; the tool the model sees is `ask_<slug-of-name>`.
    pub name: String,

    /// Base URL of the remote agent (its transport is auto-negotiated from the
    /// agent card, falling back to a direct client). Mutually exclusive with
    /// `skill`/`agent_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Resolve the peer by a skill it advertises (matched against registered
    /// agent cards). Mutually exclusive with `url`/`agent_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,

    /// Resolve the peer by its registry id (slug of its name). Mutually
    /// exclusive with `url`/`skill`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,

    /// Optional override for the tool description shown to the model. When
    /// omitted, a description is derived from the agent card at startup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The resolved one-of selecting how a [`RemoteAgentConfig`] names its peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteAgentTarget<'a> {
    /// A raw base URL, dialed directly.
    Url(&'a str),
    /// A skill to resolve against the registry.
    Skill(&'a str),
    /// A registry id to resolve against the registry.
    AgentId(&'a str),
}

impl std::fmt::Display for RemoteAgentTarget<'_> {
    /// How the peer was named, for a log line that has to say which reference
    /// resolved (or did not).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Url(url) => write!(f, "{url}"),
            Self::Skill(skill) => write!(f, "skill '{skill}'"),
            Self::AgentId(id) => write!(f, "agent id '{id}'"),
        }
    }
}

impl RemoteAgentConfig {
    /// Resolve the peer reference, enforcing that **exactly one** of `url`,
    /// `skill`, or `agent_id` is set (parse, don't validate). A missing or
    /// ambiguous reference is a [`ConfigError::ValidationError`].
    pub fn target(&self) -> Result<RemoteAgentTarget<'_>, ConfigError> {
        match (
            self.url.as_deref(),
            self.skill.as_deref(),
            self.agent_id.as_deref(),
        ) {
            (Some(url), None, None) => Ok(RemoteAgentTarget::Url(url)),
            (None, Some(skill), None) => Ok(RemoteAgentTarget::Skill(skill)),
            (None, None, Some(id)) => Ok(RemoteAgentTarget::AgentId(id)),
            (None, None, None) => Err(ConfigError::ValidationError(format!(
                "remote agent '{}' must set exactly one of `url`, `skill`, or `agent_id`",
                self.name
            ))),
            _ => Err(ConfigError::ValidationError(format!(
                "remote agent '{}' sets more than one of `url`, `skill`, `agent_id`; pick one",
                self.name
            ))),
        }
    }
}

fn default_llm_system_prompt() -> String {
    "You are a helpful assistant. Use the available tools when they give a more precise answer than guessing, then reply concisely.".to_string()
}

fn default_max_tool_rounds() -> u32 {
    4
}

impl Default for LlmHandlerConfig {
    fn default() -> Self {
        Self {
            system_prompt: default_llm_system_prompt(),
            max_tool_rounds: default_max_tool_rounds(),
            agents: Vec::new(),
            context: ContextConfig::default(),
        }
    }
}

/// How the platform packages this agent when it runs it as a managed unit
/// (`[runtime]`).
///
/// Only meaningful to a runtime that can honour it — [`ContainerRuntime`] today.
/// `a2a run` reads the config in-process and has no image to pull, so it says so
/// and runs the built-in handler instead.
///
/// [`ContainerRuntime`]: crate::ContainerRuntime
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    /// Container image running this agent, instead of the platform's base image.
    ///
    /// This is what keeps the declarative layer from being closed: an agent that
    /// needs a handler no TOML can express ships as its own image, and the
    /// platform goes back to caring only about the config, the port and the
    /// container. The image is passed to the engine verbatim, so any reference
    /// it accepts works (`ghcr.io/acme/billing:1.4`, a digest, a local tag).
    ///
    /// The image gets the same contract as the base one: its config bind-mounted
    /// read-only and named by `A2A_CONFIG`, `HOST=0.0.0.0`, its `http_port`
    /// published, and the config's `${VAR}` references passed through. Unlike
    /// the base image it is started with **no command override**, so its own
    /// `ENTRYPOINT`/`CMD` decides how to boot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Failed to parse TOML: {0}")]
    TomlError(#[from] toml::de::Error),
    #[error("Environment variable not found: {0}")]
    EnvVarError(String),
    #[error("Invalid configuration: {0}")]
    ValidationError(String),
}

/// Complete agent configuration from TOML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    /// Agent metadata
    pub agent: AgentMetadata,

    /// Typed handler selection (`[handler]`). When omitted, the legacy
    /// `agent.implementation` string is honoured via [`handler_type`].
    #[serde(default)]
    pub handler: HandlerConfig,

    /// How the platform packages this agent (`[runtime]`) — a custom image, or
    /// nothing, meaning the platform's base image runs it.
    #[serde(default)]
    pub runtime: RuntimeConfig,

    /// Server configuration
    #[serde(default)]
    pub server: ServerConfig,

    /// Skills exposed by the agent
    #[serde(default)]
    pub skills: Vec<SkillConfig>,

    /// Features enabled for the agent
    #[serde(default)]
    pub features: FeaturesConfig,

    /// LLM Configuration
    #[serde(default)]
    pub llm: Option<LlmConfig>,
}

/// LLM Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LlmConfig {
    /// LLM Provider (e.g. "openrouter", "openai", "gemini")
    pub provider: String,
    /// API key for the LLM
    pub api_key: Option<String>,
    /// Model to use
    pub model: Option<String>,
    /// What to ask this model to do with its thinking: `"off"`, `"low"`,
    /// `"medium"`, `"high"`, or a token budget (`reasoning = 2000`).
    ///
    /// Omitted, nothing is sent and the model's own default stands — a
    /// reasoning model still reasons. It sits beside `model` because that is
    /// what it belongs to: a flash model answering in one line and a frontier
    /// model doing analysis want different answers, and the agent's *handler*
    /// has no way to know which it was pointed at. OpenRouter only today.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<ReasoningSchema>"))]
    pub reasoning: Option<Reasoning>,
    /// Base URL (for providers like openai that support local LLMs like ollama)
    pub base_url: Option<String>,
    /// OpenRouter `HTTP-Referer` attribution header (ignored by other providers)
    #[serde(default)]
    pub http_referer: Option<String>,
    /// OpenRouter `X-Title` attribution header (ignored by other providers)
    #[serde(default)]
    pub x_title: Option<String>,
    /// Whether this endpoint accepts `stream_options.include_usage`, which is
    /// what makes a *streaming* response report what it cost.
    ///
    /// Omitted, it follows the endpoint: on for OpenRouter and OpenAI's own
    /// URL, off elsewhere — a local OpenAI-compatible server that rejects
    /// unknown parameters would fail the whole call, and only the URL
    /// distinguishes them. Set it for a server this cannot recognize, such as a
    /// proxy in front of OpenAI or a self-hosted vLLM that does support it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_usage: Option<bool>,
}

impl From<&LlmConfig> for LlmSettings {
    /// Lives here so `a2a run` and `a2a doctor` build a provider from the same
    /// values; two hand-written copies of this drift.
    fn from(config: &LlmConfig) -> Self {
        Self {
            provider: config.provider.clone(),
            api_key: config.api_key.clone(),
            model: config.model.clone(),
            base_url: config.base_url.clone(),
            http_referer: config.http_referer.clone(),
            x_title: config.x_title.clone(),
            reasoning: config.reasoning,
            stream_usage: config.stream_usage,
        }
    }
}

/// Schema-only mirror of what `[llm] reasoning` accepts.
///
/// [`Reasoning`] carries its own TOML surface (one parser, one error message),
/// but it lives in `a2a-agents-common`, which has no `schemars` dependency —
/// so the JSON Schema export needs a type to point `schemars(with = …)` at.
/// `reasoning_schema_matches_the_parser` keeps this honest.
#[cfg(feature = "schema")]
#[derive(JsonSchema)]
#[serde(untagged)]
pub enum ReasoningSchema {
    /// A named level, `"off"` included — the setting a small fast model usually
    /// wants, and the one an effort-only knob cannot express.
    Level(ReasoningLevelSchema),
    /// A hard cap on reasoning tokens.
    Budget(u32),
}

/// The named levels of [`ReasoningSchema`].
#[cfg(feature = "schema")]
#[derive(JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningLevelSchema {
    Off,
    Low,
    Medium,
    High,
}

impl AgentConfig {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_toml(&content)
    }

    /// Parse configuration from TOML string
    pub fn from_toml(content: &str) -> Result<Self, ConfigError> {
        // Expand environment variables
        let expanded = expand_env_vars(content)?;
        let config: AgentConfig = toml::from_str(&expanded)?;
        config.validate()?;
        Ok(config)
    }

    /// Parse for **validation**, tolerating unset `${VAR}` references.
    ///
    /// Returns the config alongside the names of variables that had no value and
    /// no `:-default` — they were substituted with a placeholder so the rest of
    /// the config could still be checked. Everything else (unknown keys, wrong
    /// types, failed [`validate`](Self::validate)) errors exactly as in
    /// [`from_toml`](Self::from_toml).
    ///
    /// This is what `a2a validate` uses: a config's shape should be checkable
    /// without the production secrets it will eventually run with.
    pub fn check_toml(content: &str) -> Result<(Self, Vec<String>), ConfigError> {
        let (expanded, unset) = expand_env_vars_lenient(content);
        let config: AgentConfig = toml::from_str(&expanded)?;
        config.validate()?;
        Ok((config, unset))
    }

    /// [`check_toml`](Self::check_toml) for a file on disk.
    pub fn check_file<P: AsRef<Path>>(path: P) -> Result<(Self, Vec<String>), ConfigError> {
        let content = std::fs::read_to_string(path)?;
        Self::check_toml(&content)
    }

    /// Resolve the handler selector, preferring `[handler].type` and falling
    /// back to the legacy `agent.implementation` (so existing configs keep
    /// working) and finally [`HandlerType::Echo`].
    pub fn handler_type(&self) -> HandlerType {
        if self.handler.r#type != HandlerType::Echo {
            return self.handler.r#type.clone();
        }
        match self
            .agent
            .implementation
            .as_deref()
            .filter(|s| !s.is_empty())
        {
            Some(implementation) => HandlerType::from(implementation),
            None => HandlerType::Echo,
        }
    }

    /// The image this agent runs from, if it brings its own.
    ///
    /// `None` means the runtime's base image (or, under `a2a run`, this binary's
    /// built-in handlers).
    pub fn image(&self) -> Option<&str> {
        self.runtime.image.as_deref()
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.agent.name.is_empty() {
            return Err(ConfigError::ValidationError(
                "Agent name cannot be empty".to_string(),
            ));
        }

        if !self.features.mcp_server.enabled && self.server.http_port == 0 {
            return Err(ConfigError::ValidationError(
                "The HTTP server port must be configured when MCP server is disabled".to_string(),
            ));
        }

        // Validate skills
        for skill in &self.skills {
            if skill.id.is_empty() {
                return Err(ConfigError::ValidationError(
                    "Skill ID cannot be empty".to_string(),
                ));
            }
        }

        // A blank image would reach the engine as an empty argument, which fails
        // as an unreadable `docker create` error rather than as the config
        // mistake it is.
        if self
            .runtime
            .image
            .as_deref()
            .is_some_and(|image| image.trim().is_empty())
        {
            return Err(ConfigError::ValidationError(
                "[runtime] image cannot be empty — name an image or omit the key".to_string(),
            ));
        }

        // A card carrying something that is not a URL is a peer that cannot
        // dial this agent, reported on the peer rather than here — so it is
        // checked where it is written.
        if let Some(url) = self.server.advertised_url.as_deref().map(str::trim)
            && !(url.starts_with("http://") || url.starts_with("https://"))
        {
            return Err(ConfigError::ValidationError(format!(
                "[server] advertised_url must be an absolute http(s) URL, got {url:?} — it goes \
                 on the agent card as the address peers dial"
            )));
        }

        // A pool that hands out no connections fails every query, and the store
        // opens on the first request rather than at startup.
        if let StorageConfig::Sqlx {
            max_connections: 0, ..
        } = &self.server.storage
        {
            return Err(ConfigError::ValidationError(
                "[server.storage] max_connections must be greater than 0 — a pool of no \
                 connections fails every query"
                    .to_string(),
            ));
        }

        // An access token is opaque, so with nowhere to check one an OAuth2
        // agent authenticates nobody — it starts, serves a card, and rejects
        // every request. Said here rather than discovered as a 401 on the first
        // call.
        if let AuthConfig::OAuth2 {
            introspection_url: None,
            ..
        } = &self.server.auth
        {
            return Err(ConfigError::ValidationError(
                "[server.auth] type = \"oauth2\" needs an `introspection_url` (RFC 7662): an \
                 access token is opaque, so without one no presented token can be validated and \
                 every request is refused"
                    .to_string(),
            ));
        }

        // Validate remote-agent references fail fast at load (exactly one of
        // url/skill/agent_id) rather than at resolve time.
        if let Some(llm) = &self.handler.llm {
            // Two entries can name different peers and derive one tool name —
            // "Weather Agent" and "weather-agent" both slugify to
            // `ask_weather_agent` — and the resolver takes the first source
            // claiming a name, so the second is advertised to the model and can
            // never be called. Decidable here with nothing connected, which is
            // why it is an error rather than the startup warning that also
            // reports it.
            let mut derived: std::collections::HashMap<String, &str> =
                std::collections::HashMap::with_capacity(llm.agents.len());
            for agent in &llm.agents {
                agent.target()?;

                let tool = crate::handlers::tools::tool_name_for(&agent.name);
                if let Some(first) = derived.insert(tool.clone(), agent.name.as_str()) {
                    return Err(ConfigError::ValidationError(format!(
                        "[[handler.llm.agents]] '{}' and '{}' both derive the tool name `{}`, \
                         so the model could only ever reach one of them — rename one",
                        first, agent.name, tool
                    )));
                }
            }

            // What is left after the reserve is what a request may use, so a
            // reserve at or above the ceiling leaves nothing for the system
            // prompt and the question — every turn would fail on the floor
            // alone. `max_input_tokens = 0` means no ceiling and no reserve
            // applies.
            let context = &llm.context;
            if context.max_input_tokens != 0
                && context.reserve_for_output >= context.max_input_tokens
            {
                return Err(ConfigError::ValidationError(format!(
                    "[handler.llm.context] reserve_for_output ({}) leaves nothing under \
                     max_input_tokens ({}) — the difference is what each request may use",
                    context.reserve_for_output, context.max_input_tokens
                )));
            }

            // A ceiling of zero refuses every write, so the model gets a tool
            // that cannot succeed and a refusal it cannot act on.
            if context.remember && context.max_state_chars == 0 {
                return Err(ConfigError::ValidationError(
                    "[handler.llm.context] remember is on with max_state_chars = 0, so every \
                     `remember` call would be refused — raise it, or set remember = false"
                        .to_string(),
                ));
            }
        }

        Ok(())
    }

    /// The base URL this agent puts on its card, and where that came from.
    ///
    /// Binding and advertising are different facts (see
    /// [`ServerConfig::advertised_url`]), and this is the one place that
    /// resolves the second from the first.
    pub fn advertised(&self) -> Advertised {
        let port = self.server.http_port;
        if let Some(url) = self
            .server
            .advertised_url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
        {
            return Advertised::Configured(url.trim_end_matches('/').to_string());
        }
        if is_wildcard_host(&self.server.host) {
            // A wildcard names every interface and therefore none: it cannot go
            // on a card. The machine the agent runs on is the only address that
            // is certainly right, so that is the guess — and it is reported as
            // one.
            return Advertised::Guessed(format!("http://localhost:{port}"));
        }
        Advertised::Bound(format!("http://{}:{}", self.server.host, port))
    }

    /// Build agent card URL from server config.
    pub fn agent_url(&self) -> String {
        self.advertised().into_url()
    }
}

/// Hosts that mean "every interface" and so name no address a peer can dial.
fn is_wildcard_host(host: &str) -> bool {
    matches!(host.trim(), "" | "0.0.0.0" | "::" | "[::]" | "*")
}

/// The address an agent publishes, and how it was arrived at.
///
/// A card's URL is what a peer dials, so being wrong here is not visible on the
/// agent that is wrong — it shows up as somebody else failing to reach it. The
/// variants exist so a report can say which of the three happened rather than
/// presenting a guess as a fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Advertised {
    /// Written down: `[server] advertised_url`, or `A2A_ADVERTISED_URL`.
    Configured(String),
    /// Derived from the bind address, which names a dialable interface.
    Bound(String),
    /// The bind address is a wildcard, so this is a guess — right on the
    /// machine the agent runs on, wrong from anywhere else.
    Guessed(String),
}

impl Advertised {
    /// The URL itself.
    pub fn url(&self) -> &str {
        match self {
            Advertised::Configured(url) | Advertised::Bound(url) | Advertised::Guessed(url) => url,
        }
    }

    /// Take the URL, for a caller that only wants the string.
    pub fn into_url(self) -> String {
        match self {
            Advertised::Configured(url) | Advertised::Bound(url) | Advertised::Guessed(url) => url,
        }
    }

    /// Whether this address was guessed rather than derived or configured.
    pub fn is_guess(&self) -> bool {
        matches!(self, Advertised::Guessed(_))
    }
}

/// Agent metadata and identity
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AgentMetadata {
    /// Agent name
    pub name: String,

    /// Agent description
    #[serde(default)]
    pub description: Option<String>,

    /// Agent version
    #[serde(default)]
    pub version: Option<String>,

    /// Provider information
    #[serde(default)]
    pub provider: Option<ProviderInfo>,

    /// Documentation URL
    #[serde(default)]
    pub documentation_url: Option<String>,

    /// The implementation handler to use for this agent (e.g. 'reimbursement', 'echo')
    /// Used primarily by the generic a2a binary.
    #[serde(default)]
    pub implementation: Option<String>,
}

/// Provider information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ProviderInfo {
    pub name: String,
    pub url: String,
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    /// Interface to bind. Defaults to `HOST`, then `127.0.0.1`.
    ///
    /// This is *only* the interface. What goes on the agent card is
    /// [`advertised_url`](Self::advertised_url), which defaults from this and
    /// is not the same question.
    #[serde(default = "default_host")]
    pub host: String,

    /// HTTP server port (0 to disable)
    #[serde(default = "default_http_port")]
    pub http_port: u16,

    /// Base URL peers should dial to reach this agent — what goes on its card.
    ///
    /// Separate from `host` because the two disagree in the deployment that
    /// matters most: a container has to bind `0.0.0.0` to be reachable through
    /// its published port, and `http://0.0.0.0:8080` is an address nobody can
    /// dial. Guessing from the bind address cannot work, since the two mistakes
    /// point opposite ways — binding a wildcard publishes an unusable address,
    /// and binding `127.0.0.1` publishes a *correct* address for an agent that
    /// is unreachable once containerised.
    ///
    /// Defaults to `A2A_ADVERTISED_URL` (which `ContainerRuntime` sets when it
    /// provisions an agent), then to `http://{host}:{port}` when `host` names a
    /// dialable interface. With a wildcard bind and nothing set, the agent
    /// advertises `http://localhost:{port}` and says it is guessing.
    #[serde(
        default = "default_advertised_url",
        skip_serializing_if = "Option::is_none"
    )]
    pub advertised_url: Option<String>,

    /// Storage configuration
    #[serde(default)]
    pub storage: StorageConfig,

    /// Authentication configuration
    #[serde(default)]
    pub auth: AuthConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            http_port: default_http_port(),
            advertised_url: default_advertised_url(),
            storage: StorageConfig::default(),
            auth: AuthConfig::default(),
        }
    }
}

/// Storage backend configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StorageConfig {
    /// In-memory storage (default)
    #[default]
    InMemory,

    /// SQLx-based persistent storage
    Sqlx {
        /// Database URL (supports env vars like ${DATABASE_URL})
        url: String,

        /// Maximum number of connections in the pool.
        ///
        /// On PostgreSQL this is a share of a server-wide limit, so a fleet
        /// against one server divides that number between its members.
        #[serde(default = "default_max_connections")]
        max_connections: u32,

        /// Log every statement the store executes, at `DEBUG`.
        #[serde(default)]
        enable_logging: bool,
    },
}

impl StorageConfig {
    /// Whether what this backend holds outlives the process.
    ///
    /// Tasks, conversations and digests all live here, so this is the answer to
    /// "does anything this agent remembers survive a restart" — and the control
    /// plane restarts agents on purpose.
    pub fn is_durable(&self) -> bool {
        matches!(self, StorageConfig::Sqlx { .. })
    }
}

/// Authentication configuration
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AuthConfig {
    /// No authentication (default for development)
    #[default]
    None,

    /// Bearer token authentication
    Bearer {
        /// List of valid tokens (supports env vars)
        tokens: Vec<String>,

        /// Optional bearer format description (e.g., "JWT")
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<String>,
    },

    /// API Key authentication
    ApiKey {
        /// Valid API keys
        keys: Vec<String>,

        /// Location of the API key: "header", "query", or "cookie"
        #[serde(default = "default_api_key_location")]
        location: String,

        /// Name of the header/query param/cookie
        #[serde(default = "default_api_key_name")]
        name: String,
    },

    /// JWT (JSON Web Token) authentication
    Jwt {
        /// JWT secret for HMAC algorithms (HS256, HS384, HS512)
        /// Use ${ENV_VAR} for environment variables
        #[serde(skip_serializing_if = "Option::is_none")]
        secret: Option<String>,

        /// RSA public key in PEM format for RSA algorithms (RS256, RS384, RS512)
        #[serde(skip_serializing_if = "Option::is_none")]
        rsa_pem_path: Option<String>,

        /// Algorithm to use (HS256, HS384, HS512, RS256, RS384, RS512)
        #[serde(default = "default_jwt_algorithm")]
        algorithm: String,

        /// Required issuer (iss claim)
        #[serde(skip_serializing_if = "Option::is_none")]
        issuer: Option<String>,

        /// Required audience (aud claim)
        #[serde(skip_serializing_if = "Option::is_none")]
        audience: Option<String>,
    },

    /// OAuth2 authentication
    OAuth2 {
        /// Client ID
        client_id: String,

        /// Client secret (use ${ENV_VAR} for environment variables)
        client_secret: String,

        /// Authorization URL
        authorization_url: String,

        /// Token URL
        token_url: String,

        /// Introspection endpoint (RFC 7662), where the authorization server
        /// publishes one.
        ///
        /// Set it for anything but a demo: an access token is opaque, so this
        /// is the only way to learn whether it is still valid and *whose* it
        /// is. Without it the agent knows the caller only by the token they
        /// presented, which changes on every refresh — so anything the agent
        /// keeps per caller, a conversation included, is lost with it.
        #[serde(skip_serializing_if = "Option::is_none")]
        introspection_url: Option<String>,

        /// Redirect URL for authorization code flow
        #[serde(skip_serializing_if = "Option::is_none")]
        redirect_url: Option<String>,

        /// OAuth2 flow type: "authorization_code" or "client_credentials"
        #[serde(default = "default_oauth2_flow")]
        flow: String,

        /// Required scopes
        #[serde(default)]
        scopes: Vec<String>,
    },
}

/// Skill configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SkillConfig {
    /// Unique skill identifier
    pub id: String,

    /// Human-readable skill name
    pub name: String,

    /// Skill description
    #[serde(default)]
    pub description: Option<String>,

    /// Keywords for skill discovery
    #[serde(default)]
    pub keywords: Vec<String>,

    /// Example queries for this skill
    #[serde(default)]
    pub examples: Vec<String>,

    /// Supported input formats
    #[serde(default = "default_formats")]
    pub input_formats: Vec<String>,

    /// Supported output formats
    #[serde(default = "default_formats")]
    pub output_formats: Vec<String>,
}

/// Features configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FeaturesConfig {
    /// Enable streaming updates
    #[serde(default)]
    pub streaming: bool,

    /// Enable push notifications
    #[serde(default)]
    pub push_notifications: bool,

    /// Enable state transition history
    #[serde(default)]
    pub state_history: bool,

    /// Enable authenticated extended card
    #[serde(default)]
    pub authenticated_card: bool,

    /// Protocol extensions (AP2, etc.)
    #[serde(default)]
    pub extensions: ExtensionsConfig,

    /// MCP server configuration (expose agent as MCP server)
    #[serde(default)]
    pub mcp_server: McpServerConfig,

    /// MCP client configuration (connect to MCP servers to use their tools)
    #[serde(default)]
    pub mcp_client: McpClientConfig,
}

impl Default for FeaturesConfig {
    fn default() -> Self {
        Self {
            streaming: true,
            push_notifications: true,
            state_history: true,
            authenticated_card: false,
            extensions: ExtensionsConfig::default(),
            mcp_server: McpServerConfig::default(),
            mcp_client: McpClientConfig::default(),
        }
    }
}

/// Protocol extensions configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ExtensionsConfig {
    /// AP2 (Agent Payments Protocol) extension
    #[serde(default)]
    pub ap2: Option<Ap2ExtensionConfig>,
}

/// AP2 extension configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Ap2ExtensionConfig {
    /// AP2 roles this agent performs (merchant, shopper, credentials-provider, payment-processor)
    pub roles: Vec<String>,

    /// Whether clients must understand AP2 to interact with this agent
    #[serde(default)]
    pub required: bool,
}

/// MCP server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct McpServerConfig {
    /// Enable MCP server (expose agent as MCP tools)
    #[serde(default)]
    pub enabled: bool,

    /// Use stdio transport (for Claude Desktop integration).
    ///
    /// Ignored when [`http.enabled`](McpHttpConfig::enabled) is set — the HTTP
    /// (Streamable HTTP) transport takes precedence, since a single process
    /// cannot own stdin/stdout for stdio and bind a socket at the same time.
    #[serde(default = "default_true")]
    pub stdio: bool,

    /// Streamable HTTP transport (for networked MCP clients).
    #[serde(default)]
    pub http: McpHttpConfig,

    /// Server name (defaults to agent name)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Server version (defaults to agent version)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            stdio: true,
            http: McpHttpConfig::default(),
            name: None,
            version: None,
        }
    }
}

/// Streamable HTTP transport configuration for the MCP server.
///
/// When [`enabled`](Self::enabled), the agent is served over MCP's Streamable
/// HTTP transport (`rmcp`'s `StreamableHttpService`) instead of stdio, mounted
/// at [`path`](Self::path) on `host:port`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct McpHttpConfig {
    /// Serve the MCP server over Streamable HTTP rather than stdio.
    #[serde(default)]
    pub enabled: bool,

    /// Host/interface to bind to.
    #[serde(default = "default_mcp_http_host")]
    pub host: String,

    /// TCP port to bind to.
    #[serde(default = "default_mcp_http_port")]
    pub port: u16,

    /// URL path the Streamable HTTP endpoint is mounted at.
    #[serde(default = "default_mcp_http_path")]
    pub path: String,

    /// Hostnames / `host:port` authorities accepted in the inbound `Host`
    /// header (DNS-rebinding protection).
    ///
    /// * Omitted → the secure default: loopback only (`localhost`, `127.0.0.1`,
    ///   `::1`).
    /// * `[]` → disable `Host` validation entirely (allow any host — required
    ///   for public binds, but **not recommended** without an upstream proxy).
    /// * Non-empty → only the listed authorities are accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_hosts: Option<Vec<String>>,

    /// Browser `Origin` values accepted on inbound requests.
    ///
    /// * Omitted (or `[]`) → `Origin` validation disabled (the rmcp default).
    /// * Non-empty → requests carrying an `Origin` must match one of these per
    ///   RFC 6454 `(scheme, host, port)`; entries must include a scheme (e.g.
    ///   `https://app.example.com`). Requests without an `Origin` still pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_origins: Option<Vec<String>>,
}

impl Default for McpHttpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: default_mcp_http_host(),
            port: default_mcp_http_port(),
            path: default_mcp_http_path(),
            allowed_hosts: None,
            allowed_origins: None,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_mcp_http_host() -> String {
    "127.0.0.1".to_string()
}

fn default_mcp_http_port() -> u16 {
    8000
}

fn default_mcp_http_path() -> String {
    "/mcp".to_string()
}

/// MCP client configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct McpClientConfig {
    /// Enable MCP client (connect to MCP servers to use their tools)
    #[serde(default)]
    pub enabled: bool,

    /// MCP servers to connect to
    #[serde(default)]
    pub servers: Vec<McpServerConnection>,
}

/// Configuration for connecting to an MCP server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct McpServerConnection {
    /// Unique name for this MCP server
    pub name: String,

    /// Command to run to start the MCP server
    pub command: String,

    /// Arguments to pass to the command
    #[serde(default)]
    pub args: Vec<String>,

    /// Environment variables to set
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,

    /// Working directory for the command
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

// Default value functions

fn default_host() -> String {
    std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string())
}

/// The address to publish, from the environment. Set by `ContainerRuntime`
/// alongside `HOST`, because whoever decided the agent binds a wildcard is also
/// the only one who knows what a peer should dial instead.
fn default_advertised_url() -> Option<String> {
    std::env::var("A2A_ADVERTISED_URL")
        .ok()
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
}

fn default_http_port() -> u16 {
    std::env::var("HTTP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080)
}

fn default_max_connections() -> u32 {
    10
}

fn default_jwt_algorithm() -> String {
    "HS256".to_string()
}

fn default_oauth2_flow() -> String {
    "authorization_code".to_string()
}

fn default_api_key_location() -> String {
    "header".to_string()
}

fn default_api_key_name() -> String {
    "X-API-Key".to_string()
}

fn default_formats() -> Vec<String> {
    vec!["text".to_string(), "data".to_string()]
}

/// The `${VAR}` / `${VAR:-default}` reference syntax shared by
/// [`expand_env_vars`] and [`referenced_env_vars`].
/// Group 1 = var name; group 2 (optional) = `:-default` fallback.
fn env_var_regex() -> &'static regex::Regex {
    use std::sync::LazyLock;
    static ENV_VAR_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)(?::-([^}]*))?\}").unwrap()
    });
    &ENV_VAR_RE
}

/// The set of environment-variable names a raw config string references via
/// `${VAR}` / `${VAR:-default}`, deduplicated and sorted.
///
/// This is the *inspection* counterpart to [`expand_env_vars`]: a runtime that
/// defers expansion to another environment (e.g. a container where `a2a run`
/// re-parses the TOML) uses it to know which variables to inject there, so
/// secrets can stay out of the on-disk TOML.
pub fn referenced_env_vars(content: &str) -> std::collections::BTreeSet<String> {
    env_var_regex()
        .captures_iter(content)
        .map(|cap| cap[1].to_string())
        .collect()
}

/// Expand environment variables in the config string.
///
/// Supports `${VAR_NAME}` and `${VAR_NAME:-default}` syntax. An unset variable
/// with no default is a hard [`ConfigError::EnvVarError`]; with a default, the
/// default (which may be empty) is substituted.
fn expand_env_vars(content: &str) -> Result<String, ConfigError> {
    let mut unset = Vec::new();
    let expanded = expand_env_vars_inner(content, &mut unset);
    match unset.first() {
        Some(var) => Err(ConfigError::EnvVarError(var.clone())),
        None => Ok(expanded),
    }
}

/// Substituted for an unset variable when expanding leniently. Non-empty so
/// fields with `String` types still parse, and recognizable if it ever leaks.
const UNSET_ENV_PLACEHOLDER: &str = "<unset>";

/// Expand for **validation**: an unset `${VAR}` becomes [`UNSET_ENV_PLACEHOLDER`]
/// instead of failing, and every such name is collected (deduplicated, in first-
/// seen order).
///
/// Checking a config's *shape* should not require holding the secrets it will
/// run with — otherwise `a2a validate` is unusable in CI and on a fresh
/// checkout, which is exactly where it is most useful. Callers that genuinely
/// need the values (running an agent) use [`expand_env_vars`] and get the hard
/// error.
fn expand_env_vars_lenient(content: &str) -> (String, Vec<String>) {
    let mut unset = Vec::new();
    let expanded = expand_env_vars_inner(content, &mut unset);
    (expanded, unset)
}

/// Shared expansion: substitutes every reference, recording the names that had
/// neither a value nor a `:-default`.
fn expand_env_vars_inner(content: &str, unset: &mut Vec<String>) -> String {
    let mut result = content.to_string();

    for cap in env_var_regex().captures_iter(content) {
        let full_match = &cap[0];
        let var_name = &cap[1];

        let value = match std::env::var(var_name) {
            Ok(value) => value,
            Err(_) => match cap.get(2) {
                Some(default) => default.as_str().to_string(),
                None => {
                    if !unset.iter().any(|v| v == var_name) {
                        unset.push(var_name.to_string());
                    }
                    UNSET_ENV_PLACEHOLDER.to_string()
                }
            },
        };

        result = result.replace(full_match, &value);
    }

    result
}

#[cfg(test)]
mod tests {
    use a2a_llm::ReasoningEffort;

    use super::*;

    /// A mistyped key must be an error, not a silent default. This is the worst
    /// failure mode a declarative config can have: `http_prot = 9999` used to
    /// validate clean and then serve on 8080.
    #[test]
    fn unknown_field_is_rejected_and_suggests_the_real_ones() {
        let toml = r#"
            [agent]
            name = "Typo Agent"

            [server]
            http_prot = 9999
        "#;
        let err = AgentConfig::from_toml(toml)
            .expect_err("a mistyped key must not be silently ignored")
            .to_string();
        assert!(err.contains("http_prot"), "must name the bad key: {err}");
        assert!(
            err.contains("http_port"),
            "must list the valid keys so the fix is obvious: {err}"
        );
    }

    /// A config writes a decimal; the struct stores hundredths so the config
    /// types around it keep deriving `Eq`.
    #[test]
    fn chars_per_token_round_trips_a_decimal() {
        let toml = r#"
            [agent]
            name = "Measured"

            [handler]
            type = "llm"

            [handler.llm.context]
            mode = "context"
            chars_per_token = 3.2
        "#;
        let config = AgentConfig::from_toml(toml).expect("a measured ratio is a valid config");
        let context = config.handler.llm.expect("llm handler").context;

        assert!((context.chars_per_token.as_f32() - 3.2).abs() < 0.001);
        // Written back out as the decimal it came in as, not as hundredths —
        // `a2a` re-serializes configs (`new`, the control plane's deploy body).
        let written = toml::to_string(&context).expect("the context config serializes");
        assert!(written.contains("chars_per_token = 3.2"), "{written}");
    }

    /// Zero would divide by zero in the estimator and a negative would invert
    /// it, so both are refused where the value is read rather than clamped
    /// silently somewhere downstream.
    #[test]
    fn a_ratio_outside_the_range_is_refused_with_the_range() {
        for value in ["0.0", "-3.5", "400"] {
            let toml = format!(
                r#"
                [agent]
                name = "Nonsense"

                [handler]
                type = "llm"

                [handler.llm.context]
                chars_per_token = {value}
                "#
            );
            let err = AgentConfig::from_toml(&toml)
                .expect_err("{value} must not be accepted")
                .to_string();
            assert!(err.contains("chars_per_token"), "{err}");
        }
    }

    /// Strictness has to reach nested tables too, not just the top level.
    #[test]
    fn unknown_field_is_rejected_in_nested_tables() {
        let toml = r#"
            [agent]
            name = "Nested"

            [handler]
            type = "llm"

            [handler.llm]
            sytsem_prompt = "oops"
        "#;
        let err = AgentConfig::from_toml(toml)
            .expect_err("nested typo must be caught")
            .to_string();
        assert!(err.contains("sytsem_prompt"), "{err}");
    }

    /// The reserve comes off the ceiling, so one at or above it leaves nothing
    /// for the system prompt and the question — an agent that fails every turn
    /// on the floor alone, and says `max_input_tokens` when it does.
    #[test]
    fn a_reserve_that_eats_the_whole_ceiling_is_rejected() {
        let toml = r#"
            [agent]
            name = "Chat"

            [handler]
            type = "llm"

            [handler.llm.context]
            mode = "context"
            max_input_tokens = 8000
            reserve_for_output = 8000
        "#;
        let err = AgentConfig::from_toml(toml)
            .expect_err("a budget with nothing left for the request must not load")
            .to_string();
        assert!(err.contains("reserve_for_output"), "{err}");
        assert!(err.contains("max_input_tokens"), "{err}");
    }

    /// A pool of no connections fails every query, and the store opens on the
    /// first request rather than at startup — so it is caught at load.
    #[test]
    fn a_pool_of_no_connections_is_rejected() {
        let toml = r#"
            [agent]
            name = "Chat"

            [server.storage]
            type = "sqlx"
            url = "sqlite:tasks.db"
            max_connections = 0
        "#;
        let err = AgentConfig::from_toml(toml)
            .expect_err("a zero-connection pool must not load")
            .to_string();
        assert!(err.contains("max_connections"), "{err}");
    }

    /// `max_input_tokens = 0` is "no ceiling", so no reserve can exceed it.
    #[test]
    fn an_uncapped_budget_takes_any_reserve() {
        let toml = r#"
            [agent]
            name = "Chat"

            [handler]
            type = "llm"

            [handler.llm.context]
            max_input_tokens = 0
            reserve_for_output = 8000
        "#;
        AgentConfig::from_toml(toml).expect("no ceiling means no conflict");
    }

    fn llm_config(reasoning_line: &str) -> Result<Option<LlmConfig>, ConfigError> {
        let toml = format!(
            r#"
            [agent]
            name = "Thinker"

            [llm]
            provider = "openrouter"
            model = "z-ai/glm-4.6"
            {reasoning_line}
        "#
        );
        AgentConfig::from_toml(&toml).map(|config| config.llm)
    }

    /// Both spellings of `[llm] reasoning` reach the same request knob, and each
    /// TOML token is pinned to the variant it must produce — the tokens live
    /// here while the wire lives in `a2a-agents-common`, so nothing but a test
    /// holds the two together.
    #[test]
    fn reasoning_accepts_a_level_or_a_token_budget() {
        for (line, expected) in [
            (r#"reasoning = "off""#, Reasoning::Off),
            (
                r#"reasoning = "low""#,
                Reasoning::Effort(ReasoningEffort::Low),
            ),
            (
                r#"reasoning = "medium""#,
                Reasoning::Effort(ReasoningEffort::Medium),
            ),
            (
                r#"reasoning = "high""#,
                Reasoning::Effort(ReasoningEffort::High),
            ),
            ("reasoning = 2000", Reasoning::Budget(2000)),
        ] {
            let llm = llm_config(line)
                .unwrap_or_else(|e| panic!("`{line}` must parse: {e}"))
                .expect("[llm] present");
            assert_eq!(llm.reasoning, Some(expected), "for `{line}`");
        }
    }

    /// The JSON Schema export describes `reasoning` through a mirror type, so
    /// the mirror has to list exactly the levels the parser takes — a schema
    /// that has drifted rejects configs the agent would have run.
    #[cfg(feature = "schema")]
    #[test]
    fn reasoning_schema_matches_the_parser() {
        let schema = serde_json::to_value(schemars::schema_for!(ReasoningLevelSchema))
            .expect("schema serializes");
        let levels = schema["enum"]
            .as_array()
            .expect("a string enum of levels")
            .iter()
            .map(|v| v.as_str().expect("string level").to_string())
            .collect::<Vec<_>>();

        assert_eq!(levels, ["off", "low", "medium", "high"]);
        for level in &levels {
            level.parse::<Reasoning>().unwrap_or_else(|e| {
                panic!("schema offers `{level}` but the parser refuses it: {e}")
            });
        }
    }

    /// Omitting it must mean "leave the model alone", not a default effort the
    /// caller never asked for and pays for on every request.
    #[test]
    fn reasoning_is_absent_unless_asked_for() {
        let llm = llm_config("").expect("parses").expect("[llm] present");
        assert_eq!(llm.reasoning, None);
    }

    /// A misspelt level must fail at load. Silently ignoring it would leave the
    /// agent thinking as hard as the model's default while the config claims
    /// otherwise — the exact silence this setting exists to end.
    #[test]
    fn a_misspelt_reasoning_level_is_a_config_error() {
        let err = llm_config(r#"reasoning = "hihg""#)
            .expect_err("a bad level must not be ignored")
            .to_string();
        assert!(err.contains("reasoning"), "must name the field: {err}");
        assert!(
            err.contains(r#""off", "low", "medium", "high""#),
            "must say what is accepted, not just that this was not: {err}"
        );
    }

    #[test]
    fn check_toml_reports_unset_env_vars_instead_of_failing() {
        let toml = r#"
            [agent]
            name = "Needs Secrets"
            description = "${A2A_DEFINITELY_UNSET_ONE}"

            [server]
            http_port = 8080
            [server.auth]
            type = "bearer"
            tokens = ["${A2A_DEFINITELY_UNSET_TWO}"]
        "#;

        // Strict parsing refuses — that is right for actually running an agent.
        assert!(AgentConfig::from_toml(toml).is_err());

        // Validation still checks the shape and tells you what is missing.
        let (config, unset) = AgentConfig::check_toml(toml).expect("shape is valid");
        assert_eq!(config.agent.name, "Needs Secrets");
        assert_eq!(
            unset,
            ["A2A_DEFINITELY_UNSET_ONE", "A2A_DEFINITELY_UNSET_TWO"]
        );
    }

    /// Leniency is scoped to *missing values* — it must not weaken structural
    /// checking, or `validate` would go back to rubber-stamping typos.
    #[test]
    fn check_toml_still_rejects_unknown_fields() {
        let toml = r#"
            [agent]
            name = "Lenient But Not Lax"

            [server]
            http_prot = 1
        "#;
        assert!(AgentConfig::check_toml(toml).is_err());
    }

    #[test]
    fn check_toml_reports_nothing_when_every_ref_has_a_default() {
        let toml = r#"
            [agent]
            name = "Defaulted"
            description = "${A2A_ALSO_UNSET:-a default}"
        "#;
        let (config, unset) = AgentConfig::check_toml(toml).unwrap();
        assert_eq!(config.agent.description.as_deref(), Some("a default"));
        assert!(unset.is_empty(), "a defaulted ref is not 'unset'");
    }

    #[test]
    fn test_minimal_config() {
        let toml = r#"
            [agent]
            name = "Test Agent"
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        assert_eq!(config.agent.name, "Test Agent");
        assert_eq!(config.server.http_port, 8080);
    }

    #[test]
    fn test_complete_config() {
        let toml = r#"
            [agent]
            name = "Reimbursement Agent"
            description = "Handles employee reimbursements"
            version = "1.0.0"

            [agent.provider]
            name = "Example Corp"
            url = "https://example.com"

            [server]
            host = "0.0.0.0"
            http_port = 3000

            [server.storage]
            type = "sqlx"
            url = "sqlite:test.db"
            max_connections = 5
            enable_logging = true

            [server.auth]
            type = "bearer"
            tokens = ["token123"]
            format = "JWT"

            [[skills]]
            id = "process_expense"
            name = "Process Expense"
            description = "Process expense reimbursements"
            keywords = ["expense", "reimbursement"]
            examples = ["Reimburse my $50 lunch"]
            input_formats = ["text", "data"]
            output_formats = ["text", "data"]

            [features]
            streaming = true
            push_notifications = true
            state_history = true
            authenticated_card = false
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        assert_eq!(config.agent.name, "Reimbursement Agent");
        assert_eq!(config.server.http_port, 3000);
        assert_eq!(config.skills.len(), 1);
        assert_eq!(config.skills[0].id, "process_expense");
        assert!(config.features.streaming);
    }

    #[test]
    fn test_env_var_expansion() {
        // SAFETY: This is a test function run in a controlled environment
        // We're setting an environment variable that won't affect other tests
        unsafe {
            std::env::set_var("TEST_TOKEN", "secret123");
        }

        let content = r#"
            [server.auth]
            type = "bearer"
            tokens = ["${TEST_TOKEN}"]
        "#;

        let expanded = expand_env_vars(content).unwrap();
        assert!(expanded.contains("secret123"));
    }

    #[test]
    fn test_referenced_env_vars_dedups_and_sorts() {
        let content = r#"
            key = "${ZED_KEY}"
            url = "${db_url:-sqlite:dev.db}"
            token = "${ZED_KEY}"
            plain = "no refs here"
        "#;
        let refs: Vec<String> = referenced_env_vars(content).into_iter().collect();
        assert_eq!(refs, ["ZED_KEY", "db_url"]);
    }

    #[test]
    fn test_referenced_env_vars_empty_when_no_refs() {
        assert!(referenced_env_vars(r#"name = "plain""#).is_empty());
    }

    #[test]
    fn test_env_var_default_used_when_unset() {
        // An unset var with a `:-default` falls back to the default.
        let content = r#"model = "${A2A_UNSET_MODEL_VAR:-gpt-4o}""#;
        let expanded = expand_env_vars(content).unwrap();
        assert_eq!(expanded, r#"model = "gpt-4o""#);
    }

    #[test]
    fn test_env_var_default_ignored_when_set() {
        // SAFETY: test-only var, unique name, controlled environment.
        unsafe {
            std::env::set_var("A2A_SET_MODEL_VAR", "claude");
        }
        let content = r#"model = "${A2A_SET_MODEL_VAR:-gpt-4o}""#;
        let expanded = expand_env_vars(content).unwrap();
        assert_eq!(expanded, r#"model = "claude""#);
    }

    #[test]
    fn test_env_var_empty_default_is_allowed() {
        let content = r#"opt = "${A2A_UNSET_OPT_VAR:-}""#;
        let expanded = expand_env_vars(content).unwrap();
        assert_eq!(expanded, r#"opt = """#);
    }

    #[test]
    fn test_env_var_without_default_still_errors() {
        let content = r#"key = "${A2A_DEFINITELY_UNSET_VAR}""#;
        assert!(matches!(
            expand_env_vars(content),
            Err(ConfigError::EnvVarError(_))
        ));
    }

    #[test]
    fn test_env_var_lowercase_name_is_expanded() {
        // SAFETY: test-only var, unique name, controlled environment.
        unsafe {
            std::env::set_var("a2a_lower_var", "from_env");
        }
        let content = r#"url = "${a2a_lower_var}""#;
        let expanded = expand_env_vars(content).unwrap();
        assert_eq!(expanded, r#"url = "from_env""#);
    }

    #[test]
    fn test_env_var_lowercase_unset_still_errors() {
        // A lowercase ref must obey the same "unset with no default is a hard
        // error" contract as uppercase — not pass through literally.
        let content = r#"url = "${a2a_unset_lower_var}""#;
        assert!(matches!(
            expand_env_vars(content),
            Err(ConfigError::EnvVarError(_))
        ));
    }

    #[test]
    #[cfg(feature = "auth")]
    fn test_jwt_auth_config() {
        let toml = r#"
            [agent]
            name = "JWT Agent"

            [server.auth]
            type = "jwt"
            secret = "my-jwt-secret"
            algorithm = "HS256"
            issuer = "https://auth.example.com"
            audience = "api://my-agent"
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        match &config.server.auth {
            AuthConfig::Jwt {
                secret,
                algorithm,
                issuer,
                audience,
                ..
            } => {
                assert_eq!(secret.as_ref().unwrap(), "my-jwt-secret");
                assert_eq!(algorithm, "HS256");
                assert_eq!(issuer.as_ref().unwrap(), "https://auth.example.com");
                assert_eq!(audience.as_ref().unwrap(), "api://my-agent");
            }
            _ => panic!("Expected JWT auth config"),
        }
    }

    #[test]
    #[cfg(feature = "auth")]
    fn test_oauth2_auth_config() {
        let toml = r#"
            [agent]
            name = "OAuth2 Agent"

            [server.auth]
            type = "oauth2"
            client_id = "my-client-id"
            client_secret = "my-client-secret"
            authorization_url = "https://provider.com/auth"
            token_url = "https://provider.com/token"
            introspection_url = "https://provider.com/introspect"
            flow = "authorization_code"
            scopes = ["read", "write"]
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        match &config.server.auth {
            AuthConfig::OAuth2 {
                client_id,
                client_secret,
                introspection_url,
                flow,
                scopes,
                ..
            } => {
                assert_eq!(client_id, "my-client-id");
                assert_eq!(client_secret, "my-client-secret");
                assert_eq!(
                    introspection_url.as_deref(),
                    Some("https://provider.com/introspect")
                );
                assert_eq!(flow, "authorization_code");
                assert_eq!(scopes.len(), 2);
                assert_eq!(scopes[0], "read");
            }
            _ => panic!("Expected OAuth2 auth config"),
        }
    }

    /// A config with just a `[server]` block. Anything the fixture does not set
    /// is cleared, so what these assert does not depend on whether the
    /// developer's shell exports `A2A_ADVERTISED_URL`.
    fn server_config(server: &str) -> AgentConfig {
        let mut config = AgentConfig::from_toml(&format!(
            r#"
            [agent]
            name = "Reachable"

            [server]
            {server}
        "#
        ))
        .expect("fixture config parses");
        if !server.contains("advertised_url") {
            config.server.advertised_url = None;
        }
        config
    }

    /// A dialable bind address answers for itself, and is not a guess.
    #[test]
    fn a_bound_address_is_advertised_as_it_stands() {
        let config = server_config("host = \"10.0.0.4\"\nhttp_port = 8080");
        assert_eq!(
            config.advertised(),
            Advertised::Bound("http://10.0.0.4:8080".into())
        );
        assert!(!config.advertised().is_guess());
    }

    /// `0.0.0.0` names every interface and therefore none. Putting it on a card
    /// hands every peer an address that cannot be dialed — the failure shows up
    /// on the peer, never on the agent that published it.
    #[test]
    fn a_wildcard_bind_is_never_advertised() {
        for host in ["0.0.0.0", "::", "[::]"] {
            let config = server_config(&format!("host = \"{host}\"\nhttp_port = 8080"));
            let advertised = config.advertised();
            assert!(advertised.is_guess(), "{host}: {advertised:?}");
            assert_eq!(advertised.url(), "http://localhost:8080");
        }
    }

    /// What is written down wins over both, and outlives a trailing slash —
    /// paths are appended to this.
    #[test]
    fn a_configured_url_wins_and_is_normalized() {
        let config = server_config(
            "host = \"0.0.0.0\"\nhttp_port = 8080\nadvertised_url = \"https://agents.example.com/\"",
        );
        assert_eq!(
            config.advertised(),
            Advertised::Configured("https://agents.example.com".into())
        );
    }

    #[test]
    fn an_advertised_url_that_is_not_a_url_is_rejected() {
        let err = AgentConfig::from_toml(
            r#"
            [agent]
            name = "Reachable"

            [server]
            advertised_url = "agents.example.com:8080"
        "#,
        )
        .expect_err("a bare host:port is not a URL")
        .to_string();
        assert!(err.contains("advertised_url"), "{err}");
    }

    /// An OAuth2 agent with nowhere to check a token authenticates nobody: it
    /// binds its port, serves a card, and refuses every request. That is a
    /// config that cannot work, not a config with a default.
    #[test]
    fn oauth2_without_an_introspection_url_is_rejected() {
        let toml = r#"
            [agent]
            name = "OAuth2 Agent"

            [server.auth]
            type = "oauth2"
            client_id = "my-client-id"
            client_secret = "my-client-secret"
            authorization_url = "https://provider.com/auth"
            token_url = "https://provider.com/token"
        "#;

        let err = AgentConfig::from_toml(toml)
            .expect_err("an agent that can authenticate nobody must not load")
            .to_string();
        assert!(err.contains("introspection_url"), "{err}");
    }

    #[test]
    fn test_validation_empty_name() {
        let toml = r#"
            [agent]
            name = ""
        "#;

        let result = AgentConfig::from_toml(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_ap2_extension_config() {
        let toml = r#"
            [agent]
            name = "Merchant Agent"

            [features.extensions.ap2]
            roles = ["merchant", "payment-processor"]
            required = true
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        let ap2 = config.features.extensions.ap2.unwrap();
        assert_eq!(ap2.roles, vec!["merchant", "payment-processor"]);
        assert!(ap2.required);
    }

    #[test]
    fn test_mcp_http_config() {
        let toml = r#"
            [agent]
            name = "HTTP MCP Agent"

            [server]
            http_port = 0

            [features.mcp_server]
            enabled = true
            stdio = false

            [features.mcp_server.http]
            enabled = true
            host = "0.0.0.0"
            port = 9000
            path = "/rpc"
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        let http = &config.features.mcp_server.http;
        assert!(http.enabled);
        assert_eq!(http.host, "0.0.0.0");
        assert_eq!(http.port, 9000);
        assert_eq!(http.path, "/rpc");
        // Security knobs omitted → None (keep rmcp's loopback-only default).
        assert!(http.allowed_hosts.is_none());
        assert!(http.allowed_origins.is_none());
    }

    #[test]
    fn test_mcp_http_security_knobs() {
        let toml = r#"
            [agent]
            name = "Public MCP Agent"

            [server]
            http_port = 0

            [features.mcp_server]
            enabled = true

            [features.mcp_server.http]
            enabled = true
            allowed_hosts = ["mcp.example.com", "mcp.example.com:8000"]
            allowed_origins = ["https://app.example.com"]
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        let http = &config.features.mcp_server.http;
        assert_eq!(
            http.allowed_hosts.as_deref(),
            Some(
                [
                    "mcp.example.com".to_string(),
                    "mcp.example.com:8000".to_string()
                ]
                .as_slice()
            )
        );
        assert_eq!(
            http.allowed_origins.as_deref(),
            Some(["https://app.example.com".to_string()].as_slice())
        );
    }

    #[test]
    fn test_mcp_http_disable_host_validation() {
        // An explicit empty list parses as Some([]) — distinct from omission —
        // and disables Host validation at the transport layer.
        let toml = r#"
            [agent]
            name = "Open MCP Agent"

            [server]
            http_port = 0

            [features.mcp_server]
            enabled = true

            [features.mcp_server.http]
            enabled = true
            allowed_hosts = []
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        assert_eq!(
            config.features.mcp_server.http.allowed_hosts.as_deref(),
            Some([].as_slice())
        );
    }

    #[test]
    fn test_mcp_http_config_defaults() {
        // Omitting [features.mcp_server.http] leaves HTTP disabled with sane defaults.
        let toml = r#"
            [agent]
            name = "Stdio MCP Agent"

            [server]
            http_port = 0

            [features.mcp_server]
            enabled = true
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        let mcp = &config.features.mcp_server;
        assert!(mcp.stdio);
        assert!(!mcp.http.enabled);
        assert_eq!(mcp.http.host, "127.0.0.1");
        assert_eq!(mcp.http.port, 8000);
        assert_eq!(mcp.http.path, "/mcp");
    }

    #[test]
    fn test_ap2_extension_config_optional() {
        let toml = r#"
            [agent]
            name = "Plain Agent"
        "#;

        let config = AgentConfig::from_toml(toml).unwrap();
        assert!(config.features.extensions.ap2.is_none());
    }

    #[test]
    fn test_handler_block_llm() {
        let toml = r#"
            [agent]
            name = "LLM Agent"

            [handler]
            type = "llm"

            [handler.llm]
            system_prompt = "be brief"
            max_tool_rounds = 2
        "#;
        let config = AgentConfig::from_toml(toml).unwrap();
        assert_eq!(config.handler_type(), HandlerType::Llm);
        let llm = config.handler.llm.unwrap();
        assert_eq!(llm.system_prompt, "be brief");
        assert_eq!(llm.max_tool_rounds, 2);
    }

    #[test]
    fn test_handler_llm_remote_agents() {
        let toml = r#"
            [agent]
            name = "Orchestrator"

            [handler]
            type = "llm"

            [handler.llm]
            system_prompt = "route work to peers"

            [[handler.llm.agents]]
            name = "Weather Agent"
            url = "http://localhost:8081"
            description = "Knows the weather"

            [[handler.llm.agents]]
            name = "billing"
            url = "http://localhost:8082"
        "#;
        let config = AgentConfig::from_toml(toml).unwrap();
        let llm = config.handler.llm.unwrap();
        assert_eq!(llm.agents.len(), 2);
        assert_eq!(llm.agents[0].name, "Weather Agent");
        assert_eq!(llm.agents[0].url.as_deref(), Some("http://localhost:8081"));
        assert_eq!(
            llm.agents[0].target().unwrap(),
            RemoteAgentTarget::Url("http://localhost:8081")
        );
        assert_eq!(
            llm.agents[0].description.as_deref(),
            Some("Knows the weather")
        );
        assert_eq!(llm.agents[1].name, "billing");
        assert!(llm.agents[1].description.is_none());
    }

    #[test]
    fn test_remote_agent_skill_and_id_refs() {
        let toml = r#"
            [agent]
            name = "Orchestrator"

            [handler]
            type = "llm"

            [[handler.llm.agents]]
            name = "Weather"
            skill = "weather-lookup"

            [[handler.llm.agents]]
            name = "Billing"
            agent_id = "billing-agent"
        "#;
        let config = AgentConfig::from_toml(toml).unwrap();
        let agents = &config.handler.llm.as_ref().unwrap().agents;
        assert_eq!(
            agents[0].target().unwrap(),
            RemoteAgentTarget::Skill("weather-lookup")
        );
        assert_eq!(
            agents[1].target().unwrap(),
            RemoteAgentTarget::AgentId("billing-agent")
        );
    }

    #[test]
    fn test_remote_agent_rejects_zero_or_multiple_refs() {
        // Zero refs.
        let none = r#"
            [agent]
            name = "Orchestrator"
            [handler]
            type = "llm"
            [[handler.llm.agents]]
            name = "Nameless"
        "#;
        assert!(AgentConfig::from_toml(none).is_err());

        // Two refs.
        let both = r#"
            [agent]
            name = "Orchestrator"
            [handler]
            type = "llm"
            [[handler.llm.agents]]
            name = "Ambiguous"
            url = "http://localhost:8081"
            skill = "weather-lookup"
        "#;
        assert!(AgentConfig::from_toml(both).is_err());
    }

    /// Two peers whose names slugify the same way advertise one tool, and only
    /// one of them can ever be called. The names differ, so nothing else in the
    /// config looks wrong.
    #[test]
    fn two_remote_agents_deriving_one_tool_name_are_refused() {
        let toml = r#"
            [agent]
            name = "Orchestrator"
            [handler]
            type = "llm"
            [[handler.llm.agents]]
            name = "Weather Agent"
            url = "http://localhost:8081"
            [[handler.llm.agents]]
            name = "weather-agent"
            url = "http://localhost:8082"
        "#;
        let err = AgentConfig::from_toml(toml).expect_err("a derived tool-name clash is an error");
        let message = err.to_string();
        // Both entries by name, and the name they collide on: renaming needs to
        // know which two and what they became.
        assert!(message.contains("Weather Agent"), "{message}");
        assert!(message.contains("weather-agent"), "{message}");
        assert!(message.contains("ask_weather_agent"), "{message}");
    }

    /// Distinct slugs are fine, including ones that only differ past the point
    /// slugify normalizes.
    #[test]
    fn remote_agents_with_distinct_tool_names_load() {
        let toml = r#"
            [agent]
            name = "Orchestrator"
            [handler]
            type = "llm"
            [[handler.llm.agents]]
            name = "Weather Agent"
            url = "http://localhost:8081"
            [[handler.llm.agents]]
            name = "Billing v2"
            url = "http://localhost:8082"
        "#;
        assert!(AgentConfig::from_toml(toml).is_ok());
    }

    #[test]
    fn test_handler_falls_back_to_implementation() {
        let toml = r#"
            [agent]
            name = "Legacy Agent"
            implementation = "reimbursement"
        "#;
        let config = AgentConfig::from_toml(toml).unwrap();
        assert_eq!(config.handler_type(), HandlerType::Reimbursement);
    }

    #[test]
    fn test_handler_defaults_to_echo() {
        let toml = r#"
            [agent]
            name = "Plain Agent"
        "#;
        let config = AgentConfig::from_toml(toml).unwrap();
        assert_eq!(config.handler_type(), HandlerType::Echo);
    }

    #[test]
    fn test_handler_custom_type_round_trips() {
        let toml = r#"
            [agent]
            name = "Custom Agent"

            [handler]
            type = "weather"
        "#;
        let config = AgentConfig::from_toml(toml).unwrap();
        assert_eq!(
            config.handler_type(),
            HandlerType::Custom("weather".to_string())
        );
        assert_eq!(config.handler.r#type.as_str(), "weather");
    }

    #[test]
    fn an_agent_can_bring_its_own_image() {
        let toml = r#"
            [agent]
            name = "Billing Agent"

            [handler]
            type = "billing"

            [runtime]
            image = "ghcr.io/acme/billing:1.4"
        "#;
        let config = AgentConfig::from_toml(toml).unwrap();
        assert_eq!(config.image(), Some("ghcr.io/acme/billing:1.4"));
        // The handler name is still the config's own: what resolves it is the
        // image, not this binary's table of built-ins.
        assert_eq!(
            config.handler_type(),
            HandlerType::Custom("billing".to_string())
        );
    }

    #[test]
    fn most_agents_bring_no_image() {
        let config = AgentConfig::from_toml("[agent]\nname = \"Plain\"\n").unwrap();
        assert_eq!(config.image(), None);
    }

    /// A blank image reaches the engine as an empty argument, where it fails as
    /// an unreadable `create` error instead of as the typo it is.
    #[test]
    fn a_blank_image_is_a_config_error() {
        for image in ["", "   "] {
            let toml = format!("[agent]\nname = \"Blank\"\n\n[runtime]\nimage = \"{image}\"\n");
            let err = AgentConfig::from_toml(&toml).expect_err("a blank image must not parse");
            assert!(err.to_string().contains("image"), "{err}");
        }
    }
}
