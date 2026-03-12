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

### ~~4. `num-bigint-dig v0.8.4` future incompatibility~~ — PARTIALLY RESOLVED
- **Cause**: Transitive dependency via `oauth2` crate
- Upgraded `oauth2` from 4.4 to 5.0 and `openidconnect` from 3.5 to 4.0
- Upgraded `reqwest` from 0.11 to 0.12 to match oauth2 5.0's dependency (eliminates dual-version)
- Adapted `OAuth2Authenticator` and `OpenIdConnectAuthenticator` for new type-state API
- Warning still present: `num-bigint-dig v0.8.4` remains a transitive dep; awaiting upstream fix

---

## Medium Priority

### ~~5. Database type detection not implemented~~ — RESOLVED
- Added `DatabaseType` enum with `from_url()` parsing in `database_config.rs`
- `SqlxTaskStorage` constructors now validate the URL scheme and reject non-SQLite URLs with a clear error message
- `DatabaseConfig::database_type()` returns `Option<DatabaseType>` instead of `&str`
- `DatabaseConfig::validate_database_support()` checks both URL recognition and feature flag enablement
- Created PostgreSQL migration `002_v030_push_configs_postgres.sql` for future use
- PostgreSQL initial schema migration already existed at `001_initial_schema_postgres.sql`

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

### ~~9. Several doctests marked `ignore`~~ — PARTIALLY RESOLVED
- Fixed 9 doctests from `ignore` to compilable (`rust`) or type-checked (`no_run`):
  - `agent_info.rs:106` — SecurityScheme example now compiles
  - `a2a-agents/src/lib.rs:45` — AgentPlugin example now compiles with AsyncMessageHandler impl
  - `a2a-client/src/components/mod.rs:17` — TaskView/MessageView example now compiles with correct API
  - `a2a-client/src/lib.rs` — WebSocket, auto_connect, and AppState examples now compile or type-check
- 5 doctests remain `ignore` (require TOML files on disk, complex Axum SSE types, or external handler types)

### ~~10. `stock_agent_types.rs` is a dead example~~ — RESOLVED
- File removed from `a2a-agents/examples/`.

---

## Low Priority

### ~~11. TODO comments in production code~~ — RESOLVED
- SecurityScheme TODO resolved by implementing `with_security_schemes()`
- Legacy re-export TODO reworded to non-TODO comment
- SQLx migration TODOs reworded to documented limitation

### ~~12. `a2a-agents` depends on `a2a-rs/full` feature~~ — RESOLVED
- Changed `a2a-agents/Cargo.toml` from `a2a-rs/full` to `a2a-rs` with granular features: `server`, `http-server`, `ws-server`, `http-client`, `tracing`
- SQLite and auth are now opt-in via `a2a-agents` feature flags (`sqlx`, `auth`)

### ~~13. Regex compilation at runtime~~ — RESOLVED
- Migrated all `lazy_static!` to `std::sync::LazyLock` in `a2a-agents/src/utils/parsing.rs`, `a2a-agents/src/core/config.rs`, and `a2a-agents-common/src/nlp/entity.rs`. Removed `lazy_static` dependency from both crates.

### ~~14. `a2a-mcp/rust-sdk` embedded git repository~~ — RESOLVED
- Added `a2a-mcp/rust-sdk/` to `.gitignore` to prevent tracking the nested git repo

### ~~15. Webhook URL validation incomplete~~ — RESOLVED
- Added HTTPS-only validation to `validate_push_notification_url()` in both sync and async notification manager traits. HTTP is only allowed for `localhost`/`127.0.0.1`/`::1` (development).

---

## Resolved (prior sessions)

- [x] Lifetime mismatch in `tests/common/test_handler.rs` (`&'a` on trait impl methods)
- [x] `test_task_list_include_artifacts` asserting wrong variable
- [x] `stock_agent_types.rs` referencing missing `yfinance_rs` dependency
- [x] Failing doctests in `a2a-agent-reimbursement` (`ReimbursementHandler::new()` signature)
- [x] Failing doctests in `a2a-agents` (undefined `my_handler`, incomplete `AgentPlugin` impl)
- [x] Failing doctests in `a2a-agents-common` (table format assertion, entity regex pattern)
- [x] Failing doctests in `a2a-client` (outdated API references)
- [x] All workspace tests now pass: `cargo test --workspace` green
- [x] Issue #4: Upgraded oauth2 4.4→5.0, openidconnect 3.5→4.0, reqwest 0.11→0.12
- [x] Issue #5: DatabaseType enum, URL validation, PostgreSQL migration
- [x] Issue #9: 9 doctests fixed from `ignore` to compilable
- [x] Issue #12: Granular feature flags for a2a-agents dependency on a2a-rs
- [x] Issue #14: `.gitignore` entry for `a2a-mcp/rust-sdk/`
