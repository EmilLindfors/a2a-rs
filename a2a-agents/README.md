# A2A Agents - Modern Framework Examples

This crate provides example agent implementations using the modern A2A protocol framework in Rust. Currently featuring a reimbursement agent that demonstrates best practices for building production-ready agents.

## Overview

This implementation showcases the modern A2A framework architecture:

1. **Hexagonal Architecture**: Clean separation between domain logic and adapters
2. **Framework Integration**: Uses `DefaultBusinessHandler` and `InMemoryTaskStorage`
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

Run the modern reimbursement agent server:

```bash
# Run both HTTP and WebSocket servers (default)
cargo run --bin reimbursement_server

# Run HTTP server only on custom port
cargo run --bin reimbursement_server -- --mode http --port 8080

# Run WebSocket server only
cargo run --bin reimbursement_server -- --mode websocket

# Custom host and port
cargo run --bin reimbursement_server -- --host 0.0.0.0 --port 9000 --mode both
```

### Available Endpoints

**HTTP Server (default port 10002):**
- Agent Card: `http://localhost:10002/agent-card`
- Skills List: `http://localhost:10002/skills`
- A2A Protocol: `http://localhost:10002/` (JSON-RPC)

**WebSocket Server (default port 10003):**
- WebSocket Endpoint: `ws://localhost:10003/`

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