use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::error::A2AError;

/// Standard JSON-RPC 2.0 message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCMessage {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
}

impl Default for JSONRPCMessage {
    fn default() -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: None,
        }
    }
}

/// JSON-RPC 2.0 error object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl From<A2AError> for JSONRPCError {
    fn from(error: A2AError) -> Self {
        let value = error.to_jsonrpc_error();

        // Extract the fields from the JSON value
        if let Value::Object(map) = value {
            let code = map
                .get("code")
                .and_then(|v| v.as_i64())
                .map(|v| v as i32)
                .unwrap_or(-32603); // Internal error code as fallback

            let message = map
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Internal error")
                .to_string();

            let data = map.get("data").cloned();

            Self {
                code,
                message,
                data,
            }
        } else {
            // Fallback to internal error if the JSON structure is unexpected
            Self {
                code: -32603,
                message: "Internal error".to_string(),
                data: None,
            }
        }
    }
}

/// JSON-RPC 2.0 request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JSONRPCRequest {
    /// Create a new JSON-RPC request with the given method and parameters
    pub fn new(method: String, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::String(uuid::Uuid::new_v4().to_string())),
            method,
            params,
        }
    }

    /// Create a new JSON-RPC request with the given method, parameters, and ID
    pub fn with_id(method: String, params: Option<Value>, id: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method,
            params,
        }
    }
}

/// JSON-RPC 2.0 response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JSONRPCError>,
}

impl JSONRPCResponse {
    /// Create a successful JSON-RPC response
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create an error JSON-RPC response
    pub fn error(id: Option<Value>, error: JSONRPCError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// JSON-RPC 2.0 notification (request without id)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JSONRPCNotification {
    /// Create a new JSON-RPC notification
    pub fn new(method: String, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method,
            params,
        }
    }
}
