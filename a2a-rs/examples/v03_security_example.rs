//! Example demonstrating A2A Protocol v1.0.0 security features
//!
//! This example shows how to use the new v1.0.0 security features including:
//! - SecurityScheme types (API Key, HTTP Bearer, OAuth2, OpenID Connect, mTLS)
//! - Security requirements at agent and skill levels
//! - Agent card signatures
//! - Extended card support

use a2a_rs::domain::{
    AgentCapabilities, AgentCard, AgentCardSignature, AgentSkill, AuthorizationCodeOAuthFlow,
    OAuthFlows, SecurityScheme, security_scheme,
};
use std::collections::HashMap;

fn main() {
    println!("=== A2A Protocol v1.0.0 Security Features Example ===\n");

    // Example 1: Agent with multiple security schemes
    println!("1. Creating an agent with multiple security schemes:\n");

    let mut security_schemes = HashMap::new();

    // Add HTTP Bearer authentication
    security_schemes.insert(
        "bearer".to_string(),
        SecurityScheme::http(
            "bearer".to_string(),
            Some("JWT".to_string()),
            Some("JWT Bearer token authentication".to_string()),
        ),
    );

    // Add API Key authentication
    security_schemes.insert(
        "api_key".to_string(),
        SecurityScheme::api_key(
            "X-API-Key".to_string(),
            "header".to_string(),
            Some("API Key in header".to_string()),
        ),
    );

    // Add mTLS authentication (new in v1.0.0)
    security_schemes.insert(
        "mtls".to_string(),
        SecurityScheme::mutual_tls(
            Some("Client certificate authentication".to_string()),
        ),
    );

    // Add OAuth2 with metadata URL (new in v1.0.0)
    let mut scopes = HashMap::new();
    scopes.insert("read:data".to_string(), "Read access to data".to_string());
    scopes.insert("write:data".to_string(), "Write access to data".to_string());

    let oauth_flows = OAuthFlows::authorization_code(AuthorizationCodeOAuthFlow {
        authorization_url: "https://auth.example.com/oauth/authorize".to_string(),
        token_url: "https://auth.example.com/oauth/token".to_string(),
        refresh_url: "https://auth.example.com/oauth/refresh".to_string(),
        scopes,
        ..Default::default()
    });

    security_schemes.insert(
        "oauth2".to_string(),
        SecurityScheme::oauth2(
            oauth_flows,
            Some("OAuth 2.0 authentication".to_string()),
            Some("https://auth.example.com/.well-known/oauth-authorization-server".to_string()),
        ),
    );

    println!("Security schemes defined:");
    for (name, scheme) in &security_schemes {
        let scheme_type = match scheme.scheme.as_ref() {
            Some(security_scheme::Scheme::ApiKeySecurityScheme(_)) => "API Key",
            Some(security_scheme::Scheme::HttpAuthSecurityScheme(_)) => "HTTP",
            Some(security_scheme::Scheme::Oauth2SecurityScheme(_)) => "OAuth2",
            Some(security_scheme::Scheme::OpenIdConnectSecurityScheme(_)) => "OpenID Connect",
            Some(security_scheme::Scheme::MtlsSecurityScheme(_)) => "Mutual TLS",
            None => "None",
        };
        println!("  - {}: {}", name, scheme_type);
    }

    // Example 2: Agent-level security requirements
    println!("\n2. Defining agent-level security requirements:\n");

    // Require either bearer token OR mTLS
    let mut bearer_req = HashMap::new();
    bearer_req.insert("bearer".to_string(), Vec::new());

    let mut mtls_req = HashMap::new();
    mtls_req.insert("mtls".to_string(), Vec::new());

    let agent_security = vec![bearer_req, mtls_req];

    println!("Agent security: Requires EITHER bearer token OR mTLS");

    // Example 3: Skill-level security requirements
    println!("\n3. Creating a skill with specific security requirements:\n");

    let mut oauth2_req = HashMap::new();
    oauth2_req.insert(
        "oauth2".to_string(),
        vec!["read:data".to_string(), "write:data".to_string()],
    );

    let secure_skill = AgentSkill::new(
        "data-processor".to_string(),
        "Data Processor".to_string(),
        "Process sensitive data with OAuth2 authentication".to_string(),
        vec!["data".to_string(), "secure".to_string()],
    )
    .with_input_modes(vec!["text".to_string(), "json".to_string()])
    .with_output_modes(vec!["text".to_string(), "json".to_string()])
    .with_security(vec![oauth2_req]); // Skill requires OAuth2 with specific scopes

    println!("Skill '{}' requires OAuth2 with scopes:", secure_skill.name);
    for req in &secure_skill.security_requirements {
        for (scheme, scopes) in &req.schemes {
            println!("  - {}: {:?}", scheme, scopes.list);
        }
    }

    // Example 4: Agent card with signature
    println!("\n4. Creating an agent card with digital signature:\n");

    let mut signature_header = HashMap::new();
    signature_header.insert(
        "alg".to_string(),
        serde_json::json!("RS256"), // RSA with SHA-256
    );
    signature_header.insert("kid".to_string(), serde_json::json!("key-2024-01"));

    let signature_header_val = serde_json::to_value(signature_header).unwrap();
    let signature_header_struct = serde_json::from_value(signature_header_val).unwrap();

    let signature = AgentCardSignature::new(
        "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9".to_string(),
        "dGhpc19pc19hX3NpZ25hdHVyZV9leGFtcGxl".to_string(), // Base64-encoded
        Some(signature_header_struct),
    );

    println!("Agent card signature:");
    println!("  - Algorithm: RS256");
    println!("  - Key ID: key-2024-01");
    println!("  - Protected: {}", signature.protected);

    // Example 5: Complete agent card with v1.0.0 features
    println!("\n5. Building a complete secure agent card:\n");

    let agent_card = AgentCard::builder()
        .name("Secure Data Agent".to_string())
        .description("An agent demonstrating v1.0.0 security features".to_string())
        .url("https://agent.example.com".to_string())
        .version("1.0.0".to_string())
        .documentation_url("https://docs.example.com/agent".to_string())
        .capabilities(AgentCapabilities {
            streaming: Some(true),
            push_notifications: Some(false),
            extended_agent_card: Some(false),
            ..Default::default()
        })
        .default_input_modes(vec!["text".to_string(), "json".to_string()])
        .default_output_modes(vec!["text".to_string(), "json".to_string()])
        .skills(vec![secure_skill])
        // v1.0.0 fields
        .security_schemes(security_schemes)
        .security(agent_security)
        .signatures(vec![signature])
        .supports_extended_agent_card(true)
        .build();

    println!("Agent Card created:");
    println!("  - Name: {}", agent_card.name);
    println!("  - URL: {}", agent_card.url());
    println!(
        "  - Security Schemes: {}",
        agent_card.security_schemes.len()
    );
    println!(
        "  - Agent Security Requirements: {}",
        agent_card.security_requirements.len()
    );
    println!("  - Skills: {}", agent_card.skills.len());
    println!("  - Signed: {}", !agent_card.signatures.is_empty());
    println!(
        "  - Extended Card Support: {}",
        agent_card.supports_extended_agent_card()
    );

    // Example 6: Serializing to JSON
    println!("\n6. Serializing agent card to JSON:\n");

    match serde_json::to_string_pretty(&agent_card) {
        Ok(json) => {
            println!("{}", json);
        }
        Err(e) => {
            eprintln!("Error serializing agent card: {}", e);
        }
    }

    println!("\n=== Example completed successfully! ===");
}
