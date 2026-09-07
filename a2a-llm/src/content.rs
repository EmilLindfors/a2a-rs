//! What a message says, when not all of it is text.
//!
//! [`ChatMessage::content`] used to be `Option<String>`, which is the shape
//! every chat-completion API takes for prose and none of them takes for bytes.
//! A caller holding an image, a PDF or a recording had nowhere to put it, so it
//! sent the file's *name* and withheld the file — which reads to the model as a
//! question about nothing.
//!
//! [`MessageContent`] is that string or an ordered list of [`ContentPart`]s.
//! The text-only case is still a string on the wire (`#[serde(untagged)]`), so
//! a conversation persisted before this existed still deserializes, and the
//! `String` constructors on [`ChatMessage`] are unchanged.
//!
//! Bytes are held decoded, as `Vec<u8>`. Both providers want base64 on the
//! wire and each spells the envelope differently, so encoding is the
//! provider's job and this type stays the thing a caller actually has.
//!
//! [`ChatMessage`]: crate::ChatMessage
//! [`ChatMessage::content`]: crate::ChatMessage::content

use serde::{Deserialize, Serialize};

/// The content of one message: prose, or parts when some of it is not prose.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// Prose, and nothing else. The wire form is a bare string.
    Text(String),
    /// An ordered list of parts, at least one of which is usually not text.
    Parts(Vec<ContentPart>),
}

impl MessageContent {
    /// The content as one string, when it is text and nothing else.
    ///
    /// `None` for a parts list — including a parts list that happens to hold
    /// only text — because a caller reaching for this wants to know it is
    /// seeing the whole message. Use [`to_text`](Self::to_text) to read what
    /// text there is regardless.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Parts(_) => None,
        }
    }

    /// Every text part, joined with a newline. Parts carrying bytes or a URI
    /// contribute nothing — this is the text, not a description of the
    /// message.
    pub fn to_text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Parts(parts) => parts
                .iter()
                .filter_map(ContentPart::as_text)
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    /// The parts, with a text-only content read as a single text part.
    pub fn parts(&self) -> impl Iterator<Item = &ContentPart> {
        // `Text` has no `ContentPart` to borrow, so the two arms cannot share
        // an iterator over `&ContentPart` without one existing somewhere.
        // Boxing the pair is cheaper than making callers match.
        let iter: Box<dyn Iterator<Item = &ContentPart>> = match self {
            Self::Text(_) => Box::new(std::iter::empty()),
            Self::Parts(parts) => Box::new(parts.iter()),
        };
        iter
    }

    /// Whether any part carries something a text channel cannot: bytes, or a
    /// URI naming them.
    ///
    /// What a consumer checks before deciding a message can go somewhere only
    /// prose fits — a summarizer, a token estimate, a provider that takes no
    /// parts.
    pub fn has_media(&self) -> bool {
        self.parts()
            .any(|part| !matches!(part, ContentPart::Text { .. }))
    }
}

impl From<String> for MessageContent {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&String> for MessageContent {
    fn from(text: &String) -> Self {
        Self::Text(text.clone())
    }
}

impl From<&str> for MessageContent {
    fn from(text: &str) -> Self {
        Self::Text(text.to_string())
    }
}

impl From<Vec<ContentPart>> for MessageContent {
    fn from(parts: Vec<ContentPart>) -> Self {
        Self::Parts(parts)
    }
}

impl From<ContentPart> for MessageContent {
    fn from(part: ContentPart) -> Self {
        Self::Parts(vec![part])
    }
}

/// `Vec<u8>` in a `Debug` line is one number per byte. A megabyte of PDF in a
/// log is not a diagnostic, so the bytes are reported as a count.
impl std::fmt::Debug for MessageContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text(text) => f.debug_tuple("Text").field(text).finish(),
            Self::Parts(parts) => f.debug_tuple("Parts").field(parts).finish(),
        }
    }
}

/// One part of a message.
///
/// `#[non_exhaustive]`: the set grows with what providers accept. Build one
/// with [`text`](Self::text), [`blob`](Self::blob) or [`uri`](Self::uri).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ContentPart {
    /// Prose.
    Text { text: String },
    /// Bytes carried in the request itself, with the MIME type that says how
    /// to read them.
    ///
    /// Every provider caps how much it will take this way (a few megabytes);
    /// past that the file belongs behind a [`Uri`](Self::Uri).
    Blob {
        /// The MIME type, e.g. `image/png`, `application/pdf`. Required: it is
        /// what decides how each provider frames the bytes, and a provider
        /// given no type has nothing to guess from.
        mime_type: String,
        #[serde(with = "base64_bytes")]
        data: Vec<u8>,
        /// The file name, where there is one. Sent where the provider has a
        /// field for it; a model reads it as a label.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// Bytes the provider fetches, named by URI.
    ///
    /// Gemini takes any MIME type this way. OpenAI takes only images; anything
    /// else reaches the model as a line of text naming the file, because there
    /// is no field on that API to put it in.
    Uri {
        mime_type: String,
        uri: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
}

impl ContentPart {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn blob(mime_type: impl Into<String>, data: Vec<u8>) -> Self {
        Self::Blob {
            mime_type: mime_type.into(),
            data,
            name: None,
        }
    }

    pub fn uri(mime_type: impl Into<String>, uri: impl Into<String>) -> Self {
        Self::Uri {
            mime_type: mime_type.into(),
            uri: uri.into(),
            name: None,
        }
    }

    /// The same part with a file name attached. No-op on a text part, which
    /// has nothing to name.
    pub fn named(mut self, file_name: impl Into<String>) -> Self {
        match &mut self {
            Self::Text { .. } => {}
            Self::Blob { name, .. } | Self::Uri { name, .. } => *name = Some(file_name.into()),
        }
        self
    }

    /// The text of a text part; `None` for anything else.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }

    /// The MIME type, for a part that has one.
    pub fn mime_type(&self) -> Option<&str> {
        match self {
            Self::Text { .. } => None,
            Self::Blob { mime_type, .. } | Self::Uri { mime_type, .. } => Some(mime_type),
        }
    }

    /// The file name, for a part that was given one.
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Text { .. } => None,
            Self::Blob { name, .. } | Self::Uri { name, .. } => name.as_deref(),
        }
    }

    /// A file's name and type as one phrase, in whatever combination it
    /// actually has — for the places a provider can only name what it could
    /// not send.
    pub(crate) fn describe(&self) -> String {
        match (self.name(), self.mime_type()) {
            (Some(name), Some(mime)) => format!("{name} ({mime})"),
            (Some(name), None) => name.to_string(),
            (None, Some(mime)) => format!("unnamed {mime} file"),
            (None, None) => "unnamed file".to_string(),
        }
    }
}

