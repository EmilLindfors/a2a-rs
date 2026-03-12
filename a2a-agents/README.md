# A2A Agents

Example agent implementations for the A2A Protocol with production-ready patterns and **declarative configuration**.

## 🚀 Quick Start (New Builder API)

Create a production-ready agent in just **~30 lines of code** instead of ~300!

### 1. Define your agent (`agent.toml`)

```toml
[agent]
name = "My Agent"
description = "A helpful agent"

[[skills]]
id = "my_skill"
name = "My Skill"
description = "What this skill does"
```

### 2. Implement your handler

```rust
use a2a_rs::port::AsyncMessageHandler;
use async_trait::async_trait;

#[derive(Clone)]
struct MyHandler;

#[async_trait]
impl AsyncMessageHandler for MyHandler {
    async fn process_message(/* ... */) -> Result<Task, A2AError> {
        // Your business logic here
    }
}
```

### 3. Build and run!

```rust
use a2a_agents::AgentBuilder;
use a2a_rs::InMemoryTaskStorage;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    AgentBuilder::from_file("agent.toml")?
        .with_handler(MyHandler)
        .with_storage(InMemoryTaskStorage::new())
        .build()?
        .run()
        .await?;
    Ok(())
}
```

**That's it!** The framework handles servers, agent cards, authentication, and more.

📚 **[See complete Builder API documentation →](BUILDER_API.md)**

## Overview

This crate provides two approaches for building agents:

### ✨ New: Declarative Builder API (Recommended)

- **90% less boilerplate** - ~30 lines vs ~300 lines
- **TOML configuration** - Define agents declaratively
- **Environment-aware** - Built-in env var interpolation
- **Type-safe** - Configuration validated at load time
- **Production-ready** - Batteries included

**Examples:**
- [`examples/minimal_agent.rs`](examples/minimal_agent.rs) - Echo agent (~50 lines)
- [`examples/reimbursement_builder.rs`](examples/reimbursement_builder.rs) - Full-featured agent

### Traditional Approach

The original hexagonal architecture approach with manual wiring:

1. **Hexagonal Architecture**: Clean separation between domain logic and adapters
2. **Framework Integration**: Uses `DefaultRequestProcessor` and storage backends
3. **Protocol Compliance**: Full A2A protocol support with HTTP and WebSocket transports
4. **Modern Patterns**: Async/await, builder patterns, and structured error handling

## Architecture

### ReimbursementMessageHandler

The core business logic implementing `AsyncMessageHandler`:

- Processes reimbursement requests using the A2A message format
- Generates interactive forms for expense submissions
- Validates and approves reimbursement requests
- Returns structured responses with proper task states

### ModernReimbursementServer

The server implementation using framework components:

- Integrates with `DefaultBusinessHandler` for request processing
- Uses `InMemoryTaskStorage` for task persistence
- Configures `SimpleAgentInfo` with agent capabilities
- Supports both HTTP and WebSocket transports

## Usage

### Quick Start - Unified Demo (Recommended)

Run the complete demo with both agent backend and web frontend in a single command:

```bash
# Run everything (agent backend + web UI)
cargo run --bin reimbursement_demo

# Open your browser to http://localhost:3000
```

This starts:
- **Agent Backend** on `http://localhost:8080` (HTTP) and `ws://localhost:8081` (WebSocket)
- **Web Frontend** on `http://localhost:3000`

The frontend automatically connects to the local agent and provides an interactive interface for submitting expenses and viewing tasks.

### Advanced Usage

Run specific components:

```bash
# Run only the agent backend (HTTP + WebSocket)
cargo run --bin reimbursement_demo -- --mode agent

# Run only the web frontend (point it to an existing agent)
AGENT_HTTP_URL=http://localhost:8080 cargo run --bin reimbursement_demo -- --mode frontend

# Customize ports
cargo run --bin reimbursement_demo -- \
  --agent-http-port 8080 \
  --agent-ws-port 8081 \
  --frontend-port 3000

# Run only HTTP transport for agent
cargo run --bin reimbursement_demo -- --transport http

# Enable WebSocket support in frontend
cargo run --bin reimbursement_demo -- --frontend-use-websocket
```

### Available Endpoints

**Agent Backend:**
- HTTP API: `http://localhost:8080` (JSON-RPC)
- WebSocket: `ws://localhost:8081`
- Agent Card: `http://localhost:8080/agent-card`

**Web Frontend:**
- Main UI: `http://localhost:3000`
- Task List: `http://localhost:3000/tasks`
- Expense Form: `http://localhost:3000/expense/new`

## Example Conversation

Here's an example conversation with the reimbursement agent:

1. User: "Can you reimburse me $50 for the team lunch yesterday?"

2. Agent: *Returns a form*
   ```json
   {
     "type": "form",
     "form": {
       "type": "object",
       "properties": {
         "date": {
           "type": "string",
           "format": "date",
           "description": "Date of expense",
           "title": "Date"
         },
         "amount": {
           "type": "string",
           "format": "number",
           "description": "Amount of expense",
           "title": "Amount"
         },
         "purpose": {
           "type": "string",
           "description": "Purpose of expense",
           "title": "Purpose"
         },
         "request_id": {
           "type": "string",
           "description": "Request id",
           "title": "Request ID"
         }
       },
       "required": ["request_id", "date", "amount", "purpose"]
     },
     "form_data": {
       "request_id": "request_id_1234567",
       "date": "<transaction date>",
       "amount": "50",
       "purpose": " the team lunch yesterday"
     }
   }
   ```

3. User: *Submits the filled form*
   ```json
   {
     "request_id": "request_id_1234567",
     "date": "2023-10-15",
     "amount": "50",
     "purpose": "team lunch with product team"
   }
   ```

4. Agent: "Your reimbursement request has been approved. Request ID: request_id_1234567"

## Current Limitations

This example implementation demonstrates the framework architecture but has simplified business logic:

- **Message Processing**: Basic pattern matching instead of LLM integration
- **Storage**: In-memory storage (framework supports SQLx for production)
- **Authentication**: Not implemented (framework supports Bearer/OAuth2)
- **Form Processing**: Simple JSON forms without complex validation

## Future Enhancements

See [TODO.md](./TODO.md) for the comprehensive modernization roadmap including:

1. **Phase 2**: Production features (SQLx storage, authentication)
2. **Phase 3**: AI/LLM integration for natural language processing
3. **Phase 4**: Additional agent examples (document analysis, research assistant)
4. **Phase 5**: Comprehensive testing and documentation
5. **Phase 6**: Docker support and production deployment

## Framework Features Demonstrated

- ✅ **AsyncMessageHandler** trait implementation
- ✅ **DefaultBusinessHandler** integration  
- ✅ **InMemoryTaskStorage** for task persistence
- ✅ **SimpleAgentInfo** for agent metadata
- ✅ **HTTP and WebSocket** transport support
- ✅ **Structured error handling** with A2AError
- ✅ **Modern async/await** patterns
- ✅ **Builder patterns** for complex objects