//! Starter configs for `a2a new`.
//!
//! Scaffolding is where most people meet the config schema, so these templates
//! are written to be *read*: every non-obvious key carries a comment explaining
//! what it does and what happens if you leave it out. A generated file should
//! answer "what else can I put here?" without a trip to `print-schema`.
//!
//! Rendering is pure — no filesystem, no environment. The binary owns writing
//! the file, which keeps the templates unit-testable against the real parser
//! (see the tests below: every template must round-trip through
//! [`AgentConfig`](super::AgentConfig)).

use crate::utils::slugify;

/// Which starter config to generate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTemplate {
    /// Minimal echo agent — no LLM, no external services, runs immediately.
    Echo,
    /// Config-driven LLM handler answering in natural language.
    Llm,
    /// LLM agent wired to an MCP server, exposing its tools to the model.
    Mcp,
    /// LLM agent that delegates to peer A2A agents as tools.
    Orchestrator,
}

impl AgentTemplate {
    /// Every template, for CLI help and exhaustive tests.
    pub const ALL: [AgentTemplate; 4] = [
        AgentTemplate::Echo,
        AgentTemplate::Llm,
        AgentTemplate::Mcp,
        AgentTemplate::Orchestrator,
    ];

    /// The `--template` value that selects this one.
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentTemplate::Echo => "echo",
            AgentTemplate::Llm => "llm",
            AgentTemplate::Mcp => "mcp",
            AgentTemplate::Orchestrator => "orchestrator",
        }
    }

    /// The port a scaffolded agent binds unless overridden. The orchestrator
    /// differs so it can run alongside an agent scaffolded from another template
    /// without a port clash — the common first multi-agent setup.
    ///
    /// Both stay in the 80xx band, clear of the control plane's `:9090`: an
    /// orchestrator and a control plane are exactly the pair someone runs
    /// together, and the collision was silent enough that the losing bind read
    /// as the agent simply not answering.
    pub fn default_port(&self) -> u16 {
        match self {
            AgentTemplate::Orchestrator => 8090,
            _ => 8080,
        }
    }

    /// Whether running this template usefully needs an LLM API key.
    pub fn needs_llm_key(&self) -> bool {
        !matches!(self, AgentTemplate::Echo)
    }

    /// Render a starter config for an agent called `name` on `port`.
    pub fn render(&self, name: &str, port: u16) -> String {
        let skill_id = slugify(name, '-');
        match self {
            AgentTemplate::Echo => echo(name, port, &skill_id),
            AgentTemplate::Llm => llm(name, port),
            AgentTemplate::Mcp => mcp(name, port),
            AgentTemplate::Orchestrator => orchestrator(name, port),
        }
    }
}

impl std::fmt::Display for AgentTemplate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for AgentTemplate {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        AgentTemplate::ALL
            .into_iter()
            .find(|t| t.as_str() == s)
            .ok_or_else(|| {
                let known: Vec<&str> = AgentTemplate::ALL.iter().map(|t| t.as_str()).collect();
                format!(
                    "unknown template '{s}' (expected one of {})",
                    known.join(", ")
                )
            })
    }
}

fn echo(name: &str, port: u16, skill_id: &str) -> String {
    format!(
        r#"# {name} — a minimal A2A agent.
#
# Runs as-is with no API keys and no external services:
#   a2a validate --config {skill_id}.toml
#   a2a run --config {skill_id}.toml

[agent]
name = "{name}"
description = "Echoes back whatever you send."
# version = "0.1.0"

[server]
# Omit `host` to bind whatever HOST is set to (0.0.0.0 in the container image).
host = "127.0.0.1"
http_port = {port}

[server.storage]
# "inmemory" loses tasks on restart. Use type = "sqlx" with a `url` to persist.
type = "inmemory"

# Skills are what the agent advertises on its card; peers discover it by these.
[[skills]]
id = "{skill_id}"
name = "Echo"
description = "Echoes back whatever you send."
keywords = ["echo", "test"]
examples = ["Say hello"]

[features]
streaming = true
push_notifications = false
state_history = true
"#
    )
}

fn llm(name: &str, port: u16) -> String {
    format!(
        r#"# {name} — a config-driven LLM agent. No custom Rust required.
#
# Set one of OPENAI_API_KEY / GEMINI_API_KEY / OPENROUTER_API_KEY for
# natural-language answers. Without a key the agent still runs and replies with a
# deterministic fallback, so this is safe to start in CI.

[agent]
name = "{name}"
description = "Answers questions in natural language."
version = "0.1.0"

[server]
host = "127.0.0.1"
http_port = {port}

[server.storage]
type = "inmemory"

[[skills]]
id = "chat"
name = "Chat"
description = "Answer questions in natural language."
keywords = ["chat", "ask", "question"]
examples = ["What can you do?", "Explain how A2A works"]
input_formats = ["text"]
output_formats = ["text"]

[handler]
# Selects the generic LLM handler built into the `a2a` binary.
type = "llm"

[handler.llm]
system_prompt = "You are a concise, helpful assistant running as an A2A agent."
# How many model <-> tool round-trips before giving up.
max_tool_rounds = 4

[features]
streaming = true
push_notifications = false
state_history = true
"#
    )
}

