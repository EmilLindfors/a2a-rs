//! Storage adapter implementations

#[cfg(feature = "server")]
pub mod task_storage;

#[cfg(feature = "server")]
pub use task_storage::InMemoryTaskStorage;
