# Known Issues and Technical Debt

Tracked issues for the a2a-rs workspace, organized by priority.

---

## High Priority

### 1. `a2a-mcp` not functional — broken API surface
- **Location**: `a2a-mcp/`
- **Status**: Not in workspace, stub only
- **Problem**: The crate references RMCP SDK APIs that don't exist or have changed (`rmcp::ServiceExt`, `rmcp::model::Tool`, etc.). The embedded `rust-sdk/` is a git repo that shouldn't be committed directly.
- **Action**: Either implement against the actual RMCP API or remove the crate. If keeping, add as a proper git submodule or vendored dependency.

### 2. `with_authentication()` is a no-op stub
- **Location**: `a2a-rs/src/adapter/business/agent_info.rs:95`
- **Problem**: `SimpleAgentInfo::with_authentication()` takes a `Vec<String>` but does nothing — just returns `self`. SecurityScheme types exist in the domain but are never wired into the agent card builder.
- **Action**: Implement SecurityScheme integration so agents can declare their auth requirements in the card.

### 3. Context ID hardcoded to `"default"`
- **Locations**: Storage adapters (InMemoryTaskStorage, SqlxStorage)
- **Problem**: When creating tasks, the context_id is sometimes hardcoded rather than propagated from the request. This breaks multi-tenant/multi-session scenarios.
- **Action**: Propagate actual context_id from task creation params through the storage layer.

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

### 6. Unsafe integer casts
- **Location**: `a2a-rs/src/domain/core/task.rs:402` — `history_length.unwrap() as usize`
- **Location**: `a2a-rs/src/domain/protocols/json_rpc.rs:41` — `v as i32` (i64 to i32)
- **Problem**: Silent truncation/overflow on edge-case values.
- **Action**: Use `TryFrom` or add bounds checking.

### 7. Silent timestamp fallback
- **Location**: `a2a-rs/src/adapter/storage/sqlx_storage.rs:670`
- **Problem**: `DateTime::from_timestamp_millis().unwrap_or(Utc::now())` — invalid timestamps silently become "now" instead of returning an error.
- **Action**: Return an error or log a warning for invalid stored timestamps.

### 8. Missing test coverage for several crates
- **a2a-client**: No unit tests (only doctests)
- **a2a-agents-common**: Inline tests only, no integration tests
- **a2a-agent-reimbursement**: Handler tests only, no integration/e2e tests
- **Action**: Add test suites, especially for client HTTP/WebSocket operations and agent lifecycle.

### 9. Several doctests marked `ignore`
- **Locations**: `a2a-agents/src/lib.rs`, `a2a-agents/src/core/mod.rs`, `a2a-client/src/lib.rs`, `a2a-client/src/components/mod.rs`
- **Problem**: Doctests reference APIs that have evolved (e.g., `Message::builder().text()`, `my_handler` placeholder). They were marked `ignore` to fix compilation but should be rewritten as proper compilable examples.
- **Action**: Update doctests to use the current API with concrete types.

### 10. `stock_agent_types.rs` is a dead example
- **Location**: `a2a-agents/examples/stock_agent_types.rs`
- **Problem**: References `yfinance_rs` crate that isn't a dependency. It's a types-only file, not a runnable example. Currently has a dummy `main()`.
- **Action**: Either add `yfinance_rs` as an optional dependency or remove/rename this file to a non-example location.

---

## Low Priority

### 11. TODO comments in production code
- `a2a-rs/src/adapter/business/agent_info.rs:97` — "TODO: Implement SecurityScheme integration"
- `a2a-rs/src/adapter/mod.rs:19` — "TODO: Remove these in a future version" (legacy re-exports)
- `a2a-rs/src/adapter/storage/sqlx_storage.rs` — Multiple TODOs about database detection
- **Action**: Resolve or convert to tracked issues.

### 12. `a2a-agents` depends on `a2a-rs/full` feature
- **Location**: `a2a-agents/Cargo.toml:11`
- **Problem**: Pulls in all features (SQLite, PostgreSQL, auth, all transports) even when agents may only need HTTP. Increases compile times.
- **Action**: Create a more granular feature set (e.g., `a2a-rs/agent-framework`) or let consumers choose.

### 13. Regex compilation at runtime
- **Locations**: `a2a-agents/src/utils/parsing.rs` (lazy_static), `a2a-agents/src/core/config.rs:489`
- **Problem**: Minor — regexes compiled on first use with `.unwrap()`. Safe since patterns are static, but could use `once_cell::sync::Lazy` instead of `lazy_static`.
- **Action**: Consider migrating to `std::sync::LazyLock` (stable in Rust 1.80+).

### 14. `a2a-mcp/rust-sdk` embedded git repository
- **Location**: `a2a-mcp/rust-sdk/`
- **Problem**: Nested git repo causes warnings on `git add -A`. Won't be cloned properly by consumers.
- **Action**: Add to `.gitignore`, convert to git submodule, or vendor properly.

### 15. Webhook URL validation incomplete
- **Problem**: Push notification webhook URLs are validated for format but not for reachability or allowed schemes.
- **Action**: Consider restricting to HTTPS-only in production mode.

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
