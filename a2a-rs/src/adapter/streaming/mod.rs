//! Streaming adapters: real-time fan-out of task updates to subscribers.
//!
//! This is the technical-concern bucket for the [`AsyncStreamingHandler`] port
//! (`.claude/rules/hexagonal_architecture.md` §3). It holds the in-process
//! subscriber registry — distinct from the storage adapters, which are
//! persistence-only and do not fan out updates.
//!
//! [`AsyncStreamingHandler`]: crate::port::AsyncStreamingHandler

mod in_memory;

pub use in_memory::InMemoryStreamingHandler;
