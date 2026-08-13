//! Pre-flight requirements: what a config needs from the machine it runs on.
//!
//! A declarative agent fails in a distinctive way — the config is *valid* and the
//! run still does not work, because the port is taken, the MCP command is not
//! installed, or no model key is set. Each of those surfaces at runtime as a
//! different confusing symptom (a bind error buried in a log, a tool that quietly
//! does not exist, an agent that answers with a canned fallback).
//!
//! This module names those needs as data. Deriving them is **pure** — it reads
//! only the config — so the rules are unit-tested here, while probing the actual
//! machine (binding a port, searching `PATH`, reading the environment) stays in
//! the binary where the I/O belongs. `a2a doctor` is the two halves joined.

use a2a_agents_common::llm::LlmSettings;

use crate::core::config::AgentConfig;
use crate::core::config::HandlerType;

/// Something that has to hold on the host for a config to run as written.
///
/// Deliberately *not* a verdict: this says what is needed, not whether it is
/// there. Whoever can see the machine decides that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requirement {
    /// The agent binds this address for its A2A HTTP server.
    HttpBind {
        /// Interface it binds.
        host: String,
        /// TCP port it binds.
        port: u16,
    },
    /// The agent also binds this address for its MCP Streamable HTTP server.
    McpHttpBind {
        /// Interface it binds.
        host: String,
        /// TCP port it binds.
        port: u16,
    },
    /// An MCP server is spawned as a child process, so its command must be
    /// runnable. When it is not, the agent still starts and the tools it
    /// advertises simply are not there.
    McpCommand {
        /// The `[[features.mcp_client.servers]]` name, for the report.
        server: String,
        /// The command as configured.
        command: String,
    },
    /// The `llm` handler needs a working provider. Without one the agent runs
    /// and answers with a deterministic fallback instead of a model; with a
    /// broken one (`a2a run` refuses to start) it does not run at all.
    ///
    /// Carries where the provider comes from so the check can build it the way
    /// `a2a run` does, rather than guessing from the presence of a variable.
    LlmProvider(LlmSource),
    /// A handler name no built-in provides. The runner falls back to echo, which
    /// means a config that looks configured behaves like a stub.
    UnknownHandler {
        /// The configured `handler.type`.
        name: String,
    },
}

/// Where an `llm` handler's provider comes from — the two arms of `a2a run`'s
/// own resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmSource {
    /// Built from the config's `[llm]` block. It may still read the environment
    /// for anything the block leaves out, including the API key.
    Config(LlmSettings),
    /// No `[llm]` block at all, so the provider is selected from the
    /// environment (OpenRouter → Gemini → OpenAI).
    Environment,
}

