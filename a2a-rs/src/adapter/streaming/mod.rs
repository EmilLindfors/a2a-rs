//! Streaming adapters: real-time fan-out of task updates to subscribers.
//!
//! This is the technical-concern bucket for the [`AsyncStreamingHandler`] port
//! (`.claude/rules/hexagonal_architecture.md` §3). It holds the in-process
//! subscriber registry — distinct from the storage adapters, which are
//! persistence-only and do not fan out updates.
//!
//! The ids a stream is resumed by, and the events it replays, come from an
//! [`AsyncEventLog`] the fan-out is built over, so durability is a matter of
//! which log it was given.
//!
//! [`AsyncStreamingHandler`]: crate::port::AsyncStreamingHandler
//! [`AsyncEventLog`]: crate::port::AsyncEventLog

mod fanout;

pub use fanout::{InMemoryStreamingHandler, StreamingFanout};
