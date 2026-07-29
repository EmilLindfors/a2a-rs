# The reimbursement reference agent

A worked example of a hand-written `AsyncMessageHandler`: an expense-reimbursement
agent that returns an interactive form, accepts the filled form back, and
approves the request. It ships with a small web frontend so the whole loop is
visible in a browser.

It exists to show the *framework* — the handler trait, storage, agent card, and
task lifecycle — not to be a good reimbursement system. For agents that do real
work from configuration alone, see the `llm` handler in
[the crate README](../README.md).

## Building it

The reimbursement agent is **opt-in**. The crate's default features are what the
`a2a` CLI needs; a sample agent has no business in everyone's build:

```bash
cargo run -p a2a-agents --features reimbursement-agent --bin reimbursement_demo
```

Everything below assumes that `--features reimbursement-agent`.

## Running the demo

Agent backend and web frontend, one command:

```bash
cargo run -p a2a-agents --features reimbursement-agent --bin reimbursement_demo
# then open http://localhost:3000
```

This starts:

- **Agent backend** on `http://localhost:8080` (HTTP / ConnectRPC)
- **Web frontend** on `http://localhost:3000`

The frontend connects to the local agent and gives you an interface for
submitting expenses and viewing tasks.

### Running the halves separately

```bash
# Agent backend only
cargo run -p a2a-agents --features reimbursement-agent --bin reimbursement_demo -- \
  --mode agent

# Frontend only, pointed at an agent that is already running
AGENT_HTTP_URL=http://localhost:8080 \
cargo run -p a2a-agents --features reimbursement-agent --bin reimbursement_demo -- \
  --mode frontend

# Custom ports
cargo run -p a2a-agents --features reimbursement-agent --bin reimbursement_demo -- \
  --agent-http-port 8080 --frontend-port 3000
```

### Endpoints

**Agent backend**

- HTTP API — `http://localhost:8080` (ConnectRPC)
- Agent card — `http://localhost:8080/.well-known/agent-card.json`

**Web frontend**

- Main UI — `http://localhost:3000`
- Task list — `http://localhost:3000/tasks`
- Expense form — `http://localhost:3000/expense/new`

## How it is put together

**`ReimbursementMessageHandler`** — the business logic, implementing
`AsyncMessageHandler`. It processes requests in the A2A message format, generates
interactive forms for expense submissions, validates and approves them, and
returns structured responses with the right task states.

**`ModernReimbursementServer`** — the server, assembled from framework parts:
`DefaultBusinessHandler` for request processing, `InMemoryTaskStorage` for task
persistence, and `SimpleAgentInfo` for the agent card, served over HTTP.

## An example conversation

**1. User:** "Can you reimburse me $50 for the team lunch yesterday?"

**2. Agent** returns a form:

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

**3. User** submits the filled form:

```json
{
  "request_id": "request_id_1234567",
  "date": "2023-10-15",
  "amount": "50",
  "purpose": "team lunch with product team"
}
```

**4. Agent:** "Your reimbursement request has been approved. Request ID:
request_id_1234567"

## What it deliberately does not do

The business logic is kept simple so the framework stays visible. The generic
`llm` handler is the path to real model-driven agents.

- **Message processing** — basic pattern matching. Use `type = "llm"` for
  LLM-driven agents.
- **Storage** — in-memory. The framework supports SQLx for production.
- **Authentication** — not wired up. The framework supports Bearer and OAuth2;
  see [authentication.md](authentication.md).
- **Form processing** — simple JSON forms, no complex validation.

## Framework features it demonstrates

- `AsyncMessageHandler` trait implementation
- `DefaultBusinessHandler` integration
- `InMemoryTaskStorage` for task persistence
- `SimpleAgentInfo` for agent metadata
- HTTP transport
- Structured error handling with `A2AError`
- Modern async/await patterns
- Builder patterns for complex objects
