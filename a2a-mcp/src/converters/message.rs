//! Converter between A2A Message and MCP Content

use crate::error::Result;
use a2a_rs::domain::{FileContent, Message, Part, Role};
use rmcp::model::{Content, RawContent};

/// Converts between A2A Messages and MCP Content
pub struct MessageConverter;

impl MessageConverter {
    /// Convert A2A Message to MCP Content array
    pub fn message_to_content(message: &Message) -> Result<Vec<Content>> {
        let mut contents = Vec::new();

        for part in &message.parts {
            match part {
                Part::Text { text, .. } => {
                    contents.push(Content::text(text));
                }
                Part::File { file, .. } => {
                    // MCP doesn't have direct file support in Content, so we encode as text with metadata
                    let file_desc = if let Some(ref name) = file.name {
                        if let Some(ref uri) = file.uri {
                            format!("File: {} ({})\nURI: {}", name, file.mime_type.as_deref().unwrap_or("unknown"), uri)
                        } else {
                            format!("File: {} ({})\n[Embedded data]", name, file.mime_type.as_deref().unwrap_or("unknown"))
                        }
                    } else if let Some(ref uri) = file.uri {
                        format!("File: {}\nType: {}", uri, file.mime_type.as_deref().unwrap_or("unknown"))
                    } else {
                        format!("File [Embedded data]\nType: {}", file.mime_type.as_deref().unwrap_or("unknown"))
                    };
                    contents.push(Content::text(file_desc));
                }
                Part::Data { data, .. } => {
                    // For structured data, serialize to JSON text
                    contents.push(Content::text(serde_json::to_string_pretty(&serde_json::Value::Object(data.clone()))?));
                }
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
                    parts.push(Part::Text {
                        text: text_content.text.clone(),
                        metadata: None,
                    });
                }
                RawContent::Image(image_content) => {
                    // Convert image to data part
                    let mut data_map = serde_json::Map::new();
                    data_map.insert("type".to_string(), serde_json::Value::String("image".to_string()));
                    data_map.insert("data".to_string(), serde_json::Value::String(image_content.data.clone()));
                    data_map.insert("mimeType".to_string(), serde_json::Value::String(image_content.mime_type.clone()));

                    parts.push(Part::Data {
                        data: data_map,
                        metadata: None,
                    });
                }
                RawContent::Resource(resource_content) => {
                    // Treat embedded resource as a file reference
                    match &resource_content.resource {
                        rmcp::model::ResourceContents::TextResourceContents { uri, mime_type, .. } => {
                            parts.push(Part::File {
                                file: FileContent {
                                    name: None,
                                    mime_type: mime_type.clone(),
                                    bytes: None,
                                    uri: Some(uri.clone()),
                                },
                                metadata: None,
                            });
                        }
                        rmcp::model::ResourceContents::BlobResourceContents { uri, mime_type, .. } => {
                            parts.push(Part::File {
                                file: FileContent {
                                    name: None,
                                    mime_type: mime_type.clone(),
                                    bytes: None,
                                    uri: Some(uri.clone()),
                                },
                                metadata: None,
                            });
                        }
                    }
                }
                RawContent::ResourceLink(resource_link) => {
                    // Treat resource link as a file reference
                    parts.push(Part::File {
                        file: FileContent {
                            name: Some(resource_link.name.clone()),
                            mime_type: resource_link.mime_type.clone(),
                            bytes: None,
                            uri: Some(resource_link.uri.clone()),
                        },
                        metadata: None,
                    });
                }
                RawContent::Audio(_audio_content) => {
                    // For now, treat audio as text description
                    parts.push(Part::Text {
                        text: "[Audio content]".to_string(),
                        metadata: None,
                    });
                }
            }
        }

        if parts.is_empty() {
            parts.push(Part::Text {
                text: String::new(),
                metadata: None,
            });
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
            match part {
                Part::Text { text, .. } => texts.push(text.clone()),
                Part::File { file, .. } => {
                    if let Some(ref name) = file.name {
                        texts.push(format!("[File: {}]", name));
                    } else if let Some(ref uri) = file.uri {
                        texts.push(format!("[File: {}]", uri));
                    } else {
                        texts.push("[File: embedded]".to_string());
                    }
                }
                Part::Data { data, .. } => {
                    texts.push(format!("[Data: {}]", serde_json::Value::Object(data.clone())));
                }
            }
        }

        texts.join("\n")
    }

    /// Extract text from MCP Content array
    pub fn extract_text_from_content(content: &[Content]) -> String {
        content
            .iter()
            .filter_map(|c| match &**c {
                RawContent::Text(text_content) => Some(text_content.text.clone()),
                RawContent::Image(_) => Some("[Image]".to_string()),
                RawContent::Resource(resource) => {
                    let uri = match &resource.resource {
                        rmcp::model::ResourceContents::TextResourceContents { uri, .. } => uri,
                        rmcp::model::ResourceContents::BlobResourceContents { uri, .. } => uri,
                    };
                    Some(format!("[Resource: {}]", uri))
                }
                RawContent::ResourceLink(resource) => Some(format!("[Resource: {}]", resource.uri)),
                RawContent::Audio(_) => Some("[Audio]".to_string()),
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
                Part::Text {
                    text: "Hello".to_string(),
                    metadata: None,
                },
                Part::Text {
                    text: "World".to_string(),
                    metadata: None,
                },
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
        assert_eq!(message.role, Role::Agent);
        assert_eq!(message.parts.len(), 1);

        if let Part::Text { text, .. } = &message.parts[0] {
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
                Part::Text {
                    text: "Line 1".to_string(),
                    metadata: None,
                },
                Part::Text {
                    text: "Line 2".to_string(),
                    metadata: None,
                },
            ])
            .message_id("test-msg".to_string())
            .build();

        let text = MessageConverter::extract_text_from_message(&message);
        assert!(text.contains("Line 1"));
        assert!(text.contains("Line 2"));
    }
}
