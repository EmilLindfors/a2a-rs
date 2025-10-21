//! Example demonstrating SQLx-based persistent storage
//!
//! This example shows how to use the SqlxTaskStorage with different databases:
//! - SQLite (file and in-memory)
//! - PostgreSQL
//!
//! Run with:
//! ```bash
//! # SQLite in-memory (default)
//! cargo run --example sqlx_storage_demo --features sqlite
//!
//! # SQLite file
//! DATABASE_URL=sqlite:tasks.db cargo run --example sqlx_storage_demo --features sqlite
//!
//! # PostgreSQL (requires running PostgreSQL server)
//! DATABASE_URL=postgres://user:password@localhost/a2a_test cargo run --example sqlx_storage_demo --features postgres
//! ```

#[cfg(feature = "sqlx-storage")]
use a2a_rs::adapter::storage::{DatabaseConfig, SqlxTaskStorage};
#[cfg(feature = "sqlx-storage")]
use a2a_rs::domain::TaskState;
#[cfg(feature = "sqlx-storage")]
use a2a_rs::port::AsyncTaskManager;

#[cfg(feature = "sqlx-storage")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("🗃️  SQLx Storage Demo");
    println!("=====================");

    // Get database configuration from environment or use default
    let config = match DatabaseConfig::from_env() {
        Ok(config) => {
            println!("📡 Using database configuration from environment");
            config
        }
        Err(_) => {
            println!("📋 Using default configuration (SQLite in-memory)");
            DatabaseConfig::default()
        }
    };

    // Validate configuration
    config
        .validate()
        .map_err(|e| format!("Invalid database configuration: {}", e))?;

    println!("🔧 Database type: {}", config.database_type());
    println!("🔗 Database URL: {}", config.url);
    println!("📊 Max connections: {}", config.max_connections);
    println!();

    // Create storage instance
    println!("🚀 Initializing SQLx storage...");
    let storage = SqlxTaskStorage::new(&config.url).await?;
    println!("✅ Storage initialized successfully");
    println!();

    // Demo: Create tasks
    println!("📝 Creating tasks...");
    let task_ids = vec!["demo-task-1", "demo-task-2", "demo-task-3"];

    for task_id in &task_ids {
        let task = storage.create_task(task_id, "demo-context").await?;
        println!(
            "  ✓ Created task: {} (status: {:?})",
            task.id, task.status.state
        );
    }
    println!();

    // Demo: Update task statuses
    println!("🔄 Updating task statuses...");
    storage
        .update_task_status("demo-task-1", TaskState::Working, None)
        .await?;
    println!("  ✓ Updated demo-task-1 to Working");

    storage
        .update_task_status("demo-task-2", TaskState::Working, None)
        .await?;
    storage
        .update_task_status("demo-task-2", TaskState::Completed, None)
        .await?;
    println!("  ✓ Updated demo-task-2 to Working, then Completed");

    storage
        .update_task_status("demo-task-3", TaskState::Working, None)
        .await?;
    println!("  ✓ Updated demo-task-3 to Working");
    println!();

    // Demo: Cancel a task
    println!("❌ Canceling a task...");
    let canceled_task = storage.cancel_task("demo-task-3").await?;
    println!(
        "  ✓ Canceled task: {} (status: {:?})",
        canceled_task.id, canceled_task.status.state
    );
    println!();

    // Demo: Retrieve tasks and show history
    println!("📖 Retrieving tasks with history...");
    for task_id in &task_ids {
        let task = storage.get_task(task_id, Some(10)).await?;
        println!("  📋 Task: {} (status: {:?})", task.id, task.status.state);

        if let Some(history) = &task.history {
            println!("     History entries: {}", history.len());

            for (i, message) in history.iter().enumerate() {
                println!("       {}. Message ID: {}", i + 1, message.message_id);
            }
        } else {
            println!("     No history available");
        }
        println!();
    }

    // Demo: Task existence checks
    println!("🔍 Checking task existence...");
    for task_id in &task_ids {
        let exists = storage.task_exists(task_id).await?;
        println!("  {} exists: {}", task_id, exists);
    }

    let exists = storage.task_exists("non-existent-task").await?;
    println!("  non-existent-task exists: {}", exists);
    println!();

    // Demo: Show configuration examples
    println!("📚 Available configuration examples:");
    let examples = DatabaseConfig::examples();
    for (name, example_config) in examples {
        println!("  {} -> {}", name, example_config.url);
    }
    println!();

    println!("✅ Demo completed successfully!");
    println!();
    println!("💡 Tips:");
    println!("   - Set DATABASE_URL environment variable to use a different database");
    println!("   - Use 'sqlite:tasks.db' for persistent SQLite storage");
    println!("   - Use 'postgres://user:pass@host/db' for PostgreSQL");
    println!("   - Task history is automatically tracked in the database");
    println!("   - The storage layer handles migrations automatically");

    Ok(())
}

#[cfg(not(feature = "sqlx-storage"))]
fn main() {
    eprintln!("❌ This example requires the 'sqlx-storage' feature.");
    eprintln!("Run with: cargo run --example sqlx_storage_demo --features sqlite");
    std::process::exit(1);
}
