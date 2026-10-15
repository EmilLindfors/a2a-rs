//! Decision models: a typed answer and its probability, instead of text.
//!
//! A decision model is asked named questions about a `state` and returns one
//! answer per question. A [`Question::Noul`] is answered with the probability
//! of yes, a [`Question::Choice`] with one of the options it offers, and a
//! [`Question::Score`] with a position on an ordered scale. The model
//! generates no text and calls no tools.
//!
//! TypeSafe's Jev, Perplexity's `pplx-decider` and Mapika's open `decider`
//! models all speak one wire format, which TypeSafe calls System One. Each
//! host serves it at a different path. [`DecisionProvider`] is that set of
//! hosts, and [`HttpDecisionModel`] is the one adapter for all of them.
//!
//! [`DecisionModel`] is the port, separate from [`LlmProvider`](crate::LlmProvider)
//! because nothing about a chat request or a chat response carries over.
//! [`decision_from_settings`] builds one from a host's configuration with no
//! I/O, so a host can check a config before it runs.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Env, describe_transport_error};

/// The most options a [`Question::Choice`] may offer. Perplexity's documented
/// limit; the open `decider` models share it.
pub const MAX_CHOICE_OPTIONS: usize = 255;

/// The most levels a [`Question::Score`] may have. TypeSafe's and Perplexity's
/// documented limit.
pub const MAX_SCORE_LEVELS: usize = 10;

/// How long one decision may take. Decision models answer in well under a
/// second; a minute covers a host in its long-context mode.
const DECISION_DEADLINE: Duration = Duration::from_secs(60);

/// One question, in the wire's spelling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Question {
    /// A yes-or-no proposition. The answer is the probability that it holds.
    Noul {
        /// The proposition, phrased so that yes is a true statement about the
        /// state.
        instructions: String,
        /// What counts as yes and what counts as no, when the instructions
        /// alone leave it open.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    /// One option out of a set. The answer names the option and gives a
    /// probability for each.
    Choice {
        /// What is being chosen.
        instructions: String,
        /// Each option's name and what it covers.
        criteria: BTreeMap<String, String>,
    },
    /// A position on an ordered scale. The answer is the probability-weighted
    /// index of the levels, `0.0` for the first.
    Score {
        /// What is being scored.
        instructions: String,
        /// The levels, lowest first. A level's index is its score.
        criteria: Vec<String>,
    },
}

/// What a [`Question::Noul`] counts as yes and as no.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NoulCriteria {
    /// What a yes looks like.
    #[serde(rename = "true")]
    pub yes: String,
    /// What a no looks like.
    #[serde(rename = "false")]
    pub no: String,
}

/// The questions one decision asks, by name. The name is the key its answer
/// comes back under.
pub type Questions = BTreeMap<String, Question>;

impl Question {
    /// The wire name of this question's type, which its answer carries too.
    pub fn kind(&self) -> &'static str {
        match self {
            Question::Noul { .. } => "noul",
            Question::Choice { .. } => "choice",
            Question::Score { .. } => "score",
        }
    }

    /// What the question asks.
    pub fn instructions(&self) -> &str {
        match self {
            Question::Noul { instructions, .. }
            | Question::Choice { instructions, .. }
            | Question::Score { instructions, .. } => instructions,
        }
    }
}

