//! `a2acli` — a small command-line client for the Agent-to-Agent (A2A) protocol.
//!
//! It drives the client [`Transport`] port from `a2a-rs` directly: `card`,
//! `send`, `get`, `cancel`, and `stream`. By default it auto-negotiates a
//! transport from the agent card (ConnectRPC preferred, JSON-RPC 2.0 as interop
//! fallback); `--transport` forces a specific wire protocol.
//!
//! It doubles as a manual cross-SDK interop harness: point it at
//! `a2a-rs/examples/jsonrpc_server.rs`, or point the official `a2aproject/a2acli`
//! at the same server, to validate wire-compat against the canonical SDKs.

use std::pin::Pin;
use std::sync::Arc;

use a2a_rs::domain::{A2AError, AgentCard, Message, Task};
use a2a_rs::{
    HttpClient, JsonRpcClient, RetryPolicy, StreamEvent, StreamItem, Transport, subscribe_resilient,
};
use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use futures::StreamExt;
use serde_json::Value;

/// A protocol-neutral stream of task update events.
type EventStream = Pin<Box<dyn futures::Stream<Item = Result<StreamEvent, A2AError>> + Send>>;

#[derive(Parser)]
#[command(name = "a2acli", version, about, long_about = None)]
struct Cli {
    /// Base URL of the A2A agent (e.g. http://localhost:8137).
    ///
    /// Falls back to the `A2A_URL` environment variable when omitted.
    #[arg(short, long, env = "A2A_URL", visible_alias = "base-url", global = true)]
    url: Option<String>,

    /// Bearer token for authenticated agents.
    ///
    /// Only applied with `--transport connectrpc|jsonrpc`; ignored in the default
    /// `auto` mode (the negotiation factories build unauthenticated clients).
    #[arg(long, env = "A2A_AUTH_TOKEN", global = true)]
    auth: Option<String>,

    /// Request timeout in seconds. Applies to explicit transports only (see `--auth`).
    #[arg(long, global = true)]
    timeout: Option<u64>,

    /// Wire transport to use.
    #[arg(long, value_enum, default_value_t = TransportChoice::Auto, global = true)]
    transport: TransportChoice,

    /// Emit raw JSON instead of human-readable output.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum TransportChoice {
    /// Negotiate from the agent card, falling back to a direct client.
    Auto,
    /// Force the ConnectRPC transport.
    Connectrpc,
    /// Force the wire-compatible JSON-RPC 2.0 transport.
    Jsonrpc,
}

#[derive(Subcommand)]
enum Command {
    /// Fetch and print the agent card.
    Card,

    /// Send a text message to a task (a task id is generated when omitted).
    Send {
        /// The message text.
        text: String,
        /// Target task id. Generated (uuid) if not provided.
        #[arg(long)]
        task_id: Option<String>,
        /// Session id to associate the message with.
        #[arg(long)]
        session_id: Option<String>,
        /// Number of history messages to return on the resulting task.
        #[arg(long)]
        history_length: Option<u32>,
    },

    /// Get a task by id.
    Get {
        /// The task id.
        task_id: String,
        /// Number of history messages to return.
        #[arg(long)]
        history_length: Option<u32>,
    },

    /// Cancel a task by id.
    Cancel {
        /// The task id.
        task_id: String,
    },