/// Everything `config` needs from its host, in report order.
pub fn requirements(config: &AgentConfig) -> Vec<Requirement> {
    let mut requirements = Vec::new();

    // Port 0 means "no HTTP server" (MCP-only agents), so nothing is bound.
    if config.server.http_port != 0 {
        requirements.push(Requirement::HttpBind {
            host: config.server.host.clone(),
            port: config.server.http_port,
        });
    }

    let mcp_server = &config.features.mcp_server;
    if mcp_server.enabled && mcp_server.http.enabled {
        requirements.push(Requirement::McpHttpBind {
            host: mcp_server.http.host.clone(),
            port: mcp_server.http.port,
        });
    }

    if config.features.mcp_client.enabled {
        for server in &config.features.mcp_client.servers {
            requirements.push(Requirement::McpCommand {
                server: server.name.clone(),
                command: server.command.clone(),
            });
        }
    }

    match config.handler_type() {
        HandlerType::Llm => {
            requirements.push(Requirement::LlmProvider(match config.llm.as_ref() {
                Some(llm) => LlmSource::Config(llm.into()),
                None => LlmSource::Environment,
            }))
        }
        HandlerType::Custom(name) => requirements.push(Requirement::UnknownHandler { name }),
        // The reimbursement agent is a sample behind an opt-in feature, so
        // whether it exists depends on how this binary was built. Unbuilt, the
        // runner falls back to echo — a config that looks configured behaving
        // like a stub is precisely what this check is for.
        #[cfg(not(feature = "reimbursement-agent"))]
        HandlerType::Reimbursement => requirements.push(Requirement::UnknownHandler {
            name: "reimbursement".to_string(),
        }),
        #[cfg(feature = "reimbursement-agent")]
        HandlerType::Reimbursement => {}
        HandlerType::Echo => {}
    }

    requirements
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(toml: &str) -> AgentConfig {
        AgentConfig::from_toml(toml).expect("fixture config parses")
    }

    #[test]
    fn an_echo_agent_only_needs_its_port() {
        let config = config(
            r#"
            [agent]
            name = "Echo"
            [server]
            host = "127.0.0.1"
            http_port = 8080
            "#,
        );
        assert_eq!(
            requirements(&config),
            [Requirement::HttpBind {
                host: "127.0.0.1".into(),
                port: 8080
            }]
        );
    }

    /// `http_port = 0` is how an MCP-only agent says "serve no HTTP"; reporting
    /// a bind for it would be a permanent false alarm.
    #[test]
    fn port_zero_needs_no_bind() {
        let config = config(
            r#"
            [agent]
            name = "Stdio Only"
            [server]
            http_port = 0
            [features.mcp_server]
            enabled = true
            "#,
        );
        assert!(requirements(&config).is_empty());
    }

    #[test]
    fn an_mcp_http_agent_needs_both_binds() {
        let config = config(
            r#"
            [agent]
            name = "Both"
            [server]
            host = "127.0.0.1"
            http_port = 8080
            [features.mcp_server]
            enabled = true
            [features.mcp_server.http]
            enabled = true
            host = "127.0.0.1"
            port = 8000
            "#,
        );
        assert_eq!(
            requirements(&config),
            [
                Requirement::HttpBind {
                    host: "127.0.0.1".into(),
                    port: 8080
                },
                Requirement::McpHttpBind {
                    host: "127.0.0.1".into(),
                    port: 8000
                },
            ]
        );
    }

    #[test]
    fn mcp_client_commands_are_requirements_only_when_enabled() {
        let toml = r#"
            [agent]
            name = "Tools"
            [server]
            http_port = 8080
            [features.mcp_client]
            enabled = {enabled}
            [[features.mcp_client.servers]]
            name = "filesystem"
            command = "npx"
            args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
        "#;

        let enabled = config(&toml.replace("{enabled}", "true"));
        assert!(requirements(&enabled).contains(&Requirement::McpCommand {
            server: "filesystem".into(),
            command: "npx".into(),
        }));

        // A disabled block is configuration kept for later, not a need.
        let disabled = config(&toml.replace("{enabled}", "false"));
        assert!(
            !disabled.features.mcp_client.servers.is_empty(),
            "the fixture must still declare a server"
        );
        assert!(
            requirements(&disabled)
                .iter()
                .all(|r| !matches!(r, Requirement::McpCommand { .. }))
        );
    }

    #[test]
    fn an_llm_agent_without_a_config_block_needs_the_environment() {
        let config = config(
            r#"
            [agent]
            name = "Chat"
            [server]
            http_port = 8080
            [handler]
            type = "llm"
            "#,
        );
        assert!(requirements(&config).contains(&Requirement::LlmProvider(LlmSource::Environment)));
    }

    /// The settings are carried through so the check builds the same provider
    /// `a2a run` will, instead of inferring one from which variables are set.
    #[test]
    fn an_llm_block_is_carried_into_the_requirement() {
        let config = config(
            r#"
            [agent]
            name = "Chat"
            [server]
            http_port = 8080
            [handler]
            type = "llm"
            [llm]
            provider = "openai"
            api_key = "sk-configured"
            model = "gpt-5"
            "#,
        );
        let expected = LlmSettings {
            provider: "openai".into(),
            api_key: Some("sk-configured".into()),
            model: Some("gpt-5".into()),
            ..Default::default()
        };
        assert!(
            requirements(&config).contains(&Requirement::LlmProvider(LlmSource::Config(expected)))
        );
    }

    /// The silent-wrong case worth naming: an unknown handler falls back to
    /// echo, so a configured-looking agent behaves like a stub.
    #[test]
    fn an_unknown_handler_is_reported() {
        let config = config(
            r#"
            [agent]
            name = "Custom"
            [server]
            http_port = 8080
            [handler]
            type = "weather"
            "#,
        );
        assert!(
            requirements(&config).contains(&Requirement::UnknownHandler {
                name: "weather".into()
            })
        );
    }
}
