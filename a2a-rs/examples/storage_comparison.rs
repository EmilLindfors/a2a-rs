//! Example comparing InMemory vs SQLx storage implementations
//!
//! This example demonstrates the differences and similarities between
//! the in-memory and SQLx storage implementations.
//!
//! Run with:
//! ```bash
//! # Compare with SQLite
//! cargo run --example storage_comparison --features sqlite
//!
//! # Compare with PostgreSQL (requires running PostgreSQL)
//! DATABASE_URL=postgres://user:password@localhost/a2a_test cargo run --example storage_comparison --features postgres
//! ```

use std::time::Duration;

use a2a_rs::adapter::storage::InMemoryTaskStorage;
use a2a_rs::domain::TaskState;
use a2a_rs::port::AsyncTaskManager;

#[cfg(feature = "sqlx-storage")]
use a2a_rs::adapter::storage::{DatabaseConfig, SqlxTaskStorage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("🏪 Storage Implementation Comparison");
    println!("===================================");
    println!();

    // Test with InMemory storage
    println!("🧠 Testing InMemory Storage");
    println!("---------------------------");
    let in_memory_storage = InMemoryTaskStorage::new();
    run_storage_tests(&in_memory_storage, "InMemory").await?;
    println!();

    // Test with SQLx storage if available
    #[cfg(feature = "sqlx-storage")]
    {
        println!("🗃️  Testing SQLx Storage");
        println!("-----------------------");

        let config = DatabaseConfig::from_env().unwrap_or_else(|_| {
            println!("ℹ️  Using default SQLite in-memory configuration");
            DatabaseConfig::default()
        });

        println!("📊 Database: {} ({})", config.url, config.database_type());

        let sqlx_storage = SqlxTaskStorage::new(&config.url).await?;
        run_storage_tests(&sqlx_storage, "SQLx").await?;
        println!();
    }

    #[cfg(not(feature = "sqlx-storage"))]
    {
        println!("⚠️  SQLx storage not available - compile with --features sqlite or --features postgres");
        println!();
    }

    // Performance comparison
    println!("⚡ Performance Comparison");
    println!("------------------------");

    println!("🧠 InMemory Storage Performance:");
    let in_memory_storage = InMemoryTaskStorage::new();
    let in_memory_time = measure_performance(&in_memory_storage).await?;
    println!("   Time for 100 operations: {:?}", in_memory_time);

    #[cfg(feature = "sqlx-storage")]
    {
        println!("🗃️  SQLx Storage Performance:");
        let config = DatabaseConfig::from_env().unwrap_or_default();
        let sqlx_storage = SqlxTaskStorage::new(&config.url).await?;
        let sqlx_time = measure_performance(&sqlx_storage).await?;
        println!("   Time for 100 operations: {:?}", sqlx_time);

        let ratio = sqlx_time.as_secs_f64() / in_memory_time.as_secs_f64();
        println!(
            "   SQLx is {:.1}x {} than InMemory",
            if ratio > 1.0 { ratio } else { 1.0 / ratio },
            if ratio > 1.0 { "slower" } else { "faster" }
        );
    }

    println!();

    // Summary
    println!("📋 Summary");
    println!("----------");
    println!("InMemory Storage:");
    println!("  ✅ Fast performance");
    println!("  ✅ No external dependencies");
    println!("  ❌ Data lost on restart");
    println!("  ❌ Single process only");
    println!();

    #[cfg(feature = "sqlx-storage")]
    {
        println!("SQLx Storage:");
        println!("  ✅ Persistent data");
        println!("  ✅ Multi-process support");
        println!("  ✅ ACID transactions");
        println!("  ✅ Supports SQLite, PostgreSQL, MySQL");
        println!("  ❌ Requires database setup");
        println!("  ❌ Slightly slower than in-memory");
        println!();
    }

    println!("💡 Use InMemory for development/testing, SQLx for production");

    Ok(())
}

async fn run_storage_tests<T: AsyncTaskManager>(
    storage: &T,
    storage_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let task_id = format!("test-task-{}", storage_name.to_lowercase());

    // Test 1: Create task
    let task = storage.create_task(&task_id, "test-context").await?;
    println!(
        "  ✓ Created task: {} (status: {:?})",
        task.id, task.status.state
    );

    // Test 2: Check existence
    let exists = storage.task_exists(&task_id).await?;
    println!("  ✓ Task exists: {}", exists);

    // Test 3: Update status
    let updated_task = storage
        .update_task_status(&task_id, TaskState::Working, None)
        .await?;
    println!(
        "  ✓ Updated to Working (status: {:?})",
        updated_task.status.state
    );

    // Test 4: Get task with history
    let task_with_history = storage.get_task(&task_id, Some(10)).await?;
    let history_count = task_with_history
        .history
        .as_ref()
        .map(|h| h.len())
        .unwrap_or(0);
    println!("  ✓ Retrieved task with {} history entries", history_count);

    // Test 5: Complete the task
    let completed_task = storage
        .update_task_status(&task_id, TaskState::Completed, None)
        .await?;
    println!(
        "  ✓ Completed task (status: {:?})",
        completed_task.status.state
    );

    // Test 6: Try to cancel completed task (should fail)
    match storage.cancel_task(&task_id).await {
        Ok(_) => println!("  ❌ Unexpected: was able to cancel completed task"),
        Err(_) => println!("  ✓ Correctly prevented canceling completed task"),
    }

    // Test 7: Create and cancel a working task
    let cancel_task_id = format!("cancel-test-{}", storage_name.to_lowercase());
    storage.create_task(&cancel_task_id, "test-context").await?;
    storage
        .update_task_status(&cancel_task_id, TaskState::Working, None)
        .await?;
    let canceled_task = storage.cancel_task(&cancel_task_id).await?;
    println!(
        "  ✓ Canceled working task (status: {:?})",
        canceled_task.status.state
    );

    Ok(())
}

async fn measure_performance<T: AsyncTaskManager>(
    storage: &T,
) -> Result<Duration, Box<dyn std::error::Error>> {
    let start = std::time::Instant::now();

    for i in 0..100 {
        let task_id = format!("perf-task-{}", i);

        // Create, update, and retrieve task
        storage.create_task(&task_id, "perf-context").await?;
        storage
            .update_task_status(&task_id, TaskState::Working, None)
            .await?;
        storage
            .update_task_status(&task_id, TaskState::Completed, None)
            .await?;
        storage.get_task(&task_id, Some(5)).await?;
    }

    Ok(start.elapsed())
}
