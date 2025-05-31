use bon::Builder;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

#[cfg(feature = "tracing")]
use tracing::instrument;

use crate::domain::error::A2AError;

/// Roles in agent communication (user or agent).
///
/// Distinguishes between messages sent by users (human or system)
/// and messages sent by agents in the conversation flow.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Agent,
}

/// File content representation supporting both embedded data and URIs.
///
/// Files can be represented either as base64-encoded embedded data
/// or as URIs pointing to external resources. The implementation
/// validates that exactly one of `bytes` or `uri` is provided.
///
/// # Example
/// ```rust
/// use a2a_rs::FileContent;
/// 
/// // Embedded file content
/// let embedded = FileContent {
///     name: Some("example.txt".to_string()),
///     mime_type: Some("text/plain".to_string()),
///     bytes: Some("SGVsbG8gV29ybGQ=".to_string()), // "Hello World" in base64
///     uri: None,
/// };
/// 
/// // URI-based file content  
/// let uri_based = FileContent {
///     name: Some("document.pdf".to_string()),
///     mime_type: Some("application/pdf".to_string()),
///     bytes: None,
///     uri: Some("https://example.com/document.pdf".to_string()),
/// };
/// ```
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
                Err(serde::de::Error::custom(format!(
                    "FileContent validation error: {}",
                    err
                )))
            }
        }
    }
}

impl FileContent {
    /// Validates that the file content is properly specified
    #[cfg_attr(feature = "tracing", instrument(skip(self), fields(
        file.name = ?self.name,
        file.has_bytes = self.bytes.is_some(),
        file.has_uri = self.uri.is_some()
    )))]
    pub fn validate(&self) -> Result<(), A2AError> {
        match (&self.bytes, &self.uri) {
            (Some(_), None) | (None, Some(_)) => {
                #[cfg(feature = "tracing")]
                tracing::debug!("File content validation successful");
                Ok(())
            }
            (Some(_), Some(_)) => {
                #[cfg(feature = "tracing")]
                tracing::error!("File content has both bytes and uri");
                Err(A2AError::InvalidParams(
                    "Cannot provide both bytes and uri".to_string(),
                ))
            }
            (None, None) => {
                #[cfg(feature = "tracing")]
                tracing::error!("File content has neither bytes nor uri");
                Err(A2AError::InvalidParams(
                    "Must provide either bytes or uri".to_string(),
                ))
            }
        }
    }
}

/// Parts that can make up a message (text, file, or structured data).\n///\n/// Messages in the A2A protocol consist of one or more parts, each of which\n/// can contain different types of content:\n/// - `Text`: Plain text content with optional metadata\n/// - `File`: File content (embedded or URI-based) with optional metadata  \n/// - `Data`: Structured JSON data with optional metadata\n///\n/// Each part type supports optional metadata for additional context.\n///\n/// # Example\n/// ```rust\n/// use a2a_rs::{Part, FileContent};\n/// use serde_json::{Map, Value};\n/// \n/// // Text part\n/// let text_part = Part::Text {\n///     text: \"Hello, world!\".to_string(),\n///     metadata: None,\n/// };\n/// \n/// // File part with metadata\n/// let mut metadata = Map::new();\n/// metadata.insert(\"source\".to_string(), Value::String(\"user_upload\".to_string()));\n/// \n/// let file_part = Part::File {\n///     file: FileContent {\n///         name: Some(\"example.txt\".to_string()),\n///         mime_type: Some(\"text/plain\".to_string()),\n///         bytes: Some(\"SGVsbG8=\".to_string()),\n///         uri: None,\n///     },\n///     metadata: Some(metadata),\n/// };\n/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
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

impl Part {
    /// Helper method to get the text content if this is a Text part
    #[cfg(test)]
    pub fn get_text(&self) -> Option<&str> {
        match self {
            Part::Text { text, .. } => Some(text),
            _ => None,
        }
    }
}

