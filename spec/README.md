# A2A Protocol Specification

This directory contains the Agent-to-Agent (A2A) Protocol specification split into focused, domain-specific files for better comprehension and compliance tracking.

## File Organization

The specification has been organized into the following files:

### Core Domain Models
- **[agent.json](./agent.json)** - Agent cards, capabilities, skills, and provider information
- **[message.json](./message.json)** - Message structures, parts (text, file, data), and content types
- **[task.json](./task.json)** - Task lifecycle, states, status, and artifacts

### Protocol Infrastructure
- **[jsonrpc.json](./jsonrpc.json)** - JSON-RPC 2.0 base types and message structures
- **[requests.json](./requests.json)** - Method-specific requests, responses, and parameters
- **[errors.json](./errors.json)** - Error codes and types (both JSON-RPC and A2A-specific)

### Specialized Features
- **[security.json](./security.json)** - Authentication schemes (API key, HTTP, OAuth2, OpenID Connect)
- **[notifications.json](./notifications.json)** - Push notification configuration and authentication
- **[events.json](./events.json)** - Streaming events for status and artifact updates

## Key A2A Protocol Methods

The specification defines the following core methods:

1. **`message/send`** - Send a message to an agent (blocking)
2. **`message/stream`** - Send a message with streaming response
3. **`tasks/get`** - Retrieve task information and history
4. **`tasks/cancel`** - Cancel an active task
5. **`tasks/pushNotificationConfig/set`** - Configure push notifications for a task
6. **`tasks/pushNotificationConfig/get`** - Retrieve push notification configuration
7. **`tasks/resubscribe`** - Resubscribe to task updates

## Task States

Tasks progress through these states:
- `submitted` - Task received by agent
- `working` - Agent is processing the task
- `input-required` - Agent needs additional input
- `completed` - Task finished successfully
- `canceled` - Task was canceled
- `failed` - Task failed due to an error
- `rejected` - Task was rejected by agent
- `auth-required` - Authentication needed
- `unknown` - State unknown

## Error Codes

The protocol defines specific error codes:
- `-32700` to `-32603` - Standard JSON-RPC errors
- `-32001` - Task not found
- `-32002` - Task not cancelable
- `-32003` - Push notifications not supported
- `-32004` - Operation not supported
- `-32005` - Content type not supported
- `-32006` - Invalid agent response

## Usage for Implementation

When implementing the A2A protocol:

1. Start with **agent.json** to understand agent capabilities and discovery
2. Reference **message.json** and **task.json** for core data structures
3. Use **requests.json** for method implementations
4. Handle errors according to **errors.json**
5. Implement security per **security.json** requirements
6. Add streaming support using **events.json**
7. Configure notifications via **notifications.json**

Each file is self-contained with proper JSON Schema references to related files, making it easy to validate specific aspects of your implementation against the protocol specification.