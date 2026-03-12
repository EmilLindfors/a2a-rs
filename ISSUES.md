# Known Issues and Technical Debt

Tracked issues for the a2a-rs workspace, organized by priority.

---

## High Priority

### 1. `a2a-mcp` not functional — broken API surface
- **Location**: `a2a-mcp/`
- **Status**: Not in workspace, stub only
- **Problem**: The crate references RMCP SDK APIs that don't exist or have changed (`rmcp::ServiceExt`, `rmcp::model::Tool`, etc.). The embedded `rust-sdk/` is a git repo that shouldn't be committed directly.
- **Action**: Either implement against the actual RMCP API or remove the crate. If keeping, add as a proper git submodule or vendored dependency.

### ~~2. `with_authentication()` is a no-op stub~~ — RESOLVED
- Replaced with `with_security_schemes(HashMap<String, SecurityScheme>)` and `with_security()` methods that properly wire SecurityScheme definitions into the agent card's `security_schemes` and `security` fields.

### ~~3. Context ID hardcoded to `"default"`~~ — RESOLVED
- Both `InMemoryTaskStorage` and `SqlxStorage` now look up the task's actual `context_id` before creating broadcast events, via `get_task_context_id()` helper methods.

### 4. `num-bigint-dig v0.8.4` future incompatibility
- **Cause**: Transitive dependency via `oauth2` crate
- **Problem**: Rust compiler warns this will be rejected in a future version.
- **Action**: Update `oauth2` dependency when a fixed version is available.

---

## Medium Priority

### 5. Database type detection not implemented
- **Location**: `a2a-rs/src/adapter/storage/sqlx_storage.rs:156`
- **Problem**: Migration code has a comment "In a real implementation, you'd detect the database type from the URL" but always runs SQLite migrations.
- **Action**: Parse the database URL scheme to select the correct migration set (SQLite vs PostgreSQL).

### ~~6. Unsafe integer casts~~ — RESOLVED
- `json_rpc.rs`: `v as i32` replaced with `i32::try_from(v).unwrap_or(-32603)`
- `task.rs`: `history_length.unwrap() as usize` replaced with `.try_into().unwrap_or(usize::MAX)`

### ~~7. Silent timestamp fallback~~ — RESOLVED
- `sqlx_storage.rs`: `unwrap_or(Utc::now())` replaced with `ok_or_else(|| A2AError::DatabaseError(...))`

### 8. Missing test coverage for several crates
- **a2a-client**: No unit tests (only doctests)
- **a2a-agents-common**: Inline tests only, no integration tests
- **a2a-agent-reimbursement**: Handler tests only, no integration/e2e tests
- **Action**: Add test suites, especially for client HTTP/WebSocket operations and agent lifecycle.

### 9. Several doctests marked `ignore`
- **Locations**: `a2a-agents/src/lib.rs`, `a2a-agents/src/core/mod.rs`, `a2a-client/src/lib.rs`, `a2a-client/src/components/mod.rs`
- **Problem**: Doctests reference APIs that have evolved (e.g., `Message::builder().text()`, `my_handler` placeholder). They were marked `ignore` to fix compilation but should be rewritten as proper compilable examples.
- **Action**: Update doctests to use the current API with concrete types.

### ~~10. `stock_agent_types.rs` is a dead example~~ — RESOLVED
- File removed from `a2a-agents/examples/`.

---

## Low Priority

### ~~11. TODO comments in production code~~ — RESOLVED
- SecurityScheme TODO resolved by implementing `with_security_schemes()`
- Legacy re-export TODO reworded to non-TODO comment
- SQLx migration TODOs reworded to documented limitation

### 12. `a2a-agents` depends on `a2a-rs/full` feature
- **Location**: `a2a-agents/Cargo.toml:11`
- **Problem**: Pulls in all features (SQLite, PostgreSQL, auth, all transports) even when agents may only need HTTP. Increases compile times.
- **Action**: Create a more granular feature set (e.g., `a2a-rs/agent-framework`) or let consumers choose.

### ~~13. Regex compilation at runtime~~ — RESOLVED
- Migrated all `lazy_static!` to `std::sync::LazyLock` in `a2a-agents/src/utils/parsing.rs`, `a2a-agents/src/core/config.rs`, and `a2a-agents-common/src/nlp/entity.rs`. Removed `lazy_static` dependency from both crates.

### 14. `a2a-mcp/rust-sdk` embedded git repository
- **Location**: `a2a-mcp/rust-sdk/`
- **Problem**: Nested git repo causes warnings on `git add -A`. Won't be cloned properly by consumers.
- **Action**: Add to `.gitignore`, convert to git submodule, or vendor properly.

### ~~15. Webhook URL validation incomplete~~ — RESOLVED
- Added HTTPS-only validation to `validate_push_notification_url()` in both sync and async notification manager traits. HTTP is only allowed for `localhost`/`127.0.0.1`/`::1` (development).

---

## Resolved (this session)

- [x] Lifetime mismatch in `tests/common/test_handler.rs` (`&'a` on trait impl methods)
- [x] `test_task_list_include_artifacts` asserting wrong variable
- [x] `stock_agent_types.rs` referencing missing `yfinance_rs` dependency
- [x] Failing doctests in `a2a-agent-reimbursement` (`ReimbursementHandler::new()` signature)
- [x] Failing doctests in `a2a-agents` (undefined `my_handler`, incomplete `AgentPlugin` impl)
- [x] Failing doctests in `a2a-agents-common` (table format assertion, entity regex pattern)
- [x] Failing doctests in `a2a-client` (outdated API references)
- [x] All workspace tests now pass: `cargo test --workspace` green
