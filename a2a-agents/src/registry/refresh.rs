//! Keeping the registry's picture of who is up from going stale.
//!
//! Registry entries are written once — at `a2a run` startup, or by
//! `ControlPlane::deploy`/`recover` — and were never revisited, so an agent that
//! died stayed discoverable and kept being handed work. [`CardRefresher`] probes
//! each registered agent on an interval and records what it found.
//!
//! The probe is a card fetch because that is the one request every A2A agent
//! must answer, and it goes through the [`CardSource`] port so the loop is
//! testable without HTTP servers.
//!
//! What it reads it also **adopts**, through
//! [`AgentRegistry::update_card`](super::AgentRegistry::update_card) — so a
//! skill added to a running agent becomes discoverable without redeploying it.
//! Deliberately not through `register`: that derives the id from the fetched
//! card's `name`, so an agent that renamed itself would land under a second id
//! with the old entry left behind, a duplicate the next skill lookup would
//! happily hand work to. A renamed card is refused and reported instead.

use std::sync::Arc;
use std::time::Duration;

use tracing::{debug, info, warn};

use super::{AgentRegistry, Liveness, RegistryError};
use crate::registry::CardSource;

/// How often to probe, when the caller does not say. Long enough not to be
/// traffic, short enough that a dead peer stops being delegated to within a
/// minute or so.
pub const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

/// What one pass over the registry found. Returned so a caller can log or test
/// a pass without inspecting the registry afterwards.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RefreshReport {
    /// Agents whose card was readable.
    pub live: usize,
    /// Agents that did not answer.
    pub unreachable: usize,
    /// Agents whose card had changed and was adopted.
    pub adopted: usize,
}

impl RefreshReport {
    /// How many agents were probed.
    pub fn probed(&self) -> usize {
        self.live + self.unreachable
    }
}

/// Probes registered agents and records whether they answered.
pub struct CardRefresher {
    registry: Arc<dyn AgentRegistry>,
    cards: Arc<dyn CardSource>,
    interval: Duration,
}

impl CardRefresher {
    /// Build a refresher over a registry and a way to read cards.
    pub fn new(registry: Arc<dyn AgentRegistry>, cards: Arc<dyn CardSource>) -> Self {
        Self {
            registry,
            cards,
            interval: DEFAULT_REFRESH_INTERVAL,
        }
    }

