# A2A-MCP Integration TODO

## Current Status

The bidirectional integration architecture is complete with:
- ✅ Core error handling and result types
- ✅ Protocol converters (Skill↔Tool, Message↔Content, Task↔CallToolResult)
- ✅ AgentToMcpBridge (A2A → MCP direction)
- ✅ McpToA2ABridge (MCP → A2A direction)
- ✅ Comprehensive documentation and examples in lib.rs

## Critical Issues to Fix

### 1. API Compatibility with a2a-rs

The a2a-rs API has evolved and our implementation needs updates:

#### Task Structure Changes
**Current a2a-rs API:**
```rust
pub struct Task {
    pub id: String,
    pub context_id: String,
    pub status: TaskStatus,
    pub artifacts: Option<Vec<Artifact>>,
    pub history: Option<Vec<Message>>,
    pub kind: Option<TaskKind>,
    pub metadata: Option<Map<String, Value>>,
}
```

**What needs updating:**
- [ ] Update `TaskResultConverter` to use `context_id` instead of assuming it
- [ ] Update to use `history` field instead of `messages`
- [ ] Remove references to `history_ttl` (no longer exists)
- [ ] Handle `kind` field properly
- [ ] Update all Task construction in tests and examples

**Files to modify:**
- `src/converters/task_result.rs`
- `src/bridge/agent_to_mcp.rs`
- `src/bridge/mcp_to_a2a.rs`

#### TaskStatus Structure Changes
**Current a2a-rs API:**
```rust
pub struct TaskStatus {
    pub state: TaskState,
    pub message: Option<Message>,  // Changed from Option<String>
    pub timestamp: Option<DateTime<Utc>>,
}
```

**What needs updating:**
- [ ] Change all `message: Some("text".to_string())` to `message: Some(Message { ... })`
- [ ] Or use `message: None` if text status isn't needed

**Files to modify:**
- `src/bridge/mcp_to_a2a.rs` (NoOpHandler test)
- Any other places creating TaskStatus

#### Part Structure Changes
**Current a2a-rs API:**
```rust
pub enum Part {
    Text { text: String, metadata: Option<Map<String, Value>> },
    File { uri: String, name: Option<String>, metadata: Option<Map<String, Value>> },
    Data { data: Map<String, Value>, metadata: Option<Map<String, Value>> },
}
```

**What needs updating:**
- [ ] Add `metadata: None` to all Part::Text constructions
- [ ] Add `metadata: None` to all Part::File constructions
- [ ] Change Part::Data to use `Map<String, Value>` instead of `Value`
- [ ] Remove `mime_type` field from Part::Data (no longer exists)
- [ ] Remove `mime_type` field from Part::File (no longer exists)

**Files to modify:**
- `src/converters/message.rs`
- `src/bridge/agent_to_mcp.rs`
- `src/bridge/mcp_to_a2a.rs`

#### Message Structure Changes
**Current a2a-rs API:**
```rust
pub struct Message {
    pub role: Role,  // Changed from String
    pub parts: Vec<Part>,
    // ...other fields
}

pub enum Role {
    User,
    Agent,
    System,
}
```

**What needs updating:**
- [ ] Change all `role: "user".to_string()` to `role: Role::User`
- [ ] Change all `role: "agent".to_string()` to `role: Role::Agent`
- [ ] Update message role comparisons to use enum
- [ ] Import `Role` enum where needed

**Files to modify:**
- `src/converters/message.rs`
- `src/converters/task_result.rs`
- `src/bridge/agent_to_mcp.rs`
- `src/bridge/mcp_to_a2a.rs`

#### HttpClient API Changes
**What needs updating:**
- [ ] Verify `send_task_message` signature matches current API
- [ ] May need to use different method or adjust parameters

**Files to check:**
- `src/bridge/agent_to_mcp.rs`

### 2. MCP ServerHandler Trait Implementation

The rmcp ServerHandler trait has specific lifetime requirements:

**Issues:**
- [ ] Fix lifetime parameters on `list_tools` method
- [ ] Fix lifetime parameters on `call_tool` method
- [ ] Fix lifetime parameters on `initialize` method
- [ ] Ensure Result types match exactly (currently `Result<T, McpError>` vs expected type)

