//! Fleet configuration — a set of agents that run together (`a2a up`).
//!
//! Multi-agent otherwise means repeating `--config` once per agent on every
//! invocation, which is neither reviewable nor version-controllable. A fleet file
//! names the set once:
//!
//! ```toml
//! name = "Demo Fleet"
//!
//! [[agents]]
//! config = "weather.toml"
//!
//! [[agents]]
//! config = "orchestrator.toml"
//! ```
//!
//! Members are **paths to agent configs**, resolved relative to the fleet file so
//! `a2a up -f demo/fleet.toml` works from any directory. The fleet file does not
//! redefine any part of an agent — [`AgentConfig`] stays the single source of
//! truth, and a fleet is only a list of them plus the invariants that exist
//! *between* them (see [`fleet_conflicts`]).
//!
//! Parsing is pure and I/O-free apart from [`FleetConfig::from_file`], so the
//! resolution and conflict rules are unit-tested against the real parser.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[cfg(feature = "schema")]
use schemars::JsonSchema;

use crate::core::config::{AgentConfig, ConfigError};
use crate::registry::AgentId;

/// A set of agents run together by `a2a up`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FleetConfig {
    /// Optional label for the fleet, shown when it starts. Purely cosmetic — the
    /// agents' own names are what peers and the registry see.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The agents to run. At least one is required: an empty fleet is a mistake
    /// worth reporting, not a no-op process that exits silently.
    pub agents: Vec<FleetMember>,
}

/// One agent in a [`FleetConfig`].
///
/// A table rather than a bare path string so the member can grow options later
/// without breaking every fleet file — the same shape as `[[skills]]` and
/// `[[handler.llm.agents]]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FleetMember {
    /// Path to the agent's TOML config, relative to the fleet file's directory
    /// (absolute paths are used as given).
    pub config: String,
}

impl FleetConfig {
    /// Parse a fleet from TOML.
    ///
    /// Unlike [`AgentConfig`], no `${VAR}` expansion happens: a fleet file names
    /// config paths, and the agent configs it points at do their own expansion
    /// when they are loaded.
    pub fn from_toml(content: &str) -> Result<Self, ConfigError> {
        let fleet: FleetConfig = toml::from_str(content)?;
        fleet.validate()?;
        Ok(fleet)
    }

