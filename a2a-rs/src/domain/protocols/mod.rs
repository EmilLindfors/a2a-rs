//! Protocol-specific types and implementations

pub mod json_rpc;

pub use json_rpc::{
    JSONRPCError, JSONRPCMessage, JSONRPCNotification, JSONRPCRequest, JSONRPCResponse,
};
