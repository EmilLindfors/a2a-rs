# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Breaking Changes — Port capability decomposition

The server-side `AsyncTaskManager` port trait carried 17 methods spanning four
distinct capabilities. It has been **removed** and split into focused capability
traits. All consumers are in-workspace; there is no deprecation shim.

#### Task ports

- **Removed** `AsyncTaskManager`.
- **Added** `AsyncTaskLifecycle` — per-task CRUD: `create`, `get`, `update_status`,
  `cancel`, `exists`.
- **Added** `AsyncTaskQuery` — cross-task listing: `list`.
- **Added** `AsyncTaskLifecycleExt` (blanket-implemented) — validation
  conveniences: `get_validated`, `cancel_validated`.
- Express requirements at the use site (e.g. `T: AsyncTaskLifecycle + AsyncTaskQuery`);
  there is no umbrella trait.

Method renames (the noun-prefix is redundant once the trait carries it):

| Old (`AsyncTaskManager`)   | New                                |
|----------------------------|------------------------------------|
| `create_task(id, ctx)`     | `AsyncTaskLifecycle::create`        |
| `get_task(id, hist)`       | `AsyncTaskLifecycle::get`           |
| `update_task_status(...)`  | `AsyncTaskLifecycle::update_status` |
| `cancel_task(id)`          | `AsyncTaskLifecycle::cancel`        |
| `task_exists(id)`          | `AsyncTaskLifecycle::exists`        |
| `list_tasks_v3(params)`    | `AsyncTaskQuery::list`              |

- **Removed** the dead `get_task_metadata` and legacy `list_tasks(context, limit)`
  methods (never called).

#### Push-notification ports

- The four v1.0.0 push-config methods moved **off** `AsyncTaskManager` and were
  reconciled into `AsyncNotificationManager`, now expressed in terms of the
  richer multi-config model: `set_config`, `get_config`, `list_configs`,
  `delete_config`.
- **Added** `AsyncNotificationManagerExt` (blanket-implemented): `validate_config`,
  `set_validated`.
- **Removed** the drifting single-config methods (`set_task_notification`,
  `get_task_notification`, `remove_task_notification`, `has_task_notification`,
  `send_test_notification`) and the unused `notify_task_status_update` /
  `notify_task_artifact_update` stubs from the async trait. The synchronous
  `NotificationManager` trait is unchanged.

#### Strongly-typed identifiers

- **Added** `TaskId`, `ContextId`, `PushConfigId` newtypes (`domain::ids`,
  re-exported from the crate root). Each validates non-emptiness on construction
  (`FromStr`/`TryFrom`), making argument-order mix-ups a compile error. They
  appear in the new port signatures; conversion from wire strings happens once at
  the RPC boundary. `#[serde(transparent)]` deserialization bypasses validation
  by design (validated at the boundary).

#### Dispatch — ports held as `Arc<dyn …>` at the composition edge

The composition-edge structs no longer carry viral generic parameters; they hold
their ports as `Arc<dyn …>` trait objects. Dispatch goes through the vtable —
one indirect call per RPC, negligible on the I/O-bound port boundary — and the
generic noise disappears from every type that holds a processor or handler.

- **`DefaultRequestProcessor`** lost its five generic parameters
  (`<M, T, N, A, S>`). It is now a plain non-generic struct with
  `Arc<dyn AsyncMessageHandler>`, `Arc<dyn AsyncTaskLifecycle>`,
  `Arc<dyn AsyncTaskQuery>`, `Arc<dyn AsyncNotificationManager>`,
  `Arc<dyn AgentInfoProvider>`, and `Arc<dyn AsyncStreamingHandler>` fields.
  Constructors (`new`, `with_handler`, `with_streaming_handler`) now take
  `impl Trait` arguments, so call sites are unchanged.
- **`DefaultMessageHandler`** lost its `<T>` parameter; it holds
  `Arc<dyn AsyncTaskLifecycle>` and its constructor takes
  `impl AsyncTaskLifecycle + 'static`.
- **`ReimbursementHandler`** (in `a2a-agents`) lost its `<T>` parameter; it holds
  `Arc<dyn AsyncTaskLifecycle>` + `Arc<dyn AsyncStreamingHandler>`. The `Clone`
  bound it forced on storage is gone (cloning an `Arc<dyn …>` is a refcount bump).

#### Migration

- Construction is source-compatible: the de-generic'd constructors accept the
  same arguments via `impl Trait`, so existing `DefaultRequestProcessor::new(…)`
  / `ReimbursementHandler::new(…)` call sites compile unchanged.
