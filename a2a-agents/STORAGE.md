# Storage Configuration

The reimbursement agent supports both in-memory and persistent storage options.

## In-Memory Storage (Default)

For development and testing, the agent uses in-memory storage by default:

```bash
# Default - uses in-memory storage
cargo run --bin reimbursement_server

# Or explicitly with environment variable
DATABASE_URL="" cargo run --bin reimbursement_server
```

## SQLx Persistent Storage

For production use, enable SQLx storage with the `sqlx` feature:

```bash
# Enable sqlx feature and specify database
cargo run --bin reimbursement_server --features sqlx
```

### Environment Variables

Configure storage using environment variables:

```bash
# SQLite (file-based)
export DATABASE_URL="sqlite:reimbursement_tasks.db"

# SQLite (in-memory, for testing)
export DATABASE_URL="sqlite::memory:"

# PostgreSQL
export DATABASE_URL="postgres://username:password@localhost/reimbursement_db"

# Optional configuration
export DATABASE_MAX_CONNECTIONS=10
export DATABASE_ENABLE_LOGGING=true
```

### Configuration File

Create a JSON configuration file:

```json
{
  "host": "127.0.0.1",
  "http_port": 8080,
  "ws_port": 8081,
  "storage": {
    "type": "Sqlx",
    "url": "sqlite:reimbursement_tasks.db",
    "max_connections": 10,
    "enable_logging": false
  }
}
```

Then run with:

```bash
cargo run --bin reimbursement_server --features sqlx -- --config config.json
```

## Database Schema

When using SQLx storage, the following tables are automatically created:

- **tasks**: Stores task metadata, status, and artifacts
- **task_history**: Chronological task state changes and messages  
- **push_notification_configs**: Webhook configurations per task

## Examples

### Development (In-memory)
```bash
cargo run --bin reimbursement_server
```

### Production SQLite
```bash
DATABASE_URL="sqlite:production.db" \
cargo run --bin reimbursement_server --features sqlx
```

### Production PostgreSQL
```bash
DATABASE_URL="postgres://user:pass@localhost/a2a_prod" \
DATABASE_MAX_CONNECTIONS=20 \
cargo run --bin reimbursement_server --features sqlx
```

### Using Config File
```bash
cargo run --bin reimbursement_server --features sqlx -- --config config.sqlx.example.json
```

## Benefits of SQLx Storage

- **Persistence**: Tasks survive server restarts
- **Multi-process**: Multiple server instances can share the database
- **ACID transactions**: Database-level consistency
- **History tracking**: Automatic task state history
- **Scalability**: Handles larger datasets than memory