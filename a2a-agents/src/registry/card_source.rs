//! Reading an agent's published card from its endpoint.
//!
//! The [`AgentRegistry`](super::AgentRegistry) *stores* cards; this port
//! *obtains* one from a running agent. They are separate capabilities because
//! the registry is a store the platform owns, while a card comes off the wire
//! from something the platform merely supervises.
//!
//! The reason it is a port at all: recovering a control plane means putting
//! already-running agents back into discovery, which requires asking each one
//! for its card. Without this seam that orchestration could only be tested with
//! real HTTP servers, and the service layer would be reaching for an adapter
//! (dependency rule).

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::RwLock;

use a2a_rs::domain::AgentCard;

/// Why a card could not be read.
///
/// One variant on purpose: from the caller's side every failure means the same
/// thing — the agent did not hand over a card — and the underlying reason is
/// diagnostic text, not something to branch on.
#[derive(Debug, Error)]
#[error("could not read the agent card at {endpoint}: {reason}")]
pub struct CardSourceError {
    /// The endpoint that was asked.
    pub endpoint: String,
    /// What went wrong, as reported by the transport.
    pub reason: String,
}

/// Obtain a running agent's published [`AgentCard`] from its endpoint.
#[async_trait]
pub trait CardSource: Send + Sync {
    /// Fetch the card served at `endpoint` (a dialable base URL).
    async fn fetch(&self, endpoint: &str) -> Result<AgentCard, CardSourceError>;
}

/// The real adapter: fetches `<endpoint>/.well-known/agent-card.json`.
#[derive(Debug, Clone, Copy, Default)]
pub struct HttpCardSource;

impl HttpCardSource {
    /// Create the adapter (it holds no state).
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CardSource for HttpCardSource {
    async fn fetch(&self, endpoint: &str) -> Result<AgentCard, CardSourceError> {
        a2a_rs::fetch_agent_card(endpoint)
            .await
            .map_err(|e| CardSourceError {
                endpoint: endpoint.to_string(),
                reason: e.to_string(),
            })
    }
}

/// An infra-free [`CardSource`] serving cards from a map.
///
/// A first-class type (hex rule 6): it lets recovery — which has to re-register
/// what it adopts, tolerate agents that do not answer, and stay idempotent — be
/// tested without standing up HTTP servers.
#[derive(Clone, Default)]
pub struct InMemoryCardSource {
    cards: Arc<RwLock<HashMap<String, AgentCard>>>,
}

impl InMemoryCardSource {
    /// Create an empty source. Every endpoint is unreachable until inserted.
    pub fn new() -> Self {
        Self::default()
    }

    /// Make `card` the answer for `endpoint`.
    pub async fn insert(&self, endpoint: impl Into<String>, card: AgentCard) {
        self.cards.write().await.insert(endpoint.into(), card);
    }
}

#[async_trait]
impl CardSource for InMemoryCardSource {
    async fn fetch(&self, endpoint: &str) -> Result<AgentCard, CardSourceError> {
        self.cards
            .read()
            .await
            .get(endpoint)
            .cloned()
            .ok_or_else(|| CardSourceError {
                endpoint: endpoint.to_string(),
                reason: "no card registered for this endpoint".to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_source_answers_only_for_known_endpoints() {
        let source = InMemoryCardSource::new();
        let card = AgentCard {
            name: "Weather Agent".to_string(),
            ..Default::default()
        };
        source.insert("http://127.0.0.1:8080", card).await;

        assert_eq!(
            source.fetch("http://127.0.0.1:8080").await.unwrap().name,
            "Weather Agent"
        );

        // An unknown endpoint fails the way an unreachable agent does, and the
        // error names it — recovery logs this per agent.
        let err = source.fetch("http://127.0.0.1:9999").await.unwrap_err();
        assert_eq!(err.endpoint, "http://127.0.0.1:9999");
        assert!(err.to_string().contains("http://127.0.0.1:9999"));
    }
}