/// Why a set of questions cannot be asked, decided without asking.
///
/// Every documented host refuses these, so a host application can check
/// them when its configuration loads rather than on the first request.
pub fn check_questions(questions: &Questions) -> Result<(), String> {
    if questions.is_empty() {
        return Err("asks no questions".to_string());
    }
    for (name, question) in questions {
        if name.trim().is_empty() {
            return Err("has a question with a blank name".to_string());
        }
        if question.instructions().trim().is_empty() {
            return Err(format!("question `{name}` has no instructions"));
        }
        match question {
            Question::Noul { .. } => {}
            // One option is not a choice: its probability is 1 whatever the
            // state says.
            Question::Choice { criteria, .. } => {
                if !(2..=MAX_CHOICE_OPTIONS).contains(&criteria.len()) {
                    return Err(format!(
                        "choice `{name}` offers {} options; it needs 2 to {MAX_CHOICE_OPTIONS}",
                        criteria.len()
                    ));
                }
            }
            Question::Score { criteria, .. } => {
                if !(2..=MAX_SCORE_LEVELS).contains(&criteria.len()) {
                    return Err(format!(
                        "score `{name}` has {} levels; it needs 2 to {MAX_SCORE_LEVELS}",
                        criteria.len()
                    ));
                }
            }
        }
    }
    Ok(())
}

/// One answer, as the model returned it.
///
/// Fields a host adds beyond these are dropped. Mapika's server adds
/// `certainty` and `level_fit`, and no other host has them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    /// The probability that the proposition holds.
    Noul {
        /// Between 0 and 1. Near 0.5 is the model saying it cannot tell.
        noul: f64,
    },
    /// The option the model picked.
    Choice {
        /// The most probable option.
        choice: String,
        /// How concentrated the distribution is, 0 to 1. Not the chance the
        /// choice is right.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confidence: Option<f64>,
        /// Each option's probability.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        probabilities: BTreeMap<String, f64>,
    },
    /// Where the state sits on the scale.
    Score {
        /// The probability-weighted level index.
        score: f64,
        /// How concentrated the distribution is, 0 to 1.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confidence: Option<f64>,
        /// Each level's probability, keyed by its index.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        probabilities: BTreeMap<String, f64>,
    },
}

impl Answer {
    /// The wire name of this answer's type.
    pub fn kind(&self) -> &'static str {
        match self {
            Answer::Noul { .. } => "noul",
            Answer::Choice { .. } => "choice",
            Answer::Score { .. } => "score",
        }
    }
}

/// What one decision cost, as the host reported it.
///
/// Integers rather than the `Option`s of [`TokenUsage`](crate::TokenUsage):
/// every documented host requires both counts in its response schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionUsage {
    /// Tokens of state and questions read.
    pub input_tokens: u64,
    /// Tokens written. Decision hosts bill none.
    pub output_tokens: u64,
    /// The charge in the host's currency, when it says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
}

/// The answers to one set of questions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    /// The model that answered, as the host names it. A host serving an alias
    /// reports the dated snapshot here.
    pub model: String,
    /// One answer per question, under the question's name.
    pub answers: BTreeMap<String, Answer>,
    /// What it cost, when the host said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<DecisionUsage>,
}

impl Decision {
    /// Whether this answers what was asked: every question once, with an
    /// answer of its type, and a choice that is one of the options.
    ///
    /// Checked rather than trusted to the host: a reply that drops a question
    /// still parses, and a caller reading the answer would find the key
    /// missing with nothing saying why.
    pub fn check(&self, questions: &Questions) -> Result<(), DecisionError> {
        for (name, question) in questions {
            let answer = self
                .answers
                .get(name)
                .ok_or_else(|| DecisionError::Mismatch(format!("no answer to `{name}`")))?;
            if answer.kind() != question.kind() {
                return Err(DecisionError::Mismatch(format!(
                    "`{name}` is a {} question and the answer is a {}",
                    question.kind(),
                    answer.kind()
                )));
            }
            if let (Question::Choice { criteria, .. }, Answer::Choice { choice, .. }) =
                (question, answer)
                && !criteria.contains_key(choice)
            {
                return Err(DecisionError::Mismatch(format!(
                    "`{name}` chose {choice:?}, which is not one of its options"
                )));
            }
        }
        if let Some(extra) = self.answers.keys().find(|k| !questions.contains_key(*k)) {
            return Err(DecisionError::Mismatch(format!(
                "an answer to `{extra}`, which was not asked"
            )));
        }
        Ok(())
    }
}

