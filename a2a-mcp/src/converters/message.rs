//! Converter between A2A Message and MCP Content

use crate::error::Result;
use a2a_rs::domain::{Message, Part, Role};
use base64::Engine;
use rmcp::model::{ContentBlock, ResourceContents};

/// Converts between A2A Messages and MCP Content
pub struct MessageConverter;

impl MessageConverter {
    /// Convert A2A Message to MCP Content array
    pub fn message_to_content(message: &Message) -> Result<Vec<ContentBlock>> {
        let mut contents = Vec::new();

        for part in &message.parts {
            use a2a_rs::domain::generated::part;
            match &part.content {
                Some(part::Content::Text(text)) => {
                    contents.push(ContentBlock::text(text.clone()));
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
                    contents.push(ContentBlock::text(file_desc));
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
                    contents.push(ContentBlock::text(file_desc));
                }
                Some(part::Content::Data(value)) => {
                    // For structured data, serialize to JSON text
                    contents.push(ContentBlock::text(serde_json::to_string_pretty(&value)?));
                }
                None => {}
            }
        }

        if contents.is_empty() {
            contents.push(ContentBlock::text("(empty message)"));
        }

        Ok(contents)
    }

    /// Convert MCP Content array to A2A Message
    ///
    /// Uses provided Role enum value
    pub fn content_to_message(content: &[ContentBlock], role: Role) -> Result<Message> {
        let mut parts = Vec::new();

        for item in content {
            if let Some(part) = Self::content_block_to_part(item)? {
                parts.push(part);
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

    /// One MCP content block as the A2A part that carries it, or `None` for
    /// a block kind this crate does not know — the enum is open-ended.
    pub fn content_block_to_part(block: &ContentBlock) -> Result<Option<Part>> {
        Ok(Some(match block {
            ContentBlock::Text(text_content) => Part::text(text_content.text.clone()),
            ContentBlock::Image(image_content) => {
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
                Part::data(val)
            }
            ContentBlock::Resource(embedded) => Self::resource_contents_to_part(&embedded.resource),
            // A resource link is a file reference.
            ContentBlock::ResourceLink(link) => Part::file_from_uri(
                link.uri.clone(),
                Some(link.name.clone()),
                link.mime_type.clone(),
            ),
            // For now, treat audio as text description
            ContentBlock::Audio(_) => Part::text("[Audio content]".to_string()),
            _ => return Ok(None),
        }))
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
            // The enum is open-ended; a kind this crate does not know is
            // carried as nothing rather than dropped on the floor silently.
            _ => Part::text(String::new()),
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
    pub fn extract_text_from_content(content: &[ContentBlock]) -> String {
        content
            .iter()
            .map(|c| match c {
                ContentBlock::Text(text_content) => text_content.text.clone(),
                ContentBlock::Image(_) => "[Image]".to_string(),
                ContentBlock::Resource(resource) => match &resource.resource {
                    ResourceContents::TextResourceContents { uri, .. }
                    | ResourceContents::BlobResourceContents { uri, .. } => {
                        format!("[Resource: {}]", uri)
                    }
                    _ => "[Resource]".to_string(),
                },
                ContentBlock::ResourceLink(resource) => format!("[Resource: {}]", resource.uri),
                ContentBlock::Audio(_) => "[Audio]".to_string(),
                _ => "[Unknown content]".to_string(),
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
        let content = vec![ContentBlock::text("Hello MCP")];

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
        let text =
            ResourceContents::text("# Views", "catalogue://views").with_mime_type("text/markdown");
        let part = MessageConverter::resource_contents_to_part(&text);
        assert_eq!(
            part.content,
            Some(part::Content::Text("# Views".to_string()))
        );
        assert_eq!(part.media_type, "text/markdown");

        let blob = ResourceContents::blob(
            base64::engine::general_purpose::STANDARD.encode(b"\x00\x01"),
            "file:///a.bin",
        )
        .with_mime_type("application/octet-stream");
        let part = MessageConverter::resource_contents_to_part(&blob);
        assert_eq!(part.content, Some(part::Content::Raw(vec![0, 1])));
        assert_eq!(part.filename, "file:///a.bin");

        // An embedded resource in tool content takes the same path.
        let embedded = ContentBlock::resource(text);
        let message = MessageConverter::content_to_message(&[embedded], Role::Agent).unwrap();
        assert_eq!(
            message.parts[0].content,
            Some(part::Content::Text("# Views".to_string()))
        );
    }
}
