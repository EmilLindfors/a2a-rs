//! Helpers for declaring AP2 support in an A2A `AgentCard`.



use a2a_rs::{AgentCapabilities, AgentCard, AgentExtension};

use crate::types::{AP2_EXTENSION_URI, Ap2Role};

/// Create an [`AgentExtension`] declaring AP2 support with the given roles.
///
/// Agents that perform the `Merchant` role SHOULD set `required` to `true`.
pub fn ap2_extension(roles: Vec<Ap2Role>, required: bool) -> AgentExtension {
    let roles_json: Vec<serde_json::Value> = roles
        .iter()
        .map(|r| serde_json::Value::String(r.as_str().to_string()))
        .collect();

    let mut params_map = serde_json::Map::new();
    params_map.insert("roles".to_string(), serde_json::Value::Array(roles_json));
    let struct_val = serde_json::Value::Object(params_map);
    let params_struct = serde_json::from_value::<::buffa_types::google::protobuf::Struct>(struct_val)
        .unwrap_or_default();

    AgentExtension {
        uri: AP2_EXTENSION_URI.to_string(),
        description: "Agent Payments Protocol (AP2) v0.1".to_string(),
        required,
        params: ::buffa::MessageField::some(params_struct),
        ..Default::default()
    }
}

/// Check whether an [`AgentCard`] declares AP2 extension support.
pub fn supports_ap2(card: &AgentCard) -> bool {
    card.capabilities
        .as_option()
        .map(|caps| caps.extensions.iter().any(|e| e.uri == AP2_EXTENSION_URI))
        .unwrap_or(false)
}

/// Extract the declared AP2 roles from an [`AgentCard`].
///
/// Returns `None` if the card does not declare AP2 support.
pub fn get_ap2_roles(card: &AgentCard) -> Option<Vec<Ap2Role>> {
    let caps = card.capabilities.as_option()?;
    let ap2_ext = caps.extensions.iter().find(|e| e.uri == AP2_EXTENSION_URI)?;
    let params = ap2_ext.params.as_option()?;
    let json_val = serde_json::to_value(params).ok()?;
    let roles_val = json_val.get("roles")?;
    serde_json::from_value(roles_val.clone()).ok()
}

/// Check whether an [`AgentCard`] declares a specific AP2 role.
pub fn has_ap2_role(card: &AgentCard, role: &Ap2Role) -> bool {
    get_ap2_roles(card).is_some_and(|roles| roles.contains(role))
}

/// Convenience method: add an AP2 extension to existing [`AgentCapabilities`].
pub fn with_ap2(
    mut capabilities: AgentCapabilities,
    roles: Vec<Ap2Role>,
    required: bool,
) -> AgentCapabilities {
    let ext = ap2_extension(roles, required);
    capabilities.extensions.push(ext);
    capabilities
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merchant_card() -> AgentCard {
        AgentCard::builder()
            .name("Test Merchant".into())
            .description("A test merchant agent".into())
            .url("https://merchant.example.com".into())
            .version("1.0.0".into())
            .capabilities(with_ap2(
                AgentCapabilities::default(),
                vec![Ap2Role::Merchant],
                true,
            ))
            .skills(vec![])
            .default_input_modes(vec!["text".into()])
            .default_output_modes(vec!["text".into()])
            .build()
    }

    fn plain_card() -> AgentCard {
        AgentCard::builder()
            .name("Plain Agent".into())
            .description("No AP2".into())
            .url("https://example.com".into())
            .version("1.0.0".into())
            .capabilities(AgentCapabilities::default())
            .skills(vec![])
            .default_input_modes(vec!["text".into()])
            .default_output_modes(vec!["text".into()])
            .build()
    }

    #[test]
    fn supports_ap2_positive() {
        assert!(supports_ap2(&merchant_card()));
    }

    #[test]
    fn supports_ap2_negative() {
        assert!(!supports_ap2(&plain_card()));
    }

    #[test]
    fn get_roles() {
        let roles = get_ap2_roles(&merchant_card()).unwrap();
        assert_eq!(roles, vec![Ap2Role::Merchant]);
    }

    #[test]
    fn has_role() {
        let card = merchant_card();
        assert!(has_ap2_role(&card, &Ap2Role::Merchant));
        assert!(!has_ap2_role(&card, &Ap2Role::Shopper));
    }

    #[test]
    fn ap2_extension_serialization() {
        let ext = ap2_extension(vec![Ap2Role::Shopper, Ap2Role::CredentialsProvider], true);
        let json = serde_json::to_value(&ext).unwrap();
        assert_eq!(json["uri"], AP2_EXTENSION_URI);
        assert_eq!(json["required"], true);
        let roles = json["params"]["roles"].as_array().unwrap();
        assert_eq!(roles[0], "shopper");
        assert_eq!(roles[1], "credentials-provider");
    }
}