/// Why a decision could not be had.
#[derive(Debug, thiserror::Error)]
pub enum DecisionError {
    /// The host could not be reached, or did not answer in time.
    #[error("could not reach the decision model: {0}")]
    Transport(String),
    /// The host answered with an error.
    #[error("the decision model refused ({status}): {message}")]
    Refused {
        /// The HTTP status.
        status: u16,
        /// The host's own words, or the body when it sent none.
        message: String,
    },
    /// The host answered with something that is not a decision.
    #[error("the decision model's reply did not parse: {0}")]
    Malformed(String),
    /// The host answered, but not the questions that were asked.
    #[error("the decision model did not answer what was asked: {0}")]
    Mismatch(String),
}

/// Something that answers typed questions about a state.
#[async_trait]
pub trait DecisionModel: Send + Sync {
    /// Answer `questions` about `state`. A JSON string is text; an object or
    /// an array is passed as the structure it is.
    async fn decide(&self, state: &Value, questions: &Questions)
    -> Result<Decision, DecisionError>;
}

/// The in-memory adapter: answers every request with the decision it was
/// built with.
///
/// For a host that has to run with no model behind it, and for testing what
/// a caller does with a decision.
#[derive(Debug, Clone)]
pub struct FixedDecision(pub Decision);

#[async_trait]
impl DecisionModel for FixedDecision {
    async fn decide(
        &self,
        _state: &Value,
        _questions: &Questions,
    ) -> Result<Decision, DecisionError> {
        Ok(self.0.clone())
    }
}

/// Which host a decision goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionProvider {
    /// OpenRouter's Decisions API, which serves Jev and other hosted models.
    OpenRouter,
    /// Perplexity's Decisions API, serving `pplx-decider`.
    Perplexity,
    /// Any server speaking TypeSafe's System One format at a URL the settings
    /// give: TypeSafe itself, or Mapika's `decider` serving locally.
    SystemOne,
}

/// Every name [`DecisionProvider::from_name`] accepts, for the error that
/// lists them.
pub const DECISION_PROVIDERS: [&str; 3] = ["openrouter", "perplexity", "systemone"];

impl DecisionProvider {
    /// The provider a configuration names, or `None` for a name no adapter
    /// serves.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "openrouter" => Some(Self::OpenRouter),
            "perplexity" => Some(Self::Perplexity),
            "systemone" => Some(Self::SystemOne),
            _ => None,
        }
    }

    /// The name a configuration spells it with.
    pub fn name(self) -> &'static str {
        match self {
            Self::OpenRouter => "openrouter",
            Self::Perplexity => "perplexity",
            Self::SystemOne => "systemone",
        }
    }

    /// The endpoint when the settings name none.
    pub fn default_url(self) -> Option<&'static str> {
        match self {
            Self::OpenRouter => Some("https://openrouter.ai/api/alpha/decisions"),
            Self::Perplexity => Some("https://api.perplexity.ai/v1/decisions"),
            Self::SystemOne => None,
        }
    }

    /// The variable the key is read from when the settings give none.
    ///
    /// A System One server may need no key, so none is read ambiently for
    /// it: a key meant for one host, sent to whatever `url` names, is a leak.
    pub fn key_var(self) -> Option<&'static str> {
        match self {
            Self::OpenRouter => Some("OPENROUTER_API_KEY"),
            Self::Perplexity => Some("PERPLEXITY_API_KEY"),
            Self::SystemOne => None,
        }
    }
}

/// Decision model settings from a host's configuration.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct DecisionSettings {
    /// One of [`DECISION_PROVIDERS`].
    pub provider: String,
    /// The model, as the host names it: `typesafe/jev-1.13` on OpenRouter,
    /// `pplx-decider-v1.1-27b` on Perplexity.
    pub model: String,
    /// The bearer token. When `None`, [`DecisionProvider::key_var`] is read.
    pub api_key: Option<String>,
    /// The full endpoint. Required for `systemone`; the hosted providers
    /// default to their own.
    pub url: Option<String>,
}

