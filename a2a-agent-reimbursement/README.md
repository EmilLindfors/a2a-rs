# A2A Reimbursement Agent

An intelligent expense reimbursement agent built on the [A2A Protocol v0.3.0](https://github.com/yourusername/a2a-rs).

## Features

- 💬 **Interactive Conversation Flow** - Natural language interface for collecting reimbursement information
- 🤖 **AI-Powered Responses** - Uses OpenAI-compatible APIs for intelligent responses
- ✅ **Data Validation** - Structured validation of expense data
- 💾 **Persistent Storage** - SQLite storage for task history
- 🔌 **Multi-Transport** - HTTP and WebSocket support
- 🎯 **Plugin Architecture** - Implements `AgentPlugin` trait for automatic skill discovery

## Installation

```toml
[dependencies]
a2a-agent-reimbursement = "0.1"
```

## Quick Start

```rust
use a2a_agent_reimbursement::ReimbursementHandler;
use a2a_rs::InMemoryTaskStorage;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create storage and handler
    let storage = InMemoryTaskStorage::default();
    let handler = ReimbursementHandler::new(storage);

    // The handler implements AgentPlugin and AsyncMessageHandler
    // Wire it up to your server using a2a-rs server components

    Ok(())
}
```

## Agent Skills

The reimbursement agent provides these skills (auto-discovered via `AgentPlugin`):

### 1. Submit Reimbursement Request
**Keywords:** reimburse, reimbursement, expense, receipt, refund, claim, submit

Guides users through the complete reimbursement submission process:
- Collects expense details (amount, category, date, description)
- Handles receipt uploads
- Validates data against business rules
- Generates structured reimbursement requests

### 2. Track Request Status
**Keywords:** status, track, check, where is, progress

Allows users to check the status of their reimbursement requests.

### 3. Get Help
**Keywords:** help, how, what, info, information

Provides guidance on the reimbursement process.

## Configuration

Create a configuration file `reimbursement.toml`:

```toml
[agent]
name = "Reimbursement Agent"
description = "Helps users submit expense reimbursement requests"
base_url = "http://localhost:8080"

[server]
http_port = 8080
ws_port = 8081

[server.storage]
type = "sqlx"
url = "${DATABASE_URL}"
```

## Environment Variables

- `DATABASE_URL` - Database connection string (e.g., `sqlite:reimbursements.db`)
- `OPENAI_API_KEY` - API key for AI-powered responses
- `OPENAI_API_BASE` - Base URL for OpenAI-compatible API (optional)

## Running the Agent

### As a Library

```rust
use a2a_agent_reimbursement::ReimbursementHandler;
use a2a_agents::core::AgentBuilder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Using the builder pattern from a2a-agents
    let storage = a2a_rs::InMemoryTaskStorage::default();
    let handler = ReimbursementHandler::new(storage.clone());

    // Set up and run your server with the handler
    // See examples for complete server setup

    Ok(())
}
```

### As a Binary

```bash
cargo run --bin reimbursement-agent -- --port 8080
```

## Examples

See the `examples/` directory for complete working examples:

- `simple.rs` - Basic agent setup with in-memory storage

## Architecture

This agent follows the modular A2A architecture:

```
a2a-agent-reimbursement
├── Core Dependencies
│   ├── a2a-rs (Protocol implementation)
│   ├── a2a-agents (Framework & builder)
│   └── a2a-agents-common (Shared utilities)
│
├── Agent Implementation
│   ├── handler.rs (Message processing)
│   ├── plugin.rs (AgentPlugin trait)
│   ├── types.rs (Domain types)
│   ├── ai_client.rs (AI integration)
│   └── config.rs (Configuration)
│
└── Distribution
    ├── Library (for integration)
    └── Binary (standalone)
```

## Domain Types

### ReimbursementRequest

Represents an expense reimbursement request with:
- Amount and currency
- Expense category (Travel, Meals, Equipment, etc.)
- Date of expense
- Description
- Receipt attachments
- Justification

### ProcessingStatus

Tracks the lifecycle of a reimbursement:
- Submitted
- UnderReview
- Approved
- Rejected
- Paid

### ExpenseCategory

Supported expense categories:
- Travel
- Meals
- Accommodation
- Equipment
- Software
- Training
- Other

## Features

- `default` - Includes SQLx storage support
- `sqlx` - Enable SQLx-based persistent storage
- `auth` - Enable authentication features

## Development

### Building

```bash
cargo build
```

### Testing

```bash
cargo test
```

### Running Examples

```bash
cargo run --example simple
```

## License

MIT

## Contributing

Contributions welcome! This agent demonstrates the modular A2A architecture pattern.

## See Also

- [A2A Protocol Specification](https://github.com/yourusername/a2a-rs/tree/master/spec)
- [A2A Framework](https://github.com/yourusername/a2a-rs/tree/master/a2a-agents)
- [A2A Common Utilities](https://github.com/yourusername/a2a-rs/tree/master/a2a-agents-common)