    /// Probe on a different interval than [`DEFAULT_REFRESH_INTERVAL`].
    pub fn every(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Probe every registered agent once.
    ///
    /// Errors are per agent and never abort the pass: one unreachable agent
    /// must not stop the others from being checked, which is the whole reason
    /// to check them.
    pub async fn refresh_once(&self) -> Result<RefreshReport, RegistryError> {
        let mut report = RefreshReport::default();
        for agent in self.registry.list().await? {
            let liveness = match self.cards.fetch(&agent.endpoint).await {
                Ok(card) => {
                    report.live += 1;
                    // Only when it changed: in the steady state this is every
                    // agent on every tick, and a write lock per agent per pass
                    // buys nothing.
                    if card != agent.card {
                        match self.registry.update_card(&agent.id, card).await {
                            Ok(()) => {
                                report.adopted += 1;
                                info!(agent = %agent.id, "adopted an updated agent card");
                            }
                            // Repeats every pass while the name stays changed,
                            // because the entry never becomes the new card.
                            // That is the point: it is a state someone has to
                            // resolve, not a transient.
                            Err(e @ RegistryError::Renamed { .. }) => {
                                warn!(agent = %agent.id, error = %e, "keeping the card on file")
                            }
                            Err(e) => debug!(agent = %agent.id, error = %e, "could not adopt card"),
                        }
                    }
                    Liveness::Live
                }
                Err(e) => {
                    report.unreachable += 1;
                    // Not a warning on its own: an agent being down is what
                    // this loop exists to notice, and warning every interval
                    // for the same dead agent is noise.
                    debug!(agent = %agent.id, error = %e, "liveness probe failed");
                    Liveness::Unreachable
                }
            };

            // Only report a *change*, so a steady state is silent and an agent
            // dying or coming back is one line.
            if agent.liveness != liveness {
                match liveness {
                    Liveness::Unreachable => warn!(
                        agent = %agent.id,
                        endpoint = %agent.endpoint,
                        "agent stopped answering; ranked behind live peers for skill lookups"
                    ),
                    _ => info!(agent = %agent.id, endpoint = %agent.endpoint, "agent is answering"),
                }
            }

            // Deregistered mid-pass: nothing to record, and not an error.
            if let Err(RegistryError::NotFound(id)) = self.registry.mark(&agent.id, liveness).await
            {
                debug!(agent = %id, "deregistered while its probe was in flight");
            }
        }
        Ok(report)
    }

    /// Probe on the interval, forever. Intended to be `tokio::spawn`ed.
    ///
    /// A pass that cannot even list the registry is logged and retried on the
    /// next tick rather than ending the loop: the alternative leaves liveness
    /// frozen at whatever it last was, with nothing saying so.
    pub async fn run(self) {
        let mut ticker = tokio::time::interval(self.interval);
        // `interval`'s first tick is immediate, and probing at t=0 is wrong
        // wherever this is spawned: `a2a run` registers its agents from their
        // configs a moment *before* starting them, so an immediate pass would
        // mark the whole fleet unreachable and warn about it. Whoever just
        // registered has the freshest information there is; `Unknown` until the
        // first real pass is the honest state, and it is already the one that
        // costs nothing.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            match self.refresh_once().await {
                Ok(report) if report.probed() > 0 => {
                    debug!(
                        live = report.live,
                        unreachable = report.unreachable,
                        adopted = report.adopted,
                        "liveness pass"
                    )
                }
                Ok(_) => {}
                Err(e) => warn!(error = %e, "could not read the registry to probe it"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{AgentId, InMemoryAgentRegistry, InMemoryCardSource};
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

    async fn registered(
        registry: &Arc<dyn AgentRegistry>,
        name: &str,
        skill: &str,
        endpoint: &str,
    ) -> AgentId {
        registry
            .register(card(name, skill), endpoint.to_string())
            .await
            .unwrap()
    }

    async fn liveness_of(registry: &Arc<dyn AgentRegistry>, id: &AgentId) -> Liveness {
        registry
            .get(id)
            .await
            .unwrap()
            .expect("registered")
            .liveness
    }

    #[tokio::test]
    async fn a_pass_records_who_answered() {
        let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::new());
        let up = registered(&registry, "Up", "forecasting", "http://up").await;
        let down = registered(&registry, "Down", "forecasting", "http://down").await;

        let cards = InMemoryCardSource::new();
        cards.insert("http://up", card("Up", "forecasting")).await;

        let report = CardRefresher::new(registry.clone(), Arc::new(cards))
            .refresh_once()
            .await
            .unwrap();

        assert_eq!(
            report,
            RefreshReport {
                live: 1,
                unreachable: 1,
                // The card served is the one on file, so there is nothing to
                // adopt.
                adopted: 0,
            }
        );
        assert_eq!(liveness_of(&registry, &up).await, Liveness::Live);
        assert_eq!(liveness_of(&registry, &down).await, Liveness::Unreachable);
    }

    /// A skill added to a running agent is invisible until something re-reads
    /// its card. This loop already re-reads it, so it may as well keep it.
    #[tokio::test]
    async fn a_skill_added_while_running_becomes_discoverable() {
        let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::new());
        registered(&registry, "Weather", "forecasting", "http://weather").await;

        let cards = InMemoryCardSource::new();
        cards
            .insert("http://weather", card("Weather", "storm-warnings"))
            .await;

        let report = CardRefresher::new(registry.clone(), Arc::new(cards))
            .refresh_once()
            .await
            .unwrap();

        assert_eq!(report.adopted, 1);
        assert_eq!(
            registry
                .find_by_skill("storm-warnings")
                .await
                .unwrap()
                .len(),
            1,
            "the new skill has to be findable, which is the whole point"
        );
    }

    /// The trap this deliberately does not fall into: adopting a renamed card
    /// through `register` would file it under a second id and leave the old
    /// entry behind, and the next skill lookup would hand work to whichever it
    /// found first.
    #[tokio::test]
    async fn a_renamed_card_is_refused_rather_than_duplicated() {
        let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::new());
        let id = registered(&registry, "Weather", "forecasting", "http://weather").await;

        let cards = InMemoryCardSource::new();
        cards
            .insert("http://weather", card("Weather Two", "forecasting"))
            .await;

        let report = CardRefresher::new(registry.clone(), Arc::new(cards))
            .refresh_once()
            .await
            .unwrap();

        // Still reachable — the card was readable, which is what liveness asks.
        assert_eq!(report.live, 1);
        assert_eq!(report.adopted, 0);
        assert_eq!(registry.list().await.unwrap().len(), 1, "no duplicate");
        assert_eq!(
            registry
                .get(&id)
                .await
                .unwrap()
                .expect("still there")
                .card
                .name,
            "Weather"
        );
    }

