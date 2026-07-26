# A2A Protocol Specification (v1.0.0)

This directory vendors the **Agent-to-Agent (A2A) Protocol v1.0.0** definitions that
`a2a-rs` generates its domain types from.

A2A v1.0.0 is **proto-first**: the canonical contract is the Protocol Buffer
definition, and the JSON wire format is derived from it via the **ProtoJSON**
serialization rules ([ADR-001](https://github.com/a2aproject/A2A/blob/main/adrs/adr-001-protojson-serialization.md),
accepted 2025-11-18). There is no hand-written JSON Schema in this layout.

## Files

- **[a2a.proto](./a2a.proto)** — the canonical A2A protocol definition
  (package `lf.a2a.v1`, service `A2AService`). This is the source of truth; the
  generated Rust domain types are built from it.
- **[google/](./google)** — `google.api` / well-known-type proto dependencies
  imported by `a2a.proto`.
- **[specification.json](./specification.json)** — a small index stub listing the
  top-level definitions. **Not** a normative schema; the proto is authoritative.
- **[CHANGELOG.md](./CHANGELOG.md)** — upstream spec changelog.

## Wire format (v1.0.0)

JSON bindings (JSON-RPC 2.0 and HTTP+JSON/REST) serialize the proto messages as
**ProtoJSON**:

- **Enums** are the proto value names, **SCREAMING_SNAKE_CASE** — e.g.
  `"role": "ROLE_USER"`, `"state": "TASK_STATE_COMPLETED"`. (Per ADR-001 this is a
  deliberate breaking change from the pre-1.0 lowercase form like `"user"`.)
- **Fields** are camelCase (with snake_case accepted on input).
- **Timestamps** (`google.protobuf.Timestamp`) are RFC 3339 strings.
- **Error details** use the ProtoJSON `Any` representation (`@type`), with
  `google.rpc` `ErrorInfo` / `BadRequest` where applicable.

## Method names

The JSON-RPC and gRPC bindings use the **same PascalCase method names** as the
proto RPCs (spec §5.3 Method Mapping Reference); REST maps them to custom-verb
HTTP endpoints:

| Operation | JSON-RPC / gRPC method | REST endpoint |
|---|---|---|
| Send message | `SendMessage` | `POST /message:send` |
| Send streaming message | `SendStreamingMessage` | `POST /message:stream` |
| Get task | `GetTask` | `GET /tasks/{id}` |
| List tasks | `ListTasks` | `GET /tasks` |
| Cancel task | `CancelTask` | `POST /tasks/{id}:cancel` |
| Subscribe to task | `SubscribeToTask` | `POST /tasks/{id}:subscribe` |
| Create push notification config | `CreateTaskPushNotificationConfig` | `POST /tasks/{id}/pushNotificationConfigs` |
| Get push notification config | `GetTaskPushNotificationConfig` | `GET /tasks/{id}/pushNotificationConfigs/{configId}` |
| List push notification configs | `ListTaskPushNotificationConfigs` | `GET /tasks/{id}/pushNotificationConfigs` |
| Delete push notification config | `DeleteTaskPushNotificationConfig` | `DELETE /tasks/{id}/pushNotificationConfigs/{configId}` |
| Get extended Agent Card | `GetExtendedAgentCard` | `GET /extendedAgentCard` |

> **Pre-1.0 note.** The older Google A2A spec used slash-style JSON-RPC methods
> (`message/send`, `tasks/get`, `tasks/cancel`, `tasks/pushNotificationConfig/set`, …)
> and lowercase enum values (`"user"`, `"working"`). Those are **not** v1.0.0.
> Clients still on the pre-1.0 wire will not interoperate with a v1.0.0 server
> without a compatibility shim. See the live spec at
> <https://a2a-protocol.org/latest/specification/>.

## Task states (`TaskState`)

`TASK_STATE_UNSPECIFIED`, `TASK_STATE_SUBMITTED`, `TASK_STATE_WORKING`,
`TASK_STATE_INPUT_REQUIRED`, `TASK_STATE_COMPLETED`, `TASK_STATE_CANCELED`,
`TASK_STATE_FAILED`, `TASK_STATE_REJECTED`, `TASK_STATE_AUTH_REQUIRED`.

## A2A-specific JSON-RPC error codes

- `-32700` … `-32603` — standard JSON-RPC errors
- `-32001` — Task not found
- `-32002` — Task not cancelable
- `-32003` — Push notifications not supported
- `-32004` — Unsupported operation
- `-32005` — Content type not supported
- `-32006` — Invalid agent response
- `-32007` — Authenticated extended card not configured

## AP2 (Agent Payments Protocol) extension

AP2 is implemented as a **separate crate** (`a2a-ap2`) that depends on `a2a-rs`,
not as part of this core spec. See that crate for the mandate/receipt types and
the extension URI.