    /// Subscribe to a task's update stream and print events as they arrive.
    Stream {
        /// The task id.
        task_id: String,
        /// Reconnect with exponential backoff on disconnect.
        #[arg(long)]
        resilient: bool,
        /// Resume from this event id (gap-free resume works against a2a-rs servers).
        #[arg(long)]
        last_event_id: Option<u64>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let url = cli
        .url
        .clone()
        .context("no agent URL: pass --url/-u or set A2A_URL")?;

    match &cli.command {
        Command::Card => {
            let card = a2a_rs::fetch_agent_card(&url)
                .await
                .context("fetching agent card")?;
            emit_card(cli.json, &card)?;
        }

        Command::Send {
            text,
            task_id,
            session_id,
            history_length,
        } => {
            let transport = build_transport(&cli, &url).await?;
            let task_id = task_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let message = Message::user_text(text.clone(), uuid::Uuid::new_v4().to_string());
            let task = transport
                .send_task_message(&task_id, &message, session_id.as_deref(), *history_length)
                .await
                .context("sending message")?;
            emit_task(cli.json, &task)?;
        }

        Command::Get {
            task_id,
            history_length,
        } => {
            let transport = build_transport(&cli, &url).await?;
            let task = transport
                .get_task(task_id, *history_length)
                .await
                .context("getting task")?;
            emit_task(cli.json, &task)?;
        }

        Command::Cancel { task_id } => {
            let transport = build_transport(&cli, &url).await?;
            let task = transport
                .cancel_task(task_id)
                .await
                .context("cancelling task")?;
            emit_task(cli.json, &task)?;
        }

        Command::Stream {
            task_id,
            resilient,
            last_event_id,
        } => {
            let transport = build_transport(&cli, &url).await?;
            let mut stream: EventStream = if *resilient {
                subscribe_resilient(
                    transport.clone(),
                    task_id.clone(),
                    None,
                    *last_event_id,
                    RetryPolicy::default(),
                )
            } else {
                let last = last_event_id.map(|id| id.to_string());
                transport
                    .subscribe_to_task(task_id, None, last.as_deref())
                    .await
                    .context("subscribing to task")?
            };
            while let Some(event) = stream.next().await {
                let event = event.context("stream error")?;
                emit_event(cli.json, &event)?;
            }
        }
    }

    Ok(())
}

/// Build a transport from the global args. `card` doesn't need this (it uses the
/// plain `fetch_agent_card` HTTP GET); everything else drives the `Transport` port.
async fn build_transport(cli: &Cli, url: &str) -> anyhow::Result<Arc<dyn Transport>> {
    let transport: Box<dyn Transport> = match cli.transport {
        TransportChoice::Auto => {
            if cli.auth.is_some() || cli.timeout.is_some() {
                tracing::warn!(
                    "--auth/--timeout are ignored in `auto` transport mode; \
                     use --transport connectrpc|jsonrpc to apply them"
                );
            }
            a2a_rs::auto_connect(url)
                .await
                .context("auto-connecting to agent")?
        }
        TransportChoice::Connectrpc => {
            let mut client = match &cli.auth {
                Some(token) => HttpClient::with_auth(url.to_string(), token.clone()),
                None => HttpClient::new(url.to_string()),
            };
            if let Some(secs) = cli.timeout {
                client = client.with_timeout(secs);
            }
            Box::new(client)
        }
        TransportChoice::Jsonrpc => {
            let mut client = match &cli.auth {
                Some(token) => JsonRpcClient::with_auth(url.to_string(), token.clone()),
                None => JsonRpcClient::new(url.to_string()),
            };
            if let Some(secs) = cli.timeout {
                client = client.with_timeout(secs);
            }
            Box::new(client)
        }
    };
    Ok(Arc::from(transport))
}

// --- output -----------------------------------------------------------------
//
// Human output is derived from the serialized (ProtoJSON, camelCase) value with
// defensive key lookups, so it doesn't couple to the build-time generated field
// idents. `--json` always prints the authoritative pretty JSON.

fn emit_card(json: bool, card: &AgentCard) -> anyhow::Result<()> {
    let value = serde_json::to_value(card)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }

    let s = |key: &str| str_field(&value, key);
    println!("{} v{}", or_dash(s("name")), or_dash(s("version")));
    if let Some(desc) = s("description") {
        println!("  {desc}");
    }
    if let Some(ifaces) = array_field(&value, "supportedInterfaces") {
        println!("  interfaces:");
        for iface in ifaces {
            println!(
                "    - {} {}",
                or_dash(str_field(iface, "protocolBinding")),
                or_dash(str_field(iface, "url")),
            );
        }
    }
    if let Some(skills) = array_field(&value, "skills") {
        println!("  skills:");
        for skill in skills {
            println!(
                "    - {}: {}",
                or_dash(str_field(skill, "name")),
                or_dash(str_field(skill, "description")),
            );
        }
    }
    Ok(())
}

fn emit_task(json: bool, task: &Task) -> anyhow::Result<()> {
    let value = serde_json::to_value(task)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    println!("task {}", or_dash(str_field(&value, "id")));
    if let Some(ctx) = str_field(&value, "contextId") {
        println!("  context: {ctx}");
    }
    println!("  state:   {}", or_dash(task_state(&value)));
    Ok(())
}

fn emit_event(json: bool, event: &StreamEvent) -> anyhow::Result<()> {
    let (kind, payload) = match &event.item {
        StreamItem::Task(t) => ("task", serde_json::to_value(t)?),
        StreamItem::StatusUpdate(u) => ("status", serde_json::to_value(u)?),
        StreamItem::ArtifactUpdate(a) => ("artifact", serde_json::to_value(a)?),
    };

    if json {
        let envelope = serde_json::json!({
            "eventId": event.event_id,
            "type": kind,
            "payload": payload,
        });
        println!("{}", serde_json::to_string(&envelope)?);
        return Ok(());
    }

    let id = event
        .event_id
        .map(|n| format!("#{n} "))
        .unwrap_or_default();
    match kind {
        "task" => println!(
            "{id}● task {} [{}]",
            or_dash(str_field(&payload, "id")),
            or_dash(task_state(&payload)),
        ),
        "status" => println!("{id}◌ status [{}]", or_dash(task_state(&payload))),
        _ => {
            let artifact = payload.get("artifact");
            let name = artifact
                .and_then(|a| str_field(a, "name"))
                .or_else(|| artifact.and_then(|a| str_field(a, "artifactId")));
            println!("{id}▣ artifact {}", or_dash(name));
        }
    }
    Ok(())
}

// --- small JSON helpers ------------------------------------------------------

fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn array_field<'a>(value: &'a Value, key: &str) -> Option<&'a Vec<Value>> {
    value.get(key).and_then(Value::as_array).filter(|a| !a.is_empty())
}

/// A task's `status.state`, e.g. `"TASK_STATE_SUBMITTED"`.
fn task_state(value: &Value) -> Option<&str> {
    value.get("status").and_then(|s| str_field(s, "state"))
}

fn or_dash(value: Option<&str>) -> &str {
    value.unwrap_or("-")
}