    /// A dead agent stays registered. Its entry is the record that it exists,
    /// and an operator asking "where did it go" needs it — hide, never discard.
    #[tokio::test]
    async fn an_unreachable_agent_is_not_deregistered() {
        let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::new());
        registered(&registry, "Down", "forecasting", "http://down").await;

        CardRefresher::new(registry.clone(), Arc::new(InMemoryCardSource::new()))
            .refresh_once()
            .await
            .unwrap();

        assert_eq!(registry.list().await.unwrap().len(), 1);
    }

    /// What the liveness is *for*: a skill lookup hands work to the peer that
    /// answered, while the dead one is still returned behind it — so an
    /// orchestrator with no live option reports a connection failure rather
    /// than "nobody provides this skill".
    #[tokio::test]
    async fn skill_lookups_prefer_the_peer_that_answered() {
        let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::new());
        // Registered first, so only the probe can move it behind the other.
        registered(&registry, "Down", "forecasting", "http://down").await;
        registered(&registry, "Up", "forecasting", "http://up").await;

        let cards = InMemoryCardSource::new();
        cards.insert("http://up", card("Up", "forecasting")).await;
        CardRefresher::new(registry.clone(), Arc::new(cards))
            .refresh_once()
            .await
            .unwrap();

        let matches = registry.find_by_skill("forecasting").await.unwrap();
        assert_eq!(matches.len(), 2, "the dead peer is ranked, not dropped");
        assert_eq!(matches[0].endpoint, "http://up");
    }

    /// An agent that comes back is delegated to again. A one-way transition
    /// would need a restart to clear, which is worse than not probing at all.
    #[tokio::test]
    async fn an_agent_that_comes_back_is_marked_live_again() {
        let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::new());
        let id = registered(&registry, "Flaky", "forecasting", "http://flaky").await;
        let cards = Arc::new(InMemoryCardSource::new());
        let refresher = CardRefresher::new(registry.clone(), cards.clone());

        refresher.refresh_once().await.unwrap();
        assert_eq!(liveness_of(&registry, &id).await, Liveness::Unreachable);

        cards
            .insert("http://flaky", card("Flaky", "forecasting"))
            .await;
        refresher.refresh_once().await.unwrap();
        assert_eq!(liveness_of(&registry, &id).await, Liveness::Live);
    }

    /// Re-registering is a new claim about where the agent is; what was probed
    /// was the old address.
    #[tokio::test]
    async fn re_registering_resets_liveness() {
        let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::new());
        let id = registered(&registry, "Moved", "forecasting", "http://old").await;
        let cards = InMemoryCardSource::new();
        cards
            .insert("http://old", card("Moved", "forecasting"))
            .await;
        CardRefresher::new(registry.clone(), Arc::new(cards))
            .refresh_once()
            .await
            .unwrap();
        assert_eq!(liveness_of(&registry, &id).await, Liveness::Live);

        registered(&registry, "Moved", "forecasting", "http://new").await;
        assert_eq!(liveness_of(&registry, &id).await, Liveness::Unknown);
    }
}
