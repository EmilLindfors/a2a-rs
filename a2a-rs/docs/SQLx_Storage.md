# SQLx Storage Implementation

`SqlxTaskStorage` persists tasks, history, push configs and conversations to
**SQLite or PostgreSQL**. The backend follows from the URL scheme at runtime, so
one binary serves either.

## Features

- **Persistent task storage** - Tasks survive application restarts
- **Multi-process support** - Multiple processes can share the same database
- **ACID transactions** - Ensures data consistency
- **Automatic migrations** - Database schema is set up automatically, and the
  migrations re-run safely on every start (on PostgreSQL under an advisory lock,
  so several agents starting at once is fine)
- **Push notification persistence** - Notification configurations are stored in the database

## Setup

### Dependencies

Add the backend you need to your `Cargo.toml`:

```toml
[dependencies]
a2a-rs = { version = "0.6", features = ["sqlite"] }               # SQLite
a2a-rs = { version = "0.6", features = ["postgres"] }             # PostgreSQL
a2a-rs = { version = "0.6", features = ["sqlite", "postgres"] }   # decided by the URL
```

MySQL is not supported. A `mysql:` URL is recognized so the error can say so
rather than failing on an unknown scheme; there was a `mysql` cargo feature until
0.6, and all it did was compile a driver nothing used.

### Which backend the URL picks

| URL | Backend |
|---|---|
| `sqlite::memory:` | SQLite, one database per store, gone when it is dropped |
| `sqlite:tasks.db` | SQLite file |
| `postgres://user:pass@host/db` | PostgreSQL |

The schema is written twice, once per dialect, under `migrations/sqlite/` and
`migrations/postgres/` — identity columns, timestamp defaults and the
`updated_at` triggers cannot be spelled the same way. The *queries* are written
once and the differences (parameter placeholders, the two upserts, the timestamp
comparison) live in one internal `Dialect` type, so the two backends cannot
drift apart the way two copies of a store would.

JSON payloads are stored as text on both, including PostgreSQL. The store reads
and writes them as serialized strings and never queries into them, and one query
path across both backends is worth more than `jsonb` operators nothing uses.

### Database Configuration

Use the `DatabaseConfig` builder to configure your database connection:

```rust
use a2a_rs::adapter::storage::{DatabaseConfig, SqlxTaskStorage};

// SQLite in-memory (for testing)
let config = DatabaseConfig::default();

// SQLite file
let config = DatabaseConfig::builder()
    .url("sqlite:tasks.db".to_string())
    .max_connections(5)
    .build();

// PostgreSQL
let config = DatabaseConfig::builder()
    .url("postgres://user:password@localhost/myapp".to_string())
    .max_connections(20)
    .timeout_seconds(10)
    .build();

// From environment variables
let config = DatabaseConfig::from_env()?;
```

`SqlxTaskStorage::new` takes a URL and everything else at its default. To size
the pool, turn statement logging on, swap the push sender, or run your own
migrations, open the store through its builder:

```rust
use a2a_rs::adapter::storage::{SqlxStorageBuilder, SqlxTaskStorage};
use std::time::Duration;

let storage = SqlxTaskStorage::builder("postgres://user:pass@localhost/a2a")
    .max_connections(20)
    .acquire_timeout(Duration::from_secs(10))
    .log_statements(true)
    .migrations([include_str!("../migrations/001_my_agent.sql")])
    .connect()
    .await?;

// Or take the pool settings from a `DatabaseConfig`:
let storage = SqlxStorageBuilder::from_config(&config).connect().await?;
```

Unset, the pool is sized as sqlx sizes it: 10 connections, a 30-second acquire
timeout. Statement logging is off unless asked for — sqlx logs every statement
at `DEBUG` by default, and this crate makes that a choice.

### Environment Variables

Set these environment variables for automatic configuration:

- `DATABASE_URL` - Required, the database connection URL
- `DATABASE_MAX_CONNECTIONS` - Optional, defaults to 10
- `DATABASE_TIMEOUT_SECONDS` - Optional, defaults to 30
- `DATABASE_ENABLE_LOGGING` - Optional, defaults to false

## Usage

### Basic Usage

