# Agent Builder API Guide

The Agent Builder API provides a declarative, configuration-driven approach to building A2A agents with minimal boilerplate.

## Overview

Instead of manually wiring together handlers, storage, servers, and agent metadata, you can now define your agent in a TOML configuration file and use the builder API to construct it with just a few lines of code.

**Before (traditional approach):** ~300 lines of boilerplate
**After (builder API):** ~30 lines of code

## Quick Start

### 1. Create an agent configuration file

Create `my_agent.toml`:

```toml
[agent]
name = "My Agent"
description = "A simple agent"

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
    async fn process_message<'a>(
        &self,
        task_id: &'a str,
        message: &'a Message,
        session_id: Option<&'a str>,
    ) -> Result<Task, A2AError> {
        // Your business logic here
        todo!()
    }

    async fn validate_message<'a>(&self, message: &'a Message) -> Result<(), A2AError> {
        // Validation logic here
        Ok(())
    }
}
```

### 3. Build and run

```rust
use a2a_agents::AgentBuilder;
use a2a_rs::InMemoryTaskStorage;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    AgentBuilder::from_file("my_agent.toml")?
        .with_handler(MyHandler)
        .with_storage(InMemoryTaskStorage::new())
        .build()?
        .run()
        .await?;

    Ok(())
}
```

That's it! The framework handles:
- Agent card generation
- Server setup (HTTP and/or WebSocket)
- Storage wiring
- Authentication
- Skill registration
- Feature configuration

## Configuration Reference

### Complete Example

```toml
[agent]
name = "Reimbursement Agent"
description = "Handles employee reimbursement requests"
version = "1.0.0"
documentation_url = "https://example.com/docs"

[agent.provider]
name = "Example Corp"
url = "https://example.com"

[server]
host = "127.0.0.1"
http_port = 8080       # Set to 0 to disable HTTP
ws_port = 8081         # Set to 0 to disable WebSocket

[server.storage]
type = "sqlx"
url = "${DATABASE_URL}"  # Environment variable interpolation
max_connections = 10
enable_logging = false

# Alternative: In-memory storage
# [server.storage]
# type = "inmemory"

[server.auth]
type = "bearer"
tokens = ["${AUTH_TOKEN}"]
format = "JWT"

# Alternative: No authentication
# [server.auth]
# type = "none"

# Alternative: API key authentication
# [server.auth]
# type = "apikey"
# keys = ["key1", "key2"]
# location = "header"  # or "query" or "cookie"
# name = "X-API-Key"

[[skills]]
id = "process_reimbursement"
name = "Process Reimbursement"
description = "Handles reimbursement workflows"
keywords = ["reimbursement", "expense", "finance"]
examples = [
    "Reimburse my $50 lunch",
    "Submit travel expenses"
]
input_formats = ["text", "data", "file"]
output_formats = ["text", "data"]

[features]
streaming = true
push_notifications = true
state_history = true
authenticated_card = false
```

### Configuration Sections

#### `[agent]` - Required

Agent metadata and identity:

- `name` (required): Human-readable agent name
- `description` (optional): Brief description of what the agent does
- `version` (optional): Semantic version of the agent
- `documentation_url` (optional): Link to agent documentation

#### `[agent.provider]` - Optional

Information about the organization providing the agent:

- `name`: Provider name
- `url`: Provider website URL

#### `[server]` - Optional (defaults provided)

Server configuration:

- `host` (default: `127.0.0.1`): Host to bind to
- `http_port` (default: `8080`): HTTP server port (0 to disable)
- `ws_port` (default: `8081`): WebSocket server port (0 to disable)

#### `[server.storage]` - Optional (defaults to in-memory)

Storage backend configuration:

**In-memory storage:**
```toml
[server.storage]
type = "inmemory"
```

**SQLx storage:**
```toml
[server.storage]
type = "sqlx"
url = "sqlite:data.db"  # or postgres://... or mysql://...
max_connections = 10
enable_logging = false
```

#### `[server.auth]` - Optional (defaults to none)

Authentication configuration:

**No authentication:**
```toml
[server.auth]
type = "none"
```

**Bearer token:**
```toml
[server.auth]
type = "bearer"
tokens = ["token1", "token2"]
format = "JWT"  # Optional
```

**API key:**
```toml
[server.auth]
type = "apikey"
keys = ["key1", "key2"]
location = "header"  # or "query" or "cookie"
name = "X-API-Key"
```

#### `[[skills]]` - Optional (can have multiple)

Skills exposed by the agent:

- `id` (required): Unique skill identifier
- `name` (required): Human-readable skill name
- `description` (optional): What the skill does
- `keywords` (optional): Keywords for discovery
- `examples` (optional): Example queries
- `input_formats` (optional, default: `["text", "data"]`): Supported input formats
- `output_formats` (optional, default: `["text", "data"]`): Supported output formats

#### `[features]` - Optional (defaults shown)

Features to enable:

```toml
[features]
streaming = true              # Enable streaming updates
push_notifications = true     # Enable push notifications
state_history = true          # Enable state transition history
authenticated_card = false    # Require auth for extended card
```

## Environment Variable Interpolation

Configuration values can reference environment variables using `${VAR_NAME}` syntax:

```toml
[server.storage]
type = "sqlx"
url = "${DATABASE_URL}"

[server.auth]
type = "bearer"
tokens = ["${AUTH_TOKEN}"]
```

## Builder API

### Loading Configuration

```rust
// From file
let builder = AgentBuilder::from_file("agent.toml")?;

// From string
let builder = AgentBuilder::from_toml(toml_string)?;

// Programmatically
let config = AgentConfig { /* ... */ };
let builder = AgentBuilder::new(config);
```

### Adding Components

```rust
builder
    .with_handler(MyHandler)      // Set message handler
    .with_storage(MyStorage)      // Set storage backend
    .with_config(|config| {       // Modify configuration
        config.server.http_port = 9000;
    })
```

### Building and Running

```rust
// Build the runtime
let runtime = builder.build()?;

// Run all configured servers
runtime.run().await?;

// Or run specific servers
runtime.start_http().await?;
runtime.start_websocket().await?;
runtime.start_all().await?;
```

## Examples

### Minimal Echo Agent

See `examples/minimal_agent.rs` for a complete minimal example (< 50 lines).

### Full-Featured Reimbursement Agent

See `examples/reimbursement_builder.rs` for a comprehensive example with:
- SQLx storage
- AI integration
- Custom business logic
- Configuration overrides

## Migration Guide

### From Traditional Approach

**Before:**
```rust
// Manual setup (~300 lines)
let storage = SqlxTaskStorage::new("sqlite:db").await?;
let handler = MyHandler::new(storage.clone());
let agent_info = SimpleAgentInfo::new("Agent", "http://localhost:8080")
    .with_description("...")
    .with_streaming()
    .with_push_notifications()
    .add_skill(...);
let processor = DefaultRequestProcessor::new(
    handler,
    storage.clone(),
    storage.clone(),
    agent_info.clone()
);
let server = HttpServer::new(processor, agent_info, "127.0.0.1:8080");
server.start().await?;
```

**After:**
```rust
// Declarative setup (~10 lines)
AgentBuilder::from_file("agent.toml")?
    .with_handler(MyHandler::new())
    .with_storage(storage)
    .build()?
    .run()
    .await?;
```

### Benefits

- **90% less boilerplate** - Focus on business logic, not wiring
- **Declarative configuration** - Change behavior without recompiling
- **Environment-aware** - Easy config management across environments
- **Type-safe** - Configuration validated at load time
- **Clear separation** - Config, handlers, and runtime are distinct
- **Progressive disclosure** - Simple cases are simple, complex cases possible

## Advanced Usage

### Custom Storage with Migrations

```rust
let migrations = &[
    include_str!("../migrations/001_init.sql"),
    include_str!("../migrations/002_add_fields.sql"),
];

let storage = SqlxTaskStorage::with_migrations(&db_url, migrations).await?;

AgentBuilder::from_file("agent.toml")?
    .with_storage(storage)
    .with_handler(handler)
    .build()?
    .run()
    .await?;
```

### Runtime Configuration Override

```rust
AgentBuilder::from_file("agent.toml")?
    .with_config(|config| {
        // Override from environment at runtime
        if let Ok(port) = env::var("PORT") {
            config.server.http_port = port.parse().unwrap();
        }

        // Conditionally enable features
        if cfg!(debug_assertions) {
            config.server.auth = AuthConfig::None;
        }
    })
    .with_handler(handler)
    .with_storage(storage)
    .build()?
    .run()
    .await?;
```

### Multiple Agents in One Process

```rust
let agent1 = AgentBuilder::from_file("agent1.toml")?
    .with_config(|c| c.server.http_port = 8080)
    .with_handler(Handler1)
    .with_storage(storage1)
    .build()?;

let agent2 = AgentBuilder::from_file("agent2.toml")?
    .with_config(|c| c.server.http_port = 8081)
    .with_handler(Handler2)
    .with_storage(storage2)
    .build()?;

// Run both concurrently
tokio::try_join!(
    agent1.start_http(),
    agent2.start_http()
)?;
```

## Best Practices

1. **Use TOML for static configuration** - Agent metadata, skills, default ports
2. **Use environment variables for secrets** - Database URLs, auth tokens
3. **Override at runtime when needed** - Port numbers, debug settings
4. **Keep handlers focused** - Only business logic, no infrastructure
5. **Validate early** - Configuration is validated at load time
6. **Test with in-memory** - Use `type = "inmemory"` for tests
7. **Version your config** - Track `agent.version` for compatibility

## Troubleshooting

### Configuration not found

```
Error: Failed to read config file: No such file or directory
```

Ensure the path to your TOML file is correct. Use absolute paths or paths relative to the working directory.

### Environment variable not set

```
Error: Environment variable not found: DATABASE_URL
```

Set the required environment variable:
```bash
export DATABASE_URL="sqlite:data.db"
```

Or use a `.env` file with `dotenvy::dotenv()`.

### Validation error

```
Error: Invalid configuration: Agent name cannot be empty
```

Check that all required fields are set in your TOML file.

## Next Steps

- See `examples/minimal_agent.rs` for a simple echo agent
- See `examples/reimbursement_builder.rs` for a full-featured example
- Read the [A2A Protocol specification](../../spec/README.md)
- Explore [handler patterns](./HANDLER_PATTERNS.md) for common use cases
