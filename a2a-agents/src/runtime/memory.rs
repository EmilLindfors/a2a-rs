//! [`InMemoryAgentRuntime`] — a process-free fake for tests and composition.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::{AgentRuntime, AgentSpec, RuntimeError, RuntimeHealth, RuntimeStatus};
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
}

impl InMemoryAgentRuntime {
    /// Create an empty runtime.
    pub fn new() -> Self {
        Self::default()
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
}