/// A message in the A2A protocol containing parts and metadata.
///
/// Messages are the primary unit of communication in the A2A protocol.
/// Each message has a role (user or agent), one or more content parts,
/// and various IDs for tracking and organization.
///
/// # Example
/// ```rust
/// use a2a_rs::{Message, Role, Part};
/// 
/// let message = Message::builder()
///     .role(Role::User)
///     .parts(vec![Part::Text {
///         text: "Hello, agent!".to_string(),
///         metadata: None,
///     }])
///     .message_id("msg-123".to_string())
///     .build();
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
pub struct Message {
    pub role: Role,
    #[builder(default = Vec::new())]
    pub parts: Vec<Part>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "referenceTaskIds")]
    pub reference_task_ids: Option<Vec<String>>,
    #[serde(rename = "messageId")]
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "taskId")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "contextId")]
    pub context_id: Option<String>,
    #[builder(default = "message".to_string())]
    pub kind: String, // Always "message"
}

/// An artifact produced by an agent during task processing.
///
/// Artifacts represent outputs, intermediate results, or side effects
/// produced by agents while processing tasks. They can contain various
/// types of content and include metadata for organization and discovery.
///
/// # Example
/// ```rust
/// use a2a_rs::{Artifact, Part};
/// 
/// let artifact = Artifact {
///     artifact_id: "artifact-123".to_string(),
///     name: Some("Generated Report".to_string()),
///     description: Some("Analysis report generated from data".to_string()),
///     parts: vec![Part::Text {
///         text: "Report content here...".to_string(),
///         metadata: None,
///     }],
///     metadata: None,
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    #[serde(rename = "artifactId")]
    pub artifact_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parts: Vec<Part>,
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
    pub fn file_from_bytes(bytes: String, name: Option<String>, mime_type: Option<String>) -> Self {
        let file_content = FileContent {
            name,
            mime_type,
            bytes: Some(bytes),
            uri: None,
        };

        // Validates implicitly as it only has bytes and no URI
        debug_assert!(
            file_content.validate().is_ok(),
            "FileContent validation failed"
        );

        Part::File {
            file: file_content,
            metadata: None,
        }
    }

    /// Create a file part from a URI
    pub fn file_from_uri(uri: String, name: Option<String>, mime_type: Option<String>) -> Self {
        let file_content = FileContent {
            name,
            mime_type,
            bytes: None,
            uri: Some(uri),
        };

        // Validates implicitly as it only has URI and no bytes
        debug_assert!(
            file_content.validate().is_ok(),
            "FileContent validation failed"
        );

        Part::File {
            file: file_content,
            metadata: None,
        }
    }

    /// Create a builder-style text part that can be chained
    pub fn text_builder(content: String) -> PartBuilder {
        PartBuilder::new_text(content)
    }

    /// Create a builder-style data part that can be chained
    pub fn data_builder(data: Map<String, Value>) -> PartBuilder {
        PartBuilder::new_data(data)
    }

    /// Create a builder-style file part that can be chained
    pub fn file_builder() -> FilePartBuilder {
        FilePartBuilder::new()
    }
}

/// Builder for Part instances
pub struct PartBuilder {
    part: Part,
}

impl PartBuilder {
    fn new_text(content: String) -> Self {
        Self {
            part: Part::Text {
                text: content,
                metadata: None,
            },
        }
    }

    fn new_data(data: Map<String, Value>) -> Self {
        Self {
            part: Part::Data {
                data,
                metadata: None,
            },
        }
    }

    /// Add metadata to any part type
    pub fn with_metadata(mut self, metadata: Map<String, Value>) -> Self {
        match &mut self.part {
            Part::Text {
                metadata: ref mut meta,
                ..
            } => *meta = Some(metadata),
            Part::Data {
                metadata: ref mut meta,
                ..
            } => *meta = Some(metadata),
            Part::File {
                metadata: ref mut meta,
                ..
            } => *meta = Some(metadata),
        }
        self
    }

    /// Build the final Part
    pub fn build(self) -> Part {
        self.part
    }
}

/// Builder for file parts with validation
pub struct FilePartBuilder {
    name: Option<String>,
    mime_type: Option<String>,
    bytes: Option<String>,
    uri: Option<String>,
    metadata: Option<Map<String, Value>>,
}

