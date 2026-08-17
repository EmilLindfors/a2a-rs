//! What `AgentBuilder::build_wired` connects.
//!
//! The failures this covers are all of one shape: the config is right, the
//! store is right, the handler is right, and the wire between two of them is
//! missing. An LLM agent kept its conversation in the process while
//! `[server.storage] type = "sqlx"` sat in the config; a reimbursement agent
//! broadcast status updates into a streaming backend the transport never
//! subscribed to. Every layer passed its own tests both times.

use a2a_agents::core::builder::{AgentBuilder, AutoStorage};
use a2a_rs::domain::{A2AError, Message, Task, TaskStatusUpdateEvent};
use a2a_rs::port::{
    AsyncMessageHandler, AsyncStreamingHandler, RequestContext, StreamingSubscriber,
};
use async_trait::async_trait;

const SQLX_AGENT: &str = r#"
    [agent]
    name = "Wired"
    description = "checks what the builder connects"
    [server]
    http_port = 18999
    [server.storage]
    type = "sqlx"
    url = "sqlite::memory:"
"#;

const INMEMORY_AGENT: &str = r#"
    [agent]
    name = "Wired"
    description = "checks what the builder connects"
    [server]
    http_port = 18999
"#;

/// Answers nothing; these tests are about what it was handed, not what it does
/// with it.
#[derive(Clone)]
struct Inert;

#[async_trait]
impl AsyncMessageHandler for Inert {
    async fn process_message(
        &self,
        _task_id: &str,
        _message: &Message,
        _ctx: &RequestContext,
    ) -> Result<Task, A2AError> {
        Err(A2AError::Internal("not called".to_string()))
    }
}

struct Silent;

#[async_trait]
impl StreamingSubscriber<TaskStatusUpdateEvent> for Silent {
    async fn on_update(&self, _update: TaskStatusUpdateEvent) -> Result<(), A2AError> {
        Ok(())
    }
}

/// The handler broadcasts to the instance it was handed and the transport
/// subscribes through the one on the server. They have to share one registry,
/// or a `tasks/subscribe` client waits forever on an agent that is emitting
/// updates the whole time.
#[tokio::test]
async fn the_handler_and_the_transport_share_one_streaming_backend()
-> Result<(), Box<dyn std::error::Error>> {
    let mut handler_side = None;
    let server = AgentBuilder::from_toml(INMEMORY_AGENT)?
        .build_wired(|ports| {
            handler_side = Some(ports.streaming.clone());
            Inert
        })
        .await?;

    let handler_side = handler_side.expect("build_wired builds the handler from the ports");
    let transport_side = server
        .streaming()
        .expect("build_wired attaches the streaming backend it built");

    transport_side
        .add_status_subscriber("t1", Box::new(Silent))
        .await?;

    // Clones of one `InMemoryStreamingHandler` share a subscriber registry; two
    // separate instances report 0 here, which is the bug this exists to catch.
    assert_eq!(handler_side.get_subscriber_count("t1").await?, 1);
    Ok(())
}

/// A handler that reads a conversation back has to read it out of the store
/// `[server.storage]` selected, or `type = "sqlx"` validates, passes
/// `a2a doctor` — which reports the conversation as durable — and is lost on
/// every restart.
#[tokio::test]
async fn the_handler_is_given_the_store_the_config_selected()
-> Result<(), Box<dyn std::error::Error>> {
    let mut handler_side = None;
    let _server = AgentBuilder::from_toml(SQLX_AGENT)?
        .build_wired(|ports| {
            handler_side = Some(ports.storage.clone());
            Inert
        })
        .await?;

    let storage = handler_side.expect("build_wired builds the handler from the ports");
    assert!(
        matches!(storage, AutoStorage::Sqlx(_)),
        "[server.storage] type = \"sqlx\" has to reach the handler"
    );
    Ok(())
}

/// And the default is the other one, so the assertion above is about the config
/// rather than about whichever store `build_wired` happens to build.
#[tokio::test]
async fn an_unconfigured_store_is_in_memory() -> Result<(), Box<dyn std::error::Error>> {
    let mut handler_side = None;
    let _server = AgentBuilder::from_toml(INMEMORY_AGENT)?
        .build_wired(|ports| {
            handler_side = Some(ports.storage.clone());
            Inert
        })
        .await?;

    assert!(matches!(
        handler_side.expect("build_wired builds the handler from the ports"),
        AutoStorage::InMemory(_)
    ));
    Ok(())
}
