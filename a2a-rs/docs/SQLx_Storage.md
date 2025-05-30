# SQLx Storage Implementation

The a2a-rs library now includes a persistent storage alternative to the in-memory storage using SQLx, supporting SQLite, PostgreSQL, and MySQL databases.

## Features

- **Persistent task storage** - Tasks survive application restarts
- **Multi-process support** - Multiple processes can share the same database
- **ACID transactions** - Ensures data consistency
- **Automatic migrations** - Database schema is set up automatically
- **Database flexibility** - Supports SQLite, PostgreSQL, and MySQL
- **Push notification persistence** - Notification configurations are stored in the database

## Setup

### Dependencies

Add the SQLx storage feature to your `Cargo.toml`:

```toml
[dependencies]
a2a-rs = { version = "0.1", features = ["sqlite"] }  # For SQLite
# or
a2a-rs = { version = "0.1", features = ["postgres"] }  # For PostgreSQL  
# or
a2a-rs = { version = "0.1", features = ["mysql"] }     # For MySQL
```

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
2. **Database Maintenance**: Regular vacuuming/optimization for SQLite, standard maintenance for PostgreSQL/MySQL
3. **Monitoring**: Enable query logging during development with `enable_logging: true`
4. **Backup Strategy**: Implement regular database backups for production deployments
5. **Migration Strategy**: The current implementation runs migrations on startup - consider external migration tools for production

## Limitations

1. **Single Database Type**: Currently optimized for SQLite, PostgreSQL and MySQL support is experimental
2. **Schema Evolution**: No migration strategy for schema changes beyond the initial setup
3. **History Loading**: Full history reconstruction from database is not fully implemented
4. **Concurrent Access**: While ACID-compliant, high-concurrency scenarios may need additional optimization

## Testing

Run the SQLx storage tests:

```bash
cargo test --test sqlx_storage_test --features sqlite
```

The tests cover:
- Task lifecycle operations
- Concurrent access patterns
- Push notification persistence
- Database migrations
- Error handling scenarios