impl FilePartBuilder {
    fn new() -> Self {
        Self {
            name: None,
            mime_type: None,
            bytes: None,
            uri: None,
            metadata: None,
        }
    }

    /// Set the file name
    pub fn name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    /// Set the MIME type
    pub fn mime_type(mut self, mime_type: String) -> Self {
        self.mime_type = Some(mime_type);
        self
    }

    /// Set file content as base64 bytes
    pub fn bytes(mut self, bytes: String) -> Self {
        self.bytes = Some(bytes);
        self.uri = None; // Clear URI if setting bytes
        self
    }

    /// Set file URI
    pub fn uri(mut self, uri: String) -> Self {
        self.uri = Some(uri);
        self.bytes = None; // Clear bytes if setting URI
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, metadata: Map<String, Value>) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Build the file part with validation
    pub fn build(self) -> Result<Part, A2AError> {
        let file_content = FileContent {
            name: self.name,
            mime_type: self.mime_type,
            bytes: self.bytes,
            uri: self.uri,
        };

        // Validate the file content
        file_content.validate()?;

        Ok(Part::File {
            file: file_content,
            metadata: self.metadata,
        })
    }
}

/// Helper methods for creating messages
impl Message {
    /// Create a new user message with a single text part
    pub fn user_text(text: String, message_id: String) -> Self {
        Self {
            role: Role::User,
            parts: vec![Part::text(text)],
            metadata: None,
            reference_task_ids: None,
            message_id,
            task_id: None,
            context_id: None,
            kind: "message".to_string(),
        }
    }

    /// Create a new agent message with a single text part
    pub fn agent_text(text: String, message_id: String) -> Self {
        Self {
            role: Role::Agent,
            parts: vec![Part::text(text)],
            metadata: None,
            reference_task_ids: None,
            message_id,
            task_id: None,
            context_id: None,
            kind: "message".to_string(),
        }
    }

    /// Add a part to this message
    pub fn add_part(&mut self, part: Part) {
        // If it's a file part, validate the file content
        if let Part::File { file, .. } = &part {
            // In debug mode, we'll assert that the file content is valid
            debug_assert!(
                file.validate().is_ok(),
                "Invalid file content in Part::File"
            );
        }

        self.parts.push(part);
    }

    /// Add a part to this message, validating and returning Result
    #[cfg_attr(feature = "tracing", instrument(skip(self, part), fields(
        message.id = %self.message_id
    )))]
    pub fn add_part_validated(&mut self, part: Part) -> Result<(), A2AError> {
        // If it's a file part, validate the file content
        if let Part::File { file, .. } = &part {
            file.validate()?;
        }

        #[cfg(feature = "tracing")]
        {
            let part_type = match &part {
                Part::Text { .. } => "text",
                Part::File { .. } => "file",
                Part::Data { .. } => "data",
            };
            tracing::debug!(part_type = part_type, "Part added successfully to message");
        }

        self.parts.push(part);
        Ok(())
    }

    /// Validate a message (useful after building with builder)
    #[cfg_attr(feature = "tracing", instrument(skip(self), fields(
        message.id = %self.message_id,
        message.role = ?self.role,
        message.parts_count = self.parts.len(),
        message.kind = %self.kind
    )))]
    pub fn validate(&self) -> Result<(), A2AError> {
        #[cfg(feature = "tracing")]
        tracing::debug!("Validating message");

        // Validate all file parts
        for (index, part) in self.parts.iter().enumerate() {
            if let Part::File { file, .. } = part {
                #[cfg(feature = "tracing")]
                tracing::trace!("Validating file part at index {}", index);
                file.validate()?;
            }
        }

        // Validate that kind is "message"
        if self.kind != "message" {
            #[cfg(feature = "tracing")]
            tracing::error!("Invalid message kind: {}", self.kind);
            return Err(A2AError::InvalidParams(
                "Message kind must be 'message'".to_string(),
            ));
        }

        #[cfg(feature = "tracing")]
        tracing::debug!("Message validation successful");
        Ok(())
    }
}
