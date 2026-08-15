//! Locating a delegation peer, at the moment the model calls its tool.
//!
//! [`DiscoveredPeer`] is the [`PeerResolver`] adapter that closes the loop
//! between the two halves of the multi-agent platform: the registry knows where
//! agents are, and a delegation tool needs one *now*. Resolution was a startup
//! pass before, which under a control plane means an orchestrator is blind to
//! every agent deployed after it and keeps dialing the old address of one that
//! moved.
//!
//! The dialed transport is cached by endpoint, because connecting is not free —
//! it fetches the peer's card and negotiates a protocol. A lookup that returns
//! the same endpoint reuses the connection; a different one replaces it.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tracing::debug;

use a2a_rs::port::Transport;

use super::{AgentId, AgentRegistry};
use crate::handlers::tools::{PeerResolver, PeerUnavailable};

/// How to find the peer's current endpoint.
enum Lookup {
    /// A base URL written in the config. Nothing to look up — but still dialed
    /// on demand, so an orchestrator can start before the agent it delegates to.
    Endpoint(String),
    /// A registered agent id.
    Id(Arc<dyn AgentRegistry>, AgentId),
    /// Any agent advertising this skill; first match wins.
    Skill(Arc<dyn AgentRegistry>, String),
}

/// The peer as last dialed. Kept together so the endpoint that a transport was
/// built for cannot drift from the transport itself.
struct Connected {
    endpoint: String,
    transport: Arc<dyn Transport>,
}

/// A peer located on demand and dialed lazily.
pub struct DiscoveredPeer {
    lookup: Lookup,
    connected: Mutex<Option<Connected>>,
}

impl DiscoveredPeer {
    /// A peer at a fixed base URL, dialed on first use.
    pub fn at(endpoint: impl Into<String>) -> Self {
        Self::with(Lookup::Endpoint(endpoint.into()))
    }

    /// A peer looked up by registry id on every call.
    pub fn by_id(registry: Arc<dyn AgentRegistry>, id: AgentId) -> Self {
        Self::with(Lookup::Id(registry, id))
    }

    /// A peer looked up by advertised skill on every call, so whichever agent
    /// provides it at the time gets the work.
    pub fn by_skill(registry: Arc<dyn AgentRegistry>, skill: impl Into<String>) -> Self {
        Self::with(Lookup::Skill(registry, skill.into()))
    }

    fn with(lookup: Lookup) -> Self {
        Self {
            lookup,
            connected: Mutex::new(None),
        }
    }

    /// Where the peer is right now.
    async fn endpoint(&self) -> Result<String, PeerUnavailable> {
        match &self.lookup {
            Lookup::Endpoint(url) => Ok(url.clone()),
            Lookup::Id(registry, id) => registry
                .get(id)
                .await
                .map_err(|e| PeerUnavailable::new(format!("registry lookup failed: {e}")))?
                .map(|found| found.endpoint)
                .ok_or_else(|| {
                    PeerUnavailable::new(format!("no agent is registered with id '{id}'"))
                }),
            Lookup::Skill(registry, skill) => {
                let mut matches = registry
                    .find_by_skill(skill)
                    .await
                    .map_err(|e| PeerUnavailable::new(format!("registry lookup failed: {e}")))?;
                if matches.is_empty() {
                    return Err(PeerUnavailable::new(format!(
                        "no agent advertises the '{skill}' skill"
                    )));
                }
                if matches.len() > 1 {
                    debug!(
                        skill,
                        candidates = matches.len(),
                        "several agents advertise this skill; delegating to the first"
                    );
                }
                Ok(matches.remove(0).endpoint)
            }
        }
    }
}

#[async_trait]
impl PeerResolver for DiscoveredPeer {
    async fn resolve(&self) -> Result<Arc<dyn Transport>, PeerUnavailable> {
        let endpoint = self.endpoint().await?;

        // The lock is held across the dial on purpose: concurrent tool calls to
        // a peer nobody has reached yet should negotiate once, not N times.
        let mut connected = self.connected.lock().await;
        if let Some(existing) = connected.as_ref()
            && existing.endpoint == endpoint
        {
            return Ok(existing.transport.clone());
        }

        let transport: Arc<dyn Transport> = a2a_rs::auto_connect(&endpoint)
            .await
            .map_err(|e| PeerUnavailable::new(format!("could not connect to {endpoint}: {e}")))?
            .into();
        *connected = Some(Connected {
            endpoint,
            transport: transport.clone(),
        });
        Ok(transport)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::InMemoryAgentRegistry;
    use a2a_rs::domain::{AgentCard, AgentSkill};

    fn card(name: &str, skill: &str) -> AgentCard {
        let mut card = AgentCard {
            name: name.to_string(),
            ..Default::default()
        };
        card.skills = vec![AgentSkill::new(
            skill.to_string(),
            "S".to_string(),
            "d".to_string(),
            vec![],
        )];
        card
    }

    /// Resolution has to fail *softly*: the model is told the tool is unusable
    /// so it can route around, rather than the orchestrator's task breaking.
    #[tokio::test]
    async fn an_unregistered_peer_is_unavailable_not_an_error() {
        let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::new());
        let peer = DiscoveredPeer::by_id(registry, AgentId::from("billing"));

        let Err(err) = peer.resolve().await else {
            panic!("nothing is registered, so nothing can resolve");
        };
        assert!(err.to_string().contains("billing"), "{err}");
    }

    #[tokio::test]
    async fn an_unmatched_skill_names_the_skill() {
        let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::new());
        let peer = DiscoveredPeer::by_skill(registry, "forecasting");

        let Err(err) = peer.resolve().await else {
            panic!("nothing advertises the skill, so nothing can resolve");
        };
        assert!(err.to_string().contains("forecasting"), "{err}");
    }

    /// The point of resolving per call: an agent that registers after the
    /// orchestrator started is reachable. Only the lookup is exercised here —
    /// dialing needs a live peer, which `agent_as_tool_test` covers.
    #[tokio::test]
    async fn a_late_joiner_is_found_by_a_lookup_that_missed_before() {
        let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::new());
        let peer = DiscoveredPeer::by_skill(registry.clone(), "forecasting");
        assert!(peer.endpoint().await.is_err(), "nothing registered yet");

        registry
            .register(
                card("Weather", "forecasting"),
                "http://late:8080".to_string(),
            )
            .await
            .unwrap();

        assert_eq!(peer.endpoint().await.unwrap(), "http://late:8080");
    }

    /// A peer that moved is dialed at its new address, not its old one.
    #[tokio::test]
    async fn a_moved_peer_resolves_to_its_new_endpoint() {
        let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::new());
        registry
            .register(
                card("Weather", "forecasting"),
                "http://old:8080".to_string(),
            )
            .await
            .unwrap();
        let peer = DiscoveredPeer::by_id(registry.clone(), AgentId::from("weather"));
        assert_eq!(peer.endpoint().await.unwrap(), "http://old:8080");

        registry
            .register(
                card("Weather", "forecasting"),
                "http://new:8080".to_string(),
            )
            .await
            .unwrap();
        assert_eq!(peer.endpoint().await.unwrap(), "http://new:8080");
    }
}
