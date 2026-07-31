//! HTTP client adapter for the control-plane API — the other side of
//! [`control_plane_router`](super::control_plane_router).
//!
//! Until this existed the control plane could only be driven with `curl`, which
//! made the daemon story unusable without either Terraform or a hand-rolled
//! request. `a2a deploy/ps/logs/stop` are thin printers over this type, and the
//! Terraform provider will target the same routes.
//!
//! It is a plain adapter, not a port implementation: nothing in the inner layers
//! needs to *call* a control plane, so introducing a trait here would be a port
//! with one implementation and no consumer.

use reqwest::StatusCode;
use thiserror::Error;

use super::wire::{AgentLogs, AgentStatus, ApiErrorBody, DeployRequest, ListQuery};
use super::{DeployedAgent, ListFilter};
use crate::registry::AgentId;

/// Why a control-plane call failed.
///
/// The distinctions are the ones a caller acts on differently: a bad token is
/// fixed by the operator, a missing agent by naming another one, an unreachable
/// URL by starting the control plane — and everything else is the control
/// plane's own diagnosis, which is worth passing through verbatim (it is where
/// "line 4: unknown key `http_prot`" and "references env vars that are not
/// allowed" come from).
#[derive(Debug, Error)]
pub enum ControlPlaneClientError {
    /// The control plane could not be reached at all.
    #[error("could not reach the control plane at {url}: {source}")]
    Unreachable {
        /// The base URL that was dialled.
        url: String,
        /// The underlying transport error.
        #[source]
        source: reqwest::Error,
    },

    /// The request was rejected for want of a valid bearer token.
    #[error(
        "the control plane rejected the token \
         (pass --token, or set A2A_CONTROL_PLANE_TOKEN)"
    )]
    Unauthorized,

    /// No agent with that id is deployed.
    #[error("no agent '{0}' is deployed (see `a2a ps`)")]
    NotFound(AgentId),

    /// This control plane cannot serve the request — e.g. logs from a runtime
    /// that does not capture them.
    #[error("{0}")]
    Unsupported(String),

    /// The control plane refused or failed the request, and said why.
    #[error("control plane returned {status}: {message}")]
    Api {
        /// The HTTP status it answered with.
        status: StatusCode,
        /// Its own description of the failure.
        message: String,
    },

    /// A response arrived but was not the shape this client expects.
    #[error("could not read the control plane's response: {0}")]
    Malformed(#[source] reqwest::Error),
}

/// Calls the control-plane API on behalf of the CLI.
///
/// Cheap to clone (`reqwest::Client` is an `Arc` inside). Holds the bearer token
/// the API requires; `None` is only useful against a `--no-auth` control plane.
#[derive(Debug, Clone)]
pub struct ControlPlaneClient {
    base_url: String,
    token: Option<String>,
    http: reqwest::Client,
}

impl ControlPlaneClient {
    /// Target the control plane at `base_url` (e.g. `http://127.0.0.1:9090`).
    ///
    /// A trailing slash is trimmed, so a pasted URL and a typed one behave the
    /// same.
    pub fn new(base_url: impl Into<String>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        Self {
            base_url,
            token: None,
            http: reqwest::Client::new(),
        }
    }

    /// Present this bearer token on every request.
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Present `token` if there is one — the `Option`-shaped form of
    /// [`with_token`](Self::with_token), so a caller resolving a flag or an env
    /// var does not have to branch.
    pub fn with_optional_token(self, token: Option<impl Into<String>>) -> Self {
        match token {
            Some(token) => self.with_token(token),
            None => self,
        }
    }

    /// The control plane this client talks to.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Deploy an agent from **raw, unexpanded** config TOML.
    ///
    /// Deliberately takes the text as it sits on disk: `${VAR}` refs are the
    /// control plane's to resolve, against its own environment and its
    /// allowlist. Expanding them here would put the operator's secrets on the
    /// wire and require the deploying machine to hold them.
    pub async fn deploy(
        &self,
        config_toml: impl Into<String>,
    ) -> Result<DeployedAgent, ControlPlaneClientError> {
        let request = self.http.post(self.url("/agents")).json(&DeployRequest {
            config_toml: config_toml.into(),
        });
        self.send(request, None).await
    }

