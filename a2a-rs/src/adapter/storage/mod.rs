//! Storage adapter implementations

#[cfg(feature = "server")]
pub mod task_storage;

#[cfg(feature = "sqlx-storage")]
pub mod sqlx_storage;

#[cfg(feature = "sqlx-storage")]
pub mod database_config;

/// Where the two SQL backends disagree. Internal to the storage adapter: which
/// dialect is in use follows from the URL, and nothing outside picks one.
#[cfg(feature = "sqlx-storage")]
mod dialect;

#[cfg(feature = "server")]
pub use task_storage::InMemoryTaskStorage;

#[cfg(feature = "sqlx-storage")]
pub use sqlx_storage::{SqlxStorageBuilder, SqlxTaskStorage};

#[cfg(feature = "sqlx-storage")]
pub use database_config::{DatabaseConfig, DatabaseType};
