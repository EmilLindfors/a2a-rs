//! Example demonstrating A2A Protocol v0.3.0 security features
//!
//! This example shows how to use the new v0.3.0 security features including:
//! - SecurityScheme types (API Key, HTTP Bearer, OAuth2, OpenID Connect, mTLS)
//! - Security requirements at agent and skill levels
//! - Agent card signatures
//! - Extended card support

use a2a_rs::domain::{
    AgentCapabilities, AgentCard, AgentCardSignature, AgentSkill, AuthorizationCodeOAuthFlow,
    OAuthFlows, SecurityScheme,
};
use std::collections::HashMap;

fn main() {
    println!("=== A2A Protocol v0.3.0 Security Features Example ===\n");

    // Example 1: Agent with multiple security schemes
    println!("1. Creating an agent with multiple security schemes:\n");

    let mut security_schemes = HashMap::new();

    // Add HTTP Bearer authentication
    security_schemes.insert(
        "bearer".to_string(),
        SecurityScheme::Http {
            scheme: "bearer".to_string(),
            bearer_format: Some("JWT".to_string()),
            description: Some("JWT Bearer token authentication".to_string()),
        },
    );

    // Add API Key authentication
    security_schemes.insert(
        "api_key".to_string(),
        SecurityScheme::ApiKey {
            location: "header".to_string(),
            name: "X-API-Key".to_string(),
            description: Some("API Key in header".to_string()),
        },
    );

    // Add mTLS authentication (new in v0.3.0)
    security_schemes.insert(
        "mtls".to_string(),
        SecurityScheme::MutualTls {
            description: Some("Client certificate authentication".to_string()),
        },
    );

    // Add OAuth2 with metadata URL (new in v0.3.0)
    let mut scopes = HashMap::new();
    scopes.insert("read:data".to_string(), "Read access to data".to_string());
    scopes.insert("write:data".to_string(), "Write access to data".to_string());

    let oauth_flows = OAuthFlows {
        authorization_code: Some(AuthorizationCodeOAuthFlow {
            authorization_url: "https://auth.example.com/oauth/authorize".to_string(),
            token_url: "https://auth.example.com/oauth/token".to_string(),
            refresh_url: Some("https://auth.example.com/oauth/refresh".to_string()),
            scopes,
        }),
        client_credentials: None,
        implicit: None,
        password: None,
    };

    security_schemes.insert(
        "oauth2".to_string(),
        SecurityScheme::OAuth2 {
            flows: Box::new(oauth_flows),
            description: Some("OAuth 2.0 authentication".to_string()),
            // RFC 8414 metadata URL (new in v0.3.0)
            metadata_url: Some(
                "https://auth.example.com/.well-known/oauth-authorization-server".to_string(),
            ),
        },
    );

    println!("Security schemes defined:");
    for (name, scheme) in &security_schemes {
        let scheme_type = match scheme {
            SecurityScheme::ApiKey { .. } => "API Key",
            SecurityScheme::Http { .. } => "HTTP",
            SecurityScheme::OAuth2 { .. } => "OAuth2",
            SecurityScheme::OpenIdConnect { .. } => "OpenID Connect",
            SecurityScheme::MutualTls { .. } => "Mutual TLS",
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
    if let Some(security) = &secure_skill.security {
        for req in security {
            for (scheme, scopes) in req {
                println!("  - {}: {:?}", scheme, scopes);
            }
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

    let signature = AgentCardSignature {
        protected: "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9".to_string(),
        signature: "dGhpc19pc19hX3NpZ25hdHVyZV9leGFtcGxl".to_string(), // Base64-encoded
        header: Some(signature_header),
    };

    println!("Agent card signature:");
    println!("  - Algorithm: RS256");
    println!("  - Key ID: key-2024-01");
    println!("  - Protected: {}", signature.protected);

    // Example 5: Complete agent card with v0.3.0 features
    println!("\n5. Building a complete secure agent card:\n");

    let agent_card = AgentCard::builder()
        .name("Secure Data Agent".to_string())
        .description("An agent demonstrating v0.3.0 security features".to_string())
        .url("https://agent.example.com".to_string())
        .version("1.0.0".to_string())
        .maybe_documentation_url(Some("https://docs.example.com/agent".to_string()))
        .capabilities(AgentCapabilities {
            streaming: true,
            push_notifications: false,
            state_transition_history: true,
            extensions: None,
        })
        .default_input_modes(vec!["text".to_string(), "json".to_string()])
        .default_output_modes(vec!["text".to_string(), "json".to_string()])
        .skills(vec![secure_skill])
        // v0.3.0 fields
        .maybe_security_schemes(Some(security_schemes))
        .maybe_security(Some(agent_security))
        .maybe_signatures(Some(vec![signature]))
        .maybe_supports_authenticated_extended_card(Some(true))
        .build();

    println!("Agent Card created:");
    println!("  - Name: {}", agent_card.name);
    println!("  - URL: {}", agent_card.url);
    println!(
        "  - Security Schemes: {}",
        agent_card.security_schemes.as_ref().unwrap().len()
    );
    println!(
        "  - Agent Security Requirements: {}",
        agent_card.security.as_ref().unwrap().len()
    );
    println!("  - Skills: {}", agent_card.skills.len());
    println!("  - Signed: {}", agent_card.signatures.is_some());
    println!(
        "  - Extended Card Support: {}",
        agent_card
            .supports_authenticated_extended_card
            .unwrap_or(false)
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
