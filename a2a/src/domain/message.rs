use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::domain::error::A2AError;

/// Roles in agent communication
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Agent,
}

/// File content representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "mimeType")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<String>, // Base64 encoded
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
}

impl FileContent {
    /// Validates that the file content is properly specified
    pub fn validate(&self) -> Result<(), A2AError> {
        match (&self.bytes, &self.uri) {
            (Some(_), None) | (None, Some(_)) => Ok(()),
            (Some(_), Some(_)) => Err(A2AError::InvalidParams("Cannot provide both bytes and uri".to_string())),
            (None, None) => Err(A2AError::InvalidParams("Must provide either bytes or uri".to_string())),
        }
    }
}

/// Parts that can make up a message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Part {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<Map<String, Value>>,
    },
    #[serde(rename = "file")]
    File {
        file: FileContent,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<Map<String, Value>>,
    },
    #[serde(rename = "data")]
    Data {
        data: Map<String, Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<Map<String, Value>>,
    },
}

/// A message in the A2A protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub parts: Vec<Part>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

/// An artifact produced by an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parts: Vec<Part>,
    #[serde(default)]
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub append: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "lastChunk")]
    pub last_chunk: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

/// Helper methods for creating parts
impl Part {
    /// Create a text part
    pub fn text(content: String) -> Self {
        Part::Text {
            text: content,
            metadata: None,
        }
    }

    /// Create a text part with metadata
    pub fn text_with_metadata(content: String, metadata: Map<String, Value>) -> Self {
        Part::Text {
            text: content,
            metadata: Some(metadata),
        }
    }

    /// Create a data part
    pub fn data(data: Map<String, Value>) -> Self {
        Part::Data {
            data,
            metadata: None,
        }
    }

    /// Create a file part from base64 encoded data
    pub fn file_from_bytes(
        bytes: String,
        name: Option<String>,
        mime_type: Option<String>,
    ) -> Self {
        Part::File {
            file: FileContent {
                name,
                mime_type,
                bytes: Some(bytes),
                uri: None,
            },
            metadata: None,
        }
    }

    /// Create a file part from a URI
    pub fn file_from_uri(
        uri: String,
        name: Option<String>,
        mime_type: Option<String>,
    ) -> Self {
        Part::File {
            file: FileContent {
                name,
                mime_type,
                bytes: None,
                uri: Some(uri),
            },
            metadata: None,
        }
    }
}

/// Helper methods for creating messages
impl Message {
    /// Create a new user message with a single text part
    pub fn user_text(text: String) -> Self {
        Self {
            role: Role::User,
            parts: vec![Part::text(text)],
            metadata: None,
        }
    }

    /// Create a new agent message with a single text part
    pub fn agent_text(text: String) -> Self {
        Self {
            role: Role::Agent,
            parts: vec![Part::text(text)],
            metadata: None,
        }
    }

    /// Add a part to this message
    pub fn add_part(&mut self, part: Part) {
        self.parts.push(part);
    }
}