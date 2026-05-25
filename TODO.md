# A2A-RS Follow-Ups and Future Work

## Agent Payments Protocol (AP2) Integration
- Expand `a2a-ap2` crate to fully support AP2 primitives (Payment Request, Payment Receipt).
- Bridge AP2 features with native LLM tool calling (allow LLMs to request and verify payments).
- Add robust tests and error handling for AP2 flows.

## Complex Agent Example
- Create a comprehensive "kitchen-sink" example showcasing all components:
  - LLM Provider integration (OpenAI/Gemini).
  - MCP tool bridging (`AgentToMcpBridge` & `McpToA2ABridge`).
  - Streaming interactions to a Web Client (`a2a-client`).
  - Declarative TOML configuration.
  - A2A native tasks and progress tracking.

## Streaming Improvements
- Add support for partial/incremental tool call streaming (instead of waiting for the full JSON string to parse) to allow UIs to show function call progress in real time.
- Implement robust retry mechanisms and exponential backoff for SSE stream interruptions.
- Expand streaming integrations natively into the `a2a-client` framework.

## General
- Refine existing Rustdoc examples and ensure they are all compile-checked.
- Resolve any remaining compilation warnings across the workspace.
