//! Streaming adapters: real-time fan-out of task updates to subscribers.
//!
//! This is the technical-concern bucket for the [`AsyncStreamingHandler`] port
//! (`.claude/rules/hexagonal_architecture.md` §3). It holds the in-process
//! subscriber registry that, after the Phase 4 struct-split, is no longer the
//! storage adapters' responsibility.
//!
//! [`AsyncStreamingHandler`]: crate::port::AsyncStreamingHandler

mod in_memory;

pub use in_memory::InMemoryStreamingHandler;
