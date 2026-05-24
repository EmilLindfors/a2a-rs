//! Converter between A2A Message and MCP Content

use crate::error::Result;
use a2a_rs::domain::{Message, Part, Role};
use rmcp::model::{Content, RawContent};

/// Converts between A2A Messages and MCP Content
pub struct MessageConverter;

impl MessageConverter {
    /// Convert A2A Message to MCP Content array
    pub fn message_to_content(message: &Message) -> Result<Vec<Content>> {
        let mut contents = Vec::new();

        for part in &message.parts {
            use a2a_rs::domain::generated::part;
            match &part.content {
                Some(part::Content::Text(text)) => {
                    contents.push(Content::text(text.clone()));
                }
                Some(part::Content::Raw(_)) => {
                    let file_desc = if !part.filename.is_empty() {
                        format!(
                            "File: {} ({})\n[Embedded data]",
                            part.filename,
                            if part.media_type.is_empty() { "unknown" } else { &part.media_type }
                        )
                    } else {
                        format!(
                            "File [Embedded data]\nType: {}",
                            if part.media_type.is_empty() { "unknown" } else { &part.media_type }
                        )
                    };
                    contents.push(Content::text(file_desc));
                }
                Some(part::Content::Url(url)) => {
                    let file_desc = if !part.filename.is_empty() {
                        format!(
                            "File: {} ({})\nURI: {}",
                            part.filename,
                            if part.media_type.is_empty() { "unknown" } else { &part.media_type },
                            url
                        )
                    } else {
                        format!(
                            "File: {}\nType: {}",
                            url,
                            if part.media_type.is_empty() { "unknown" } else { &part.media_type }
                        )
                    };
                    contents.push(Content::text(file_desc));
                }
                Some(part::Content::Data(value)) => {
                    // For structured data, serialize to JSON text
                    contents.push(Content::text(serde_json::to_string_pretty(&value)?));
                }
                None => {}
            }
        }

        if contents.is_empty() {
            contents.push(Content::text("(empty message)"));
        }

        Ok(contents)
    }

    /// Convert MCP Content array to A2A Message
    ///
    /// Uses provided Role enum value
    pub fn content_to_message(content: &[Content], role: Role) -> Result<Message> {
        let mut parts = Vec::new();

        for item in content {
            // Match on the dereferenced RawContent
            match &**item {
                RawContent::Text(text_content) => {
                    parts.push(Part::text(text_content.text.clone()));
                }
                RawContent::Image(image_content) => {
                    // Convert image to data part
                    let mut data_map = serde_json::Map::new();
                    data_map.insert(
                        "type".to_string(),
                        serde_json::Value::String("image".to_string()),
                    );
                    data_map.insert(
                        "data".to_string(),
                        serde_json::Value::String(image_content.data.clone()),
                    );
                    data_map.insert(
                        "mimeType".to_string(),
                        serde_json::Value::String(image_content.mime_type.clone()),
                    );

                    let val: ::buffa_types::google::protobuf::Value = serde_json::from_value(serde_json::Value::Object(data_map))?;
                    parts.push(Part::data(val));
                }
                RawContent::Resource(resource_content) => {
                    // Treat embedded resource as a file reference
                    match &resource_content.resource {
                        rmcp::model::ResourceContents::TextResourceContents {
                            uri,
                            mime_type,
                            ..
                        } => {
                            parts.push(Part::file_from_uri(uri.clone(), None, mime_type.clone()));
                        }
                        rmcp::model::ResourceContents::BlobResourceContents {
                            uri,
                            mime_type,
                            ..
                        } => {
                            parts.push(Part::file_from_uri(uri.clone(), None, mime_type.clone()));
                        }
                    }
                }
                RawContent::ResourceLink(resource_link) => {
                    // Treat resource link as a file reference
                    parts.push(Part::file_from_uri(
                        resource_link.uri.clone(),
                        Some(resource_link.name.clone()),
                        resource_link.mime_type.clone(),
                    ));
                }
                RawContent::Audio(_audio_content) => {
                    // For now, treat audio as text description
                    parts.push(Part::text("[Audio content]".to_string()));
                }
            }
        }

        if parts.is_empty() {
            parts.push(Part::text(String::new()));
        }

        Ok(Message::builder()
            .role(role)
            .parts(parts)
            .message_id(uuid::Uuid::new_v4().to_string())
            .build())
    }

    /// Extract text content from A2A message
    pub fn extract_text_from_message(message: &Message) -> String {
        let mut texts = Vec::new();

        for part in &message.parts {
            use a2a_rs::domain::generated::part;
            match &part.content {
                Some(part::Content::Text(text)) => texts.push(text.clone()),
                Some(part::Content::Raw(_)) => {
                    let name = &part.filename;
                    if !name.is_empty() {
                        texts.push(format!("[File: {}]", name));
                    } else {
                        texts.push("[File: embedded]".to_string());
                    }
                }
                Some(part::Content::Url(url)) => {
                    let name = &part.filename;
                    if !name.is_empty() {
                        texts.push(format!("[File: {}]", name));
                    } else if !url.is_empty() {
                        texts.push(format!("[File: {}]", url));
                    } else {
                        texts.push("[File: embedded]".to_string());
                    }
                }
                Some(part::Content::Data(data)) => {
                    if let Ok(data_json) = serde_json::to_string(data) {
                        texts.push(format!("[Data: {}]", data_json));
                    } else {
                        texts.push("[Data]".to_string());
                    }
                }
                None => {}
            }
        }

        texts.join("\n")
    }

    /// Extract text from MCP Content array
    pub fn extract_text_from_content(content: &[Content]) -> String {
        content
            .iter()
            .map(|c| match &**c {
                RawContent::Text(text_content) => text_content.text.clone(),
                RawContent::Image(_) => "[Image]".to_string(),
                RawContent::Resource(resource) => {
                    let uri = match &resource.resource {
                        rmcp::model::ResourceContents::TextResourceContents { uri, .. } => uri,
                        rmcp::model::ResourceContents::BlobResourceContents { uri, .. } => uri,
                    };
                    format!("[Resource: {}]", uri)
                }
                RawContent::ResourceLink(resource) => format!("[Resource: {}]", resource.uri),
                RawContent::Audio(_) => "[Audio]".to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_to_content() {
        let message = Message::builder()
            .role(Role::User)
            .parts(vec![
                Part::text("Hello".to_string()),
                Part::text("World".to_string()),
            ])
            .message_id("test-msg".to_string())
            .build();

        let content = MessageConverter::message_to_content(&message).unwrap();
        assert_eq!(content.len(), 2);
    }

    #[test]
    fn test_content_to_message() {
        let content = vec![Content::text("Hello MCP")];

        let message = MessageConverter::content_to_message(&content, Role::Agent).unwrap();
        assert_eq!(message.role, buffa::enumeration::EnumValue::Known(Role::ROLE_AGENT));
        assert_eq!(message.parts.len(), 1);

        use a2a_rs::domain::generated::part;
        if let Some(part::Content::Text(text)) = &message.parts[0].content {
            assert_eq!(text, "Hello MCP");
        } else {
            panic!("Expected text part");
        }
    }

    #[test]
    fn test_extract_text_from_message() {
        let message = Message::builder()
            .role(Role::User)
            .parts(vec![
                Part::text("Line 1".to_string()),
                Part::text("Line 2".to_string()),
            ])
            .message_id("test-msg".to_string())
            .build();

        let text = MessageConverter::extract_text_from_message(&message);
        assert!(text.contains("Line 1"));
        assert!(text.contains("Line 2"));
    }
}
