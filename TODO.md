# A2A Implementation TODO List

## High Priority

1. Fix remaining unused variables in a2a-agents crate
   - Prefix unused variables with underscores in reimbursement_agent/agent.rs, reimbursement_agent/task_manager.rs, and reimbursement_agent/server.rs

2. Build and run HTTP server and client examples to verify functionality
   - Test both server and client functionality
   - Verify task history functionality works
   - Test file content validation

3. Build and run WebSocket server and client examples to verify functionality
   - Verify task streaming works
   - Test message exchange in both directions

## Medium Priority

4. Address duplicated attribute warnings across various modules
   - Remove duplicated `#![cfg(feature = "...")]` attributes from modules that already have them at the parent level

5. Document rustls configuration in the README for users without OpenSSL development libraries
   - Explain the switch to rustls from native-tls
   - Add build instructions for environments without OpenSSL

6. Create integration tests to validate the A2A implementation
   - Add tests for HTTP transport
   - Add tests for WebSocket transport
   - Verify protocol compliance

## Low Priority

7. Add Default implementation for ReimbursementAgent as suggested by Clippy

8. Refactor complex types in AgentTaskManager to use type aliases for better readability
   - Simplify status_subscribers and artifact_subscribers types

9. Fix unnecessary unwrap_or_else() on None value in ReimbursementAgent

10. Refactor add_comprehensive_skill method to reduce parameter count
    - Consider using a builder pattern or a config struct