//! A served agent card has to carry every field the spec marks REQUIRED.
//!
//! ProtoJSON omits an empty repeated field and an empty string, so a REQUIRED
//! field left at its default vanishes from the wire rather than arriving as
//! `[]` or `""` — and a client that models it as non-optional refuses the whole
//! card. That is not a discovery-only failure: a client fetches the card before
//! every call, so it fails everything.
//!
//! Found with the official `a2aproject/a2acli` against `examples/jsonrpc_server`
//! on 2026-08-21: `SimpleAgentInfo::add_skill` hardcoded empty tags, the served
//! skill therefore had no `tags` key, and every subcommand died with
//! "error decoding response body" at the card fetch.

#![cfg(feature = "http-server")]

use a2a_rs::adapter::SimpleAgentInfo;
use a2a_rs::services::server::AgentInfoProvider;

/// Fields `a2a.proto` marks `(google.api.field_behavior) = REQUIRED`.
const CARD_REQUIRED: &[&str] = &[
    "name",
    "description",
    "version",
    "supportedInterfaces",
    "capabilities",
    "defaultInputModes",
    "defaultOutputModes",
    "skills",
];
const SKILL_REQUIRED: &[&str] = &["id", "name", "description", "tags"];
const INTERFACE_REQUIRED: &[&str] = &["url", "protocolBinding", "protocolVersion"];

/// Deliberately the plainest card a caller can build — `new` plus one skill,
/// nothing optional. Anything a bare card omits is what every user of the
/// builder ships by default.
#[tokio::test]
async fn a_plainly_built_card_carries_every_required_field() {
    let info = SimpleAgentInfo::new("Card Agent".to_string(), "http://127.0.0.1:1".to_string())
        .add_skill(
            "echo".to_string(),
            "Echo".to_string(),
            Some("Echoes input".to_string()),
            vec!["echo".to_string()],
        );

    let card = info.get_agent_card().await.expect("card");
    let json = serde_json::to_value(&card).expect("the card serializes");

    for field in CARD_REQUIRED {
        assert!(
            json.get(field).is_some(),
            "card is missing REQUIRED `{field}`: {json:#}"
        );
    }

    let skill = &json["skills"][0];
    for field in SKILL_REQUIRED {
        assert!(
            skill.get(field).is_some(),
            "skill is missing REQUIRED `{field}`: {skill:#}"
        );
    }

    let interface = &json["supportedInterfaces"][0];
    for field in INTERFACE_REQUIRED {
        assert!(
            interface.get(field).is_some(),
            "interface is missing REQUIRED `{field}`: {interface:#}"
        );
    }
}
