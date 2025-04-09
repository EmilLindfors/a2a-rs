# Reimbursement Agent

This is an example implementation of a reimbursement agent using the A2A protocol in Rust. The agent handles reimbursement requests for employees, processing expense forms and providing status updates.

## Overview

This implementation is a port of the Google Python reimbursement agent to Rust using our A2A protocol framework. It demonstrates how to:

1. Create a custom agent that processes natural language requests
2. Implement form handling and validation
3. Support streaming responses
4. Bridge the agent with the A2A protocol

## Components

### ReimbursementAgent

The core agent implementation that:

- Creates request forms for reimbursements
- Validates submitted forms
- Processes reimbursement requests
- Provides status updates

### AgentTaskManager

A task manager that bridges the A2A protocol with the reimbursement agent:

- Handles task creation and management
- Processes user messages
- Manages streaming updates
- Handles cancellation

### A2AServer

A server implementation that:

- Exposes the agent via HTTP and/or WebSocket
- Provides agent metadata via an agent card
- Handles JSON-RPC requests according to the A2A protocol

## Usage

To run the reimbursement agent server:

```bash
cargo run --bin reimbursement_server --features http-server
```

Or with WebSocket support:

```bash
cargo run --bin reimbursement_server --features ws-server
```

You can specify host and port:

```bash
cargo run --bin reimbursement_server --features http-server -- --host 0.0.0.0 --port 8080
```

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

## Notes on Implementation

This implementation is simplified compared to a production system:
- It simulates LLM interactions rather than using a real LLM
- The form processing is hardcoded rather than using NLP
- The storage is in-memory rather than using a database
- Authentication is not implemented

In a real-world implementation, you would:
1. Integrate with a real LLM API
2. Add proper database storage for requests
3. Implement authentication and authorization
4. Enhance error handling and validation
5. Add logging and monitoring