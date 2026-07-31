//! [`InMemoryAgentRuntime`] — a process-free fake for tests and composition.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::{AgentRuntime, AgentSpec, Recovered, RuntimeError, RuntimeHealth, RuntimeStatus};
use crate::registry::AgentId;

/// An [`AgentRuntime`] that tracks lifecycle state in a map **without spawning
/// any processes**.
///
/// A first-class adapter (hex rule 6 — not test-only): services like
/// [`ControlPlane`](crate::control_plane::ControlPlane) are unit-tested against
/// it, and it serves as a dev substrate when real process isolation isn't wanted.
/// `start` reports [`RuntimeHealth::Healthy`] immediately — there is no real
/// process or card to probe, so it models the happy path the composition needs.
#[derive(Clone, Default)]
pub struct InMemoryAgentRuntime {
    agents: Arc<Mutex<HashMap<AgentId, (AgentSpec, RuntimeHealth)>>>,
    /// Output attributed to each agent, oldest first. Populated by tests via
    /// [`push_log`](Self::push_log) — nothing here produces output on its own.
    logs: Arc<Mutex<HashMap<AgentId, Vec<String>>>>,
}

impl InMemoryAgentRuntime {
    /// Create an empty runtime.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a line as though the agent had printed it.
    ///
    /// The stand-in for a backend that captures output, so everything above the
    /// port — [`ControlPlane::logs`](crate::ControlPlane::logs), the HTTP route,
    /// the client — is exercisable without a container engine or a child
    /// process. Lines are kept for unknown ids too, so a test can seed a log
    /// before provisioning.
    pub async fn push_log(&self, id: &AgentId, line: impl Into<String>) {
        self.logs
            .lock()
            .await
            .entry(id.clone())
            .or_default()
            .push(line.into());
    }
}

#[async_trait]
impl AgentRuntime for InMemoryAgentRuntime {
    async fn provision(&self, spec: AgentSpec) -> Result<AgentId, RuntimeError> {
        let id = spec.id.clone();
        self.agents
            .lock()
            .await
            .insert(id.clone(), (spec, RuntimeHealth::Provisioned));
        Ok(id)
    }

    /// Reports everything it holds as [`Recovered::Adopted`].
    ///
    /// This fake stands in for a *durable* backend, so it answers like one: the
    /// map plays the part the container engine plays for real. That is what lets
    /// [`ControlPlane::recover`](crate::ControlPlane::recover) — which has to
    /// re-register cards, tolerate unreachable agents, and stay idempotent — be
    /// tested without Docker or a network.
    async fn recover(&self) -> Result<Recovered<AgentId>, RuntimeError> {
        let mut adopted: Vec<AgentId> = self.agents.lock().await.keys().cloned().collect();
        adopted.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        Ok(Recovered::Adopted(adopted))
    }

    async fn start(&self, id: &AgentId) -> Result<(), RuntimeError> {
        let mut guard = self.agents.lock().await;
        let (_, health) = guard
            .get_mut(id)
            .ok_or_else(|| RuntimeError::NotFound(id.clone()))?;
        if *health == RuntimeHealth::Healthy {
            return Err(RuntimeError::AlreadyRunning(id.clone()));
        }
        *health = RuntimeHealth::Healthy;
        Ok(())
    }

    async fn stop(&self, id: &AgentId) -> Result<(), RuntimeError> {
        let mut guard = self.agents.lock().await;
        let (_, health) = guard
            .get_mut(id)
            .ok_or_else(|| RuntimeError::NotFound(id.clone()))?;
        *health = RuntimeHealth::Stopped;
        Ok(())
    }

    async fn health(&self, id: &AgentId) -> Result<RuntimeHealth, RuntimeError> {
        self.agents
            .lock()
            .await
            .get(id)
            .map(|(_, health)| *health)
            .ok_or_else(|| RuntimeError::NotFound(id.clone()))
    }

    async fn list(&self) -> Result<Vec<RuntimeStatus>, RuntimeError> {
        Ok(self
            .agents
            .lock()
            .await
            .values()
            .map(|(spec, health)| RuntimeStatus {
                id: spec.id.clone(),
                health: *health,
                endpoint: spec.endpoint.clone(),
            })
            .collect())
    }

    /// Serve back whatever [`push_log`](Self::push_log) recorded.
    ///
    /// Answers like a backend that *does* capture output — an empty list means
    /// the agent printed nothing, never [`RuntimeError::Unsupported`] — for the
    /// same reason [`recover`](Self::recover) answers `Adopted`: this fake
    /// stands in for the durable case so the layers above it can be tested.
    async fn logs(&self, id: &AgentId, tail: Option<usize>) -> Result<Vec<String>, RuntimeError> {
        if !self.agents.lock().await.contains_key(id) {
            return Err(RuntimeError::NotFound(id.clone()));
        }
        let lines = self.logs.lock().await.get(id).cloned().unwrap_or_default();
        Ok(match tail {
            Some(n) => lines[lines.len().saturating_sub(n)..].to_vec(),
            None => lines,
        })
    }
}
