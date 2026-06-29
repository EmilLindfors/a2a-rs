//! Agent registry / discovery.
//!
//! The [`AgentRegistry`] port is a **platform capability**: it stores the
//! [`AgentCard`]s of known agents alongside a dialable endpoint, so an
//! orchestrator can find peers by **skill** instead of a hard-coded URL. This is
//! the discovery half of the multi-agent platform — the delegation half is
//! [`A2aAgentToolSource`](crate::A2aAgentToolSource).
//!
//! Per the hexagonal rules this lives in the platform layer (never in the pure
//! `a2a-rs` protocol crate): a capability port plus an in-memory adapter. A
//! future sqlx- or control-plane-backed adapter is a drop-in behind the same
//! port — which is why every method returns [`Result`] even though
//! [`InMemoryAgentRegistry`] never actually fails.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::RwLock;

use a2a_rs::domain::AgentCard;

/// A stable, user-predictable identifier for a registered agent.
///
/// Derived by slugifying the agent's name (lowercase, non-alphanumeric → `-`),
/// so a config can reference a peer with `agent_id = "weather-agent"` and have
/// it resolve deterministically.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentId(String);

impl AgentId {
    /// Derive an id from a free-form agent name.
    pub fn from_name(name: &str) -> Self {
        Self(crate::utils::slugify(name, '-'))
    }

    /// The id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// All conversions route through `from_name` so a raw lookup key (an HTTP path
// param, a config `agent_id` ref) canonicalizes to the same slug the stored id
// was built from. `slugify` is idempotent, so converting an already-canonical
// slug is a no-op; without this, `agent_id = "Weather Agent"` would silently
// miss a registry whose key is `weather-agent`.
impl From<&str> for AgentId {
    fn from(s: &str) -> Self {
        Self::from_name(s)
    }
}

impl From<String> for AgentId {
    fn from(s: String) -> Self {
        Self::from_name(&s)
    }
}

impl FromStr for AgentId {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_name(s))
    }
}

/// An agent known to the registry: its discovery metadata ([`AgentCard`]) plus
/// the endpoint to dial.
///
/// `endpoint` is kept separate from `card.url` on purpose: the externally
/// reachable address can differ from what the card advertises (NAT, container
/// networking), so the registry dials `endpoint` and discovers against `card`.
#[derive(Debug, Clone)]
pub struct RegisteredAgent {
    /// Registry-assigned id (derived from the card's name).
    pub id: AgentId,
    /// The agent's published card — name, description, skills, capabilities.
    pub card: AgentCard,
    /// Dialable base URL for the agent's A2A endpoint.
    pub endpoint: String,
}

/// Errors a registry operation can return.
#[derive(Debug, Error)]
pub enum RegistryError {
    /// No agent is registered under the given id.
    #[error("no agent registered with id '{0}'")]
    NotFound(AgentId),
}

/// The discovery capability the platform needs: register agents and find them
/// by id or skill. One trait per capability (hex rule 2); implemented by
/// [`InMemoryAgentRegistry`] today, a persistent adapter later.
#[async_trait]
pub trait AgentRegistry: Send + Sync {
    /// Register (or replace) an agent. The id is derived from the card's name;
    /// re-registering the same name upserts (last-writer-wins), which keeps a
    /// future card-refresh loop idempotent.
    async fn register(&self, card: AgentCard, endpoint: String) -> Result<AgentId, RegistryError>;

    /// Remove an agent. Returns [`RegistryError::NotFound`] if it was not
    /// registered.
    async fn deregister(&self, id: &AgentId) -> Result<(), RegistryError>;

    /// Look up an agent by id; `Ok(None)` when absent.
    async fn get(&self, id: &AgentId) -> Result<Option<RegisteredAgent>, RegistryError>;

    /// Find every agent whose card advertises a matching skill. A skill matches
    /// when its `id` or any of its `tags` equals `skill`, case-insensitively.
    async fn find_by_skill(&self, skill: &str) -> Result<Vec<RegisteredAgent>, RegistryError>;

    /// List every registered agent.
    async fn list(&self) -> Result<Vec<RegisteredAgent>, RegistryError>;
}

/// In-memory [`AgentRegistry`] adapter — the default, infra-free implementation.
///
/// A first-class type (not test-only, per hex rule 6): services and tests run
/// against it without standing up external infrastructure. Cheap to `clone`
/// (shares one map); reads (`find_by_skill`, `get`) dominate writes, hence the
/// [`RwLock`].
#[derive(Clone, Default)]
pub struct InMemoryAgentRegistry {
    agents: Arc<RwLock<HashMap<AgentId, RegisteredAgent>>>,
}

impl InMemoryAgentRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }
}

/// True if `card` advertises a skill matching `query` by id or tag,
/// case-insensitively.
fn card_has_skill(card: &AgentCard, query: &str) -> bool {
    card.skills.iter().any(|skill| {
        skill.id.eq_ignore_ascii_case(query)
            || skill.tags.iter().any(|tag| tag.eq_ignore_ascii_case(query))
    })
}

#[async_trait]
impl AgentRegistry for InMemoryAgentRegistry {
    async fn register(&self, card: AgentCard, endpoint: String) -> Result<AgentId, RegistryError> {
        let id = AgentId::from_name(&card.name);
        let entry = RegisteredAgent {
            id: id.clone(),
            card,
            endpoint,
        };
        self.agents.write().await.insert(id.clone(), entry);
        Ok(id)
    }

    async fn deregister(&self, id: &AgentId) -> Result<(), RegistryError> {
        self.agents
            .write()
            .await
            .remove(id)
            .map(|_| ())
            .ok_or_else(|| RegistryError::NotFound(id.clone()))
    }

