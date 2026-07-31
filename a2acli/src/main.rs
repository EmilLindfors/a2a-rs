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

use std::borrow::Cow;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use a2a_rs::domain::{A2AError, AgentCard, Message, SendCompletion, Task, TaskStateExt};
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
    #[arg(
        short,
        long,
        env = "A2A_URL",
        visible_alias = "base-url",
        global = true
    )]
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
        /// Print the acknowledgement without waiting for the agent's reply.
        ///
        /// Agents that answer asynchronously (the `llm` handler, for one) return
        /// `working` here and deliver the reply on a later `get`.
        #[arg(long)]
        no_wait: bool,
        /// Seconds to wait for the agent to finish before giving up on it.
        ///
        /// Distinct from `--timeout`, which bounds a single request.
        #[arg(long, default_value_t = 30, value_name = "SECS")]
        wait_timeout: u64,
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
            no_wait,
            wait_timeout,
        } => {
            let transport = build_transport(&cli, &url).await?;
            let task_id = task_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let message = Message::user_text(text.clone(), uuid::Uuid::new_v4().to_string());
            // `--no-wait` has to reach the server too. Asking a conformant agent
            // to block and then declining to wait for it just moves the wait
            // somewhere the flag cannot switch off.
            let completion = if *no_wait {
                SendCompletion::WhenCreated
            } else {
                SendCompletion::WhenSettled
            };
            let mut task = transport
                .send_task_message(
                    &task_id,
                    &message,
                    session_id.as_deref(),
                    *history_length,
                    completion,
                )
                .await
                .context("sending message")?;
            // A conformant agent has already settled the task by the time it
            // answers; this loop is the fallback for one that ignores
            // `return_immediately`.
            if !no_wait && !is_settled(&task) {
                task = poll_until_settled(
                    transport.as_ref(),
                    &task_id,
                    *history_length,
                    Duration::from_secs(*wait_timeout),
                )
                .await?;
            }
            emit_task(cli.json, &task)?;
            if !cli.json && !is_settled(&task) {
                println!();
                println!("the agent is still working; follow it with:");
                println!("  a2acli --url {url} stream {task_id}");
                println!("  a2acli --url {url} get {task_id}");
            }
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

/// Whether the agent has stopped making progress on its own — either finished,
/// or waiting on the caller. Anything else means a reply is still coming.
///
/// Delegates to the domain so the CLI, the server's blocking `SendMessage`, and
/// the subscription-close rule all stop at the same set of states.
fn is_settled(task: &Task) -> bool {
    task.status.state.is_settled()
}

/// Poll `get_task` until the task settles or the budget runs out.
///
/// The fallback for an agent that ignores `return_immediately` and answers
/// asynchronously: it reports `working` with no reply attached, and without
/// this a freshly scaffolded `llm` agent looks like it did nothing.
async fn poll_until_settled(
    transport: &dyn Transport,
    task_id: &str,
    history_length: Option<u32>,
    budget: Duration,
) -> anyhow::Result<Task> {
    const INTERVAL: Duration = Duration::from_millis(250);
    let deadline = tokio::time::Instant::now() + budget;
    let mut announced = false;

    loop {
        let now = tokio::time::Instant::now();
        tokio::time::sleep(INTERVAL.min(deadline.saturating_duration_since(now))).await;
        let task = transport
            .get_task(task_id, history_length)
            .await
            .context("polling task")?;
        if is_settled(&task) || tokio::time::Instant::now() >= deadline {
            return Ok(task);
        }
        // Straight to stderr rather than `tracing`: the default filter is `warn`,
        // so a logged line would never reach the person waiting on the prompt —
        // and the report on stdout has to stay greppable.
        if !announced {
            announced = true;
            eprintln!(
                "waiting up to {}s for the agent to reply (--no-wait to skip)...",
                budget.as_secs()
            );
        }
    }
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
    println!("  state:   {}", task_state_label(&value));

    // The agent's answer lives in the status message and in any artifacts.
    // Printing only id and state made a working agent look like a broken one.
    if let Some(reply) = value
        .get("status")
        .and_then(|status| status.get("message"))
        .and_then(parts_text)
    {
        println!();
        println!("{reply}");
    }
    for artifact in array_field(&value, "artifacts").into_iter().flatten() {
        let Some(body) = parts_text(artifact) else {
            continue;
        };
        let name = str_field(artifact, "name").or_else(|| str_field(artifact, "artifactId"));
        println!();
        println!("--- {} ---", or_dash(name));
        println!("{body}");
    }
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

    let id = event.event_id.map(|n| format!("#{n} ")).unwrap_or_default();
    match kind {
        "task" => println!(
            "{id}● task {} [{}]",
            or_dash(str_field(&payload, "id")),
            task_state_label(&payload),
        ),
        "status" => {
            println!("{id}◌ status [{}]", task_state_label(&payload));
            print_indented(
                payload
                    .get("status")
                    .and_then(|status| status.get("message"))
                    .and_then(parts_text),
            );
        }
        _ => {
            let artifact = payload.get("artifact");
            let name = artifact
                .and_then(|a| str_field(a, "name"))
                .or_else(|| artifact.and_then(|a| str_field(a, "artifactId")));
            println!("{id}▣ artifact {}", or_dash(name));
            print_indented(artifact.and_then(parts_text));
        }
    }
    Ok(())
}

// --- small JSON helpers ------------------------------------------------------

fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn array_field<'a>(value: &'a Value, key: &str) -> Option<&'a Vec<Value>> {
    value
        .get(key)
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
}

/// A task's `status.state` as it appears on the wire, e.g. `"TASK_STATE_SUBMITTED"`.
fn task_state(value: &Value) -> Option<&str> {
    value.get("status").and_then(|s| str_field(s, "state"))
}

/// The same state as a person would say it: `TASK_STATE_INPUT_REQUIRED` reads
/// `input-required`. The proto name belongs on the wire and in `--json`.
fn task_state_label(value: &Value) -> String {
    let Some(state) = task_state(value) else {
        return "-".to_string();
    };
    state
        .strip_prefix("TASK_STATE_")
        .unwrap_or(state)
        .to_ascii_lowercase()
        .replace('_', "-")
}

/// The text of a `parts[]`-carrying value (a message or an artifact), with
/// non-text parts named rather than dropped. `None` when there is nothing to show.
fn parts_text(container: &Value) -> Option<String> {
    let rendered: Vec<Cow<'_, str>> = array_field(container, "parts")?
        .iter()
        .map(render_part)
        .collect();
    Some(rendered.join("\n"))
}

/// Text parts verbatim; everything else named in brackets, so a file part is
/// never mistaken for the agent having said its filename.
fn render_part(part: &Value) -> Cow<'_, str> {
    if let Some(text) = str_field(part, "text") {
        return Cow::Borrowed(text);
    }
    let what = str_field(part, "filename")
        .or_else(|| str_field(part, "url"))
        .or_else(|| str_field(part, "mediaType"))
        .unwrap_or("non-text content");
    Cow::Owned(format!("[{what}]"))
}

/// Print a block of agent text under the line it belongs to, or nothing at all.
fn print_indented(text: Option<String>) {
    for line in text.iter().flat_map(|text| text.lines()) {
        println!("   {line}");
    }
}

fn or_dash(value: Option<&str>) -> &str {
    value.unwrap_or("-")
}
