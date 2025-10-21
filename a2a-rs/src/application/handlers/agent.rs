use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::AgentCard;

/// Empty params type for agent/getExtendedCard
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GetExtendedCardParams {}

/// Request to get an extended agent card (v0.3.0)
///
/// This method returns an extended version of the agent card that may include
/// sensitive information only available to authenticated clients. The response
/// is identical to the regular agent card structure but may contain additional
/// fields or data that are not exposed in the public agent card endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetExtendedCardRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: GetExtendedCardParams,
}

impl GetExtendedCardRequest {
    pub fn new() -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::String(uuid::Uuid::new_v4().to_string())),
            method: "agent/getExtendedCard".to_string(),
            params: GetExtendedCardParams::default(),
        }
    }

    pub fn with_id(mut self, id: Value) -> Self {
        self.id = Some(id);
        self
    }
}

impl Default for GetExtendedCardRequest {
    fn default() -> Self {
        Self::new()
    }
}

/// Response to a get extended card request (v0.3.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetExtendedCardResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<AgentCard>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::domain::protocols::JSONRPCError>,
}

impl GetExtendedCardResponse {
    pub fn success(id: Option<Value>, card: AgentCard) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(card),
            error: None,
        }
    }

    pub fn error(id: Option<Value>, error: crate::domain::protocols::JSONRPCError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

// ===== v0.3.0 New API Methods =====

/// Empty params type for agent/getAuthenticatedExtendedCard (v0.3.0)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GetAuthenticatedExtendedCardParams {}

/// Request to get an authenticated extended agent card (v0.3.0)
///
/// This method returns an extended version of the agent card that includes
/// authenticated-only information. Clients must be authenticated to call this method.
/// If the agent doesn't support authenticated extended cards, it will return
/// an AuthenticatedExtendedCardNotConfigured error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetAuthenticatedExtendedCardRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: GetAuthenticatedExtendedCardParams,
}

impl GetAuthenticatedExtendedCardRequest {
    pub fn new() -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::String(uuid::Uuid::new_v4().to_string())),
            method: "agent/getAuthenticatedExtendedCard".to_string(),
            params: GetAuthenticatedExtendedCardParams::default(),
        }
    }

    pub fn with_id(mut self, id: Value) -> Self {
        self.id = Some(id);
        self
    }
}

impl Default for GetAuthenticatedExtendedCardRequest {
    fn default() -> Self {
        Self::new()
    }
}

/// Response to a get authenticated extended card request (v0.3.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetAuthenticatedExtendedCardResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<AgentCard>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::domain::protocols::JSONRPCError>,
}

impl GetAuthenticatedExtendedCardResponse {
    pub fn success(id: Option<Value>, card: AgentCard) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(card),
            error: None,
        }
    }

    pub fn error(id: Option<Value>, error: crate::domain::protocols::JSONRPCError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}
