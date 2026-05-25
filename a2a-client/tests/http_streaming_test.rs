use a2a_client::WebA2AClient;
use a2a_client::components::create_sse_stream;
use std::sync::Arc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_sse_stream_reconnection_logic() {
    // Start a mock server
    let mock_server = MockServer::start().await;

    // We mock the ConnectRPC endpoint to always return 500 Internal Server Error
    // This will cause `subscribe_to_task` to fail and trigger the retry logic.
    Mock::given(method("POST"))
        .and(path("/a2a.v1.AgentService/SubscribeToTask"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock_server)
        .await;

    let client = Arc::new(WebA2AClient::new_http(mock_server.uri()));

    // Create the SSE stream wrapper
    // We expect this to retry a few times and then eventually yield an error/close
    let sse = create_sse_stream(client.clone(), "test-task-1".to_string());

    use axum::response::IntoResponse;
    let response = sse.into_response();
    let body = response.into_body();

    // We only wait a short time to verify that multiple retries occurred,
    // rather than waiting for all 15 retries to exhaust (~2 minutes).
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let _bytes = axum::body::to_bytes(body, usize::MAX).await;
    })
    .await;

    // We can verify that the mock server received multiple requests (initial + retries)
    // In 3 seconds, with 500ms, 1s, 2s backoff, it should make at least 2 or 3 requests.
    let requests = mock_server.received_requests().await.unwrap();
    assert!(
        requests.len() > 1,
        "Should have received multiple requests due to retry logic"
    );
}