**Files to modify:**
- `src/bridge/agent_to_mcp.rs` (all ServerHandler trait methods)

### 3. MCP Client API Usage

The RoleClient API needs verification:

**Issues:**
- [ ] Verify `RoleClient::list_tools()` method exists and signature
- [ ] Verify `RoleClient::call_tool()` method exists and signature
- [ ] May need to use different MCP client type or call pattern

**Files to modify:**
- `src/bridge/mcp_to_a2a.rs`

## Enhancement Tasks

### Phase 1: Get It Compiling
1. [ ] Fix all Task structure usages
2. [ ] Fix all Part structure usages
3. [ ] Fix all Message/Role usages
4. [ ] Fix TaskStatus structure usages
5. [ ] Fix ServerHandler trait implementation
6. [ ] Fix MCP client API calls
7. [ ] Run `cargo check` successfully

### Phase 2: Testing
1. [ ] Create unit tests for SkillToolConverter
2. [ ] Create unit tests for MessageConverter
3. [ ] Create unit tests for TaskResultConverter
4. [ ] Create integration test for AgentToMcpBridge
5. [ ] Create integration test for McpToA2ABridge
6. [ ] Test with actual MCP server (e.g., counter example from rust-sdk)
7. [ ] Test with actual A2A agent (e.g., from a2a-agents)

### Phase 3: Examples
1. [ ] Create `examples/a2a_as_mcp_server.rs`
   - Expose an A2A agent as MCP server via stdio
   - Use AgentToMcpBridge
   - Connect to real or mock A2A agent

2. [ ] Create `examples/a2a_with_mcp_tools.rs`
   - A2A agent that can call MCP tools
   - Use McpToA2ABridge
   - Demonstrate tool call message format

3. [ ] Create `examples/bidirectional_demo.rs`
   - Show both directions working together
   - A2A agent with MCP tools that is also exposed as MCP server

### Phase 4: Documentation
1. [ ] Add rustdoc examples to all public APIs
2. [ ] Create user guide in README.md
3. [ ] Document tool call message format for MCP→A2A
4. [ ] Add architecture diagram
5. [ ] Document limitations and edge cases

### Phase 5: Advanced Features
1. [ ] Support streaming (A2A streaming ↔ MCP sampling)
2. [ ] Support MCP resources ↔ A2A artifacts mapping
3. [ ] Support MCP prompts ↔ A2A skills mapping
4. [ ] Authentication bridging (A2A auth ↔ MCP auth)
5. [ ] Better error propagation and context
6. [ ] Support for task cancellation
7. [ ] Support for task resubscription
8. [ ] Metrics and observability integration

## Quick Fixes Checklist

Priority fixes to get compiling:

- [ ] `src/converters/message.rs`:
  - Import `Role` enum
  - Change `role: "user"` to `role: Role::User`
  - Add `metadata: None` to all Part constructions
  - Handle Part::Data as `Map<String, Value>` not `Value`

- [ ] `src/converters/task_result.rs`:
  - Change Task field access: `.messages` → `.history.unwrap_or_default()`
  - Remove `history_ttl` field
  - Add `context_id` field

- [ ] `src/bridge/agent_to_mcp.rs`:
  - Fix ServerHandler method signatures
  - Update Task construction
  - Add `metadata: None` to Part::Text

- [ ] `src/bridge/mcp_to_a2a.rs`:
  - Fix AsyncMessageHandler signature (already done)
  - Update Task construction with new fields
  - Fix Part construction with metadata
  - Fix Role usage (enum not string)
  - Fix MCP client method calls

## Reference Resources

- **A2A Protocol Spec**: `../spec/`
- **A2A Core Types**: `../a2a-rs/src/domain/core/`
- **MCP Rust SDK Examples**: `rust-sdk/examples/servers/`
- **MCP Counter Example**: `rust-sdk/examples/servers/src/common/counter.rs`
- **A2A Agent Examples**: `../a2a-agents/examples/`

## Notes

- The architecture and design are solid
- Main issue is API surface changes in a2a-rs
- Once compilation issues fixed, integration should work well
- rmcp crate is kept as reference in `rust-sdk/` directory
- Not included in parent workspace due to nested workspace conflicts
