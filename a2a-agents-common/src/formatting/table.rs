//! Table formatting utilities.

/// Builder for creating markdown-formatted tables.
///
/// # Example
///
/// ```
/// use a2a_agents_common::formatting::TableFormatter;
///
/// let table = TableFormatter::new()
///     .header(&["Symbol", "Price", "Change"])
///     .row(&["AAPL", "$178.42", "+1.22%"])
///     .row(&["TSLA", "$242.84", "+2.15%"])
///     .build();
///
/// assert!(table.contains("Symbol") && table.contains("Price") && table.contains("Change"));
/// ```
#[derive(Debug, Clone)]
pub struct TableFormatter {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    alignment: Vec<Alignment>,
}

/// Column alignment for table formatting.
#[derive(Debug, Clone, Copy)]
pub enum Alignment {
    Left,
    Center,
    Right,
}

impl TableFormatter {
    /// Create a new table formatter.
    pub fn new() -> Self {
        Self {
            headers: Vec::new(),
            rows: Vec::new(),
            alignment: Vec::new(),
        }
    }

    /// Set the table headers.
    pub fn header(mut self, headers: &[&str]) -> Self {
        self.headers = headers.iter().map(|s| s.to_string()).collect();
        self.alignment = vec![Alignment::Left; headers.len()];
        self
    }

    /// Set column alignment.
    pub fn align(mut self, column: usize, alignment: Alignment) -> Self {
        if column < self.alignment.len() {
            self.alignment[column] = alignment;
        }
        self
    }

    /// Add a row to the table.
    pub fn row(mut self, cells: &[&str]) -> Self {
        self.rows
            .push(cells.iter().map(|s| s.to_string()).collect());
        self
    }

    /// Build the markdown table string.
    pub fn build(&self) -> String {
        if self.headers.is_empty() {
            return String::new();
        }

        let mut result = String::new();

        // Calculate column widths
        let mut widths: Vec<usize> = self.headers.iter().map(|h| h.len()).collect();
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(cell.len());
                }
            }
        }

        // Header row
        result.push_str("| ");
        for (i, header) in self.headers.iter().enumerate() {
            let width = widths.get(i).copied().unwrap_or(header.len());
            result.push_str(&self.pad(header, width, Alignment::Left));
            result.push_str(" | ");
        }
        result.push('\n');

        // Separator row
        result.push('|');
        for (i, width) in widths.iter().enumerate() {
            let alignment = self.alignment.get(i).copied().unwrap_or(Alignment::Left);
            result.push_str(&self.separator(*width, alignment));
            result.push('|');
        }
        result.push('\n');

        // Data rows
        for row in &self.rows {
            result.push_str("| ");
            for (i, cell) in row.iter().enumerate() {
                let width = widths.get(i).copied().unwrap_or(cell.len());
                let alignment = self.alignment.get(i).copied().unwrap_or(Alignment::Left);
                result.push_str(&self.pad(cell, width, alignment));
                result.push_str(" | ");
            }
            result.push('\n');
        }

        result
    }

    fn pad(&self, text: &str, width: usize, alignment: Alignment) -> String {
        if text.len() >= width {
            return text.to_string();
        }

        let padding = width - text.len();
        match alignment {
            Alignment::Left => format!("{}{}", text, " ".repeat(padding)),
            Alignment::Right => format!("{}{}", " ".repeat(padding), text),
            Alignment::Center => {
                let left_pad = padding / 2;
                let right_pad = padding - left_pad;
                format!("{}{}{}", " ".repeat(left_pad), text, " ".repeat(right_pad))
            }
        }
    }

    fn separator(&self, width: usize, alignment: Alignment) -> String {
        let dashes = "-".repeat(width);
        match alignment {
            Alignment::Left => format!(" {dashes} "),
            Alignment::Right => format!(" {dashes}:"),
            Alignment::Center => format!(":{dashes}:"),
        }
    }
}

impl Default for TableFormatter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_table() {
        let table = TableFormatter::new()
            .header(&["Name", "Age"])
            .row(&["Alice", "30"])
            .row(&["Bob", "25"])
            .build();

        assert!(table.contains("| Name  | Age |"));
        assert!(table.contains("| Alice | 30  |"));
        assert!(table.contains("| Bob   | 25  |"));
    }

    #[test]
    fn test_alignment() {
        let table = TableFormatter::new()
            .header(&["Left", "Center", "Right"])
            .align(0, Alignment::Left)
            .align(1, Alignment::Center)
            .align(2, Alignment::Right)
            .row(&["A", "B", "C"])
            .build();

        assert!(table.contains("| Left | Center | Right |"));
        // Check separator row for alignment markers
        assert!(table.contains("----")); // Left aligned
        assert!(table.contains(":")); // Center has colons
        assert!(table.contains(":")); // Right has colon
    }

    #[test]
    fn test_varying_widths() {
        let table = TableFormatter::new()
            .header(&["Short", "Medium", "VeryLongHeader"])
            .row(&["A", "B", "C"])
            .row(&["XXX", "YYY", "ZZZ"])
            .build();

        // Should pad to match the longest content in each column
        assert!(table.contains("VeryLongHeader"));
    }

    #[test]
    fn test_empty_table() {
        let table = TableFormatter::new().build();
        assert_eq!(table, "");
    }

    #[test]
    fn test_stock_example() {
        let table = TableFormatter::new()
            .header(&["Symbol", "Price", "Change"])
            .align(1, Alignment::Right)
            .align(2, Alignment::Right)
            .row(&["AAPL", "$178.42", "+1.22%"])
            .row(&["TSLA", "$242.84", "+2.15%"])
            .build();

        assert!(table.contains("Symbol"));
        assert!(table.contains("AAPL"));
        assert!(table.contains("$178.42"));
    }
}
