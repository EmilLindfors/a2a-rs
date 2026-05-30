# Learning from the official `a2aproject/a2a-rs` SDK

A comparison of our hexagonal `a2a-rs` against the canonical A2A SDK at
[`github.com/a2aproject/a2a-rs`](https://github.com/a2aproject/a2a-rs), with
concrete improvement recommendations. A clone lives at `./a2aproject/` (untracked).

> **Date:** 2026-05-29 · **Their version:** edition 2024, Rust 1.85, crates published as `*-lf` (a2a-lf, a2a-server-lf, a2a-client-lf).

---

## 1. What the official SDK is

It is the **canonical** Rust implementation of the A2A protocol, owned by the
`a2aproject` GitHub org. A 7-crate workspace, **not** hexagonal — a flat types
crate plus transport adapters plus an axum server framework. Its overriding
design priority is **wire-compatibility with the Go and C# SDKs**, above
architectural separation.

| Their crate | Role |
|---|---|
| `a2a` (`a2a-lf`) | Pure protocol types, hand-written serde for exact wire shape |
| `a2a-client` (`a2a-client-lf`) | Multi-transport client + agent-card protocol negotiation |
| `a2a-server` (`a2a-server-lf`) | axum server framework |
| `a2a-pb` | Protobuf schema, generated types, ProtoJSON (pbjson), native↔proto conversion |
| `a2a-grpc` | tonic-based gRPC client/server bindings |
| `a2a-slimrpc` | SLIMRPC (agntcy `slim_bindings`) bindings |
| `a2acli` (`a2a-cli`) | Standalone client CLI |

### Transports they support
JSON-RPC 2.0 · REST / HTTP+JSON · gRPC (tonic) · SLIMRPC — all four behind a
single client-side `Transport` trait.

---

## 2. Headline finding — wire interoperability ⚠️

The official SDK is engineered to be **byte-compatible** with the other A2A
SDKs. Two mechanisms:

- **Field-presence union serialization.** Discriminated unions carry *no* tag —
  only the active field is present. `SendMessageResponse::Task(t)` →
  `{"task": {...}}`; `StreamResponse::StatusUpdate(e)` →
  `{"statusUpdate": {...}}`. This matches the Go/C#/Python wire format exactly.
- **ProtoJSON.** JSON-RPC and REST bodies are generated from the protobuf schema
  via `pbjson`, with null-field patching so `null` collections deserialize as
  empty `Vec`. This keeps JSON faithful to the canonical `a2a.proto`.

The A2A specification mandates **JSON-RPC 2.0** and **HTTP+JSON (REST)** as the
baseline bindings (method names like `SendMessage`, `SendStreamingMessage`,
`GetTask`, `CancelTask`, `SubscribeToTask`, push-config CRUD).

**Our server speaks ConnectRPC (over `buffa`).** The Connect protocol is *not*
the same wire format as JSON-RPC 2.0. Consequence: an off-the-shelf A2A client
(or the official `a2acli`) most likely **cannot talk to our server**, and our
client cannot talk to a standard A2A agent. For a protocol implementation this
is the single most important thing to resolve.

**Action:** verify interop empirically (point the official `a2acli` at one of
our example agents), then add a wire-compatible JSON-RPC 2.0 + HTTP+JSON
transport adapter (see `JSONRPC_ADAPTER_PLAN.md`).

---

## 3. Their JSON-RPC / REST wire contract (what we must match)

### JSON-RPC 2.0 (single `POST` endpoint)
Request envelope:
```json
{ "jsonrpc": "2.0", "id": <string|number|null>, "method": "SendMessage", "params": { ... } }
```
Response envelope: `{ "jsonrpc": "2.0", "id": ..., "result": {...} }` or
`{ "jsonrpc": "2.0", "id": ..., "error": { "code": i32, "message": "...", "data": [typed details] } }`.

Methods (PascalCase): `SendMessage`, `SendStreamingMessage`, `GetTask`,
`ListTasks`, `CancelTask`, `SubscribeToTask`, `CreateTaskPushNotificationConfig`,
`GetTaskPushNotificationConfig`, `ListTaskPushNotificationConfigs`,
`DeleteTaskPushNotificationConfig`, `GetExtendedAgentCard`. Streaming methods
(`SendStreamingMessage`, `SubscribeToTask`) respond with **SSE**, one
JSON-RPC-response-shaped event per line.

### Error codes
A2A-specific: `TASK_NOT_FOUND -32001`, `TASK_NOT_CANCELABLE -32002`,
`PUSH_NOTIFICATION_NOT_SUPPORTED -32003`, `UNSUPPORTED_OPERATION -32004`,
`CONTENT_TYPE_NOT_SUPPORTED -32005`, `INVALID_AGENT_RESPONSE -32006`,
`EXTENDED_CARD_NOT_CONFIGURED -32007`, `EXTENSION_SUPPORT_REQUIRED -32008`,
`VERSION_NOT_SUPPORTED -32009`. Plus standard JSON-RPC (`-32700`/`-32600`/
`-32601`/`-32602`/`-32603`).

### REST routes (HTTP+JSON)
```
POST   /message:send                                 (+ legacy /message/send)
POST   /message:stream                  → SSE        (+ legacy /message/stream)
GET    /tasks/{id}
GET    /tasks
POST   /tasks/{id}/cancel               (legacy)
GET    /tasks/{id}/subscribe            → SSE (legacy)
POST   /tasks/{id}/pushNotificationConfigs
GET    /tasks/{id}/pushNotificationConfigs
GET    /tasks/{id}/pushNotificationConfigs/{cfg}
DELETE /tasks/{id}/pushNotificationConfigs/{cfg}
GET    /extendedAgentCard
GET    /.well-known/agent-card.json
```

---

## 4. Design patterns worth adopting

### 4.1 Client-side `Transport` port + card-driven negotiation
Their client abstracts all four protocols behind one trait, then negotiates from
the agent card's `supportedInterfaces`:
```rust
#[async_trait] #[auto_impl(Box)]
pub trait Transport: Send + Sync {
    async fn send_message(&self, p: &ServiceParams, r: &SendMessageRequest) -> Result<SendMessageResponse, A2AError>;
    async fn send_streaming_message(&self, ...) -> Result<BoxStream<'static, Result<StreamResponse, A2AError>>, A2AError>;
    // get_task, list_tasks, cancel_task, subscribe_to_task, push-config CRUD, destroy …
}
#[async_trait]
pub trait TransportFactory: Send + Sync {
    fn protocol(&self) -> &str;                  // "JSONRPC", "GRPC", …
    async fn create(&self, card: &AgentCard, iface: &AgentInterface) -> Result<Box<dyn Transport>, A2AError>;
}
```
`A2AClientFactory` ranks a card's interfaces by client preference + version and
connects to the first that succeeds.

**Why it fits us:** `Transport` *is* a client-side **port** in hex terms, and
each protocol is an **adapter**. This is more naturally hexagonal than their flat
layout — we already have the vocabulary for it.

### 4.2 Server DX — executor yields a stream, framework does the rest
```rust
#[async_trait]
pub trait AgentExecutor: Send + Sync + 'static {
    fn execute(&self, ctx: ExecutorContext) -> BoxStream<'static, Result<StreamResponse, A2AError>>;
    fn cancel(&self,  ctx: ExecutorContext) -> BoxStream<'static, Result<StreamResponse, A2AError>>;
}
```
The framework persists task state, broadcasts to subscribers, and fires push
notifications — the executor just yields events. Contrast ours: handlers host the
`TaskStatusBroadcast` mixin and call `update_and_broadcast` at each transition.
Theirs is a lighter DX; ours is more explicit. Worth weighing.

### 4.3 `ExecutionManager` — subscriber replay
A broadcast channel per task with **sequence tracking**: a late subscriber gets a
snapshot of current task state + all future events, and lag is detectable. More
robust than naive fan-out.

### 4.4 Smaller concrete wins
- **`TaskStore` versioning** — `create`/`update` return a `u64` version (optimistic concurrency).
- **Multi-tenancy** — a `tenant: Option<String>` field threaded through every request. We have none.
- **`CallInterceptor`** before/after middleware on *both* client and server (auth, logging, metrics as a chain) — cleaner than scattered auth adapters.
- **Typed error details** — Google RPC `ErrorInfo` / `BadRequest` / `FieldViolation`, surfaced in the JSON-RPC `error.data` array. Our `A2AError` is thinner (`JsonRpc` + `ValidationError`).
- **`a2acli`** — a standalone CLI for card inspection / send / stream / task management. Excellent for testing and interop checks.

---

## 5. Where our design is deliberately better — do not blindly copy

- **Hexagonal layering + capability-decomposed ports** (`AsyncTaskLifecycle`,
  `AsyncTaskQuery`, `AsyncNotificationManager`, `AsyncStreamingHandler`,
  `AsyncMessageHandler`) is cleaner separation than their fatter `RequestHandler`
  / `TaskStore`.
- **Validating newtype IDs** (`TaskId` / `ContextId` / `PushConfigId`) — they use
  raw `String` aliases on purpose (to avoid deserialization friction). Ours is
  more type-safe; keep it.
- Their **hand-written field-presence serde** is a real maintenance cost they pay
  *only* for interop. If we adopt wire-compat we inherit that cost — go in eyes
  open, and isolate it in an adapter-side wire module (see plan).

---

## 6. Recommendation, in priority order

1. **Verify wire interop first.** Does anything official talk to us? Build an
   interop test against `a2acli`. This determines whether the rest matters.
2. **Add a wire-compatible JSON-RPC 2.0 + HTTP+JSON transport adapter** in our hex
   layout (see `JSONRPC_ADAPTER_PLAN.md`). This is the highest-value change.
3. **Adopt the client-side `Transport` port + card negotiation** pattern.
4. Cherry-pick the cheap wins: `TaskStore` versioning, `CallInterceptor` chain,
   typed error details, a small CLI.

---

## 7. Side-by-side summary

| Dimension | Ours (`EmilLindfors/a2a-rs`) | Official (`a2aproject/a2a-rs`) |
|---|---|---|
| Architecture | Hexagonal (domain/port/adapter/application) | Flat types + transport adapters + axum framework |
| Transports | ConnectRPC (+ HTTP) | JSON-RPC 2.0, REST, gRPC, SLIMRPC |
| Wire compat with Go/C# SDKs | **Unverified / likely no** | First-class (field-presence + ProtoJSON) |
| IDs | Validating newtypes | Raw `String` aliases |
| Ports | Capability-decomposed, `Arc<dyn>` | `RequestHandler` + `TaskStore` + `AgentExecutor` |
| Cross-port orchestration | `TaskStatusBroadcast` mixin | Framework-driven (executor yields events) |
| Subscriber replay | (verify) | `ExecutionManager` broadcast + sequence tracking |
| Storage versioning | No | Yes (`u64`) |
| Multi-tenancy | No | `tenant` everywhere |
| Middleware | Auth adapters | `CallInterceptor` (client + server) |
| Error details | `JsonRpc` + `ValidationError` | Typed Google-RPC details |
| CLI | No | `a2acli` |
| Extras we have they don't | AP2 crate, agents framework, MCP bridge | — |
