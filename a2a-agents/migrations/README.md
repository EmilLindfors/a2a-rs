# Reimbursement Agent Database Migrations

This directory contains database migrations for the reimbursement agent's persistent storage.

## Running Migrations

### SQLite

1. Create the database file if it doesn't exist:
```bash
touch reimbursement.db
```

2. Run the migrations:
```bash
sqlite3 reimbursement.db < migrations/001_create_reimbursements.sql
```

### PostgreSQL

1. Create the database:
```bash
createdb reimbursement_agent
```

2. Run the migrations:
```bash
psql -d reimbursement_agent -f migrations/001_create_reimbursements.sql
```

## Rollback

To rollback a migration:

### SQLite
```bash
sqlite3 reimbursement.db < migrations/001_create_reimbursements_rollback.sql
```

### PostgreSQL
```bash
psql -d reimbursement_agent -f migrations/001_create_reimbursements_rollback.sql
```

## Migration Files

- `001_create_reimbursements.sql` - Creates the core reimbursement tables:
  - `reimbursement_requests` - Main requests table
  - `receipts` - Receipt file metadata and extracted data
  - `approval_workflow` - Approval workflow tracking

## Using with SQLx

The a2a-rs framework's SqlxTaskStorage will automatically manage the base task tables. These migrations only create the reimbursement-specific tables that extend the base functionality.

When using SqlxTaskStorage, make sure to:

1. Run the a2a-rs base migrations first (handled automatically by SqlxTaskStorage)
2. Run these reimbursement-specific migrations
3. Configure your server to use SQLx storage:

```json
{
  "storage": {
    "type": "sqlx",
    "url": "sqlite://reimbursement.db",
    "max_connections": 5,
    "enable_logging": true
  }
}
```