/// See [`MessageContent`]'s `Debug`: the bytes are a count.
impl std::fmt::Debug for ContentPart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text { text } => f.debug_struct("Text").field("text", text).finish(),
            Self::Blob {
                mime_type,
                data,
                name,
            } => f
                .debug_struct("Blob")
                .field("mime_type", mime_type)
                .field("name", name)
                .field("data", &format_args!("{} bytes", data.len()))
                .finish(),
            Self::Uri {
                mime_type,
                uri,
                name,
            } => f
                .debug_struct("Uri")
                .field("mime_type", mime_type)
                .field("uri", uri)
                .field("name", name)
                .finish(),
        }
    }
}

/// Bytes as base64 in JSON, rather than the array of numbers `Vec<u8>`
/// derives. A stored conversation is read by things other than this crate, and
/// a 300-element array where every other tool writes a string is a trap.
mod base64_bytes {
    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        base64::engine::general_purpose::STANDARD
            .decode(encoded.as_bytes())
            .map_err(D::Error::custom)
    }
}

/// The base64 a provider puts on the wire.
pub(crate) fn encode_base64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// A `data:` URL, which is how OpenAI takes bytes for every part shape it has.
pub(crate) fn data_url(mime_type: &str, bytes: &[u8]) -> String {
    format!("data:{mime_type};base64,{}", encode_base64(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The point of `untagged`: a message written before parts existed is
    /// still readable, and a text-only message written now is still readable
    /// by whatever reads the old shape.
    #[test]
    fn text_content_is_a_bare_string_on_the_wire() {
        let content = MessageContent::from("hei");
        assert_eq!(
            serde_json::to_value(&content).expect("serializes"),
            serde_json::json!("hei")
        );
        let back: MessageContent =
            serde_json::from_value(serde_json::json!("hei")).expect("deserializes");
        assert_eq!(back, content);
    }

    /// Bytes round-trip as base64, not as an array of numbers.
    #[test]
    fn a_blob_is_base64_on_the_wire() {
        let content = MessageContent::from(vec![
            ContentPart::text("what is this"),
            ContentPart::blob("image/png", vec![0, 1, 2]).named("shot.png"),
        ]);
        let json = serde_json::to_value(&content).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!([
                { "type": "text", "text": "what is this" },
                { "type": "blob", "mime_type": "image/png", "data": "AAEC", "name": "shot.png" },
            ])
        );
        let back: MessageContent = serde_json::from_value(json).expect("deserializes");
        assert_eq!(back, content);
    }

    /// A parts list holding only text is still a parts list: `as_text` is
    /// "this is the whole message", which is what a caller replacing the
    /// content needs to know.
    #[test]
    fn as_text_speaks_only_for_a_whole_text_message() {
        assert_eq!(MessageContent::from("hei").as_text(), Some("hei"));
        let parts = MessageContent::from(vec![ContentPart::text("hei")]);
        assert_eq!(parts.as_text(), None);
        assert_eq!(parts.to_text(), "hei");
    }

    /// What a consumer checks before sending a message somewhere only prose
    /// fits.
    #[test]
    fn media_is_what_a_text_channel_cannot_carry() {
        assert!(!MessageContent::from("hei").has_media());
        assert!(!MessageContent::from(vec![ContentPart::text("hei")]).has_media());
        assert!(MessageContent::from(ContentPart::blob("image/png", vec![1])).has_media());
        assert!(MessageContent::from(ContentPart::uri("image/png", "https://x/y.png")).has_media());
    }

    /// The bytes are a count, so a logged message stays a diagnostic.
    #[test]
    fn debug_does_not_print_the_bytes() {
        let content = MessageContent::from(ContentPart::blob("application/pdf", vec![7; 4096]));
        let rendered = format!("{content:?}");
        assert!(rendered.contains("4096 bytes"), "{rendered}");
        assert!(!rendered.contains(", 7,"), "{rendered}");
    }
}
