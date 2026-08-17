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

use a2a_llm::LlmSettings;

use crate::core::config::AgentConfig;
use crate::core::config::ContextMode;
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
    /// The agent binds every interface, so the address it publishes on its card
    /// is a guess: right on the machine it runs on, wrong from anywhere else.
    ///
    /// Only raised when it *is* a guess. A configured `advertised_url`, or a
    /// bind address a peer can dial, answers the question and says nothing.
    AdvertisedGuess {
        /// What the agent will publish.
        url: String,
    },
    /// The `llm` handler needs a working provider. Without one the agent runs
    /// and answers with a deterministic fallback instead of a model; with a
    /// broken one (`a2a run` refuses to start) it does not run at all.
    ///
    /// Carries where the provider comes from so the check can build it the way
    /// `a2a run` does, rather than guessing from the presence of a variable.
    LlmProvider(LlmSource),
    /// The agent carries a conversation between turns
    /// (`[handler.llm.context] mode`), so what it remembers is only as durable
    /// as the storage it is kept in.
    ///
    /// Both halves are read from the config, because the pairing is the whole
    /// question: `mode = "context"` over in-memory storage runs perfectly and
    /// forgets every conversation the moment the process restarts — which the
    /// control plane does on purpose.
    ConversationStore {
        /// What the agent is configured to remember.
        mode: ContextMode,
        /// Whether `[server.storage]` outlives the process.
        durable: bool,
    },
    /// The agent gives the model `remember` and `forget`
    /// (`[handler.llm.context] remember`), so what it was told to keep is only
    /// as durable as the storage behind it.
    ///
    /// Reported apart from [`ConversationStore`](Self::ConversationStore)
    /// because the two are configured apart: an agent that carries no
    /// transcript can still keep a state bag, and the failure is worse there —
    /// a fact the model was explicitly asked to remember is the thing a user
    /// notices going missing.
    StateStore {
        /// Whether `[server.storage]` outlives the process.
        durable: bool,
    },
    /// A handler name no built-in provides, and no image to supply one. `a2a
    /// run` refuses to start such an agent, so this is a config that cannot run
    /// anywhere as written.
    UnknownHandler {
        /// The configured `handler.type`.
        name: String,
    },
    /// The agent runs from its own image (`[runtime] image`), so its handler,
    /// its model provider and its MCP commands are all inside that image.
    ///
    /// It replaces those requirements rather than adding to them: this machine
    /// cannot see into an image, and reporting what *this* build would have
    /// needed produces confident answers about a binary that will not run.
    ContainerImage {
        /// The configured image reference.
        image: String,
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

    // Only when the agent cannot name its own address. A peer dials what is on
    // the card, so this failure never shows up on the agent that caused it.
    let advertised = config.advertised();
    if config.server.http_port != 0 && advertised.is_guess() {
        requirements.push(Requirement::AdvertisedGuess {
            url: advertised.into_url(),
        });
    }

    let mcp_server = &config.features.mcp_server;
    if mcp_server.enabled && mcp_server.http.enabled {
        requirements.push(Requirement::McpHttpBind {
            host: mcp_server.http.host.clone(),
            port: mcp_server.http.port,
        });
    }

    // An agent with its own image stops here. What it needs beyond the ports it
    // claims — a handler, a model key, an MCP command — lives inside that image,
    // and the checks below would be answering for a binary that is not the one
    // that will run.
    if let Some(image) = config.image() {
        requirements.push(Requirement::ContainerImage {
            image: image.to_string(),
        });
        return requirements;
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
            }));

            // Only the llm handler reads `[handler.llm.context]`, and only a
            // mode that reads history cares where the conversation is kept.
            let context = config.handler.llm.as_ref().map(|llm| &llm.context);
            let mode = context.map(|context| context.mode).unwrap_or_default();
            let durable = config.server.storage.is_durable();
            if mode.reads_history() {
                requirements.push(Requirement::ConversationStore { mode, durable });
            }
            if context.is_some_and(|context| context.remember) {
                requirements.push(Requirement::StateStore { durable });
            }
        }
        HandlerType::Custom(name) => requirements.push(Requirement::UnknownHandler { name }),
        // The reimbursement agent is a sample behind an opt-in feature, so
        // whether it exists depends on how this binary was built. Unbuilt, the
        // runner refuses to start — a config that names a handler this binary
        // does not have is precisely what this check is for.
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

    /// A dialable bind address answers for itself, so there is nothing to say.
    #[test]
    fn an_agent_that_can_name_its_address_is_not_warned_about_it() {
        let config = config(
            r#"
            [agent]
            name = "Echo"
            [server]
            host = "127.0.0.1"
            http_port = 8080
            "#,
        );
        assert!(
            requirements(&config)
                .iter()
                .all(|r| !matches!(r, Requirement::AdvertisedGuess { .. }))
        );
    }

    /// Binding every interface publishes an address the agent cannot know, and
    /// the failure lands on whichever peer dials it.
    #[test]
    fn a_wildcard_bind_is_reported_as_a_guessed_address() {
        let config = config(
            r#"
            [agent]
            name = "Echo"
            [server]
            host = "0.0.0.0"
            http_port = 8080
            "#,
        );
        assert!(
            requirements(&config).contains(&Requirement::AdvertisedGuess {
                url: "http://localhost:8080".into()
            })
        );
    }

    /// Saying what to advertise settles it, whatever is bound — this is the
    /// path `ContainerRuntime` takes for every agent it publishes.
    #[test]
    fn an_advertised_url_settles_the_question() {
        let config = config(
            r#"
            [agent]
            name = "Echo"
            [server]
            host = "0.0.0.0"
            http_port = 8080
            advertised_url = "http://agents.internal:8080"
            "#,
        );
        assert!(
            requirements(&config)
                .iter()
                .all(|r| !matches!(r, Requirement::AdvertisedGuess { .. }))
        );
    }

    /// An agent that remembers nothing has no conversation to lose, so pairing
    /// it with in-memory storage is not worth a word.
    #[test]
    fn an_agent_that_remembers_nothing_reports_no_store() {
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
        assert!(
            requirements(&config)
                .iter()
                .all(|r| !matches!(r, Requirement::ConversationStore { .. }))
        );
    }

    /// The pairing that runs fine and forgets everything on restart. Both
    /// halves come from the config, since neither is a fact about the host.
    #[test]
    fn carrying_a_conversation_reports_where_it_is_kept() {
        let toml = r#"
            [agent]
            name = "Chat"
            [server]
            http_port = 8080
            [server.storage]
            {storage}
            [handler]
            type = "llm"
            [handler.llm.context]
            mode = "context"
        "#;

        let memory = config(&toml.replace("{storage}", r#"type = "inmemory""#));
        assert!(
            requirements(&memory).contains(&Requirement::ConversationStore {
                mode: ContextMode::Context,
                durable: false,
            })
        );

        let sqlx =
            config(&toml.replace("{storage}", "type = \"sqlx\"\nurl = \"sqlite://agent.db\""));
        assert!(
            requirements(&sqlx).contains(&Requirement::ConversationStore {
                mode: ContextMode::Context,
                durable: true,
            })
        );
    }

    /// The state bag is configured apart from the transcript, so it is reported
    /// apart: `remember = true` under the default `mode = "none"` still needs
    /// somewhere durable to put what it was told.
    #[test]
    fn keeping_state_reports_where_it_is_kept_whatever_the_mode_is() {
        let config = config(
            r#"
            [agent]
            name = "Chat"
            [server]
            http_port = 8080
            [server.storage]
            type = "inmemory"
            [handler]
            type = "llm"
            [handler.llm.context]
            remember = true
            "#,
        );
        let requirements = requirements(&config);
        assert!(requirements.contains(&Requirement::StateStore { durable: false }));
        // And nothing about a conversation, which this agent does not carry.
        assert!(
            requirements
                .iter()
                .all(|r| !matches!(r, Requirement::ConversationStore { .. }))
        );
    }

    /// `[handler.llm.context]` is read by the `llm` handler and nobody else, so
    /// a block left behind on an echo agent is not a finding about storage.
    #[test]
    fn context_settings_on_a_non_llm_handler_are_not_a_store_requirement() {
        let config = config(
            r#"
            [agent]
            name = "Echo"
            [server]
            http_port = 8080
            [handler]
            type = "echo"
            [handler.llm.context]
            mode = "context"
            "#,
        );
        assert!(
            requirements(&config)
                .iter()
                .all(|r| !matches!(r, Requirement::ConversationStore { .. }))
        );
    }

    /// A handler name this binary does not have, and no image to supply one:
    /// nothing can run this config.
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

    /// The same handler name, with an image behind it, is the supported way to
    /// ship a handler no TOML can express — so it must stop being a problem.
    #[test]
    fn an_image_answers_for_the_handler_it_carries() {
        let config = config(
            r#"
            [agent]
            name = "Custom"
            [server]
            host = "127.0.0.1"
            http_port = 8080
            [handler]
            type = "weather"
            [runtime]
            image = "ghcr.io/acme/weather:2.0"
            "#,
        );
        assert_eq!(
            requirements(&config),
            [
                Requirement::HttpBind {
                    host: "127.0.0.1".into(),
                    port: 8080
                },
                Requirement::ContainerImage {
                    image: "ghcr.io/acme/weather:2.0".into()
                }
            ]
        );
    }

    /// What is inside the image is not this machine's to check. Reporting a
    /// missing MCP command or model key would be a confident answer about a
    /// binary that is not the one that will run.
    #[test]
    fn an_image_agents_needs_are_not_probed_here() {
        let config = config(
            r#"
            [agent]
            name = "Custom"
            [server]
            http_port = 8080
            [handler]
            type = "llm"
            [runtime]
            image = "ghcr.io/acme/weather:2.0"

            [features.mcp_client]
            enabled = true

            [[features.mcp_client.servers]]
            name = "filesystem"
            command = "definitely-not-installed"
            "#,
        );
        let requirements = requirements(&config);
        assert!(
            !requirements.iter().any(|r| matches!(
                r,
                Requirement::McpCommand { .. } | Requirement::LlmProvider(_)
            )),
            "got: {requirements:?}"
        );
    }
}