impl std::fmt::Debug for DecisionSettings {
    /// Hand-written to keep `api_key` out of the output, as for
    /// [`LlmSettings`](crate::LlmSettings).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecisionSettings")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("url", &self.url)
            .finish()
    }
}

/// A decision provider is named but cannot be built. Raised before any
/// request is made.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecisionConfigError {
    /// The provider is known and its settings cannot be used: no key, no URL.
    #[error("the {provider} decision provider is not usable: {detail}")]
    Unusable {
        /// Which provider.
        provider: &'static str,
        /// What is wrong with its settings.
        detail: String,
    },
    /// [`DecisionSettings::provider`] names something no adapter serves.
    #[error(
        "unsupported decision provider {name:?}; expected one of: {}",
        DECISION_PROVIDERS.join(", ")
    )]
    Unsupported {
        /// The provider string as configured.
        name: String,
    },
}

/// Where decisions are sent and how they authenticate.
#[derive(Clone, PartialEq, Eq)]
pub struct DecisionEndpoint {
    /// The host.
    pub provider: DecisionProvider,
    /// The model asked for.
    pub model: String,
    /// The full URL requests are posted to.
    pub url: String,
    /// The bearer token, when there is one.
    pub api_key: Option<String>,
    /// Where the key came from: `"config"`, a variable's name, or `None` when
    /// there is no key.
    pub key_from: Option<&'static str>,
}

impl std::fmt::Debug for DecisionEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecisionEndpoint")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("url", &self.url)
            .field("key_from", &self.key_from)
            .finish_non_exhaustive()
    }
}

/// A decision model built from [`DecisionSettings`], and where it sends.
#[derive(Clone)]
pub struct SelectedDecision {
    /// The model to ask.
    pub model: Arc<dyn DecisionModel>,
    /// Where it was resolved to, for a host's report.
    pub endpoint: DecisionEndpoint,
}

/// Build the decision model `settings` describe, reading the provider's key
/// variable when the settings give no key.
///
/// No I/O beyond the environment, so a host can run it to check a config.
pub fn decision_from_settings(
    settings: &DecisionSettings,
) -> Result<SelectedDecision, DecisionConfigError> {
    let endpoint = resolve_endpoint(settings, Env::os())?;
    let model =
        HttpDecisionModel::new(endpoint.clone()).map_err(|e| DecisionConfigError::Unusable {
            provider: endpoint.provider.name(),
            detail: e.to_string(),
        })?;
    Ok(SelectedDecision {
        model: Arc::new(model),
        endpoint,
    })
}

fn resolve_endpoint(
    settings: &DecisionSettings,
    env: Env<'_>,
) -> Result<DecisionEndpoint, DecisionConfigError> {
    let provider = DecisionProvider::from_name(&settings.provider).ok_or_else(|| {
        DecisionConfigError::Unsupported {
            name: settings.provider.clone(),
        }
    })?;
    let unusable = |detail: String| DecisionConfigError::Unusable {
        provider: provider.name(),
        detail,
    };
    if settings.model.trim().is_empty() {
        return Err(unusable("no model is named".to_string()));
    }
    let url = match (settings.url.as_deref(), provider.default_url()) {
        (Some(url), _) | (None, Some(url)) => url.to_string(),
        (None, None) => {
            return Err(unusable(
                "it needs the full endpoint URL, such as http://127.0.0.1:8000/v1/systemone"
                    .to_string(),
            ));
        }
    };
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(unusable(format!("{url:?} is not an http(s) address")));
    }
    let configured = settings.api_key.as_deref().filter(|k| !k.trim().is_empty());
    let (api_key, key_from) = match (configured, provider.key_var()) {
        (Some(key), _) => (Some(key.to_string()), Some("config")),
        (None, Some(var)) => match env.get(var) {
            Some(key) => (Some(key), Some(var)),
            None => return Err(unusable(format!("no key is configured and {var} is unset"))),
        },
        (None, None) => (None, None),
    };
    Ok(DecisionEndpoint {
        provider,
        model: settings.model.clone(),
        url,
        api_key,
        key_from,
    })
}

