# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added - v0.3.0 Compliance

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

#### Storage Layer
- Implemented all v0.3.0 methods in `InMemoryTaskStorage`:
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

// After (v0.3.0)
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

// After (v0.3.0)
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

// After (v0.3.0)
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

// After (v0.3.0)
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
