//! A wire-compatible JSON-RPC 2.0 A2A client — the counterpart to
//! [`jsonrpc_server`].
//!
//! It demonstrates the **auto-connect** path: fetch the agent card, negotiate a
//! transport from the interfaces it advertises ([`connect`] +
//! [`default_registry`]), and fall back to a direct [`JsonRpcClient`] if the card
//! can't be fetched or negotiated. Then it drives the negotiated
//! [`Transport`](a2a_rs::Transport) port through a full task lifecycle: send a
//! message, read the task back, subscribe to its SSE stream, and cancel it.
//!
//! Start the server in one terminal:
//! ```sh
//! cargo run -p a2a-rs --example jsonrpc_server --features jsonrpc-server
//! ```
//! …then the client in another (default target `http://127.0.0.1:8137`, or pass
//! a base URL):
//! ```sh
//! cargo run -p a2a-rs --example jsonrpc_client --features jsonrpc-client
//! cargo run -p a2a-rs --example jsonrpc_client --features jsonrpc-client -- http://127.0.0.1:8137
//! ```

use std::time::Duration;

use futures::StreamExt;

use a2a_rs::domain::Message;
use a2a_rs::{JsonRpcClient, StreamItem, Transport, connect, default_registry};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://127.0.0.1:8137".to_string());

    // 1. Auto-connect: fetch the card and let the negotiator pick a transport the
    //    client compiled in, ranked by client preference. If the card can't be
    //    fetched or negotiated, fall back to a direct JSON-RPC client.
    let transport: Box<dyn Transport> = match connect(&base_url, &default_registry()).await {
        Ok(t) => {
            println!("✅ negotiated transport: {}", t.protocol());
            t
        }
        Err(e) => {
            println!("⚠️  card negotiation failed ({e}); falling back to direct JSON-RPC client");
            Box::new(JsonRpcClient::new(base_url.clone()))
        }
    };

    // 2. Send a message — the server creates (or updates) the task and echoes it.
    let task = transport
        .send_task_message(
            "demo-task",
            &Message::user_text("hello".to_string(), "m1".to_string()),
            None,
            None,
        )
        .await?;
    println!("📨 sent message; task id = {}", task.id);

    // 3. Read the task back.
    let fetched = transport.get_task(&task.id, None).await?;
    println!("📥 get_task → state {:?}", fetched.status.state);

    // 4. Subscribe to the task's SSE stream and print the first few events. The
    //    first event is the initial task snapshot; live updates follow.
    let mut stream = transport.subscribe_to_task(&task.id, None, None).await?;
    println!("📡 subscribing (up to 3 events / 5s)…");
    for _ in 0..3 {
        match tokio::time::timeout(Duration::from_secs(5), stream.next()).await {
            Ok(Some(Ok(event))) => match &event.item {
                StreamItem::Task(t) => {
                    println!("   • snapshot: task {} ({:?})", t.id, t.status.state)
                }
                StreamItem::StatusUpdate(u) => println!("   • status: {:?}", u.status.state),
                StreamItem::ArtifactUpdate(_) => println!("   • artifact update"),
            },
            Ok(Some(Err(e))) => {
                println!("   • stream error: {e}");
                break;
            }
            Ok(None) => break, // stream ended
            Err(_) => break,   // timed out waiting for the next event
        }
    }
    drop(stream);

    // 5. Cancel the task.
    let canceled = transport.cancel_task(&task.id).await?;
    println!("🛑 canceled; final state {:?}", canceled.status.state);

    Ok(())
}
