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

        for (index, part) in message.parts.iter().enumerate() {
            let fallback = format!("attachment://part-{index}");
            if let Some(block) = Self::part_to_content_block(part, &fallback)? {
                contents.push(block);
            }
        }

        if contents.is_empty() {
            contents.push(ContentBlock::text("(empty message)"));
        }

        Ok(contents)
    }

    /// One A2A part as the MCP content block that carries it, or `None` for
    /// a part with no content.
    ///
    /// A file part keeps its bytes: an image is image content, text in a
    /// textual media type is an embedded text resource, anything else an
    /// embedded blob resource. The resource's URI is the part's file name,
    /// or `fallback_uri` when it has none. A file part by URI is a resource
    /// link. Until 2026-09-07 every file part became one line of text naming
    /// the file, so a fleet handing SQL and TOML through a tool boundary
    /// lost the file.
    pub fn part_to_content_block(part: &Part, fallback_uri: &str) -> Result<Option<ContentBlock>> {
        use a2a_rs::domain::generated::part;
        Ok(Some(match &part.content {
            Some(part::Content::Text(text)) => ContentBlock::text(text.clone()),
            Some(part::Content::Raw(bytes)) => {
                let uri = if part.filename.is_empty() {
                    fallback_uri.to_string()
                } else {
                    part.filename.clone()
                };
                let mime = if part.media_type.is_empty() {
                    None
                } else {
                    Some(part.media_type.as_str())
                };
                if mime.is_some_and(|m| m.starts_with("image/")) {
                    ContentBlock::image(
                        base64::engine::general_purpose::STANDARD.encode(bytes),
                        part.media_type.clone(),
                    )
                } else if mime.is_some_and(is_textual) {
                    match std::str::from_utf8(bytes) {
                        Ok(text) => ContentBlock::resource(
                            ResourceContents::text(text, uri)
                                .with_mime_type(part.media_type.clone()),
                        ),
                        Err(_) => Self::blob_resource(bytes, uri, mime),
                    }
                } else {
                    Self::blob_resource(bytes, uri, mime)
                }
            }
            Some(part::Content::Url(url)) => {
                let name = if part.filename.is_empty() {
                    url.clone()
                } else {
                    part.filename.clone()
                };
                let mut resource = rmcp::model::Resource::new(url.clone(), name);
                if !part.media_type.is_empty() {
                    resource = resource.with_mime_type(part.media_type.clone());
                }
                ContentBlock::resource_link(resource)
            }
            Some(part::Content::Data(value)) => {
                // For structured data, serialize to JSON text
                ContentBlock::text(serde_json::to_string_pretty(&value)?)
            }
            None => return Ok(None),
        }))
    }

    fn blob_resource(bytes: &[u8], uri: String, mime: Option<&str>) -> ContentBlock {
        ContentBlock::resource(
            ResourceContents::blob(base64::engine::general_purpose::STANDARD.encode(bytes), uri)
                .with_mime_type(mime.unwrap_or("application/octet-stream")),
        )
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
            // An image is a file part holding the bytes, or a text part
            // naming what could not be decoded.
            ContentBlock::Image(image_content) => {
                match base64::engine::general_purpose::STANDARD.decode(&image_content.data) {
                    Ok(bytes) => {
                        Part::file_from_bytes(bytes, None, Some(image_content.mime_type.clone()))
                    }
                    Err(_) => Part::text(format!("[Image: {}]", image_content.mime_type)),
                }
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
    ///
    /// A text resource whose URI has no scheme is a named file that came
    /// through `part_to_content_block`, and the name is kept as the part's
    /// `filename`. A resource read from a server always has a scheme.
    pub fn resource_contents_to_part(contents: &ResourceContents) -> Part {
        match contents {
            ResourceContents::TextResourceContents {
                uri,
                text,
                mime_type,
                ..
            } => {
                let mut part = Part::text(text.clone());
                if let Some(mime) = mime_type {
                    part.media_type = mime.clone();
                }
                if !uri.contains("://") {
                    part.filename = uri.clone();
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

/// Whether bytes in this media type are text a client can show as text:
/// `text/*`, and the structured formats a fleet hands around as files.
fn is_textual(mime: &str) -> bool {
    let mime = mime.split(';').next().unwrap_or(mime).trim();
    mime.starts_with("text/")
        || matches!(
            mime,
            "application/json"
                | "application/toml"
                | "application/yaml"
                | "application/x-yaml"
                | "application/sql"
                | "application/xml"
                | "application/javascript"
        )
        || mime.ends_with("+json")
        || mime.ends_with("+xml")
        || mime.ends_with("+toml")
        || mime.ends_with("+yaml")
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

    /// A file part crosses the bridge with its bytes and comes back a file:
    /// text in a textual type as an embedded text resource, other bytes as
    /// a blob, an image as image content, a URI as a resource link.
    #[test]
    fn a_file_part_keeps_its_bytes_across_the_bridge() {
        use a2a_rs::domain::generated::part;
        let sql = Part::file_from_bytes(
            b"select 1".to_vec(),
            Some("model.sql".to_string()),
            Some("application/sql".to_string()),
        );
        let block = MessageConverter::part_to_content_block(&sql, "attachment://0")
            .unwrap()
            .unwrap();
        let ContentBlock::Resource(embedded) = &block else {
            panic!("expected an embedded resource, got {block:?}");
        };
        assert_eq!(
            embedded.resource,
            ResourceContents::text("select 1", "model.sql").with_mime_type("application/sql")
        );
        let back = MessageConverter::content_block_to_part(&block)
            .unwrap()
            .unwrap();
        assert_eq!(
            back.content,
            Some(part::Content::Text("select 1".to_string()))
        );
        assert_eq!(
            back.filename, "model.sql",
            "a URI without a scheme is a file name"
        );
        assert_eq!(back.media_type, "application/sql");

        let binary = Part::file_from_bytes(vec![0, 159, 146], None, None);
        let block = MessageConverter::part_to_content_block(&binary, "attachment://1")
            .unwrap()
            .unwrap();
        let ContentBlock::Resource(embedded) = &block else {
            panic!("expected an embedded resource, got {block:?}");
        };
        let ResourceContents::BlobResourceContents { uri, mime_type, .. } = &embedded.resource
        else {
            panic!("expected a blob, got {:?}", embedded.resource);
        };
        assert_eq!(uri, "attachment://1");
        assert_eq!(mime_type.as_deref(), Some("application/octet-stream"));
        let back = MessageConverter::content_block_to_part(&block)
            .unwrap()
            .unwrap();
        assert_eq!(back.content, Some(part::Content::Raw(vec![0, 159, 146])));

        let png = Part::file_from_bytes(
            vec![137, 80, 78, 71],
            Some("plot.png".to_string()),
            Some("image/png".to_string()),
        );
        let block = MessageConverter::part_to_content_block(&png, "attachment://2")
            .unwrap()
            .unwrap();
        assert!(matches!(block, ContentBlock::Image(_)), "{block:?}");
        let back = MessageConverter::content_block_to_part(&block)
            .unwrap()
            .unwrap();
        assert_eq!(
            back.content,
            Some(part::Content::Raw(vec![137, 80, 78, 71]))
        );
        assert_eq!(back.media_type, "image/png");

        let by_uri = Part::file_from_uri(
            "https://example.com/a.toml".to_string(),
            Some("a.toml".to_string()),
            Some("application/toml".to_string()),
        );
        let block = MessageConverter::part_to_content_block(&by_uri, "attachment://3")
            .unwrap()
            .unwrap();
        let ContentBlock::ResourceLink(link) = &block else {
            panic!("expected a resource link, got {block:?}");
        };
        assert_eq!(link.uri, "https://example.com/a.toml");
        assert_eq!(link.name, "a.toml");
        let back = MessageConverter::content_block_to_part(&block)
            .unwrap()
            .unwrap();
        assert_eq!(
            back.content,
            Some(part::Content::Url("https://example.com/a.toml".to_string()))
        );
        assert_eq!(back.filename, "a.toml");

        // A resource read from a server keeps its address out of `filename`.
        let read = ResourceContents::text("# Views", "catalogue://views");
        assert_eq!(
            MessageConverter::resource_contents_to_part(&read).filename,
            ""
        );
    }
}