fn mcp(name: &str, port: u16) -> String {
    format!(
        r#"# {name} — an LLM agent that can call MCP tools.
#
# The [[features.mcp_client.servers]] entry below spawns an MCP stdio server as a
# child process and exposes its tools to the model. Point `command`/`args` at any
# MCP server — an `npx` package, a compiled binary, a script.
#
# Set an LLM key (OPENAI_API_KEY / GEMINI_API_KEY / OPENROUTER_API_KEY) so the
# model can decide when to call a tool.

[agent]
name = "{name}"
description = "Answers questions, using MCP tools when they help."
version = "0.1.0"

[server]
host = "127.0.0.1"
http_port = {port}

[server.storage]
type = "inmemory"

[[skills]]
id = "assist"
name = "Assist"
description = "Answer questions, calling MCP tools when useful."
keywords = ["chat", "tools"]
examples = ["What files are in my project?"]
input_formats = ["text"]
output_formats = ["text"]

[handler]
type = "llm"

[handler.llm]
system_prompt = "You are a helpful assistant. Use the available tools when they give a more precise answer than guessing."
max_tool_rounds = 4

[features]
streaming = true
push_notifications = false
state_history = true

[features.mcp_client]
enabled = true

# One block per MCP server. `name` namespaces its tools for the model.
[[features.mcp_client.servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
"#
    )
}

fn orchestrator(name: &str, port: u16) -> String {
    format!(
        r#"# {name} — delegates to peer A2A agents as tools.
#
# Each [[handler.llm.agents]] entry becomes one `ask_<slug>` tool the model can
# call; the peer is reached over the wire with its transport auto-negotiated from
# its agent card. This is multi-agent with no custom Rust.
#
# Start a peer agent first (e.g. `a2a new Weather --template echo` on :8080),
# then run this one. Set an LLM key so the model can decide when to delegate.

[agent]
name = "{name}"
description = "Routes requests to specialist A2A agents."
version = "0.1.0"

[server]
host = "127.0.0.1"
http_port = {port}

[server.storage]
type = "inmemory"

[[skills]]
id = "route"
name = "Route"
description = "Understand a request and delegate to the right specialist agent."
keywords = ["route", "delegate", "orchestrate"]
examples = ["Ask the weather agent about tomorrow"]
input_formats = ["text"]
output_formats = ["text"]

[handler]
type = "llm"

[handler.llm]
system_prompt = "You are an orchestrator. When a specialist tool fits the request, call it and relay its answer. Otherwise answer directly and concisely."
max_tool_rounds = 4

# Name a peer by exactly one of `url`, `skill`, or `agent_id`. `skill` and
# `agent_id` are resolved against the agent registry at startup; `url` dials
# directly. `description` is optional — omit it to use the peer's own card.
[[handler.llm.agents]]
name = "Weather Agent"
url = "http://127.0.0.1:8080"
description = "Answers questions about current and forecast weather."

[features]
streaming = true
push_notifications = false
state_history = true
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;

    /// The whole point: a scaffolded config must be immediately runnable. If a
    /// template drifts from the schema, the first thing a new user does breaks.
    #[test]
    fn every_template_parses_and_validates() {
        for template in AgentTemplate::ALL {
            let rendered = template.render("My Agent", template.default_port());
            let config = AgentConfig::from_toml(&rendered)
                .unwrap_or_else(|e| panic!("template '{template}' does not parse: {e}"));

            assert_eq!(config.agent.name, "My Agent");
            assert_eq!(config.server.http_port, template.default_port());
            assert!(
                !config.skills.is_empty(),
                "template '{template}' must advertise at least one skill, or peers cannot discover it"
            );
        }
    }

    /// Templates must not reference `${{VAR}}`: a freshly scaffolded config has
    /// to run without the user first setting something. LLM keys are read from
    /// the environment by the provider, not templated into the config.
    #[test]
    fn no_template_requires_an_env_var_to_parse() {
        for template in AgentTemplate::ALL {
            let rendered = template.render("Env Free", 8080);
            let refs = crate::core::referenced_env_vars(&rendered);
            assert!(
                refs.is_empty(),
                "template '{template}' references {refs:?}, so it would not run as scaffolded"
            );
        }
    }

    #[test]
    fn llm_templates_select_the_llm_handler() {
        for template in AgentTemplate::ALL {
            let config = AgentConfig::from_toml(&template.render("H", 8080)).unwrap();
            let handler = config.handler_type().to_string();
            let expected = if template == AgentTemplate::Echo {
                "echo"
            } else {
                "llm"
            };
            assert_eq!(handler, expected, "template '{template}'");
        }
    }

    /// A name with punctuation and spaces must still yield a valid TOML string
    /// and a usable skill id.
    #[test]
    fn awkward_names_render_valid_configs() {
        let config = AgentConfig::from_toml(&AgentTemplate::Echo.render("Bob's Agent v2", 8080))
            .expect("apostrophes and digits must not break rendering");
        assert_eq!(config.agent.name, "Bob's Agent v2");
        assert_eq!(config.skills[0].id, "bob-s-agent-v2");
    }

    #[test]
    fn template_names_round_trip() {
        for template in AgentTemplate::ALL {
            assert_eq!(template.as_str().parse::<AgentTemplate>(), Ok(template));
        }
        assert!("nope".parse::<AgentTemplate>().is_err());
    }
}
