# Plan: a wire-compatible JSON-RPC 2.0 + HTTP+JSON transport adapter

Goal: make our server interoperable with the canonical A2A ecosystem (the
official `a2aproject` SDK, plus the Go/C#/Python SDKs) by adding a transport
adapter that speaks the spec-mandated **JSON-RPC 2.0** and **HTTP+JSON (REST)**
bindings — *without* touching the domain, ports, or `TaskService`.

See `OFFICIAL_SDK_COMPARISON.md` for the why. This document is the how.

---

## 1. Where it fits (hexagonal placement)

This is a **transport adapter** — a sibling of `ConnectRpcAdapter`. It wraps the
same inner [`TaskService`](a2a-rs/src/application/task_service.rs) and forwards
decoded requests to it. No port traits are held directly; all orchestration
already lives in `TaskService`.

```
adapter/transport/
  connectrpc.rs     // existing — Connect protocol over the generated A2aService
  jsonrpc.rs        // NEW — JSON-RPC 2.0 + REST, spec-compatible wire format
  http/             // existing axum host (mounts whichever adapter's router)
  wire/             // NEW — adapter-side ProtoJSON DTOs + field-presence unions
```

The `wire/` module is the **only** place the field-presence-union serde and any
ProtoJSON quirks live (rule: isolate interop maintenance cost at the edge).
Domain and ports stay clean.

Feature gate: `jsonrpc-server` (implies `server`), mirroring `http-server`.
Domain/ports must still compile with zero features.

---

## 2. The central design decision — what serializes to JSON

Our wire message types (`Task`, `Message`, `TaskStatus`, `Artifact`, `TaskState`,
…) are **re-exported from `domain::generated`** — i.e. they are `buffa`
protobuf-generated types (`a2a-rs/src/domain/core/task.rs:14`,
`.../message.rs`). The A2A *param* types (`TaskQueryParams`, `ListTasksParams`,
`MessageSendParams`, …) are hand-written serde with camelCase renames.

The canonical A2A JSON wire format is the **ProtoJSON** mapping of `a2a.proto`
(camelCase fields, enums as SCREAMING_SNAKE strings, `google.protobuf.Struct`
for metadata, base64 for `bytes`, field-presence unions with no tag).

> **⚠️ Must verify before trusting interop:** does `buffa`'s serde output match
> ProtoJSON? Specifically — field name casing, enum representation
> (string vs int), timestamp format, and `oneof`/optional handling. If `buffa`
> emits ProtoJSON-compatible JSON, we can serialize the generated types
> directly (Option A). If not, we convert through dedicated DTOs (Option B).
> **Action: write golden tests first (§6) against the official SDK's JSON before
> committing to A or B.**

### Option A — serialize the generated types directly (preferred if buffa is ProtoJSON-clean)
The `wire/` module only needs to add the **field-presence union** wrappers that
`buffa` doesn't give us as plain serde enums:

```rust
// adapter/transport/wire/mod.rs
use serde::{Serialize, Serializer, ser::SerializeMap};
use crate::domain::{Task, Message, TaskStatusUpdateEvent, TaskArtifactUpdateEvent};

/// Spec union: `SendMessage` result is either a Task or a Message, tag-free.
pub enum SendMessageResult { Task(Task), Message(Message) }

impl Serialize for SendMessageResult {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(1))?;
        match self {
            Self::Task(t)    => m.serialize_entry("task", t)?,
            Self::Message(x) => m.serialize_entry("message", x)?,
        }
        m.end()
    }
}

/// Spec union: streaming yields task | message | statusUpdate | artifactUpdate.
pub enum StreamResult {
    Task(Task),
    Message(Message),
    StatusUpdate(TaskStatusUpdateEvent),
    ArtifactUpdate(TaskArtifactUpdateEvent),
}
// same field-presence Serialize: "task" | "message" | "statusUpdate" | "artifactUpdate"
```

### Option B — dedicated ProtoJSON DTOs (if buffa drifts from ProtoJSON)
Define `wire::Task`, `wire::Message`, … as hand-written serde structs that
exactly match ProtoJSON, plus `From<domain::Task> for wire::Task` and the
reverse. More code, but total control. This is what the official SDK effectively
does via `a2a-pb`'s pbjson layer. Keep these conversions in `wire/` only.

Pick A if golden tests pass with the generated types; fall back to B per-type
where they don't.

---

## 3. JSON-RPC 2.0 binding

### Envelopes (in `wire/jsonrpc_envelope.rs`)
```rust
#[derive(Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,                 // must be "2.0"
    pub id: JsonRpcId,
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,           // "2.0"
    pub id: JsonRpcId,
    #[serde(skip_serializing_if = "Option::is_none")] pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")] pub error:  Option<JsonRpcError>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum JsonRpcId { Str(String), Num(i64), Null }   // preserve wire type

#[derive(Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,             // typed error details array
}
```

### Methods (PascalCase, per spec)
`SendMessage`, `SendStreamingMessage`, `GetTask`, `ListTasks`, `CancelTask`,
`SubscribeToTask`, `CreateTaskPushNotificationConfig`,
`GetTaskPushNotificationConfig`, `ListTaskPushNotificationConfigs`,
`DeleteTaskPushNotificationConfig`, `GetExtendedAgentCard`.

### Dispatch
Single `POST /` (or `/jsonrpc`) axum handler:
1. Parse `JsonRpcRequest`. On parse failure → JSON-RPC error `-32700`/`-32600`.
2. Extract headers → `ServiceParams`-equivalent (we currently don't thread
   headers into `TaskService` — see §5, follow-up).
3. Match `method`:
   - unary methods: deserialize `params` into the matching A2A param struct,
     call the `TaskService` method, wrap the domain result in the right
     `wire::*` union, serialize into `result`.
   - streaming methods (`SendStreamingMessage`, `SubscribeToTask`): return an
     **SSE** response, each event a `JsonRpcResponse` whose `result` is a
     `StreamResult`. First event is the initial `Task` (matching how
     `ConnectRpcAdapter` chains `stream::once(task)` ahead of the update stream,
     `connectrpc.rs:265-276`).
   - unknown method → `-32601`.

### Error mapping (domain `A2AError` → JSON-RPC code)
Mirror the spec codes (the official `error_code` module):

| `A2AError` | code |
|---|---|
| `TaskNotFound` | `-32001` |
| (task not cancelable) | `-32002` |
| `UnsupportedOperation` | `-32004` |
| `AuthenticatedExtendedCardNotConfigured` | `-32007` |
| `InvalidParams` / `ValidationError` | `-32602` |
| `MethodNotFound` | `-32601` |
| parse failure | `-32700` |
| everything else | `-32603` |

This is the JSON-RPC analogue of `connectrpc.rs::map_err`. Put it in
`wire/error.rs` as `fn a2a_to_jsonrpc(&A2AError) -> JsonRpcError`, optionally
attaching typed `data` details for `ValidationError` (field/message →
`BadRequest.fieldViolations`).

---

## 4. REST / HTTP+JSON binding

Same `TaskService`, different surface. axum router:

```
POST   /v1/message:send                          → SendMessage
POST   /v1/message:stream            (SSE)        → SendStreamingMessage
GET    /v1/tasks/{id}                             → GetTask         (query: historyLength)
GET    /v1/tasks                                  → ListTasks       (query params)
POST   /v1/tasks/{id}:cancel                      → CancelTask
GET    /v1/tasks/{id}:subscribe      (SSE)        → SubscribeToTask
POST   /v1/tasks/{id}/pushNotificationConfigs     → Create push config
GET    /v1/tasks/{id}/pushNotificationConfigs     → List push configs
GET    /v1/tasks/{id}/pushNotificationConfigs/{c} → Get push config
DELETE /v1/tasks/{id}/pushNotificationConfigs/{c} → Delete push config
GET    /v1/card                                   → GetExtendedAgentCard
```
(Confirm the exact paths/prefix against `spec/specification.json`; the official
SDK also serves legacy `/message/send` aliases — add if we need broad client
compatibility.) REST errors: HTTP status from `A2AError` (404/400/501/500) with
a JSON error body carrying the same code + message + details.

The agent-card route `GET /.well-known/agent-card.json` already exists in the
HTTP host; ensure the card's `supportedInterfaces` advertises the JSON-RPC and
REST URLs with `protocolBinding: "JSONRPC"` / `"HTTP+JSON"` so clients negotiate
to us.

---

## 5. Wiring & follow-ups

- **Mount:** the existing axum host (`adapter/transport/http/server.rs`) gains a
  way to mount `jsonrpc_router(adapter)` / `rest_router(adapter)` alongside (or
  instead of) the Connect router. The `JsonRpcAdapter` reuses
  `ConnectRpcAdapter`'s constructors' shape (`new` / `with_handler` /
  `with_streaming_handler`) so agent authors swap transports with one line.
- **Headers → service:** `TaskService` methods currently take no header/auth
  context. The official SDK threads `ServiceParams` (headers) into every call.
  This is a separate, additive change to `TaskService` (and a good moment to add
  the `CallInterceptor` middleware idea). Out of scope for the first cut; the
  adapter can authenticate at the HTTP layer for now.
- **Multi-tenancy (`tenant`):** the spec/official SDK carries a `tenant` field.
  Defer; note it for the param structs if we adopt it.

---

## 6. Verification (do this FIRST — it decides Option A vs B)

1. **Golden JSON tests.** Capture canonical request/response JSON from the
   official SDK (or `spec/specification.json` examples) for each method. Assert
   our `wire::*` serialization is byte-equivalent (modulo key order). This is
   what tells us whether `buffa`'s JSON is ProtoJSON-clean.
2. **Round-trip tests.** `wire` deserialize → domain → `wire` serialize for every
   request/response type.
3. **Live interop.** Run an example agent with the JSON-RPC adapter mounted;
   point the official `a2acli` at it:
   ```sh
   cargo run --bin a2acli -- --base-url http://localhost:PORT card
   cargo run --bin a2acli -- --base-url http://localhost:PORT send "hello"
   cargo run --bin a2acli -- --base-url http://localhost:PORT stream "hello"
   ```
   (a2acli lives in `./a2aproject/a2a-rs/a2acli`.)
4. **CI:** `cargo check --workspace --no-default-features` (ports/domain still
   feature-clean), `--all-features`, clippy `-D warnings`.

---

## 7. Step sequence

1. `wire/` module: JSON-RPC envelopes + field-presence union types + error
   mapping. Golden tests (§6.1) → decide A vs B.
2. `JsonRpcAdapter` skeleton wrapping `TaskService` (drafted in
   `a2a-rs/src/adapter/transport/jsonrpc.rs`). Implement unary methods first.
3. SSE for the two streaming methods (reuse the `stream::once(task).chain(...)`
   shape from `connectrpc.rs`).
4. REST router (§4).
5. Mount in the HTTP host behind `jsonrpc-server`; advertise both interfaces in
   the agent card.
6. Interop test against `a2acli` (§6.3). Iterate on wire drift.
7. (Follow-up PRs) header/`ServiceParams` threading + `CallInterceptor`;
   client-side `Transport` port; `tenant`.
```
