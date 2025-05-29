# A2A-RS Improvement TODO

## Phase 1: Specification Compliance (High Priority) 🔴

### Critical Issues
- [ ] **Fix push notification method names** - Update from `pushNotification` to `pushNotificationConfig`
  - [ ] Update `src/application/json_rpc.rs:295` - method name in SetTaskPushNotificationRequest
  - [ ] Update `src/application/json_rpc.rs:328` - method name in GetTaskPushNotificationRequest
  - [ ] Update any related tests and documentation

### Legacy Method Cleanup
- [ ] **Remove or deprecate legacy methods**
  - [ ] Mark `tasks/send` as deprecated in favor of `message/send`
  - [ ] Mark `tasks/sendSubscribe` as deprecated in favor of `message/stream`
  - [ ] Add migration documentation for users

### Specification Validation
- [ ] **Ensure all required spec fields are present**
  - [ ] Validate AgentCard required fields match spec exactly
  - [ ] Validate Message structure alignment
  - [ ] Validate Task structure alignment
  - [ ] Cross-reference with specification.json schema

## Phase 2: Structural Improvements (Medium Priority) 🟡

### Domain Layer Enhancement
- [ ] **Reorganize domain layer structure**
  ```
  src/domain/
  ├── core/           # Essential types (Agent, Message, Task)
  ├── events/         # Streaming events (TaskStatusUpdateEvent, TaskArtifactUpdateEvent)
  ├── protocols/      # Protocol-specific types (JSON-RPC, A2A extensions)
  └── validation/     # Cross-cutting validation logic
  ```
  - [ ] Move core types to `src/domain/core/`
  - [ ] Create `src/domain/events/` for streaming events
  - [ ] Create `src/domain/protocols/` for protocol types
  - [ ] Create `src/domain/validation/` for validation logic

### Builder Pattern Implementation
- [ ] **Add `bon` crate to dependencies**
- [ ] **Implement builders for complex types**
  - [ ] AgentCard builder with validation
  - [ ] Message builder with part validation
  - [ ] Task builder with proper defaults
  - [ ] SecurityScheme builders for each variant

### Error Structure Enhancement
- [ ] **Improve error types with more context**
  - [ ] Add structured validation errors with field-level details
  - [ ] Add error suggestions for common mistakes
  - [ ] Implement better error chaining
  - [ ] Add error recovery hints

### Application Layer Restructuring
- [ ] **Split large json_rpc.rs file (614 lines)**
  - [ ] Create `src/application/handlers/message.rs`
  - [ ] Create `src/application/handlers/task.rs`
  - [ ] Create `src/application/handlers/notification.rs`
  - [ ] Update module re-exports

### Enhanced Port Definitions
- [ ] **Create more granular port traits**
  - [ ] MessageHandler trait for message processing
  - [ ] TaskManager trait for task operations
  - [ ] NotificationManager trait for push notifications
  - [ ] StreamingHandler trait for real-time updates

## Phase 3: Enhanced Developer Experience (Low Priority) 🟢

### Observability & Logging
- [ ] **Add structured logging with tracing**
  - [ ] Add tracing dependency
  - [ ] Instrument key functions with #[instrument]
  - [ ] Add contextual logging for debugging
  - [ ] Add performance metrics collection

### Configuration Management
- [ ] **Implement comprehensive configuration system**
  - [ ] Create A2AConfig with builder pattern
  - [ ] Add environment-specific config examples
  - [ ] Add configuration validation
  - [ ] Add config file support (TOML/YAML)

### Testing Strategy
- [ ] **Expand testing coverage**
  - [ ] Add property-based tests for protocol compliance
  - [ ] Add integration tests for end-to-end workflows
  - [ ] Add performance benchmarks for streaming
  - [ ] Add fuzz testing for message parsing

### Documentation Improvements
- [ ] **Enhance API documentation**
  - [ ] Add comprehensive examples to lib.rs
  - [ ] Document common usage patterns
  - [ ] Add troubleshooting guide
  - [ ] Create migration guide for breaking changes
  - [ ] Add architecture decision records (ADRs)

### Performance Optimizations
- [ ] **Identify and fix performance bottlenecks**
  - [ ] Profile message serialization/deserialization
  - [ ] Optimize streaming performance
  - [ ] Add connection pooling for clients
  - [ ] Implement efficient task storage patterns

## Phase 4: Advanced Features (Future) 🔵

### Enhanced Security
- [ ] **Advanced authentication features**
  - [ ] Token refresh handling
  - [ ] Certificate-based authentication
  - [ ] Rate limiting and throttling
  - [ ] Audit logging for security events

### Monitoring & Metrics
- [ ] **Production monitoring capabilities**
  - [ ] Health check endpoints
  - [ ] Prometheus metrics export
  - [ ] Distributed tracing support
  - [ ] Performance dashboards

### Protocol Extensions
- [ ] **Support for future A2A protocol enhancements**
  - [ ] Plugin system for custom message types
  - [ ] Protocol versioning support
  - [ ] Backward compatibility layer
  - [ ] Forward compatibility checks

## Progress Tracking

### Completed ✅
- [x] Initial architecture assessment
- [x] Specification analysis
- [x] TODO roadmap creation

### In Progress 🔄
- [ ] (Will be updated as work begins)

### Blocked ⛔
- [ ] (None currently)

## Notes

- Maintain the hexagonal architecture - it's working well
- Focus on specification compliance first
- Don't break existing API unless absolutely necessary
- Add deprecation warnings before removing features
- Ensure all changes are backward compatible where possible

## Dependencies to Add

```toml
# For builder patterns
bon = "2.3"

# For structured logging
tracing = "0.1"
tracing-subscriber = "0.3"

# For configuration
serde_yaml = "0.9"  # If YAML config support needed
toml = "0.8"        # If TOML config support needed

# For testing
proptest = "1.4"    # Property-based testing
criterion = "0.5"   # Benchmarking
```