    /// Deployed agents with their endpoint and current health.
    ///
    /// [`ListFilter::Live`] hides agents that have been stopped; the control
    /// plane keeps their entries (their logs are still readable) but they are
    /// not part of what is running.
    pub async fn list(
        &self,
        filter: ListFilter,
    ) -> Result<Vec<DeployedAgent>, ControlPlaneClientError> {
        let request = self.http.get(self.url("/agents")).query(&ListQuery {
            all: filter == ListFilter::All,
        });
        self.send(request, None).await
    }

    /// One agent's current health.
    pub async fn status(&self, id: &AgentId) -> Result<AgentStatus, ControlPlaneClientError> {
        let request = self.http.get(self.url(&format!("/agents/{id}")));
        self.send(request, Some(id)).await
    }

    /// An agent's captured output, oldest line first, limited to the last `tail`
    /// lines when given.
    pub async fn logs(
        &self,
        id: &AgentId,
        tail: Option<usize>,
    ) -> Result<AgentLogs, ControlPlaneClientError> {
        let mut request = self.http.get(self.url(&format!("/agents/{id}/logs")));
        if let Some(tail) = tail {
            request = request.query(&[("tail", tail)]);
        }
        self.send(request, Some(id)).await
    }

    /// Stop an agent and remove it from discovery.
    pub async fn undeploy(&self, id: &AgentId) -> Result<(), ControlPlaneClientError> {
        let request = self.http.delete(self.url(&format!("/agents/{id}")));
        // 204 No Content: there is no body to decode, so this one cannot go
        // through `send`.
        let response = self.execute(request).await?;
        self.check(response, Some(id)).await.map(drop)
    }

    /// Absolute URL for an API path.
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    /// Send a request expecting a JSON body of type `T`.
    ///
    /// `id` is the agent the call was about, so a 404 can name it.
    async fn send<T: serde::de::DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
        id: Option<&AgentId>,
    ) -> Result<T, ControlPlaneClientError> {
        let response = self.execute(request).await?;
        let response = self.check(response, id).await?;
        response
            .json()
            .await
            .map_err(ControlPlaneClientError::Malformed)
    }

    /// Attach credentials and dial.
    async fn execute(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, ControlPlaneClientError> {
        let request = match &self.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        };
        request
            .send()
            .await
            .map_err(|source| ControlPlaneClientError::Unreachable {
                url: self.base_url.clone(),
                source,
            })
    }

    /// Turn a non-2xx response into the most actionable error available.
    ///
    /// The body is read for the API's own `error` message rather than reporting
    /// the status alone: the control plane is the only party that knows *which*
    /// key was unknown or *which* variable was disallowed, and dropping that
    /// would leave the operator with a bare 400.
    async fn check(
        &self,
        response: reqwest::Response,
        id: Option<&AgentId>,
    ) -> Result<reqwest::Response, ControlPlaneClientError> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let message = response
            .json::<ApiErrorBody>()
            .await
            .map(|body| body.error)
            .unwrap_or_else(|_| status.to_string());

        Err(match (status, id) {
            (StatusCode::UNAUTHORIZED, _) => ControlPlaneClientError::Unauthorized,
            (StatusCode::NOT_FOUND, Some(id)) => ControlPlaneClientError::NotFound(id.clone()),
            (StatusCode::NOT_IMPLEMENTED, _) => ControlPlaneClientError::Unsupported(message),
            _ => ControlPlaneClientError::Api { status, message },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_built_without_doubling_the_separator() {
        let client = ControlPlaneClient::new("http://127.0.0.1:9090/");
        assert_eq!(client.base_url(), "http://127.0.0.1:9090");
        assert_eq!(client.url("/agents"), "http://127.0.0.1:9090/agents");
        assert_eq!(
            client.url("/agents/weather-agent/logs"),
            "http://127.0.0.1:9090/agents/weather-agent/logs"
        );
    }

    #[test]
    fn an_absent_token_leaves_the_client_unauthenticated() {
        let client = ControlPlaneClient::new("http://x").with_optional_token(None::<String>);
        assert!(client.token.is_none());
        let client = ControlPlaneClient::new("http://x").with_optional_token(Some("t"));
        assert_eq!(client.token.as_deref(), Some("t"));
    }
}
