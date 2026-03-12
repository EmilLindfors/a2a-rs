# a2a-client TODO

This document tracks planned features and improvements for the `a2a-client` library.

## High Priority

### Transport Auto-Detection
- [ ] Implement proper transport auto-detection in `WebA2AClient::auto_connect()`
- [ ] Fetch agent card to discover available transports
- [ ] Automatically configure WebSocket if available
- [ ] Add fallback chain: WebSocket → HTTP polling

### Error Handling
- [ ] Replace remaining `anyhow` usage with `ClientError`
- [ ] Add more specific error variants for common failure cases
- [ ] Improve error messages with actionable suggestions
- [ ] Add error recovery strategies (retry logic, backoff)

### Testing
- [ ] Add unit tests for view models
- [ ] Add integration tests with mock agent
- [ ] Test SSE streaming with different scenarios
- [ ] Add property-based tests for formatters
- [ ] Test WebSocket reconnection logic

## Medium Priority

### Additional Components
- [ ] Task list component with pagination support
- [ ] File upload/download helpers
- [ ] Authentication component (OAuth2, JWT)
- [ ] Progress indicators for long-running tasks
- [ ] Real-time typing indicators

### Documentation
- [ ] Add more examples:
  - [ ] WebSocket-only client
  - [ ] File upload/download
  - [ ] Authentication flows
  - [ ] Multi-agent coordination
- [ ] Add architecture diagrams
- [ ] Create migration guide from older patterns
- [ ] Document common pitfalls and solutions

### Performance
- [ ] Add connection pooling for HTTP client
- [ ] Implement caching for agent cards
- [ ] Optimize SSE stream memory usage
- [ ] Add metrics/observability hooks
- [ ] Benchmark and profile common operations

## Low Priority

### Developer Experience
- [ ] Add builder validation with helpful error messages
- [ ] Create CLI tool for testing A2A agents
- [ ] Add debugging utilities (request/response logging)
- [ ] Improve compile-time error messages

### Advanced Features
- [ ] Support for agent discovery/registry
- [ ] Multi-agent orchestration helpers
- [ ] Request/response middleware system
- [ ] Custom serialization strategies
- [ ] Webhook verification utilities

### WebAssembly Support
- [ ] Make library compatible with WASM targets
- [ ] Add WASM-specific examples
- [ ] Document browser usage patterns

## Completed

- [x] Comprehensive README with examples
- [x] Builder pattern for `WebA2AClient`
- [x] Custom error types (`ClientError`)
- [x] Module-level documentation
- [x] SSE streaming component
- [x] Basic view models (TaskView, MessageView)
- [x] Working examples (basic_client, sse_streaming)

## Ideas for Future Consideration

- React/Yew/Leptos bindings for frontend frameworks
- GraphQL adapter for A2A protocol
- OpenTelemetry integration
- Rate limiting helpers
- Circuit breaker pattern implementation
- Agent health monitoring dashboard

## Contributing

If you'd like to work on any of these items, please:

1. Check if there's already an issue or PR for it
2. Open an issue to discuss your approach
3. Reference the TODO item in your PR

See [CONTRIBUTING.md](../CONTRIBUTING.md) for general contribution guidelines.