    async fn get(&self, id: &AgentId) -> Result<Option<RegisteredAgent>, RegistryError> {
        Ok(self.agents.read().await.get(id).cloned())
    }

    async fn find_by_skill(&self, skill: &str) -> Result<Vec<RegisteredAgent>, RegistryError> {
        Ok(self
            .agents
            .read()
            .await
            .values()
            .filter(|agent| card_has_skill(&agent.card, skill))
            .cloned()
            .collect())
    }

    async fn list(&self) -> Result<Vec<RegisteredAgent>, RegistryError> {
        Ok(self.agents.read().await.values().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_rs::domain::AgentSkill;

    fn card_with_skills(name: &str, skills: Vec<AgentSkill>) -> AgentCard {
        let mut card = AgentCard {
            name: name.to_string(),
            ..Default::default()
        };
        card.skills = skills;
        card
    }

    #[test]
    fn agent_id_is_slugified_from_name() {
        assert_eq!(
            AgentId::from_name("Weather Agent").as_str(),
            "weather-agent"
        );
        assert_eq!(AgentId::from_name("billing").as_str(), "billing");
    }

    #[test]
    fn agent_id_conversions_canonicalize_lookup_keys() {
        // A raw lookup key (HTTP path param, config `agent_id` ref) must
        // canonicalize to the same slug `from_name` produced, or lookups
        // silently miss.
        let canonical = AgentId::from_name("Weather Agent");
        assert_eq!(AgentId::from("Weather Agent"), canonical);
        assert_eq!(AgentId::from("Weather Agent".to_string()), canonical);
        assert_eq!("Weather Agent".parse::<AgentId>().unwrap(), canonical);
        // already-canonical key is a no-op
        assert_eq!(AgentId::from("weather-agent"), canonical);
    }

    #[tokio::test]
    async fn get_resolves_non_canonical_lookup_key() {
        let reg = InMemoryAgentRegistry::new();
        reg.register(card_with_skills("Weather Agent", vec![]), "http://w".into())
            .await
            .unwrap();

        // Looking up by the raw, non-canonical name resolves to the slugified
        // entry rather than silently missing.
        let got = reg.get(&AgentId::from("Weather Agent")).await.unwrap();
        assert!(got.is_some(), "non-canonical key should resolve");
        assert_eq!(got.unwrap().card.name, "Weather Agent");
    }

    #[tokio::test]
    async fn register_then_get_round_trips() {
        let reg = InMemoryAgentRegistry::new();
        let card = card_with_skills("Weather Agent", vec![]);
        let id = reg
            .register(card, "http://127.0.0.1:9000".into())
            .await
            .unwrap();
        assert_eq!(id.as_str(), "weather-agent");

        let got = reg.get(&id).await.unwrap().expect("registered agent");
        assert_eq!(got.endpoint, "http://127.0.0.1:9000");
        assert_eq!(got.card.name, "Weather Agent");

        assert!(reg.get(&AgentId::from("missing")).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn find_by_skill_matches_id_and_tag_case_insensitively() {
        let reg = InMemoryAgentRegistry::new();
        let skill = AgentSkill::new(
            "weather-lookup".into(),
            "Weather lookup".into(),
            "Looks up the weather".into(),
            vec!["forecast".into(), "meteorology".into()],
        );
        reg.register(
            card_with_skills("Weather Agent", vec![skill]),
            "http://w".into(),
        )
        .await
        .unwrap();
        reg.register(card_with_skills("Idle Agent", vec![]), "http://i".into())
            .await
            .unwrap();

        // by skill id (case-insensitive)
        let by_id = reg.find_by_skill("Weather-Lookup").await.unwrap();
        assert_eq!(by_id.len(), 1);
        assert_eq!(by_id[0].card.name, "Weather Agent");

        // by tag (case-insensitive)
        let by_tag = reg.find_by_skill("FORECAST").await.unwrap();
        assert_eq!(by_tag.len(), 1);

        // no match
        assert!(reg.find_by_skill("billing").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn find_by_skill_returns_all_matches() {
        let reg = InMemoryAgentRegistry::new();
        let mk =
            |id: &str| AgentSkill::new(id.into(), "S".into(), "d".into(), vec!["shared".into()]);
        reg.register(card_with_skills("A", vec![mk("a")]), "http://a".into())
            .await
            .unwrap();
        reg.register(card_with_skills("B", vec![mk("b")]), "http://b".into())
            .await
            .unwrap();

        let matches = reg.find_by_skill("shared").await.unwrap();
        assert_eq!(matches.len(), 2);
    }

    #[tokio::test]
    async fn register_upserts_by_name() {
        let reg = InMemoryAgentRegistry::new();
        reg.register(card_with_skills("Agent", vec![]), "http://old".into())
            .await
            .unwrap();
        reg.register(card_with_skills("Agent", vec![]), "http://new".into())
            .await
            .unwrap();

        assert_eq!(reg.list().await.unwrap().len(), 1);
        let got = reg
            .get(&AgentId::from_name("Agent"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.endpoint, "http://new");
    }

    #[tokio::test]
    async fn deregister_removes_then_errors() {
        let reg = InMemoryAgentRegistry::new();
        let id = reg
            .register(card_with_skills("Agent", vec![]), "http://a".into())
            .await
            .unwrap();

        assert!(reg.deregister(&id).await.is_ok());
        assert!(reg.list().await.unwrap().is_empty());

        match reg.deregister(&id).await {
            Err(RegistryError::NotFound(missing)) => assert_eq!(missing, id),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }
}
