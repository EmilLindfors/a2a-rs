//! Transport adapters for A2A and RMCP protocols

mod rmcp_to_a2a;
mod a2a_to_rmcp;

pub use rmcp_to_a2a::RmcpToA2aTransport;
pub use a2a_to_rmcp::A2aToRmcpTransport;