```rust
use a2a_rs::adapter::storage::SqlxTaskStorage;
use a2a_rs::port::AsyncTaskManager;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create storage instance
    let storage = SqlxTaskStorage::new("sqlite::memory:").await?;
    
    // Create a task
    let task = storage.create_task("my-task-1", "my-context").await?;
    
    // Update task status
    storage.update_task_status("my-task-1", TaskState::Working).await?;
    
    // Retrieve task with history
    let task_with_history = storage.get_task("my-task-1", Some(10)).await?;
    
    Ok(())
}
```

### Replacing InMemoryTaskStorage

The SQLx storage implements the same traits as the in-memory storage, making it a drop-in replacement:

```rust
// Before (in-memory)
let storage = InMemoryTaskStorage::new();

// After (persistent)
let storage = SqlxTaskStorage::new("sqlite:tasks.db").await?;
```

## Database Schema

The SQLx storage automatically creates the following tables:

### `tasks` table
- `id` (TEXT, PRIMARY KEY) - Task identifier
- `context_id` (TEXT) - Task context
- `status_state` (TEXT) - Current task state
- `status_message` (TEXT) - Optional status message
- `created_at` (TIMESTAMP) - Creation timestamp
- `updated_at` (TIMESTAMP) - Last update timestamp
- `metadata` (JSON) - Task metadata
- `artifacts` (JSON) - Task artifacts

### `task_history` table
- `id` (INTEGER, PRIMARY KEY) - History entry ID
- `task_id` (TEXT) - References tasks.id
- `timestamp` (TIMESTAMP) - History entry timestamp
- `status_state` (TEXT) - Task state at this point
- `message` (JSON) - Message associated with this history entry

### `push_notification_configs` table
- `task_id` (TEXT, PRIMARY KEY) - References tasks.id
- `webhook_url` (TEXT) - Push notification webhook URL
- `created_at` (TIMESTAMP) - Configuration creation timestamp

## Examples

### Run the SQLx Storage Demo

```bash
# SQLite in-memory
cargo run --example sqlx_storage_demo --features sqlite

# SQLite file
DATABASE_URL=sqlite:tasks.db cargo run --example sqlx_storage_demo --features sqlite

# PostgreSQL (requires running PostgreSQL server)
DATABASE_URL=postgres://user:password@localhost/a2a_test cargo run --example sqlx_storage_demo --features postgres
```

### Compare Storage Implementations

```bash
cargo run --example storage_comparison --features sqlite
```

## Performance Characteristics

Based on benchmarks:

- **InMemory Storage**: ~0.6ms for 100 operations
- **SQLx Storage**: ~480ms for 100 operations (800x slower)

The SQLx storage is optimized for data persistence and consistency rather than raw performance. For high-throughput scenarios where persistence isn't required, consider using the in-memory storage.

## Production Considerations

1. **Connection Pooling**: Configure appropriate `max_connections` based on your workload
2. **Database Maintenance**: Regular vacuuming/optimization for SQLite, standard maintenance for PostgreSQL
3. **Monitoring**: Enable query logging during development with `enable_logging: true`
4. **Backup Strategy**: Implement regular database backups for production deployments
5. **Migration Strategy**: The current implementation runs migrations on startup - consider external migration tools for production

## Limitations

1. **Schema Evolution**: No migration strategy for schema changes beyond the initial setup
2. **Concurrent Access**: While ACID-compliant, high-concurrency scenarios may need additional optimization

## Testing

The SQLite tests need nothing:

```bash
cargo test -p a2a-rs --test sqlx_storage_test --features sqlite
```

The PostgreSQL tests need a server, and skip without one:

```bash
docker run --rm -e POSTGRES_PASSWORD=a2a -e POSTGRES_DB=a2a -p 5432:5432 postgres:17
A2A_TEST_POSTGRES_URL=postgres://postgres:a2a@localhost/a2a \
  cargo test -p a2a-rs --features full --test postgres_storage_test
```

They cover what can only differ by backend — the schema, the two upserts, the
placeholder rewrite, the timestamp comparison, and every column the driver has
to decode. The rest of the storage behaviour is the same code and is tested on
SQLite. CI runs both.

The tests cover:
- Task lifecycle operations
- Concurrent access patterns
- Push notification persistence
- Database migrations
- Error handling scenarios