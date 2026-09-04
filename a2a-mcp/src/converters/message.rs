//! Converter between A2A Message and MCP Content

use crate::error::Result;
use a2a_rs::domain::{Message, Part, Role};
use base64::Engine;
use rmcp::model::{Content, RawContent, ResourceContents};

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
                            if part.media_type.is_empty() {
                                "unknown"
                            } else {
                                &part.media_type
                            }
                        )
                    } else {
                        format!(
                            "File [Embedded data]\nType: {}",
                            if part.media_type.is_empty() {
                                "unknown"
                            } else {
                                &part.media_type
                            }
                        )
                    };
                    contents.push(Content::text(file_desc));
                }
                Some(part::Content::Url(url)) => {
                    let file_desc = if !part.filename.is_empty() {
                        format!(
                            "File: {} ({})\nURI: {}",
                            part.filename,
                            if part.media_type.is_empty() {
                                "unknown"
                            } else {
                                &part.media_type
                            },
                            url
                        )
                    } else {
                        format!(
                            "File: {}\nType: {}",
                            url,
                            if part.media_type.is_empty() {
                                "unknown"
                            } else {
                                &part.media_type
                            }
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

                    let val: ::buffa_types::google::protobuf::Value =
                        serde_json::from_value(serde_json::Value::Object(data_map))?;
                    parts.push(Part::data(val));
                }
                RawContent::Resource(resource_content) => {
                    parts.push(Self::resource_contents_to_part(&resource_content.resource));
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

    /// The contents of one MCP resource as an A2A part that carries them.
    ///
    /// Text contents become a text part holding the text; blob contents a
    /// file part holding the decoded bytes, or the URI when the base64 does
    /// not decode. Both keep the mime type. The earlier mapping reduced every
    /// resource to a file *reference* by URI, which threw away the body a
    /// caller had read the resource for — a catalogue read at startup arrived
    /// as its own address.
    pub fn resource_contents_to_part(contents: &ResourceContents) -> Part {
        match contents {
            ResourceContents::TextResourceContents {
                text, mime_type, ..
            } => {
                let mut part = Part::text(text.clone());
                if let Some(mime) = mime_type {
                    part.media_type = mime.clone();
                }
                part
            }
            ResourceContents::BlobResourceContents {
                uri,
                blob,
                mime_type,
                ..
            } => match base64::engine::general_purpose::STANDARD.decode(blob) {
                Ok(bytes) => Part::file_from_bytes(bytes, Some(uri.clone()), mime_type.clone()),
                Err(_) => Part::file_from_uri(uri.clone(), None, mime_type.clone()),
            },
        }
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
        assert_eq!(
            message.role,
            buffa::enumeration::EnumValue::Known(Role::ROLE_AGENT)
        );
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

    /// A read resource arrives as what it holds: a text resource's text, a
    /// blob resource's bytes — not the address it was read from.
    #[test]
    fn a_resource_is_carried_as_its_contents() {
        use a2a_rs::domain::generated::part;
        let text = ResourceContents::TextResourceContents {
            uri: "catalogue://views".to_string(),
            mime_type: Some("text/markdown".to_string()),
            text: "# Views".to_string(),
            meta: None,
        };
        let part = MessageConverter::resource_contents_to_part(&text);
        assert_eq!(
            part.content,
            Some(part::Content::Text("# Views".to_string()))
        );
        assert_eq!(part.media_type, "text/markdown");

        let blob = ResourceContents::BlobResourceContents {
            uri: "file:///a.bin".to_string(),
            mime_type: Some("application/octet-stream".to_string()),
            blob: base64::engine::general_purpose::STANDARD.encode(b"\x00\x01"),
            meta: None,
        };
        let part = MessageConverter::resource_contents_to_part(&blob);
        assert_eq!(part.content, Some(part::Content::Raw(vec![0, 1])));
        assert_eq!(part.filename, "file:///a.bin");

        // An embedded resource in tool content takes the same path.
        let embedded = Content::resource(text);
        let message = MessageConverter::content_to_message(&[embedded], Role::Agent).unwrap();
        assert_eq!(
            message.parts[0].content,
            Some(part::Content::Text("# Views".to_string()))
        );
    }
}