- Code that named the processor's generic parameters
  (`DefaultRequestProcessor<M, T, N, A, S>`) must drop the type arguments — the
  type is now non-generic.
- The **HTTP client** API (`HttpClient::get_task`, `cancel_task`, etc.) is
  unaffected — those names belong to the client surface, not the server port.

### Added — cross-port `TaskStatusBroadcast` mixin

The capability-mixin pattern from `.claude/rules/hexagonal_architecture.md` §9,
applied at the port boundary (`application::task_status_broadcast`, behind the
`server` feature):

- **Added** accessor ingredients `HasTaskLifecycle` and `HasStreaming` — each
  hands out a `&dyn` **port**, never a concrete adapter.
- **Added** `TaskStatusBroadcast`, a blanket-implemented mixin giving any host
  that exposes both ingredients an `update_and_broadcast` ("commit the status
  through the lifecycle port, then announce it through the streaming port")
  method for free. A host exposing only one ingredient does not get the method —
  a `compile_fail` doc test pins that guarantee.
- `TaskService` implements both accessors (see below), so it gains
  `update_and_broadcast` without coupling its lifecycle and streaming ports.

This is additive (no behavior change to existing call paths). Consuming it in
the request flow — and shedding the storage adapter's internal self-broadcast —
is deferred (`REFACTORING_PLAN.md` §4.0.2).

### Added — application/transport split (`REFACTORING_PLAN.md` §4.2)

`DefaultRequestProcessor` previously did two jobs: orchestrating the ports and
serving as the ConnectRPC transport adapter. Those layers are now separated.

- **Added** `application::TaskService` (behind the `server` feature) — the inner
  application service. It owns the six ports as `Arc<dyn …>` and holds all
  use-case orchestration (`send_message`, `send_streaming_message`, `get`,
  `list`, `cancel`, `subscribe`, push-config CRUD, `extended_agent_card`),
  speaking only domain types and `A2AError`. It hosts the `HasTaskLifecycle` /
  `HasStreaming` accessors, so it owns `update_and_broadcast`.
- **`DefaultRequestProcessor`** is now a thin ConnectRPC transport adapter that
  decodes `buffa` wire views, delegates to a `TaskService`, and re-encodes the
  results. Its public constructors (`new`, `with_handler`,
  `with_streaming_handler`) are unchanged, so all call sites compile as before.
  `map_*` helpers and `NoopStreamingHandler` remain transport-side.

### Changed — storage no longer self-broadcasts (`REFACTORING_PLAN.md` §4.0.2)

Persistence and streaming are now decoupled in the adapters; "commit then
announce" is owned by the orchestration layer via the `TaskStatusBroadcast`
mixin.

- **`InMemoryTaskStorage` / `SqlxTaskStorage`** `update_status` and `cancel` are
  now persistence-only — they no longer call `broadcast_status_update` as a side
  effect. (Both structs still implement `AsyncStreamingHandler`; that is where
  streaming subscribers live. Shedding that role entirely is a later struct
  split, not done here.)
- **Added** `TaskStatusBroadcast::cancel_and_broadcast`, the cancellation
  counterpart to `update_and_broadcast`. `TaskService::cancel` now routes through
  it, so cancellations still reach subscribers.
- **`DefaultMessageHandler`** now hosts the broadcast mixin: it holds a streaming
  port in addition to the lifecycle port and routes every transition in
  `process_message` through `update_and_broadcast`. **Breaking:** its
  constructor takes a streaming port (and a responder — see below); use
  `DefaultMessageHandler::echo(lifecycle, streaming)` for the previous behavior.
- **`ReimbursementHandler`** (in `a2a-agents`) implements `HasTaskLifecycle` /
  `HasStreaming` and broadcasts at all five transition sites, including the
  background AI worker — its updates and push notifications no longer depend on a
  storage side effect.
- Behavioral note: an agent that drives `update_status`/`cancel` directly on
  storage no longer streams as a side effect. To announce transitions, host the
  `TaskStatusBroadcast` mixin (hold both ports) or use `DefaultMessageHandler`.

### Breaking — storage/streaming/push struct-split (`REFACTORING_PLAN.md` §4.3, final)

The storage adapters shed their two non-persistence jobs. `InMemoryTaskStorage`
and `SqlxTaskStorage` previously implemented persistence **and** streaming
fan-out **and** fired push notifications inside their broadcast helpers. Each of
those is now its own adapter behind its own port, wired at the composition edge.

- **Removed** the `AsyncStreamingHandler` impl (and the internal `subscribers`
  map) from `InMemoryTaskStorage` and `SqlxTaskStorage`. They now implement only
  `AsyncTaskLifecycle` + `AsyncTaskQuery` + `AsyncNotificationManager`
  (persistence and push-config CRUD).
- **Added** `adapter::streaming::InMemoryStreamingHandler` — the in-memory
  subscriber registry and broadcast fan-out, extracted out of the storage
  structs. Re-exported from the crate root.
- **Added** the `AsyncPushNotifier` port (`port::notification_manager`) — the
  out-of-band webhook **delivery** capability, separate from config CRUD
  (`AsyncNotificationManager`) and from streaming. `PushNotificationRegistry`
  implements it (the `PushNotificationSender` trait remains the pluggable backend
  seam: HTTP, no-op, custom). **Added** `NoopPushNotifier`, and a deref-forwarding
  impl so `Arc<dyn AsyncPushNotifier>` satisfies `impl AsyncPushNotifier`.
- **Added** `InMemoryTaskStorage::push_notifier()` / `SqlxTaskStorage::push_notifier()`
  returning the store's registry as an `Arc<dyn AsyncPushNotifier>` — so a config
  written via `set_config` is visible to the notifier at the composition edge.
- **`TaskStatusBroadcast`** gained a third ingredient `HasPushNotifier`: the
  mixin now fires push delivery (best-effort, logged on failure) alongside the
  streaming broadcast, and gained a `broadcast_artifact` method. Every host
  (`TaskService`, `ReimbursementMessageHandler`, `ResponderMessageHandler`) now
  also exposes `HasPushNotifier`.
- **Breaking constructors:** `TaskService::new`/`with_handler`,
  `ResponderMessageHandler::new`/`echo`, and `ReimbursementHandler::new`/`with_llm`
  take a separate `impl AsyncPushNotifier`; `ResponderMessageHandler` and
  `ReimbursementHandler` also take the streaming port separately (no longer
  requiring the storage to be the streaming handler). The transport adapters
  (`ConnectRpcAdapter`, `JsonRpcAdapter`) default to `NoopPushNotifier` and gained
  a `with_push_notifier` builder method.
- **Behavior change — no replay on subscribe:** `add_status_subscriber` /
  `add_artifact_subscriber` no longer replay the task's current state to a new
  subscriber (the streaming adapter has no task access). This is spec-compliant —
  the initial `Task` snapshot is delivered by `TaskService::subscribe` /
  `send_streaming_message` and emitted by the transport before stream items.

### Added — injected `Responder` on `DefaultMessageHandler`

`DefaultMessageHandler` now separates lifecycle/streaming plumbing from the
business decision of what to reply.

- **Added** the `Responder` trait (`adapter::business`) —
  `async fn respond(&self, message, task) -> Result<(Message, TaskState)>`. The
  handler does create-if-absent, history append, and broadcasting; the responder
  only decides the reply and the resulting state, getting streaming for free.
- **Added** `EchoResponder`, the reference implementation (echoes the input,
  stays `Working`).
- **`DefaultMessageHandler::new(lifecycle, streaming, responder)`** takes a
  custom responder; **`DefaultMessageHandler::echo(lifecycle, streaming)`** wires
  `EchoResponder`. Agents needing "ack now, finish later" semantics still
  implement `AsyncMessageHandler` directly.

## [0.3.0](https://github.com/EmilLindfors/a2a-rs/compare/a2a-rs-v0.2.0...a2a-rs-v0.3.0) - 2026-05-27

### Fixed

- allow clippy::result_large_err in request processor

### Other

- fmt,clippy
- Fix clippy warnings and failing tests
- migrate to Connect-Rust, refactor project structure, update protobuf specs, and clean up temporary scripts
- docs

### Changed
- Demoted the `⚠️ No WebSocket subscribers found for task` log in
  `InMemoryTaskStorage::broadcast_status_update` from WARN to DEBUG. The
  no-subscriber case is the steady state for `message/send` (non-streaming)
  flows and was previously flooding logs on every status broadcast.
  `a2a-rs/src/adapter/storage/task_storage.rs:189`.

### Added - v1.0.0 Compliance

#### New API Methods
- `tasks/list` - List tasks with comprehensive filtering and pagination
  - Filter by context_id, status, last_updated_after, and metadata
  - Offset-based pagination with page tokens
  - Configurable history length and artifact inclusion per request
  - Returns ListTasksResult with tasks, total_size, page_size, and next_page_token
- `tasks/pushNotificationConfig/list` - List all push notification configs for a task
- `tasks/pushNotificationConfig/delete` - Delete a specific push notification config
- `agent/getAuthenticatedExtendedCard` - Get extended agent card for authenticated clients

#### Core Type Enhancements
- Added `TransportProtocol` enum with JSONRPC, GRPC, and HTTP+JSON variants
- Added `AgentInterface` type for additional transport protocol interfaces
- Added `AgentExtension` type for protocol extension framework
  - URI-based extension identification
  - Optional description and required flags
  - Arbitrary parameters via JSON object
- Added `extensions` field to `Message` and `Artifact` types
- Added `extensions` field to `AgentCapabilities`

#### Agent Card Updates
- Added `protocol_version` field (defaults to "0.3.0")
- Added `preferred_transport` field (defaults to "JSONRPC")
- Added `additional_interfaces` field for multi-transport support
- Added `icon_url` field for agent branding
- Changed `signature` to `signatures` (now supports multiple signatures)

#### Push Notification Enhancements
- Added `id` field to `PushNotificationConfig` for unique identification
- Added `GetTaskPushNotificationConfigParams` for retrieving specific configs
- Added `ListTaskPushNotificationConfigParams` for listing all configs
- Added `DeleteTaskPushNotificationConfigParams` for config deletion
- Updated storage implementations with full CRUD operations for push notification configs

#### Task Management
- Added `ListTasksParams` with comprehensive filtering options:
  - `context_id`: Filter by context
  - `status`: Filter by task state
  - `page_size`: Control pagination (1-100, default 50)
  - `page_token`: Offset-based pagination
  - `history_length`: Control message history depth
  - `include_artifacts`: Toggle artifact inclusion
  - `last_updated_after`: Filter by timestamp (ms since epoch)
  - `metadata`: Filter by metadata fields
- Added `ListTasksResult` with pagination metadata
- Added `list_tasks_v3` to `AsyncTaskManager` trait
- Added push notification config management methods to `AsyncTaskManager`:
  - `get_push_notification_config`
  - `list_push_notification_configs`
  - `delete_push_notification_config`

#### Protocol Migration & ConnectRPC
- Completely migrated from legacy JSON-RPC and WebSocket transport to ConnectRPC (gRPC-compatible) over HTTP.
- Replaced manual JSON-RPC routing with `connectrpc-build` and `prost`-generated models based on the official A2A `v1.0.0` protocol buffers.
- Deleted legacy WebSocket transport infrastructure across the workspace.
- Updated `a2a-client` to utilize `connectrpc` stubs for client-server communication.
- Built and verified a resilient in-process dispatch architecture for MCP standard I/O.

#### Storage Layer
- Implemented all v1.0.0 methods in `InMemoryTaskStorage`:
  - Full filtering support for task listing
  - Timestamp-based filtering
  - Metadata filtering
  - Push notification config CRUD operations
- All new trait methods have default implementations returning `UnsupportedOperation` error
- Proper sorting of tasks by timestamp (most recent first)
- Efficient pagination with configurable page sizes

#### Error Handling
- Added `AuthenticatedExtendedCardNotConfigured` error variant
- Moved `DATABASE_ERROR` code from -32007 to -32100 to avoid conflict with spec

### Changed - Breaking Changes

#### Agent Card
- **BREAKING**: `signature` field renamed to `signatures` and changed to `Option<Vec<AgentCardSignature>>`
  - Migration: Wrap single signature in a Vec: `signature: Some(sig)` → `signatures: Some(vec![sig])`
- **BREAKING**: Added required `protocol_version` field (has default: "0.3.0")
- **BREAKING**: Added required `preferred_transport` field (has default: "JSONRPC")
- New optional fields: `additional_interfaces`, `icon_url`

#### Message and Artifact
- **BREAKING**: Added `extensions` field (optional, defaults to None)
  - Migration: Add `extensions: None` to all struct initializations

#### AgentCapabilities
- **BREAKING**: Added `extensions` field (optional, defaults to None)
  - Migration: Add `extensions: None` to all struct initializations

#### PushNotificationConfig
- **BREAKING**: Added `id` field (optional, used for multi-config support)
  - Migration: Add `id: None` to existing configs

#### MessageSendConfiguration
- **BREAKING**: `accepted_output_modes` is now optional (was required)
  - Migration: Wrap existing values in Some(): `vec![...]` → `Some(vec![...])`

#### Error Codes
- **BREAKING**: `DATABASE_ERROR` moved from -32007 to -32100
- New error code: -32007 now used for `AUTHENTICATED_EXTENDED_CARD_NOT_CONFIGURED`

### Migration Guide

#### Updating Agent Card Initializations

```rust
// Before (v0.2.x)
let card = AgentCard {
    signature: Some(my_signature),
    // ... other fields
};

// After (v1.0.0)
let card = AgentCard {
    protocol_version: "0.3.0".to_string(),  // NEW - required
    preferred_transport: "JSONRPC".to_string(),  // NEW - required
    additional_interfaces: None,  // NEW - optional
    icon_url: None,  // NEW - optional
    signatures: Some(vec![my_signature]),  // CHANGED - now plural, wrapped in Vec
    // ... other fields
};
```

#### Updating Message and Artifact Initializations

```rust
// Before (v0.2.x)
let message = Message {
    message_id: "msg-1".to_string(),
    // ... other fields
};

// After (v1.0.0)
let message = Message {
    message_id: "msg-1".to_string(),
    extensions: None,  // NEW - add this field
    // ... other fields
};

// Same for Artifact
let artifact = Artifact {
    artifact_id: "art-1".to_string(),
    extensions: None,  // NEW - add this field
    // ... other fields
};
```

#### Updating Capabilities

```rust
// Before (v0.2.x)
let caps = AgentCapabilities {
    streaming: true,
    push_notifications: false,
    state_transition_history: true,
};

// After (v1.0.0)
let caps = AgentCapabilities {
    streaming: true,
    push_notifications: false,
    state_transition_history: true,
    extensions: None,  // NEW - add this field
};
```

#### Updating Push Notification Configs

```rust
// Before (v0.2.x)
let config = PushNotificationConfig {
    url: "https://example.com/webhook".to_string(),
    token: Some("token".to_string()),
    authentication: None,
};

// After (v1.0.0)
let config = PushNotificationConfig {
    id: None,  // NEW - for multi-config support
    url: "https://example.com/webhook".to_string(),
    token: Some("token".to_string()),
    authentication: None,
};
```

#### Using New Task Listing API

```rust
use a2a_rs::domain::{ListTasksParams, TaskState};

// List tasks with filtering and pagination
let params = ListTasksParams {
    context_id: Some("ctx-123".to_string()),
    status: Some(TaskState::Working),
    page_size: Some(25),
    page_token: None,  // Start at beginning
    history_length: Some(10),
    include_artifacts: Some(true),
    last_updated_after: None,
    metadata: None,
};

let result = task_manager.list_tasks_v3(&params).await?;
println!("Found {} tasks, showing {}", result.total_size, result.tasks.len());

// Get next page if available
if !result.next_page_token.is_empty() {
    let next_params = ListTasksParams {
        page_token: Some(result.next_page_token),
        ..params
    };
    let next_result = task_manager.list_tasks_v3(&next_params).await?;
}
```

#### Managing Push Notification Configs

```rust
use a2a_rs::domain::{
    GetTaskPushNotificationConfigParams,
    ListTaskPushNotificationConfigParams,
    DeleteTaskPushNotificationConfigParams,
};

// List all configs for a task
let list_params = ListTaskPushNotificationConfigParams {
    id: "task-123".to_string(),
    metadata: None,
};
let configs = task_manager.list_push_notification_configs(&list_params).await?;

// Get a specific config
let get_params = GetTaskPushNotificationConfigParams {
    id: "task-123".to_string(),
    push_notification_config_id: Some("config-1".to_string()),
    metadata: None,
};
let config = task_manager.get_push_notification_config(&get_params).await?;

// Delete a config
let delete_params = DeleteTaskPushNotificationConfigParams {
    id: "task-123".to_string(),
    push_notification_config_id: "config-1".to_string(),
    metadata: None,
};
task_manager.delete_push_notification_config(&delete_params).await?;
```

### Notes

- All new trait methods have default implementations that return `UnsupportedOperation` error
- Existing code will continue to work after adding required fields to struct initializations
- The `InMemoryTaskStorage` implementation supports all new features
- SQLx storage implementations need to be updated to support multi-config push notifications

## [0.1.0] - 2024-XX-XX

### Added
- Initial release with A2A Protocol v0.2.x support
- HTTP and WebSocket transport implementations
- Client and server functionality
- SQLx-based persistent storage
- Authentication and security features
- Comprehensive test suite
- Documentation and examples
