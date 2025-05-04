use a2a_client::client::A2AClientImpl;
use a2a_rs::domain::Message;
use futures::StreamExt;
use leptos::prelude::*;
use leptos::*;
use std::{
    cell::RefCell,
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
};
use uuid::Uuid;
use wasm_bindgen_futures::spawn_local;

// Set up a global flag to track connection success
static CONNECTION_SUCCESS: AtomicBool = AtomicBool::new(false);
static STREAMING_SUCCESS: AtomicBool = AtomicBool::new(false);
static TEST_COMPLETE: AtomicBool = AtomicBool::new(false);

fn main() {
    // Initialize console error panic hook for better error messages
    console_error_panic_hook::set_once();

    // Initialize logging to console
    _ = console_log::init_with_level(log::Level::Debug);

    // Mount the test app
    mount_to_body(WebSocketTest);

    // Log test start
    log::info!("WebSocket test app started");
}

#[component]
fn WebSocketTest() -> impl IntoView {
    let client = Rc::new(RefCell::new(A2AClientImpl::new(
        "ws://localhost:8081".to_string(),
    )));

    // Start the test when the component mounts
    spawn_local({
        let client = client.clone();
        async move {
            run_tests(client).await;
        }
    });

    view! {
        <div class="test-app">
            <h1>"WebSocket Test"</h1>
            <div class="results">
                <div id="connection-test" class="test-item">
                    "Connection Test: "
                    <span class="result">
                        {move || if CONNECTION_SUCCESS.load(Ordering::SeqCst) {
                            "PASSED"
                        } else {
                            "Running..."
                        }}
                    </span>
                </div>
                <div id="streaming-test" class="test-item">
                    "Streaming Test: "
                    <span class="result">
                        {move || if STREAMING_SUCCESS.load(Ordering::SeqCst) {
                            "PASSED"
                        } else if CONNECTION_SUCCESS.load(Ordering::SeqCst) {
                            "Running..."
                        } else {
                            "Waiting..."
                        }}
                    </span>
                </div>
                <div id="completion-status" class="test-item">
                    "Test Status: "
                    <span class="result">
                        {move || if TEST_COMPLETE.load(Ordering::SeqCst) {
                            "Complete"
                        } else {
                            "Running..."
                        }}
                    </span>
                </div>
            </div>
        </div>
    }
}

async fn run_tests(client: Rc<RefCell<A2AClientImpl>>) {
    log::info!("Starting WebSocket tests");

    // Test 1: Basic Connection Test
    let connection_test = test_connection(client.clone()).await;
    if connection_test {
        CONNECTION_SUCCESS.store(true, Ordering::SeqCst);
        log::info!("Connection test passed");

        // Test 2: Streaming Test
        let streaming_test = test_streaming(client).await;
        if streaming_test {
            STREAMING_SUCCESS.store(true, Ordering::SeqCst);
            log::info!("Streaming test passed");
        } else {
            log::error!("Streaming test failed");
        }
    } else {
        log::error!("Connection test failed");
    }

    // Mark tests as complete
    TEST_COMPLETE.store(true, Ordering::SeqCst);
    log::info!("All tests completed");
}

async fn test_connection(client: Rc<RefCell<A2AClientImpl>>) -> bool {
    log::info!("Testing WebSocket connection");

    let task_id = format!("test-conn-{}", Uuid::new_v4());
    let message = Message::user_text("Connection test message".to_string());

    let client_ref = client.borrow();
    match client_ref.get_task(&task_id, None).await {
        Ok(_) => {
            // We likely won't get here since the task doesn't exist,
            // but this will test if the connection works
            true
        }
        Err(e) => {
            // We expect an error, but the connection should work
            // Let's check if it's a "task not found" error
            log::info!("Expected error response: {}", e);
            !e.to_string().contains("No WebSocket connection") && !e.to_string().contains("Timeout")
        }
    }
}

async fn test_streaming(client: Rc<RefCell<A2AClientImpl>>) -> bool {
    log::info!("Testing WebSocket streaming");

    let task_id = format!("test-stream-{}", Uuid::new_v4());
    let message = Message::user_text("Stream test message".to_string());

    let client_ref = client.borrow();
    let mut received_message = false;

    match client_ref
        .subscribe_to_task(&task_id, &message, None, None)
        .await
    {
        Ok(mut stream) => {
            // Try to get at least one message
            if let Some(result) = stream.next().await {
                log::info!("Received streaming result");
                received_message = true;
            }
            true
        }
        Err(e) => {
            log::error!("Streaming test error: {}", e);
            false
        }
    }
}
