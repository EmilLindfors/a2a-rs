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

use a2a_rs::domain::{
    A2AError, AgentCard, ListTasksParams, ListTasksResult, Message, SendCompletion, Task,
    TaskState, TaskStateExt,
};
use a2a_rs::{
    ClientConfig, HttpClient, JsonRpcClient, RetryPolicy, StreamEvent, StreamItem, Transport,
    subscribe_resilient,
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
    /// Applies in every transport mode, including the agent-card fetch — an
    /// agent that guards its RPC endpoints usually guards its card too.
    #[arg(long, env = "A2A_AUTH_TOKEN", global = true)]
    auth: Option<String>,

    /// Request timeout in seconds. Bounds a single request, not the whole wait
    /// for an agent's reply — that is `send --wait-timeout`.
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

/// A task state as a person spells it, mapped to the wire's enum.
///
/// A `ValueEnum` rather than a parsed string so `--state nonsense` is rejected
/// by the argument parser with the valid values listed, instead of becoming a
/// filter that silently matches nothing.
#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum StateArg {
    Submitted,
    Working,
    InputRequired,
    AuthRequired,
    Completed,
    Canceled,
    Failed,
    Rejected,
}

impl From<StateArg> for TaskState {
    fn from(arg: StateArg) -> Self {
        match arg {
            StateArg::Submitted => TaskState::Submitted,
            StateArg::Working => TaskState::Working,
            StateArg::InputRequired => TaskState::InputRequired,
            StateArg::AuthRequired => TaskState::AuthRequired,
            StateArg::Completed => TaskState::Completed,
            StateArg::Canceled => TaskState::Canceled,
            StateArg::Failed => TaskState::Failed,
            StateArg::Rejected => TaskState::Rejected,
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Fetch and print the agent card.
    Card,

    /// Send a text message to a task (a task id is generated when omitted).
    Send {
        /// The message text, or `-` to read it from stdin.
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

    /// List the agent's tasks.
    List {
        /// Only tasks in this state.
        #[arg(long, value_enum)]
        state: Option<StateArg>,
        /// Maximum number of tasks to return.
        #[arg(long, value_name = "N")]
        limit: Option<i32>,
        /// Only tasks in this context (conversation).
        #[arg(long, value_name = "ID")]
        context_id: Option<String>,
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
            let card = a2a_rs::fetch_agent_card_with(&url, &client_config(&cli))
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
            let message = Message::user_text(read_text(text)?, uuid::Uuid::new_v4().to_string());
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
                    task_id.as_deref(),
                    &message,
                    session_id.as_deref(),
                    *history_length,
                    completion,
                )
                .await
                .context("sending message")?;
            // Without `--task-id` the server named the task, so every follow-up
            // has to use the id it sent back rather than one chosen here.
            let task_id = task.id.clone();
            // A conformant agent has already settled the task by the time it
            // answers; this wait is the fallback for one that ignores
            // `return_immediately`.
            if !no_wait && !is_settled(&task) {
                task = wait_until_settled(
                    transport.as_ref(),
                    &task_id,
                    *history_length,
                    Duration::from_secs(*wait_timeout),
                )
                .await?;
            }
            emit_task(cli.json, &task)?;
            if !cli.json {
                print_next_step(&url, &task_id, &task);
            }
            return finish(&task);
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
            if !cli.json {
                print_next_step(&url, task_id, &task);
            }
            return finish(&task);
        }

        // No `finish` here: cancelling is the one command whose success is
        // measured by the *request*, not the task's state. A canceled task is
        // this command working, so reporting failure would be backwards.
        Command::Cancel { task_id } => {
            let transport = build_transport(&cli, &url).await?;
            let task = transport
                .cancel_task(task_id)
                .await
                .context("cancelling task")?;
            emit_task(cli.json, &task)?;
        }

        Command::List {
            state,
            limit,
            context_id,
        } => {
            let transport = build_transport(&cli, &url).await?;
            let params = ListTasksParams {
                status: state.map(TaskState::from),
                page_size: *limit,
                context_id: context_id.clone(),
                ..Default::default()
            };
            let result = transport
                .list_tasks(&params)
                .await
                .context("listing tasks")?;
            emit_task_list(cli.json, &result)?;
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

/// The credentials and timeout the caller supplied, in the shape every
/// connection path takes — negotiated, direct, or a bare card fetch.
fn client_config(cli: &Cli) -> ClientConfig {
    let mut config = ClientConfig::new();
    if let Some(token) = &cli.auth {
        config = config.with_auth_token(token.clone());
    }
    if let Some(secs) = cli.timeout {
        config = config.with_timeout(secs);
    }
    config
}

/// Build a transport from the global args. `card` doesn't need this (it uses the
/// plain `fetch_agent_card_with` HTTP GET); everything else drives the
/// `Transport` port.
async fn build_transport(cli: &Cli, url: &str) -> anyhow::Result<Arc<dyn Transport>> {
    let config = client_config(cli);
    let transport: Box<dyn Transport> = match cli.transport {
        TransportChoice::Auto => a2a_rs::auto_connect_with(url, &config)
            .await
            .context("auto-connecting to agent")?,
        // `try_*` rather than `HttpClient::new`, which panics on a URL
        // `http::Uri` cannot represent — and `--url` is user input.
        TransportChoice::Connectrpc => {
            let mut client = match &cli.auth {
                Some(token) => HttpClient::try_with_auth(url.to_string(), token.clone()),
                None => HttpClient::try_new(url.to_string()),
            }
            .context("building a ConnectRPC client")?;
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

/// The message body: `-` means stdin, so a long prompt can be piped or kept in
/// a file rather than fought through shell quoting.
fn read_text(text: &str) -> anyhow::Result<String> {
    use std::io::Read;

    if text != "-" {
        return Ok(text.to_string());
    }
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("reading the message from stdin")?;
    // A trailing newline is an artefact of how the text arrived, not part of
    // what the user meant to say.
    Ok(buf.trim_end_matches(['\n', '\r']).to_string())
}

/// The command worked and the *agent* failed or rejected the task.
///
/// Distinct from `1` — the command itself failing — so a script can retry a
/// timeout without retrying a refusal. Those need opposite responses, and one
/// exit code cannot ask for both.
const EXIT_TASK_FAILED: i32 = 2;

/// Exit non-zero when the agent rejected or failed the task.
///
/// The request succeeding and the work succeeding are different facts, and a
/// CLI that reports only the first turns `a2acli send … && deploy` into a
/// deploy on a failed task. Only the agent's own verdicts count: a task still
/// working is not a failure (the caller was told how to follow it), an
/// interrupted one is a question (likewise), and `Canceled` is somebody getting
/// what they asked for.
fn finish(task: &Task) -> anyhow::Result<()> {
    let state = &task.status.state;
    if *state == TaskState::Failed || *state == TaskState::Rejected {
        let label = task_state_label(&serde_json::to_value(task)?);
        eprintln!("a2acli: the agent {label} this task");
        // The task was already printed to stdout; make sure it is out before
        // exiting past the normal end of `main`.
        let _ = std::io::Write::flush(&mut std::io::stdout());
        std::process::exit(EXIT_TASK_FAILED);
    }
    Ok(())
}

/// Say what to do next, when there is anything to do.
///
/// The three cases are genuinely different and the CLI used to answer only one
/// of them. A task still running is followed with `stream`/`get`. A task in an
/// *interrupted* state is waiting on **you** — and since those states count as
/// settled, the CLI previously printed the state and fell silent, which reads
/// like the agent gave up rather than asked a question. A finished task needs
/// no advice at all.
fn print_next_step(url: &str, task_id: &str, task: &Task) {
    if task.status.state.is_interrupted() {
        println!();
        println!("the agent is waiting for you; answer on the same task with:");
        println!("  a2acli --url {url} send --task-id {task_id} \"your reply\"");
    } else if !is_settled(task) {
        println!();
        println!("the agent is still working; follow it with:");
        println!("  a2acli --url {url} stream {task_id}");
        println!("  a2acli --url {url} get {task_id}");
    }
}

/// Whether the agent has stopped making progress on its own — either finished,
/// or waiting on the caller. Anything else means a reply is still coming.
///
/// Delegates to the domain so the CLI, the server's blocking `SendMessage`, and
/// the subscription-close rule all stop at the same set of states.
fn is_settled(task: &Task) -> bool {
    task.status.state.is_settled()
}

/// Wait for `task_id` to settle, or for `budget` to run out.
///
/// The fallback for an agent that ignores `return_immediately` and answers
/// asynchronously: it reports `working` with no reply attached, and without
/// this a freshly scaffolded `llm` agent looks like it did nothing.
///
/// Prefers the agent's event stream — it wakes on the agent's actual progress
/// instead of on a timer, and an a2a-rs server closes the subscription the
/// moment the task settles. Polling remains the fallback for a server with no
/// streaming backend, or one whose stream drops before the task finishes.
async fn wait_until_settled(
    transport: &dyn Transport,
    task_id: &str,
    history_length: Option<u32>,
    budget: Duration,
) -> anyhow::Result<Task> {
    let deadline = tokio::time::Instant::now() + budget;
    let _notice = WaitNotice::after(ANNOUNCE_AFTER, budget);

    if let Ok(stream) = transport
        .subscribe_to_task(task_id, history_length, None)
        .await
    {
        watch_until_settled(stream, deadline).await;
        // Re-read rather than trusting the last event: the stream may have
        // ended on an error or a deadline, and only `get_task` honours
        // `history_length` uniformly across transports.
        let task = transport
            .get_task(task_id, history_length)
            .await
            .context("reading task after subscription")?;
        if is_settled(&task) || tokio::time::Instant::now() >= deadline {
            return Ok(task);
        }
    }

    poll_until_settled(transport, task_id, history_length, deadline).await
}

/// Drain a subscription until it reports a settled state, ends, errors, or runs
/// past `deadline`. The caller decides what actually happened by re-reading the
/// task, so every one of those is just "stop watching".
async fn watch_until_settled(mut stream: EventStream, deadline: tokio::time::Instant) {
    let drain = async {
        while let Some(Ok(event)) = stream.next().await {
            if event_settles(&event) {
                break;
            }
        }
    };
    let _ = tokio::time::timeout_at(deadline, drain).await;
}

/// Whether an event says the agent has stopped working on the task.
fn event_settles(event: &StreamEvent) -> bool {
    match &event.item {
        StreamItem::Task(task) => task.status.state.is_settled(),
        StreamItem::StatusUpdate(update) => update.status.state.is_settled(),
        StreamItem::ArtifactUpdate(_) => false,
    }
}

/// Poll `get_task` until the task settles or `deadline` passes.
async fn poll_until_settled(
    transport: &dyn Transport,
    task_id: &str,
    history_length: Option<u32>,
    deadline: tokio::time::Instant,
) -> anyhow::Result<Task> {
    const INTERVAL: Duration = Duration::from_millis(250);

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
    }
}

/// How long a wait may run before it is worth telling the person about.
const ANNOUNCE_AFTER: Duration = Duration::from_millis(250);

/// Tells whoever is at the prompt that we are waiting — but only once the wait
/// is long enough to notice, so the common case of a fast agent prints nothing.
///
/// Straight to stderr rather than `tracing`: the default filter is `warn`, so a
/// logged line would never reach them — and the report on stdout has to stay
/// greppable.
struct WaitNotice(tokio::task::JoinHandle<()>);

impl WaitNotice {
    fn after(delay: Duration, budget: Duration) -> Self {
        Self(tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            eprintln!(
                "waiting up to {}s for the agent to reply (--no-wait to skip)...",
                budget.as_secs()
            );
        }))
    }
}

impl Drop for WaitNotice {
    fn drop(&mut self) {
        self.0.abort();
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

/// One line per task, newest concerns first: id, state, and the opening of
/// whatever was last said on it — enough to recognise the task you meant.
fn emit_task_list(json: bool, result: &ListTasksResult) -> anyhow::Result<()> {
    let value = serde_json::to_value(result)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }

    let Some(tasks) = array_field(&value, "tasks") else {
        println!("no tasks");
        return Ok(());
    };
    for task in tasks {
        let summary = task
            .get("status")
            .and_then(|status| status.get("message"))
            .and_then(parts_text)
            .map(|text| first_line(&text))
            .unwrap_or_default();
        println!(
            "{:<38} {:<15} {}",
            or_dash(str_field(task, "id")),
            task_state_label(task),
            summary,
        );
    }
    // `totalSize` is the server's count of everything matching, which is not
    // the same as what fitted on this page — say both rather than imply one.
    if let Some(total) = value.get("totalSize").and_then(Value::as_i64)
        && total > tasks.len() as i64
    {
        println!("({} of {total} shown; raise --limit for more)", tasks.len());
    }
    Ok(())
}

/// The first line of `text`, elided if it is long — a list stays a list only if
/// each entry is one row.
fn first_line(text: &str) -> String {
    const WIDTH: usize = 60;
    let line = text.lines().next().unwrap_or_default().trim();
    match line.char_indices().nth(WIDTH) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line.to_string(),
    }
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
