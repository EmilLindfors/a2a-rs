# A2A Client for WebAssembly

A WebSocket client implementation for the A2A protocol, built with Leptos 8.0 for WebAssembly applications.

## Overview

This crate provides a WebAssembly-compatible client for the A2A protocol, enabling the development of web applications that can communicate with A2A servers. It's built using Leptos, a Rust framework for building reactive web applications.

## Features

- WebSocket-based communication with A2A servers
- Message streaming support for real-time updates
- Integration with Leptos for reactivity
- Built for WebAssembly environments
- Support for all common A2A operations:
  - Task creation and management
  - Message streaming
  - Task cancellation
  - Push notification configuration

## Prerequisites

- Rust and Cargo
- wasm-pack
- An A2A-compatible WebSocket server running (default: ws://localhost:8081)

## Building and Running

### Setting up the toolchain

If you haven't installed `wasm-pack` yet:

```bash
cargo install wasm-pack
```

### Building the WASM package

```bash
wasm-pack build --target web
```

### Running the application

You can use any static file server. For example, with `basic-http-server`:

```bash
cargo install basic-http-server
basic-http-server .
```

Or with Python's built-in HTTP server:

```bash
python -m http.server
```

Then open your browser at http://localhost:8000 (or whatever port your server is using).

## Structure

- `src/client/` - WebSocket client implementation for A2A
  - `client/mod.rs` - Client exports
  - `client/ws.rs` - WebSocket implementation
  - `client/error.rs` - Error types
- `src/components/` - Leptos components for the chat interface
  - `components/chat.rs` - Chat UI component
- `src/lib.rs` - Main application entry point
- `src/styles.css` - Styling for the application
- `index.html` - HTML template for the application
- `examples/` - Example applications
  - `simple_chat.rs` - Basic chat interface
  - `websocket_test.rs` - WebSocket connection tester

## Usage

### Basic Setup

```rust
use a2a_client::client::A2AClientImpl;
use a2a_rs::domain::Message;
use std::{cell::RefCell, rc::Rc};

// Create a client
let client = Rc::new(RefCell::new(
    A2AClientImpl::new("ws://your-a2a-server.com".to_string())
));

// Optionally add authentication
let client_with_auth = Rc::new(RefCell::new(
    A2AClientImpl::with_auth(
        "ws://your-a2a-server.com".to_string(),
        "your-auth-token".to_string()
    )
));
```

### Sending Messages

```rust
use a2a_rs::domain::Message;
use wasm_bindgen_futures::spawn_local;
use uuid::Uuid;

// Create a task ID
let task_id = format!("task-{}", Uuid::new_v4());

// Create an A2A message
let message = Message::user_text("Hello, A2A!");

// Send the message
spawn_local(async move {
    let client_ref = client.borrow();
    match client_ref.send_task_message(&task_id, &message, None, None).await {
        Ok(task) => {
            // Handle successful response
            console_log::log!("Message sent successfully");
        }
        Err(e) => {
            // Handle error
            console_log::error!("Error sending message: {}", e);
        }
    }
});
```

### Streaming Updates

```rust
use futures::StreamExt;
use a2a_rs::port::client::StreamItem;

spawn_local(async move {
    let client_ref = client.borrow();
    match client_ref.subscribe_to_task(&task_id, &message, None, None).await {
        Ok(mut stream) => {
            while let Some(result) = stream.next().await {
                match result {
                    Ok(StreamItem::StatusUpdate(update)) => {
                        // Handle status updates
                        if update.final_ {
                            // Final update received
                            break;
                        }
                    }
                    Ok(StreamItem::ArtifactUpdate(update)) => {
                        // Handle artifact updates
                    }
                    Err(e) => {
                        // Handle errors
                        break;
                    }
                }
            }
        }
        Err(e) => {
            // Handle subscription errors
        }
    }
});
```

### Components

The main component `Chat` provides a simple chat interface that connects to an A2A-compatible WebSocket server. It allows users to send messages and receive streaming responses.

## Configuration

By default, the client connects to `ws://localhost:8081`. To change this, modify the WebSocket URL when creating your client:

```rust
let client = A2AClientImpl::new("ws://your-server-url".to_string());
```

## Architecture

The client implementation consists of several key components:

- `WasmWebSocketClient`: Core WebSocket client implementation for WASM environments
- `A2AClientImpl`: A user-friendly wrapper providing A2A-specific functionality
- `WebSocketHandle`: Low-level WebSocket connection management
- `MessageBroadcaster`: Distributes WebSocket messages to multiple subscribers