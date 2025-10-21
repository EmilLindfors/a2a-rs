# A2A Protocol Specification Changelog

## [v0.3.0] - Specification Update

This document tracks the changes from the previous specification to the new v0.3.0 specification.

### Breaking Changes

#### AgentCard
- **ADDED** `protocolVersion` (required) - The version of the A2A protocol (defaults to "0.3.0")
- **ADDED** `preferredTransport` (optional) - The transport protocol for the preferred endpoint (defaults to "JSONRPC")
- **ADDED** `additionalInterfaces` (optional) - Array of additional supported interfaces (transport and URL combinations)
- **ADDED** `iconUrl` (optional) - URL to an icon for the agent
- **ADDED** `signatures` (optional) - Array of JSON Web Signatures for the AgentCard
- **CHANGED** Description now includes more detailed guidance on transport negotiation

#### AgentCapabilities
- **ADDED** `extensions` (optional) - Array of `AgentExtension` objects for protocol extensions

#### AgentSkill
- **ADDED** `security` (optional) - Security requirements specific to this skill

#### MessageSendConfiguration
- **CHANGED** `acceptedOutputModes` is no longer required (was required before)

#### PushNotificationConfig
- **ADDED** `id` (optional) - Unique identifier for the push notification configuration

### New Types and Definitions

#### Agent-Related Types
- **AgentInterface** - Declares a combination of URL and transport protocol
  - `url` (required) - The URL where the interface is available
  - `transport` (required) - The transport protocol supported

- **AgentExtension** - Declaration of a protocol extension
  - `uri` (required) - Unique URI identifying the extension
  - `description` (optional) - Human-readable description
  - `required` (optional) - Whether the client must understand the extension
  - `params` (optional) - Extension-specific configuration parameters

- **AgentCardSignature** - JWS signature of an AgentCard (RFC 7515)
  - `protected` (required) - Protected JWS header (Base64url-encoded)
  - `signature` (required) - Computed signature (Base64url-encoded)
  - `header` (optional) - Unprotected JWS header values

- **TransportProtocol** - Enum of supported transport protocols
  - Values: "JSONRPC", "GRPC", "HTTP+JSON"

#### Security Types
- **MutualTLSSecurityScheme** - Security scheme using mTLS authentication
  - `type` - Must be "mutualTLS"
  - `description` (optional)

- **OAuth2SecurityScheme Enhancement**
  - **ADDED** `oauth2MetadataUrl` (optional) - URL to OAuth2 authorization server metadata (RFC8414)

### New Request/Response Methods

#### tasks/list
- **ListTasksRequest** - Request to list tasks with filtering
- **ListTasksParams** - Parameters for listing tasks:
  - `contextId` (optional) - Filter by context ID
  - `status` (optional) - Filter by task state
  - `pageSize` (optional) - Maximum number of tasks (1-100, default 50)
  - `pageToken` (optional) - Pagination token
  - `historyLength` (optional) - Number of messages to include (default 0)
  - `includeArtifacts` (optional) - Whether to include artifacts (default false)
  - `lastUpdatedAfter` (optional) - Filter by last update time (milliseconds since epoch)
  - `metadata` (optional)
- **ListTasksResult** - Response containing tasks and pagination info:
  - `tasks` (required) - Array of Task objects
  - `totalSize` (required) - Total number of tasks
  - `pageSize` (required) - Number returned in this response
  - `nextPageToken` (required) - Token for next page (empty if no more results)
- **ListTasksResponse** - Success or error response
- **ListTasksSuccessResponse** - Successful response wrapper

#### tasks/pushNotificationConfig/get (Enhanced)
- **GetTaskPushNotificationConfigParams** - New params type:
  - `id` (required) - Task ID
  - `pushNotificationConfigId` (optional) - Specific config ID to retrieve
  - `metadata` (optional)
- **CHANGED** Request params now accepts either `TaskIdParams` or `GetTaskPushNotificationConfigParams`

#### tasks/pushNotificationConfig/list
- **ListTaskPushNotificationConfigRequest** - Request to list all push notification configs for a task
- **ListTaskPushNotificationConfigParams** - Parameters:
  - `id` (required) - Task ID
  - `metadata` (optional)