    /// Read and parse a fleet file.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_toml(&content)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.agents.is_empty() {
            return Err(ConfigError::ValidationError(
                "a fleet must list at least one agent ([[agents]] with a `config` path)"
                    .to_string(),
            ));
        }
        for member in &self.agents {
            if member.config.trim().is_empty() {
                return Err(ConfigError::ValidationError(
                    "a fleet member's `config` path cannot be empty".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Member config paths, resolved relative to `fleet_path` (the fleet file
    /// itself, not its directory).
    ///
    /// Relative-to-the-file is what makes a fleet portable: `a2a up -f
    /// demo/fleet.toml` from the repo root must find `demo/weather.toml`, not
    /// `./weather.toml`.
    pub fn config_paths(&self, fleet_path: &Path) -> Vec<PathBuf> {
        let base = fleet_path.parent().unwrap_or_else(|| Path::new(""));
        self.agents
            .iter()
            .map(|member| {
                let path = Path::new(&member.config);
                if path.is_absolute() || base.as_os_str().is_empty() {
                    path.to_path_buf()
                } else {
                    base.join(path)
                }
            })
            .collect()
    }
}

/// The `config` value to record in a fleet file for an agent config on disk —
/// the inverse of [`FleetConfig::config_paths`].
///
/// Relative to the fleet file's directory whenever the config sits under it, so
/// the pair stays portable and reviewable. Anything else is absolutized rather
/// than written as-is: a relative path that does not resolve against the fleet
/// file's directory would silently point somewhere else the moment the fleet is
/// run from another directory, which is the one property `config_paths` exists
/// to guarantee.
///
/// Both paths are taken as the caller sees them, so `cwd` is what a relative
/// path resolves against.
pub fn member_path(fleet_path: &Path, config_path: &Path, cwd: &Path) -> String {
    let base = fleet_path.parent().unwrap_or_else(|| Path::new(""));
    if let Ok(relative) = config_path.strip_prefix(base) {
        return relative.to_string_lossy().replace('\\', "/");
    }
    let absolute = if config_path.is_absolute() {
        config_path.to_path_buf()
    } else {
        cwd.join(config_path)
    };
    absolute.to_string_lossy().replace('\\', "/")
}

/// Render a `[[agents]]` block to append to a fleet file.
///
/// Text, not `toml::to_string`: re-serializing would drop the comments and
/// ordering a hand-written fleet file carries, and a scaffolding step that
/// reformats the user's file is a scaffolding step nobody runs twice.
pub fn member_block(config: &str) -> String {
    format!("\n[[agents]]\nconfig = {config:?}\n")
}

/// The header a scaffolded fleet file starts with, before its first member.
pub fn fleet_header(name: &str) -> String {
    format!(
        "# A fleet: the agents that make up one system, in one file.\n\
         #\n\
         #   a2a validate --fleet <this file>   # each member, plus the rules between them\n\
         #   a2a up -f <this file>              # run them all\n\
         #\n\
         # Member paths resolve against this file's directory, so the fleet runs\n\
         # from anywhere.\n\
         \n\
         name = {name:?}\n"
    )
}

/// A problem that exists only *between* fleet members, so no single
/// [`AgentConfig`] can detect it.
///
/// Both variants are silent-wrong failures at runtime rather than loud ones,
/// which is why they are worth a pre-flight check: a port clash surfaces as one
/// agent's bind error buried in the log of a process that otherwise came up, and
/// an id clash surfaces as delegation that reaches the wrong agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FleetConflict {
    /// Two or more members bind the same TCP port; all but one fail to bind.
    Port {
        /// The contested port.
        port: u16,
        /// Config paths claiming it, in fleet order.
        members: Vec<String>,
    },
    /// Two or more members' names slugify to the same [`AgentId`]. Registration
    /// upserts by id, so the last one wins and peers resolving by `agent_id` (or
    /// by a shared skill) silently reach only that one.
    AgentId {
        /// The contested registry id.
        id: String,
        /// Config paths claiming it, in fleet order.
        members: Vec<String>,
    },
}

impl std::fmt::Display for FleetConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FleetConflict::Port { port, members } => write!(
                f,
                "port {port} is claimed by {} — only the first to bind will start",
                members.join(", ")
            ),
            FleetConflict::AgentId { id, members } => write!(
                f,
                "agent id '{id}' is claimed by {} — registration overwrites, so peers \
                 resolving by skill or agent_id reach only the last one",
                members.join(", ")
            ),
        }
    }
}

