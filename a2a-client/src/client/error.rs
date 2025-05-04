use a2a_rs::domain::A2AError;
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum WebSocketClientError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Message error: {0}")]
    Message(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Connection closed")]
    Closed,

    #[error("Operation timed out")]
    Timeout,

    #[error("JavaScript error: {0}")]
    JsError(String),
}

impl From<WebSocketClientError> for A2AError {
    fn from(error: WebSocketClientError) -> Self {
        A2AError::Internal(error.to_string())
    }
}

impl From<wasm_bindgen::JsValue> for WebSocketClientError {
    fn from(value: wasm_bindgen::JsValue) -> Self {
        Self::JsError(format!("{:?}", value))
    }
}
