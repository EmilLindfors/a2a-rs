mod error;
mod ws;

pub use error::WebSocketClientError;
pub use ws::{A2AClientImpl, WasmWebSocketClient};
