use serde::{Deserialize, Serialize, Deserializer};
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
#[derive(Debug, Clone, Serialize)]
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

// Custom FileContent deserializer that validates the content
// during deserialization
impl<'de> Deserialize<'de> for FileContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Use a helper struct to deserialize the raw data
        #[derive(Deserialize)]
        struct FileContentHelper {
            name: Option<String>,
            #[serde(rename = "mimeType")]
            mime_type: Option<String>,
            bytes: Option<String>,
            uri: Option<String>,
        }
        
        let helper = FileContentHelper::deserialize(deserializer)?;
        
        // Create the FileContent
        let file_content = FileContent {
            name: helper.name,
            mime_type: helper.mime_type,
            bytes: helper.bytes,
            uri: helper.uri,
        };
        
        // Validate and return
        match file_content.validate() {
            Ok(_) => Ok(file_content),
            Err(err) => {
                // Convert the A2AError to a serde error
                Err(serde::de::Error::custom(format!("FileContent validation error: {}", err)))
            }
        }
    }
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
        let file_content = FileContent {
            name,
            mime_type,
            bytes: Some(bytes),
            uri: None,
        };
        
        // Validates implicitly as it only has bytes and no URI
        debug_assert!(file_content.validate().is_ok(), "FileContent validation failed");
        
        Part::File {
            file: file_content,
            metadata: None,
        }
    }

    /// Create a file part from a URI
    pub fn file_from_uri(
        uri: String,
        name: Option<String>,
        mime_type: Option<String>,
    ) -> Self {
        let file_content = FileContent {
            name,
            mime_type,
            bytes: None,
            uri: Some(uri),
        };
        
        // Validates implicitly as it only has URI and no bytes
        debug_assert!(file_content.validate().is_ok(), "FileContent validation failed");
        
        Part::File {
            file: file_content,
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
        // If it's a file part, validate the file content
        if let Part::File { file, .. } = &part {
            // In debug mode, we'll assert that the file content is valid
            debug_assert!(file.validate().is_ok(), "Invalid file content in Part::File");
        }
        
        self.parts.push(part);
    }
    
    /// Add a part to this message, validating and returning Result
    pub fn add_part_validated(&mut self, part: Part) -> Result<(), A2AError> {
        // If it's a file part, validate the file content
        if let Part::File { file, .. } = &part {
            file.validate()?;
        }
        
        self.parts.push(part);
        Ok(())
    }
}