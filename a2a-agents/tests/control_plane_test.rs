//! HTTP-level test of the control-plane API.
//!
//! Serves [`control_plane_router`] over an `InMemoryAgentRuntime` (no child
//! processes) on an ephemeral port, then drives the real `POST/GET/DELETE
//! /agents` surface with `reqwest` — the same shape the Terraform provider will
//! call. Proves deploy → discover → undeploy round-trips over the wire, and that
//! the surface is closed to callers without the bearer token.
//!
//! The second half drives the *same* router through `ControlPlaneClient`, which
//! is what `a2a deploy/ps/logs/stop` run on. Both sides of the contract are
//! exercised against each other here, so a field renamed on one and not the
//! other fails a test rather than a deployment.

use std::sync::Arc;

use async_trait::async_trait;

use a2a_agents::registry::AgentId;
use a2a_agents::runtime::{AgentSpec, Recovered, RuntimeError, RuntimeStatus};
use a2a_agents::{
    AgentRegistry, AgentRuntime, ControlPlane, ControlPlaneAuth, ControlPlaneClient,
    ControlPlaneClientError, DeployedAgent, InMemoryAgentRegistry, InMemoryAgentRuntime,
    InMemoryCardSource, ListFilter, RuntimeHealth, control_plane_router,
};

/// The token the test control plane requires.
const TOKEN: &str = "test-control-plane-token";

