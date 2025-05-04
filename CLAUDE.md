# Claude Code Guide for a2a-rs

This file contains information for Claude Code when working with this repository.

## Project Overview

a2a-rs is a Rust implementation of the Agent-to-Agent (A2A) Protocol, built with hexagonal architecture principles. The project enables agent-to-agent communication with support for multiple transport protocols (HTTP and WebSocket).

## Repository Structure

- **a2a-rs**: Core implementation with domain models, ports, and adapters
  - `domain/`: Core business entities and rules (agent, message, task)
  - `port/`: Interface definitions (client, server)
  - `adapter/`: Concrete implementations (http, websocket)
  - `application/`: Service coordination layer (json-rpc)

- **a2a-client**: Client implementations and utilities
  - HTTP and WebSocket client implementations
  - Example applications (WASM-based)

- **a2a-agents**: Server-side agent implementations
  - Reimbursement agent example

## Key Architectural Concepts

1. **Hexagonal Architecture**: Clear separation between:
   - Domain logic (core business rules)
   - Ports (interfaces)
   - Adapters (technology-specific implementations)

2. **Transport Protocols**:
   - HTTP for request/response
   - WebSocket for streaming and real-time updates

3. **Task Management**:
   - Task creation, tracking, and state transitions
   - History recording with configurable depth

4. **Agent Capabilities**:
   - Skill definition and discovery
   - Role-based messaging

## Important Commands

### Build and Test

```bash
# Build all crates
cargo build

# Run tests
cargo test

# Run specific tests
cargo test --package a2a-rs
```

### Run Examples

```bash
# HTTP examples
cargo run --example http_server
cargo run --example http_client

# WebSocket examples
cargo run --example websocket_server
cargo run --example websocket_client
```

## Server Examples Notes

### HTTP Server
The HTTP server example starts a server on port 8080:
```bash
cargo run --example http_server
```

This provides the following endpoints:
- Agent card: http://127.0.0.1:8080/agent-card
- Skills list: http://127.0.0.1:8080/skills
- Specific skill: http://127.0.0.1:8080/skills/echo

### HTTP Client
The HTTP client example connects to the HTTP server and demonstrates how to:
```bash
cargo run --example http_client
```

This client requires the HTTP server to be running and:
- Creates a task with a generated UUID
- Sends a message with text and file content to the server
- Retrieves task history with different limits
- Cancels the task

### WebSocket Server
The WebSocket server example starts a server on port 8081:
```bash
cargo run --example websocket_server
```

This provides a WebSocket endpoint at: ws://127.0.0.1:8081

### WebSocket Client
The WebSocket client example connects to the WebSocket server:
```bash
cargo run --example websocket_client
```

This client:
- Establishes a WebSocket connection to the server (ws://localhost:8081)
- Creates a task with a UUID and sends a message with text and data parts
- Subscribes to real-time task updates using streaming
- Processes different types of updates:
  - Task updates: Initial task state
  - Status updates: Changes in task status and content
  - Artifact updates: Additional data or results
- Shows how to handle the final update marker
- Falls back to task cancellation if needed

## WASM Client Examples Notes

The `a2a-client` examples are WASM-based applications using the Leptos framework. To run them:

1. First start the WebSocket server:
   ```bash
   cargo run --example websocket_server
   ```

2. The WASM client examples require:
   - Trunk tool for building: `cargo binstall trunk`
   - wasm32-unknown-unknown target: `rustup target add wasm32-unknown-unknown`
   - Additional dependencies for compiling with ring crypt:
     - clang toolchain: `apt-get install clang` (on Debian/Ubuntu)

3. Known issues with client examples:
   - The `simple_chat.rs` example is missing handling for `StreamItem::Task(_)` variant
   - WebSocket client testing requires proper build environment for WASM
   - The examples expect a WebSocket server running at `ws://localhost:8081`
   - Ring crypt dependency causes issues when building for wasm32-unknown-unknown

4. WASM Client Implementation Details:
   - Uses web_sys for WebSocket communication
   - Implements proper message broadcasting for multiple subscribers
   - Handles connection establishment and reconnection
   - Provides streaming support through Futures streams

5. For client development:
   ```bash
   cd a2a-client
   trunk serve --example simple_chat
   ```

## Common Patterns

1. **Error Handling**: Uses `thiserror` for structured error types
2. **Async Programming**: Uses `tokio` runtime and `async-trait`
3. **Serialization**: Uses `serde` for JSON handling
4. **Builder Pattern**: For complex object construction
5. **Trait-based Interfaces**: For port definitions

## Testing Approach

- Unit tests alongside implementation files
- Integration tests in the `tests/` directory
- WebSocket-specific tests for streaming functionality
- Push notification tests for delivery reliability

## Recent Changes

- Improved WebSocket client streaming support
- Migration from OpenSSL to rustls
- Enhanced task history and state tracking
- Push notification reliability improvements

## Documentation

- README.md: Project overview
- IMPLEMENTATION_GUIDE.md: Details on implementing the A2A protocol
- A2A_COMPLIANCE_ASSESSMENT.md: Protocol compliance information
- Code documentation via rustdoc

## Code Style Guidelines

- Follow Rust standard naming conventions
- Use descriptive error types with thiserror
- Implement proper error handling with Result types
- Use async/await for asynchronous operations
- Utilize builder patterns for complex object construction
- Leverage trait-based abstractions for interfaces