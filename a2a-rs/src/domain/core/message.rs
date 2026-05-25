use crate::domain::error::A2AError;

// Re-export the generated types so downstream code gets them from `domain::core::message`
pub use crate::domain::generated::{Message, Part, Role, Artifact, part};

#[allow(non_upper_case_globals)]
impl Role {
    pub const User: Self = Self::ROLE_USER;
    pub const Agent: Self = Self::ROLE_AGENT;
}


impl Part {
    /// Create a text part
    #[inline]
    pub fn text(content: String) -> Self {
        Self {
            content: Some(part::Content::Text(content)),
            ..Default::default()
        }
    }

    /// Create a text part with metadata
    #[inline]
    pub fn text_with_metadata(content: String, metadata: ::buffa_types::google::protobuf::Struct) -> Self {
        Self {
            content: Some(part::Content::Text(content)),
            metadata: ::buffa::MessageField::some(metadata),
            ..Default::default()
        }
    }

    /// Create a data part
    #[inline]
    pub fn data(data: ::buffa_types::google::protobuf::Value) -> Self {
        Self {
            content: Some(part::Content::Data(Box::new(data))),
            ..Default::default()
        }
    }

    /// Create a file part from base64 encoded data (or bytes)
    pub fn file_from_bytes(bytes: Vec<u8>, name: Option<String>, mime_type: Option<String>) -> Self {
        Self {
            content: Some(part::Content::Raw(bytes)),
            filename: name.unwrap_or_default(),
            media_type: mime_type.unwrap_or_default(),
            ..Default::default()
        }
    }

    /// Create a file part from a URI
    pub fn file_from_uri(uri: String, name: Option<String>, mime_type: Option<String>) -> Self {
        Self {
            content: Some(part::Content::Url(uri)),
            filename: name.unwrap_or_default(),
            media_type: mime_type.unwrap_or_default(),
            ..Default::default()
        }
    }

    /// Helper method to get the text content if this is a Text part
    pub fn get_text(&self) -> Option<&str> {
        match &self.content {
            Some(part::Content::Text(text)) => Some(text.as_str()),
            _ => None,
        }
    }

    /// Validate the part content
    pub fn validate(&self) -> Result<(), A2AError> {
        match &self.content {
            Some(_) => Ok(()),
            None => Err(A2AError::InvalidParams(
                "Part must contain content (text, raw, url, or data)".to_string(),
            )),
        }
    }

    /// Create a builder-style text part that can be chained
    pub fn text_builder(content: String) -> PartBuilder {
        PartBuilder {
            part: Self::text(content),
        }
    }

    /// Create a builder-style data part that can be chained
    pub fn data_builder(data: ::buffa_types::google::protobuf::Value) -> PartBuilder {
        PartBuilder {
            part: Self::data(data),
        }
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
    /// Add metadata to any part type
    pub fn with_metadata(mut self, metadata: ::buffa_types::google::protobuf::Struct) -> Self {
        self.part.metadata = ::buffa::MessageField::some(metadata);
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
    bytes: Option<Vec<u8>>,
    uri: Option<String>,
    metadata: Option<::buffa_types::google::protobuf::Struct>,
}

impl Default for FilePartBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl FilePartBuilder {
    pub fn new() -> Self {
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

    /// Set file content as bytes
    pub fn bytes(mut self, bytes: Vec<u8>) -> Self {
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
    pub fn with_metadata(mut self, metadata: ::buffa_types::google::protobuf::Struct) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Build the file part with validation
    pub fn build(self) -> Result<Part, A2AError> {
        let content = match (self.bytes, self.uri) {
            (Some(b), None) => part::Content::Raw(b),
            (None, Some(u)) => part::Content::Url(u),
            (Some(_), Some(_)) => {
                return Err(A2AError::InvalidParams(
                    "Cannot provide both bytes and uri".to_string(),
                ));
            }
            (None, None) => {
                return Err(A2AError::InvalidParams(
                    "Must provide either bytes or uri".to_string(),
                ));
            }
        };

        Ok(Part {
            content: Some(content),
            filename: self.name.unwrap_or_default(),
            media_type: self.mime_type.unwrap_or_default(),
            metadata: self.metadata.into(),
            ..Default::default()
        })
    }
}

/// Builder for Message instances to keep compatibility
pub struct MessageBuilder {
    message_id: String,
    context_id: String,
    task_id: String,
    role: Role,
    parts: Vec<Part>,
    metadata: Option<::buffa_types::google::protobuf::Struct>,
    extensions: Vec<String>,
    reference_task_ids: Vec<String>,
}

impl Default for MessageBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageBuilder {
    pub fn new() -> Self {
        Self {
            message_id: String::new(),
            context_id: String::new(),
            task_id: String::new(),
            role: Role::ROLE_UNSPECIFIED,
            parts: Vec::new(),
            metadata: None,
            extensions: Vec::new(),
            reference_task_ids: Vec::new(),
        }
    }

    pub fn message_id(mut self, message_id: String) -> Self {
        self.message_id = message_id;
        self
    }

    pub fn context_id(mut self, context_id: String) -> Self {
        self.context_id = context_id;
        self
    }

    pub fn task_id(mut self, task_id: String) -> Self {
        self.task_id = task_id;
        self
    }

    pub fn role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }

    pub fn parts(mut self, parts: Vec<Part>) -> Self {
        self.parts = parts;
        self
    }

    pub fn metadata(mut self, metadata: ::buffa_types::google::protobuf::Struct) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn extensions(mut self, extensions: Vec<String>) -> Self {
        self.extensions = extensions;
        self
    }

    pub fn reference_task_ids(mut self, reference_task_ids: Vec<String>) -> Self {
        self.reference_task_ids = reference_task_ids;
        self
    }

    pub fn build(self) -> Message {
        Message {
            message_id: self.message_id,
            context_id: self.context_id,
            task_id: self.task_id,
            role: ::buffa::EnumValue::from(self.role),
            parts: self.parts,
            metadata: self.metadata.into(),
            extensions: self.extensions,
            reference_task_ids: self.reference_task_ids,
            ..Default::default()
        }
    }
}

impl Message {
    /// Create a new Message builder
    pub fn builder() -> MessageBuilder {
        MessageBuilder::new()
    }

    /// Create a new user message with a single text part
    pub fn user_text(text: String, message_id: String) -> Self {
        Self {
            role: ::buffa::EnumValue::from(Role::ROLE_USER),
            parts: vec![Part::text(text)],
            message_id,
            ..Default::default()
        }
    }

    /// Create a new agent message with a single text part
    pub fn agent_text(text: String, message_id: String) -> Self {
        Self {
            role: ::buffa::EnumValue::from(Role::ROLE_AGENT),
            parts: vec![Part::text(text)],
            message_id,
            ..Default::default()
        }
    }

    /// Add a part to this message
    pub fn add_part(&mut self, part: Part) {
        self.parts.push(part);
    }

    /// Add a part to this message, validating and returning Result
    pub fn add_part_validated(&mut self, part: Part) -> Result<(), A2AError> {
        part.validate()?;
        self.parts.push(part);
        Ok(())
    }

    /// Validate a message
    pub fn validate(&self) -> Result<(), A2AError> {
        for part in &self.parts {
            part.validate()?;
        }
        Ok(())
    }
}
