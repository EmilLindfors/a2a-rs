//! The quick start from the workspace README: an agent whose business logic is
//! one `Responder`, served over HTTP, and a client that sends it a message.
//!
//! The README quotes this file, so it is the copy CI compiles — change the two
//! together.
//!
//! ```sh
//! cargo run -p a2a-rs --example quickstart --features http-server,http-client
//! ```

use std::time::Duration;

use async_trait::async_trait;

use a2a_rs::Transport;
use a2a_rs::adapter::business::{Responder, ResponderMessageHandler};
use a2a_rs::adapter::{
    ConnectRpcAdapter, HttpClient, HttpServer, InMemoryStreamingHandler, InMemoryTaskStorage,
    SimpleAgentInfo,
};
use a2a_rs::domain::{A2AError, Message, Part, Role, SendCompletion, Task, TaskState};

/// The agent: reply with the message in capitals, and finish the task.
struct Shout;

#[async_trait]
impl Responder for Shout {
    async fn respond(
        &self,
        message: &Message,
        task: &Task,
    ) -> Result<(Message, TaskState), A2AError> {
        let text: Vec<&str> = message.parts.iter().filter_map(|p| p.get_text()).collect();
        let reply = Message::builder()
            .role(Role::Agent)
            .parts(vec![Part::text(text.join(" ").to_uppercase())])
            .message_id(uuid::Uuid::new_v4().to_string())
            .task_id(task.id.clone())
            .context_id(message.context_id.clone())
            .build();
        Ok((reply, TaskState::Completed))
    }
}

async fn serve(address: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Storage, streaming and push are ports; these are the in-memory adapters.
    // `ResponderMessageHandler` owns the task lifecycle and announces every
    // transition, so `Shout` only decides what to say.
    let storage = InMemoryTaskStorage::new();
    let streaming = InMemoryStreamingHandler::new();
    let handler = ResponderMessageHandler::new(
        storage.clone(),
        streaming.clone(),
        storage.push_notifier(),
        Shout,
    );

    let card = SimpleAgentInfo::new("shout".to_string(), format!("http://{address}"))
        .with_description("Repeats what you say, louder".to_string())
        .with_streaming();
    let processor = ConnectRpcAdapter::new(handler, storage.clone(), storage, card.clone())
        .with_streaming_handler(streaming);

    HttpServer::new(processor, card, address.to_string())
        .start()
        .await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let address = "127.0.0.1:8080";
    let server = tokio::spawn(serve(address));
    tokio::time::sleep(Duration::from_millis(300)).await;

    let client = HttpClient::new(format!("http://{address}"));
    let message = Message::user_text("hello, agent".to_string(), uuid::Uuid::new_v4().to_string());

    // `WhenSettled` is the A2A default: the server answers once the task has
    // finished, so the reply is already in the task's history. `None` lets the
    // server name the task.
    let task = client
        .send_task_message(None, &message, None, None, SendCompletion::WhenSettled)
        .await?;

    println!("task {} is {:?}", task.id, task.status.state);
    if let Some(text) = task.history.last().and_then(|m| m.parts[0].get_text()) {
        println!("agent said: {text}");
    }

    server.abort();
    Ok(())
}