/// Temp dir the API writes deployed configs into. Removed on drop.
struct TempDir {
    path: std::path::PathBuf,
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Serve a control plane on an ephemeral port, returning its base URL.
async fn serve(auth: ControlPlaneAuth, config_dir: &TempDir) -> String {
    serve_over(
        Arc::new(InMemoryAgentRuntime::new()),
        Arc::new(InMemoryCardSource::new()),
        auth,
        config_dir,
    )
    .await
}

/// A runtime that supervises fine but cannot serve logs — what
/// `LocalProcessRuntime` is without a log directory, and what a future
/// log-less backend would be.
///
/// Delegating rather than faking the whole port: the behaviour under test is
/// how [`RuntimeError::Unsupported`] travels out through the HTTP status and
/// back into a client error, and everything else has to keep working for the
/// agent to be deployable in the first place.
struct NoLogsRuntime(InMemoryAgentRuntime);

#[async_trait]
impl AgentRuntime for NoLogsRuntime {
    async fn provision(&self, spec: AgentSpec) -> Result<AgentId, RuntimeError> {
        self.0.provision(spec).await
    }
    async fn recover(&self) -> Result<Recovered<AgentId>, RuntimeError> {
        self.0.recover().await
    }
    async fn start(&self, id: &AgentId) -> Result<(), RuntimeError> {
        self.0.start(id).await
    }
    async fn stop(&self, id: &AgentId) -> Result<(), RuntimeError> {
        self.0.stop(id).await
    }
    async fn health(&self, id: &AgentId) -> Result<RuntimeHealth, RuntimeError> {
        self.0.health(id).await
    }
    async fn list(&self) -> Result<Vec<RuntimeStatus>, RuntimeError> {
        self.0.list().await
    }
    async fn logs(&self, _id: &AgentId, _tail: Option<usize>) -> Result<Vec<String>, RuntimeError> {
        Err(RuntimeError::Unsupported {
            operation: "logs",
            reason: "this backend does not capture agent output".to_string(),
        })
    }
}

/// Serve a control plane over an existing runtime — the seam a restart test
/// needs, since "restarting" means a new service over a backend that survived.
///
/// Recovery runs before serving, exactly as `a2a control-plane` does: the point
/// of the API coming up is that its first answer is true.
async fn serve_over(
    runtime: Arc<dyn AgentRuntime>,
    cards: Arc<InMemoryCardSource>,
    auth: ControlPlaneAuth,
    config_dir: &TempDir,
) -> String {
    let registry: Arc<dyn AgentRegistry> = Arc::new(InMemoryAgentRegistry::new());
    let cp = Arc::new(ControlPlane::new(runtime, registry, cards));
    cp.recover().await.expect("recover before serving");
    let router = control_plane_router(cp, config_dir.path.clone(), auth);

    // Bind first, then serve, so requests can connect immediately.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

fn temp_dir(tag: &str) -> TempDir {
    TempDir {
        path: std::env::temp_dir().join(format!("cp_http_{tag}_{}", std::process::id())),
    }
}

const ECHO_TOML: &str = r#"
[agent]
name = "Http Agent"

[handler]
type = "echo"

[server]
host = "127.0.0.1"
http_port = 8200
"#;

/// A second agent on its own port, for the restart test (its card is what the
/// in-memory card source serves back during recovery).
const RESTART_TOML: &str = r#"
[agent]
name = "Restart Agent"

[handler]
type = "echo"

[server]
host = "127.0.0.1"
http_port = 8201
"#;

#[tokio::test]
async fn deploy_list_status_undeploy_over_http() {
    let config_dir = temp_dir("rt");
    let base = serve(ControlPlaneAuth::bearer(TOKEN), &config_dir).await;
    let client = reqwest::Client::new();

    // POST /agents — deploy from rendered TOML.
    let resp = client
        .post(format!("{base}/agents"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "config_toml": ECHO_TOML }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let deployed: DeployedAgent = resp.json().await.unwrap();
    assert_eq!(deployed.id, "http-agent");
    assert_eq!(deployed.endpoint, "http://127.0.0.1:8200");
    assert_eq!(deployed.health, RuntimeHealth::Healthy);

    // GET /agents — lists the deployed agent as Healthy.
    let listed: Vec<DeployedAgent> = client
        .get(format!("{base}/agents"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].health, RuntimeHealth::Healthy);

    // GET /agents/:id — health of a single agent.
    let resp = client
        .get(format!("{base}/agents/http-agent"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    // GET an unknown agent → 404.
    let resp = client
        .get(format!("{base}/agents/nope"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);

    // DELETE /agents/:id — undeploy.
    let resp = client
        .delete(format!("{base}/agents/http-agent"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

    // It has left the listing: an undeployed agent that kept showing up in
    // `a2a ps` reads as one that refused to go away.
    let listed: Vec<DeployedAgent> = client
        .get(format!("{base}/agents"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(listed.is_empty(), "stopped agents are hidden by default");

    // …but it is not forgotten — `?all=true` is `docker ps -a`, and its logs are
    // still readable, which is when they matter most.
    let listed: Vec<DeployedAgent> = client
        .get(format!("{base}/agents?all=true"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listed[0].health, RuntimeHealth::Stopped);
}

/// A bounced control plane must still know its fleet. Without recovery this is
/// the worst API answer available: `GET /agents` returns `[]` and `DELETE`
/// returns 404 while the agents are still serving — so an operator (or a
/// Terraform `Read`) concludes they were destroyed and redeploys on top of them.
#[tokio::test]
async fn a_restarted_control_plane_still_reports_the_running_fleet() {
    let config_dir = temp_dir("restart");
    // The runtime is the durable half; the registry is not, and is deliberately
    // recreated by `serve_over` on each "start".
    let runtime: Arc<dyn AgentRuntime> = Arc::new(InMemoryAgentRuntime::new());
    let cards = Arc::new(InMemoryCardSource::new());
    cards
        .insert(
            "http://127.0.0.1:8201",
            a2a_rs::domain::AgentCard {
                name: "Restart Agent".to_string(),
                ..Default::default()
            },
        )
        .await;
    let client = reqwest::Client::new();

    let base = serve_over(
        runtime.clone(),
        cards.clone(),
        ControlPlaneAuth::bearer(TOKEN),
        &config_dir,
    )
    .await;
    let resp = client
        .post(format!("{base}/agents"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "config_toml": RESTART_TOML }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);

    // The restart: a fresh service and a fresh registry over the same backend.
    let base = serve_over(runtime, cards, ControlPlaneAuth::bearer(TOKEN), &config_dir).await;

    let listed: Vec<DeployedAgent> = client
        .get(format!("{base}/agents"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listed.len(), 1, "the fleet must survive the restart");
    assert_eq!(listed[0].id, "restart-agent");
    assert_eq!(listed[0].health, RuntimeHealth::Healthy);

    // And it is manageable through the new process, not merely visible.
    let resp = client
        .delete(format!("{base}/agents/restart-agent"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::NO_CONTENT,
        "an adopted agent must be stoppable after a restart"
    );
}

/// Deploying is remote code execution, so every route must reject a caller who
/// cannot present the token — including the read-only ones, which otherwise leak
/// the deployed fleet.
#[tokio::test]
async fn every_route_rejects_callers_without_the_token() {
    let config_dir = temp_dir("auth");
    let base = serve(ControlPlaneAuth::bearer(TOKEN), &config_dir).await;
    let client = reqwest::Client::new();

    let unauthorized = reqwest::StatusCode::UNAUTHORIZED;

    // No credentials at all.
    assert_eq!(
        client
            .post(format!("{base}/agents"))
            .json(&serde_json::json!({ "config_toml": ECHO_TOML }))
            .send()
            .await
            .unwrap()
            .status(),
        unauthorized,
        "deploy must not be reachable unauthenticated"
    );
    for url in [
        format!("{base}/agents"),
        format!("{base}/agents/http-agent"),
    ] {
        assert_eq!(
            client.get(&url).send().await.unwrap().status(),
            unauthorized,
            "GET {url} must not be reachable unauthenticated"
        );
    }
    assert_eq!(
        client
            .delete(format!("{base}/agents/http-agent"))
            .send()
            .await
            .unwrap()
            .status(),
        unauthorized,
        "undeploy must not be reachable unauthenticated"
    );

    // Wrong token, right shape.
    assert_eq!(
        client
            .get(format!("{base}/agents"))
            .bearer_auth("not-the-token")
            .send()
            .await
            .unwrap()
            .status(),
        unauthorized
    );

    // A near-miss (correct prefix) must fail too — guards against a truncating
    // or prefix-based comparison.
    assert_eq!(
        client
            .get(format!("{base}/agents"))
            .bearer_auth(&TOKEN[..TOKEN.len() - 1])
            .send()
            .await
            .unwrap()
            .status(),
        unauthorized
    );

    // Nothing was deployed by any of the rejected calls.
    let listed: Vec<DeployedAgent> = client
        .get(format!("{base}/agents"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        listed.is_empty(),
        "rejected calls must not have side effects"
    );
}

/// The explicit dev-loop opt-out still works — otherwise the escape hatch the
/// CLI advertises would be a lie.
#[tokio::test]
async fn disabled_auth_accepts_unauthenticated_calls() {
    let config_dir = temp_dir("noauth");
    let base = serve(ControlPlaneAuth::Disabled, &config_dir).await;

    let resp = reqwest::Client::new()
        .get(format!("{base}/agents"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
}

/// The whole lifecycle as the CLI drives it, through the client rather than
/// hand-built requests — so the client and the router are checked against each
/// other, not each against a copy of the wire format.
#[tokio::test]
async fn the_client_drives_the_full_lifecycle() {
    let config_dir = temp_dir("client");
    let runtime = Arc::new(InMemoryAgentRuntime::new());
    let base = serve_over(
        runtime.clone(),
        Arc::new(InMemoryCardSource::new()),
        ControlPlaneAuth::bearer(TOKEN),
        &config_dir,
    )
    .await;
    let client = ControlPlaneClient::new(&base).with_token(TOKEN);
    let id = AgentId::from("http-agent");

    // Nothing deployed yet — an empty fleet, not an error.
    assert!(client.list(ListFilter::Live).await.unwrap().is_empty());

    let deployed = client.deploy(ECHO_TOML).await.expect("deploy");
    assert_eq!(deployed.id, "http-agent");
    assert_eq!(deployed.health, RuntimeHealth::Healthy);

    let listed = client.list(ListFilter::Live).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "http-agent");

    let status = client.status(&id).await.expect("status");
    assert_eq!(status.id, "http-agent");
    assert_eq!(status.health, RuntimeHealth::Healthy);

    // An agent that has printed nothing reports an empty log — distinct from a
    // backend that cannot answer at all (below).
    assert!(client.logs(&id, None).await.expect("logs").lines.is_empty());

    // Seed output the way a real backend would have captured it, and check the
    // whole log and a tail of it come back oldest-first.
    for line in ["first", "second", "third"] {
        runtime.push_log(&id, line).await;
    }
    assert_eq!(
        client.logs(&id, None).await.expect("logs").lines,
        ["first", "second", "third"]
    );
    assert_eq!(
        client.logs(&id, Some(2)).await.expect("logs").lines,
        ["second", "third"]
    );

    client.undeploy(&id).await.expect("undeploy");
    assert_eq!(
        client.status(&id).await.expect("status").health,
        RuntimeHealth::Stopped
    );
}

/// Each failure has to arrive as the variant the CLI branches on, not as a bare
/// status code — that is the difference between "no agent 'x' is deployed" and
/// "control plane returned 404".
#[tokio::test]
async fn client_errors_name_what_the_operator_has_to_fix() {
    let config_dir = temp_dir("client_err");
    let base = serve(ControlPlaneAuth::bearer(TOKEN), &config_dir).await;
    let ghost = AgentId::from("ghost");

    let wrong_token = ControlPlaneClient::new(&base).with_token("not-the-token");
    assert!(matches!(
        wrong_token.list(ListFilter::Live).await,
        Err(ControlPlaneClientError::Unauthorized)
    ));
    // No token at all reads the same way — the API deliberately does not
    // distinguish them, and neither should the client.
    assert!(matches!(
        ControlPlaneClient::new(&base).list(ListFilter::Live).await,
        Err(ControlPlaneClientError::Unauthorized)
    ));

    let client = ControlPlaneClient::new(&base).with_token(TOKEN);
    assert!(matches!(
        client.status(&ghost).await,
        Err(ControlPlaneClientError::NotFound(id)) if id == ghost
    ));
    assert!(matches!(
        client.undeploy(&ghost).await,
        Err(ControlPlaneClientError::NotFound(_))
    ));

    // A rejected config comes back as the control plane's own diagnosis, which
    // is the only place the offending key is known.
    let err = client
        .deploy("[agent]\nname = \"Typo\"\n\n[server]\nhttp_prot = 9999\n")
        .await
        .expect_err("an unknown key must not deploy");
    assert!(
        err.to_string().contains("http_prot"),
        "the control plane's diagnosis must survive the trip back: {err}"
    );

    // A config naming secrets the operator has not permitted is the *caller's*
    // mistake, and its message says exactly what to change. Answering 500 would
    // read to an operator — and to any alerting — as the server breaking.
    let resp = reqwest::Client::new()
        .post(format!("{base}/agents"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({
            "config_toml": "[agent]\nname = \"Leaky\"\ndescription = \"${A2A_TEST_SECRET}\"\n",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
    assert!(
        resp.text().await.unwrap().contains("--allow-env"),
        "the fix has to travel with the rejection"
    );

    // The client must never invent an agent: a rejected deploy leaves nothing.
    assert!(client.list(ListFilter::Live).await.unwrap().is_empty());
}

/// The pre-flight `a2a up` runs over a fleet file has to run against the *live*
/// fleet too. Without it the second deploy reports `ok … healthy`: the agent's
/// process loses the bind race, but the card probe answers from the agent that
/// won it.
#[tokio::test]
async fn deploying_onto_a_held_port_is_a_conflict_not_a_second_agent() {
    let config_dir = temp_dir("portclash");
    let base = serve(ControlPlaneAuth::Disabled, &config_dir).await;
    let client = ControlPlaneClient::new(&base);

    client.deploy(ECHO_TOML).await.expect("the first agent");

    let squatter = ECHO_TOML.replace("Http Agent", "Squatter");
    let err = client
        .deploy(&squatter)
        .await
        .expect_err("the second must not be reported as deployed");
    assert!(
        matches!(&err, ControlPlaneClientError::Api { status, message }
            if *status == reqwest::StatusCode::CONFLICT && message.contains("8200")),
        "got: {err}"
    );

    let listed = client.list(ListFilter::All).await.expect("list");
    assert_eq!(listed.len(), 1, "nothing half-deployed was left behind");
    assert_eq!(listed[0].id, "http-agent");
}

/// "I do not keep logs" travels all the way from the adapter to the CLI as its
/// own thing. Reporting it as an empty log would tell an operator their crashing
/// agent printed nothing.
#[tokio::test]
async fn a_backend_that_cannot_serve_logs_says_so_end_to_end() {
    let config_dir = temp_dir("nologs");
    let base = serve_over(
        Arc::new(NoLogsRuntime(InMemoryAgentRuntime::new())),
        Arc::new(InMemoryCardSource::new()),
        ControlPlaneAuth::Disabled,
        &config_dir,
    )
    .await;
    let client = ControlPlaneClient::new(&base);
    let id = AgentId::from("http-agent");
    client.deploy(ECHO_TOML).await.expect("deploy");

    // 501 at the wire, `Unsupported` at the client — carrying the adapter's
    // reason, which is where the operator learns what to change.
    assert_eq!(
        reqwest::get(format!("{base}/agents/http-agent/logs"))
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::NOT_IMPLEMENTED
    );
    let err = client.logs(&id, None).await.expect_err("logs unsupported");
    assert!(
        matches!(&err, ControlPlaneClientError::Unsupported(reason)
            if reason.contains("does not capture")),
        "got: {err}"
    );

    // And the rest of the surface is unaffected — a runtime without logs is
    // still a runtime.
    assert_eq!(client.list(ListFilter::Live).await.unwrap().len(), 1);
}