/// Check the invariants that hold across a fleet, reporting every conflict
/// rather than stopping at the first.
///
/// Pure over already-loaded configs: the caller owns reading files (and
/// reporting per-file errors), so this stays unit-testable without a filesystem.
pub fn fleet_conflicts<'a, I>(members: I) -> Vec<FleetConflict>
where
    I: IntoIterator<Item = (&'a str, &'a AgentConfig)>,
{
    // BTreeMaps so the report is deterministic (by port, then by id) while each
    // group keeps fleet order.
    let mut by_port: BTreeMap<u16, Vec<String>> = BTreeMap::new();
    let mut by_id: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for (label, config) in members {
        // Port 0 means "no HTTP server" (MCP-only agents), so it never clashes.
        if config.server.http_port != 0 {
            by_port
                .entry(config.server.http_port)
                .or_default()
                .push(label.to_string());
        }
        by_id
            .entry(AgentId::from_name(&config.agent.name).to_string())
            .or_default()
            .push(label.to_string());
    }

    let ports = by_port
        .into_iter()
        .filter(|(_, members)| members.len() > 1)
        .map(|(port, members)| FleetConflict::Port { port, members });
    let ids = by_id
        .into_iter()
        .filter(|(_, members)| members.len() > 1)
        .map(|(id, members)| FleetConflict::AgentId { id, members });

    ports.chain(ids).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(name: &str, port: u16) -> AgentConfig {
        AgentConfig::from_toml(&format!(
            r#"
            [agent]
            name = "{name}"

            [server]
            http_port = {port}
            "#
        ))
        .expect("fixture config parses")
    }

    #[test]
    fn parses_a_fleet() {
        let fleet = FleetConfig::from_toml(
            r#"
            name = "Demo"

            [[agents]]
            config = "weather.toml"

            [[agents]]
            config = "orchestrator.toml"
            "#,
        )
        .unwrap();

        assert_eq!(fleet.name.as_deref(), Some("Demo"));
        assert_eq!(fleet.agents.len(), 2);
        assert_eq!(fleet.agents[0].config, "weather.toml");
    }

    /// Same contract as `AgentConfig`: a typo is an error, not a silent default.
    #[test]
    fn unknown_keys_are_rejected() {
        let err = FleetConfig::from_toml(
            r#"
            naem = "Typo"

            [[agents]]
            config = "a.toml"
            "#,
        )
        .expect_err("a mistyped key must not be ignored")
        .to_string();
        assert!(err.contains("naem"), "{err}");

        let err = FleetConfig::from_toml(
            r#"
            [[agents]]
            cnofig = "a.toml"
            "#,
        )
        .expect_err("a mistyped member key must not be ignored")
        .to_string();
        assert!(err.contains("cnofig"), "{err}");
    }

    #[test]
    fn an_empty_fleet_is_an_error() {
        assert!(FleetConfig::from_toml("agents = []").is_err());
        assert!(FleetConfig::from_toml(r#"name = "Nothing""#).is_err());
    }

    /// Portability: members resolve against the fleet file's directory, so a
    /// fleet can be run from anywhere.
    #[test]
    fn member_paths_resolve_relative_to_the_fleet_file() {
        let fleet = FleetConfig::from_toml(
            r#"
            [[agents]]
            config = "weather.toml"

            [[agents]]
            config = "nested/billing.toml"
            "#,
        )
        .unwrap();

        let paths = fleet.config_paths(Path::new("demo/fleet.toml"));
        assert_eq!(paths[0], PathBuf::from("demo").join("weather.toml"));
        assert_eq!(
            paths[1],
            PathBuf::from("demo").join("nested").join("billing.toml")
        );

        // A bare filename has no parent directory to join against.
        let paths = fleet.config_paths(Path::new("fleet.toml"));
        assert_eq!(paths[0], PathBuf::from("weather.toml"));
    }

    #[test]
    fn absolute_member_paths_are_left_alone() {
        let fleet = FleetConfig::from_toml(&format!(
            r#"
            [[agents]]
            config = {:?}
            "#,
            absolute("weather.toml").display()
        ))
        .unwrap();

        assert_eq!(
            fleet.config_paths(Path::new("demo/fleet.toml"))[0],
            absolute("weather.toml")
        );
    }

    fn absolute(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    /// The round trip that makes a scaffolded fleet portable: what `member_path`
    /// records has to be what `config_paths` resolves back to.
    #[test]
    fn a_recorded_member_resolves_back_to_the_config_it_named() {
        let cwd = Path::new("/work");
        for (fleet, config) in [
            ("fleet.toml", "weather.toml"),
            ("demo/fleet.toml", "demo/weather.toml"),
            ("demo/fleet.toml", "demo/nested/billing.toml"),
        ] {
            let member = member_path(Path::new(fleet), Path::new(config), cwd);
            let parsed = FleetConfig::from_toml(&format!(
                "{}{}",
                fleet_header("Test"),
                member_block(&member)
            ))
            .expect("a scaffolded fleet parses");
            assert_eq!(
                parsed.config_paths(Path::new(fleet)),
                [PathBuf::from(config)],
                "member {member:?} for fleet {fleet:?}"
            );
        }
    }

    /// A config that is not under the fleet file's directory cannot be recorded
    /// relative to it, and writing it as-is would resolve somewhere else the
    /// moment the fleet ran from another directory. Absolutize instead.
    #[test]
    fn a_config_outside_the_fleets_directory_is_recorded_absolutely() {
        let member = member_path(
            Path::new("demo/fleet.toml"),
            Path::new("elsewhere/weather.toml"),
            Path::new("/work"),
        );
        assert_eq!(member, "/work/elsewhere/weather.toml");
        assert!(Path::new(&member).is_absolute() || cfg!(windows));
    }

    #[test]
    fn a_scaffolded_fleet_header_is_a_valid_empty_fleet_once_a_member_is_added() {
        let toml = format!("{}{}", fleet_header("My Fleet"), member_block("a.toml"));
        let fleet = FleetConfig::from_toml(&toml).expect("parses");
        assert_eq!(fleet.name.as_deref(), Some("My Fleet"));
        assert_eq!(fleet.agents[0].config, "a.toml");
    }

    #[test]
    fn a_clean_fleet_has_no_conflicts() {
        let (a, b) = (agent("Weather", 8080), agent("Billing", 8081));
        assert!(fleet_conflicts([("a.toml", &a), ("b.toml", &b)]).is_empty());
    }

    #[test]
    fn duplicate_ports_are_reported_with_both_members() {
        let (a, b) = (agent("Weather", 8080), agent("Billing", 8080));
        let conflicts = fleet_conflicts([("a.toml", &a), ("b.toml", &b)]);
        assert_eq!(
            conflicts,
            [FleetConflict::Port {
                port: 8080,
                members: vec!["a.toml".into(), "b.toml".into()],
            }]
        );
        assert!(conflicts[0].to_string().contains("8080"));
    }

    /// Distinct names that slugify to the same id still collide in the registry,
    /// which is exactly the case a per-config check cannot see.
    #[test]
    fn colliding_agent_ids_are_reported() {
        let (a, b) = (agent("Weather Agent", 8080), agent("weather agent", 8081));
        let conflicts = fleet_conflicts([("a.toml", &a), ("b.toml", &b)]);
        assert_eq!(
            conflicts,
            [FleetConflict::AgentId {
                id: "weather-agent".into(),
                members: vec!["a.toml".into(), "b.toml".into()],
            }]
        );
    }

    /// Reporting stops at neither the first conflict nor the first kind — a
    /// pre-flight check that surfaced one problem per run would take as many
    /// runs as there are mistakes.
    #[test]
    fn every_conflict_is_reported_not_just_the_first() {
        let a = agent("Dup", 8080);
        let b = agent("Dup", 8080);
        let c = agent("Other", 8081);
        let d = agent("Third", 8081);
        let conflicts = fleet_conflicts([
            ("a.toml", &a),
            ("b.toml", &b),
            ("c.toml", &c),
            ("d.toml", &d),
        ]);
        assert_eq!(conflicts.len(), 3, "two port clashes and one id clash");
    }

    /// `http_port = 0` means "serve no HTTP" (MCP-only agents), so several
    /// members can share it without conflicting.
    #[test]
    fn port_zero_never_conflicts() {
        let a = AgentConfig::from_toml(
            r#"
            [agent]
            name = "Mcp One"
            [server]
            http_port = 0
            [features.mcp_server]
            enabled = true
            "#,
        )
        .unwrap();
        let b = AgentConfig::from_toml(
            r#"
            [agent]
            name = "Mcp Two"
            [server]
            http_port = 0
            [features.mcp_server]
            enabled = true
            "#,
        )
        .unwrap();
        assert!(fleet_conflicts([("a.toml", &a), ("b.toml", &b)]).is_empty());
    }
}
