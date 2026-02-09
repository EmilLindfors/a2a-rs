//! Test ZAI - A2A Agent Testing with ZAI and Multi-Provider LLM Support
//!
//! This library provides tools for testing A2A agent patterns with
//! the genai crate and CONSTRUCT-based prompt generation.

pub mod construct_gen;

pub use construct_gen::{
    AgentPersona, ConstraintSpec, ConstructGenerator, ToolSpec,
};
