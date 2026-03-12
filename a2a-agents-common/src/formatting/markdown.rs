//! Markdown formatting utilities.

/// Helper for building markdown-formatted text.
///
/// # Example
///
/// ```
/// use a2a_agents_common::formatting::MarkdownFormatter;
///
/// let md = MarkdownFormatter::new()
///     .heading(1, "Stock Analysis")
///     .paragraph("Here's the current price:")
///     .bold("AAPL: $178.42")
///     .build();
///
/// assert!(md.contains("# Stock Analysis"));
/// ```
#[derive(Debug, Clone, Default)]
pub struct MarkdownFormatter {
    content: Vec<String>,
}

impl MarkdownFormatter {
    /// Create a new markdown formatter.
    pub fn new() -> Self {
        Self {
            content: Vec::new(),
        }
    }

    /// Add a heading at the specified level (1-6).
    pub fn heading(mut self, level: u8, text: &str) -> Self {
        let level = level.min(6).max(1);
        let prefix = "#".repeat(level as usize);
        self.content.push(format!("{} {}\n", prefix, text));
        self
    }

    /// Add a paragraph of text.
    pub fn paragraph(mut self, text: &str) -> Self {
        self.content.push(format!("{}\n", text));
        self
    }

    /// Add bold text.
    pub fn bold(mut self, text: &str) -> Self {
        self.content.push(format!("**{}**\n", text));
        self
    }

    /// Add italic text.
    pub fn italic(mut self, text: &str) -> Self {
        self.content.push(format!("*{}*\n", text));
        self
    }

    /// Add a code block with optional language.
    pub fn code_block(mut self, code: &str, language: Option<&str>) -> Self {
        if let Some(lang) = language {
            self.content.push(format!("```{}\n{}\n```\n", lang, code));
        } else {
            self.content.push(format!("```\n{}\n```\n", code));
        }
        self
    }

    /// Add inline code.
    pub fn inline_code(mut self, code: &str) -> Self {
        self.content.push(format!("`{}`", code));
        self
    }

    /// Add a bullet list.
    pub fn bullet_list(mut self, items: &[&str]) -> Self {
        for item in items {
            self.content.push(format!("- {}\n", item));
        }
        self.content.push("\n".to_string());
        self
    }

    /// Add a numbered list.
    pub fn numbered_list(mut self, items: &[&str]) -> Self {
        for (i, item) in items.iter().enumerate() {
            self.content.push(format!("{}. {}\n", i + 1, item));
        }
        self.content.push("\n".to_string());
        self
    }

    /// Add a horizontal rule.
    pub fn horizontal_rule(mut self) -> Self {
        self.content.push("---\n".to_string());
        self
    }

    /// Add a blockquote.
    pub fn blockquote(mut self, text: &str) -> Self {
        self.content.push(format!("> {}\n", text));
        self
    }

    /// Add a link.
    pub fn link(mut self, text: &str, url: &str) -> Self {
        self.content.push(format!("[{}]({})", text, url));
        self
    }

    /// Add an image.
    pub fn image(mut self, alt: &str, url: &str) -> Self {
        self.content.push(format!("![{}]({})\n", alt, url));
        self
    }

    /// Add raw text without formatting.
    pub fn raw(mut self, text: &str) -> Self {
        self.content.push(text.to_string());
        self
    }

    /// Add a newline.
    pub fn newline(mut self) -> Self {
        self.content.push("\n".to_string());
        self
    }

    /// Build the final markdown string.
    pub fn build(&self) -> String {
        self.content.join("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heading() {
        let md = MarkdownFormatter::new().heading(1, "Title").build();
        assert_eq!(md, "# Title\n");

        let md = MarkdownFormatter::new().heading(2, "Subtitle").build();
        assert_eq!(md, "## Subtitle\n");
    }

    #[test]
    fn test_paragraph() {
        let md = MarkdownFormatter::new()
            .paragraph("This is a paragraph.")
            .build();
        assert_eq!(md, "This is a paragraph.\n");
    }

    #[test]
    fn test_bold_italic() {
        let md = MarkdownFormatter::new()
            .bold("Bold text")
            .italic("Italic text")
            .build();
        assert!(md.contains("**Bold text**"));
        assert!(md.contains("*Italic text*"));
    }

    #[test]
    fn test_code_block() {
        let md = MarkdownFormatter::new()
            .code_block("let x = 5;", Some("rust"))
            .build();
        assert!(md.contains("```rust"));
        assert!(md.contains("let x = 5;"));
    }

    #[test]
    fn test_bullet_list() {
        let md = MarkdownFormatter::new()
            .bullet_list(&["Item 1", "Item 2", "Item 3"])
            .build();
        assert!(md.contains("- Item 1"));
        assert!(md.contains("- Item 2"));
    }

    #[test]
    fn test_numbered_list() {
        let md = MarkdownFormatter::new()
            .numbered_list(&["First", "Second", "Third"])
            .build();
        assert!(md.contains("1. First"));
        assert!(md.contains("2. Second"));
        assert!(md.contains("3. Third"));
    }

    #[test]
    fn test_complex_document() {
        let md = MarkdownFormatter::new()
            .heading(1, "Report")
            .paragraph("Summary of findings:")
            .bullet_list(&["Finding 1", "Finding 2"])
            .horizontal_rule()
            .heading(2, "Details")
            .code_block("example code", Some("python"))
            .build();

        assert!(md.contains("# Report"));
        assert!(md.contains("## Details"));
        assert!(md.contains("---"));
    }
}