- **ListTaskPushNotificationConfigResponse** - Success or error response
- **ListTaskPushNotificationConfigSuccessResponse** - Returns array of `TaskPushNotificationConfig`

#### tasks/pushNotificationConfig/delete
- **DeleteTaskPushNotificationConfigRequest** - Request to delete a specific push notification config
- **DeleteTaskPushNotificationConfigParams** - Parameters:
  - `id` (required) - Task ID
  - `pushNotificationConfigId` (required) - Config ID to delete
  - `metadata` (optional)
- **DeleteTaskPushNotificationConfigResponse** - Success or error response
- **DeleteTaskPushNotificationConfigSuccessResponse** - Returns null on success

#### agent/getAuthenticatedExtendedCard
- **GetAuthenticatedExtendedCardRequest** - Request for extended agent card with authentication
- **GetAuthenticatedExtendedCardResponse** - Success or error response
- **GetAuthenticatedExtendedCardSuccessResponse** - Returns an `AgentCard` object

### New Error Types

- **AuthenticatedExtendedCardNotConfiguredError** (code: -32007)
  - Message: "Authenticated Extended Card is not configured"
  - Used when agent doesn't have an authenticated extended card configured

### Updated Union Types

#### A2ARequest
- **ADDED** `ListTasksRequest`
- **ADDED** `ListTaskPushNotificationConfigRequest`
- **ADDED** `DeleteTaskPushNotificationConfigRequest`
- **ADDED** `GetAuthenticatedExtendedCardRequest`

#### A2AError
- **ADDED** `AuthenticatedExtendedCardNotConfiguredError`

#### JSONRPCResponse
- **ADDED** `ListTasksSuccessResponse`
- **ADDED** `ListTaskPushNotificationConfigSuccessResponse`
- **ADDED** `DeleteTaskPushNotificationConfigSuccessResponse`
- **ADDED** `GetAuthenticatedExtendedCardSuccessResponse`

#### SecurityScheme
- **ADDED** `MutualTLSSecurityScheme`

### Enhanced Descriptions and Documentation

Many type descriptions have been enhanced with:
- More detailed explanations
- Usage examples
- Best practices guidance
- RFC references where applicable
- Clearer field descriptions

### Extension Support

The specification now has first-class support for extensions through:
- `AgentExtension` type for declaring supported extensions
- `extensions` field in `AgentCapabilities`
- `extensions` field in `Message` and `Artifact` for extension URIs
- `metadata` fields for extension-specific data

### Artifact and Message Updates

#### Artifact
- **ADDED** `extensions` (optional) - Array of extension URIs relevant to this artifact
- **CHANGED** `metadata` description now clarifies it's for extension-specific identifiers

#### Message
- **ADDED** `extensions` (optional) - Array of extension URIs relevant to this message
- **CHANGED** `metadata` description now clarifies it's for extension-specific identifiers

### Summary of Impact

This update represents a **minor version bump** from an implied v0.2.x to v0.3.0 because:

1. **New Features**: Multiple new methods and capabilities added
2. **Enhanced Flexibility**: Support for multiple transports and interfaces
3. **Better Extension Support**: First-class extension mechanism
4. **Improved Security**: mTLS support and enhanced OAuth2 configuration
5. **Better Task Management**: List and filter tasks, manage multiple push notification configs
6. **Backward Compatibility Considerations**: Most changes are additive, but `protocolVersion` is now required in `AgentCard`

### Migration Guide

To migrate from the old specification to v0.3.0:

1. **Update AgentCard**:
   - Add `protocolVersion: "0.3.0"` field
   - Optionally add `preferredTransport` (defaults to "JSONRPC")
   - Consider adding `additionalInterfaces` if supporting multiple transports
   - Add `iconUrl` if you have an agent icon

2. **Update Implementations**:
   - Implement new task management methods if needed (`tasks/list`)
   - Implement push notification list/delete if using push notifications
   - Consider implementing `agent/getAuthenticatedExtendedCard` if applicable

3. **Review Security**:
   - Consider adding mTLS support if needed
   - Update OAuth2 configs with metadata URL if applicable

4. **Extension Support**:
   - If using extensions, declare them in `AgentCapabilities.extensions`
   - Use `extensions` fields in Messages and Artifacts to indicate extension usage

5. **Code Updates**:
   - Handle new error code -32007
   - Update request/response type unions to include new methods
