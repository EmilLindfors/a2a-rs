//! Integration tests for agent/getAuthenticatedExtendedCard endpoint (v1.0.0)

#![cfg(all(feature = "http-client", feature = "http-server"))]

mod common;

use a2a_rs::{
    adapter::{
        DefaultRequestProcessor, HttpClient, HttpServer, InMemoryTaskStorage, SimpleAgentInfo,
    },
    domain::A2AError,
};
use common::TestBusinessHandler;
use std::time::Duration;
use tokio::sync::oneshot;

async fn setup_server(port: u16, supports_authenticated_card: bool) -> oneshot::Sender<()> {
    let storage = InMemoryTaskStorage::new();
    let handler = TestBusinessHandler::with_storage(storage);

    // Create a single agent info instance to use for both processor and server
    let mut agent_info = SimpleAgentInfo::new(
        "Authenticated Card Test Agent".to_string(),
        format!("http://localhost:{}", port),
    )
    .with_description("Agent for testing authenticated extended card".to_string());

    if supports_authenticated_card {
        agent_info = agent_info.with_authenticated_extended_card();
    }

    // Clone the agent info for the processor
    let processor = DefaultRequestProcessor::with_handler(handler, agent_info.clone());

    let server = HttpServer::new(processor, agent_info, format!("127.0.0.1:{}", port));

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        tokio::select! {
            _ = server.start() => {},
            _ = shutdown_rx => {}
        }
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    shutdown_tx
}

#[tokio::test]
async fn test_get_authenticated_extended_card_not_configured() {
    let port = 9100;
    let shutdown_tx = setup_server(port, false).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    let response = client.get_extended_agent_card(None).await;

    // Should return error -32007
    assert!(response.is_err(), "Should have error response");
    if let Err(A2AError::JsonRpc { code, message, .. }) = response {
        assert_eq!(
            code, -32007,
            "Should return error code -32007 (AUTHENTICATED_EXTENDED_CARD_NOT_CONFIGURED)"
        );
        assert!(
            message.contains("not configured")
                || message.contains("not supported")
                || message.contains("not available"),
            "Error message should indicate card not configured: {}",
            message
        );
    } else {
        panic!("Expected JsonRpc error, got {:?}", response);
    }

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_get_authenticated_extended_card_success() {
    let port = 9101;
    let shutdown_tx = setup_server(port, true).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    let response = client.get_extended_agent_card(None).await;

    assert!(response.is_ok(), "Should not have error response");
    let card = response.unwrap();

    // Verify it's a valid agent card
    assert_eq!(card.name, "Authenticated Card Test Agent");
    assert!(!card.description.is_empty());
    assert_eq!(card.protocol_version(), "1.0");

    // Verify this is the authenticated version (may have additional info)
    // The authenticated card should support this capability
    assert!(
        card.capabilities.extended_agent_card.unwrap_or(false),
        "Authenticated card should indicate support"
    );

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_authenticated_card_vs_regular_card() {
    let port = 9102;
    let shutdown_tx = setup_server(port, true).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Get regular card via HTTP endpoint
    let http_client = reqwest::Client::new();
    let regular_card_response = http_client
        .get(format!("http://localhost:{}/agent-card", port))
        .send()
        .await
        .expect("Failed to fetch regular agent card");

    let regular_card: a2a_rs::domain::AgentCard = regular_card_response
        .json()
        .await
        .expect("Failed to parse regular agent card");

    // Get authenticated extended card
    let auth_card = client
        .get_extended_agent_card(None)
        .await
        .expect("Failed to get authenticated extended card");

    // Both should have same basic info
    assert_eq!(regular_card.name, auth_card.name);
    assert_eq!(regular_card.url(), auth_card.url());
    assert_eq!(
        regular_card.protocol_version(),
        auth_card.protocol_version()
    );

    // Both should indicate support for authenticated extended card
    assert!(
        regular_card
            .capabilities
            .extended_agent_card
            .unwrap_or(false)
    );
    assert!(auth_card.capabilities.extended_agent_card.unwrap_or(false));

    shutdown_tx.send(()).ok();
}

#[tokio::test]
async fn test_authenticated_card_error_structure() {
    let port = 9103;
    let shutdown_tx = setup_server(port, false).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Try to get authenticated card when not configured
    let response = client.get_extended_agent_card(None).await;

    assert!(response.is_err());
    let error = response.unwrap_err();

    // Verify error structure matches JSON-RPC spec
    if let A2AError::JsonRpc { code, message, .. } = error {
        assert_eq!(code, -32007);
        assert!(!message.is_empty());
    } else {
        panic!("Expected JsonRpc error, got {:?}", error);
    }

    shutdown_tx.send(()).ok();
}

#[test]
fn test_authenticated_extended_card_error_code() {
    // Test that the error enum produces correct error code
    let error = A2AError::AuthenticatedExtendedCardNotConfigured;
    let jsonrpc_error = error.to_jsonrpc_error();

    assert_eq!(jsonrpc_error["code"], -32007);
    assert_eq!(
        jsonrpc_error["message"],
        "Authenticated Extended Card is not configured"
    );
}

#[tokio::test]
async fn test_authenticated_card_with_extensions() {
    let port = 9104;
    let shutdown_tx = setup_server(port, true).await;

    let client = HttpClient::new(format!("http://localhost:{}", port));

    // Get authenticated card
    let card = client
        .get_extended_agent_card(None)
        .await
        .expect("Failed to get card");

    // Card should have v1.0.0 fields
    assert_eq!(card.protocol_version(), "1.0");
    assert_eq!(card.preferred_transport(), "JSONRPC");

    // Should be able to have extensions (even if empty)
    // This verifies the authenticated card includes all v1.0.0 fields
    let capabilities = &card.capabilities;
    // Extensions field should be present in capabilities
    // (may be None or Some(vec![]))
    let _ = &capabilities.extensions;

    shutdown_tx.send(()).ok();
}