/// The one adapter for every [`DecisionProvider`]: a JSON POST of
/// `{model, state, questions}`, answered with `{model, answers, usage}`.
pub struct HttpDecisionModel {
    client: reqwest::Client,
    endpoint: DecisionEndpoint,
}

impl HttpDecisionModel {
    /// A client for `endpoint`. Fails only when the HTTP client cannot be
    /// built, which is a missing CA bundle.
    pub fn new(endpoint: DecisionEndpoint) -> Result<Self, DecisionError> {
        let client = reqwest::Client::builder()
            .timeout(DECISION_DEADLINE)
            .build()
            .map_err(|e| {
                DecisionError::Transport(format!(
                    "building the HTTP client: {}",
                    describe_transport_error(&e)
                ))
            })?;
        Ok(Self { client, endpoint })
    }

    /// Where this client sends.
    pub fn endpoint(&self) -> &DecisionEndpoint {
        &self.endpoint
    }
}

/// The error body every documented host sends: `{"error": {"message": …}}`.
#[derive(Deserialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    message: String,
}

#[async_trait]
impl DecisionModel for HttpDecisionModel {
    async fn decide(
        &self,
        state: &Value,
        questions: &Questions,
    ) -> Result<Decision, DecisionError> {
        let body = serde_json::json!({
            "model": self.endpoint.model,
            "state": state,
            "questions": questions,
        });
        let mut request = self.client.post(&self.endpoint.url).json(&body);
        if let Some(key) = &self.endpoint.api_key {
            request = request.bearer_auth(key);
        }
        let response = request
            .send()
            .await
            .map_err(|e| DecisionError::Transport(describe_transport_error(&e)))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| DecisionError::Transport(describe_transport_error(&e)))?;
        if !status.is_success() {
            let message = serde_json::from_str::<ErrorBody>(&text)
                .map(|b| b.error.message)
                .unwrap_or(text);
            return Err(DecisionError::Refused {
                status: status.as_u16(),
                message,
            });
        }
        let decision: Decision =
            serde_json::from_str(&text).map_err(|e| DecisionError::Malformed(e.to_string()))?;
        decision.check(questions)?;
        Ok(decision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn questions(value: Value) -> Questions {
        serde_json::from_value(value).expect("questions parse")
    }

    fn triage() -> Questions {
        questions(json!({
            "refund": {"type": "noul", "instructions": "a refund is requested"},
            "team": {"type": "choice", "instructions": "who",
                     "criteria": {"billing": "money", "technical": "bugs"}},
        }))
    }

    #[test]
    fn a_noul_criteria_is_sent_under_true_and_false() {
        let q = questions(json!({
            "refund": {"type": "noul", "instructions": "a refund is requested",
                       "criteria": {"true": "asks for money back", "false": "anything else"}},
        }));
        assert_eq!(
            serde_json::to_value(&q["refund"]).unwrap()["criteria"],
            json!({"true": "asks for money back", "false": "anything else"})
        );
    }

    #[test]
    fn a_misspelled_field_is_refused() {
        let err = serde_json::from_value::<Questions>(json!({
            "team": {"type": "choice", "instructions": "who", "options": {"a": "x", "b": "y"}},
        }))
        .unwrap_err();
        assert!(err.to_string().contains("options"), "{err}");
    }

    #[test]
    fn a_choice_of_one_and_a_scale_of_eleven_are_refused() {
        let one = questions(json!({
            "team": {"type": "choice", "instructions": "who", "criteria": {"billing": "money"}},
        }));
        assert!(check_questions(&one).unwrap_err().contains("1 options"));

        let levels: Vec<String> = (0..11).map(|i| format!("level {i}")).collect();
        let eleven = questions(json!({
            "urgency": {"type": "score", "instructions": "how soon", "criteria": levels},
        }));
        assert!(check_questions(&eleven).unwrap_err().contains("11 levels"));
        assert!(check_questions(&Questions::new()).is_err());
        check_questions(&triage()).unwrap();
    }

    fn decision(answers: Value) -> Decision {
        serde_json::from_value(json!({"model": "m", "answers": answers})).unwrap()
    }

    #[test]
    fn a_decision_that_answers_what_was_asked_passes_and_drops_extra_fields() {
        let d = decision(json!({
            "refund": {"type": "noul", "noul": 0.97},
            "team": {"type": "choice", "choice": "billing", "confidence": 0.9,
                     "probabilities": {"billing": 0.95, "technical": 0.05},
                     "x_p_max": 0.95},
        }));
        d.check(&triage()).unwrap();
        assert!(serde_json::to_string(&d).unwrap().find("x_p_max").is_none());
    }

    #[test]
    fn a_decision_that_drops_mistypes_or_invents_is_refused() {
        let dropped = decision(json!({"refund": {"type": "noul", "noul": 0.9}}));
        assert!(
            dropped
                .check(&triage())
                .unwrap_err()
                .to_string()
                .contains("no answer to `team`")
        );

        let mistyped = decision(json!({
            "refund": {"type": "score", "score": 1.0},
            "team": {"type": "choice", "choice": "billing"},
        }));
        assert!(
            mistyped
                .check(&triage())
                .unwrap_err()
                .to_string()
                .contains("noul question")
        );

        let invented = decision(json!({
            "refund": {"type": "noul", "noul": 0.9},
            "team": {"type": "choice", "choice": "sales"},
        }));
        assert!(
            invented
                .check(&triage())
                .unwrap_err()
                .to_string()
                .contains("\"sales\"")
        );
    }

    fn settings(provider: &str, url: Option<&str>) -> DecisionSettings {
        DecisionSettings {
            provider: provider.to_string(),
            model: "typesafe/jev-1.13".to_string(),
            api_key: None,
            url: url.map(str::to_string),
        }
    }

    #[test]
    fn openrouter_reads_its_key_from_the_environment_and_names_where_it_came_from() {
        let lookup = |var: &str| (var == "OPENROUTER_API_KEY").then(|| "sk-or".to_string());
        let resolved = resolve_endpoint(&settings("openrouter", None), Env::new(&lookup)).unwrap();
        assert_eq!(resolved.url, "https://openrouter.ai/api/alpha/decisions");
        assert_eq!(resolved.api_key.as_deref(), Some("sk-or"));
        assert_eq!(resolved.key_from, Some("OPENROUTER_API_KEY"));
        assert!(!format!("{resolved:?}").contains("sk-or"));
    }

    #[test]
    fn a_hosted_provider_without_a_key_is_refused() {
        let none = |_: &str| None;
        let err = resolve_endpoint(&settings("perplexity", None), Env::new(&none)).unwrap_err();
        assert!(err.to_string().contains("PERPLEXITY_API_KEY"), "{err}");
    }

    #[test]
    fn systemone_needs_a_url_and_reads_no_key_ambiently() {
        let any = |_: &str| Some("a key for some other host".to_string());
        assert!(resolve_endpoint(&settings("systemone", None), Env::new(&any)).is_err());
        let local = resolve_endpoint(
            &settings("systemone", Some("http://127.0.0.1:8000/v1/systemone")),
            Env::new(&any),
        )
        .unwrap();
        assert_eq!(local.api_key, None);
    }

    #[test]
    fn an_unknown_provider_lists_the_known_ones() {
        let none = |_: &str| None;
        let err = resolve_endpoint(&settings("typesafe", None), Env::new(&none)).unwrap_err();
        assert!(
            err.to_string()
                .contains("openrouter, perplexity, systemone"),
            "{err}"
        );
    }
}
