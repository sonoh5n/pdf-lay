//! Table to text conversion.
//!
//! Converts `TableGrid` into `TableRepresentation` (Markdown/CSV/PlainText).

use crate::types::TableRepresentation;

use super::grid_builder::TableGrid;

/// Converts a `TableGrid` into various text representations.
pub struct TableTextConverter;

impl TableTextConverter {
    /// Convert grid to Markdown table format.
    pub fn to_markdown(grid: &TableGrid, caption: Option<&str>) -> TableRepresentation {
        let mut md = String::new();

        // Build the header row strings.
        // For multi-header grids the last header row is used as the Markdown column labels.
        let header_strs: Vec<String> = if grid.header.is_empty() {
            // Generate placeholder header columns when no header row was detected.
            (0..grid.column_count)
                .map(|i| format!("Col{}", i + 1))
                .collect()
        } else {
            // Use the last header row (handles multi-header by flattening to the deepest level).
            grid.header.last().unwrap().clone()
        };

        // Markdown has no multi-row-header syntax, so upper header rows
        // (group/spanning labels above the column labels) would otherwise be
        // silently dropped. Emit them as a bold annotation line immediately
        // before the table instead (No Silent Drop; see
        // docs/refactor/phase2_llm_output.md#P2-8). The full, un-flattened
        // rows are also carried in `header_rows` below.
        if grid.header.len() > 1 {
            for upper_row in &grid.header[..grid.header.len() - 1] {
                let non_empty: Vec<String> = upper_row
                    .iter()
                    .filter(|c| !c.is_empty())
                    .map(|c| Self::escape_pipe(c))
                    .collect();
                if !non_empty.is_empty() {
                    md.push_str("**");
                    md.push_str(&non_empty.join(" | "));
                    md.push_str("**\n\n");
                }
            }
        }

        // | Col1 | Col2 | Col3 |
        md.push_str("| ");
        md.push_str(
            &header_strs
                .iter()
                .map(|h| Self::escape_pipe(h))
                .collect::<Vec<_>>()
                .join(" | "),
        );
        md.push_str(" |\n");

        // | --- | --- | --- |
        md.push_str("| ");
        md.push_str(
            &header_strs
                .iter()
                .map(|_| "---".to_string())
                .collect::<Vec<_>>()
                .join(" | "),
        );
        md.push_str(" |\n");

        // Data rows.
        for row in &grid.rows {
            md.push_str("| ");
            let padded: Vec<String> = (0..grid.column_count)
                .map(|i| Self::escape_pipe(row.get(i).map(|s| s.as_str()).unwrap_or("")))
                .collect();
            md.push_str(&padded.join(" | "));
            md.push_str(" |\n");
        }

        TableRepresentation::Markdown {
            header: header_strs,
            rows: grid.rows.clone(),
            caption: caption.map(|s| s.to_string()),
            markdown_text: md,
            header_rows: grid.header.clone(),
        }
    }

    /// Convert grid to CSV format.
    pub fn to_csv(grid: &TableGrid, caption: Option<&str>) -> TableRepresentation {
        let mut csv = String::new();

        let header_strs: Vec<String> = if grid.header.is_empty() {
            (0..grid.column_count)
                .map(|i| format!("Col{}", i + 1))
                .collect()
        } else {
            grid.header.last().unwrap().clone()
        };

        // Header row(s). Unlike Markdown, CSV has no column-label
        // restriction, so every header row is emitted as its own leading
        // CSV line — multi-row headers are not flattened away (No Silent
        // Drop; see docs/refactor/phase2_llm_output.md#P2-8).
        if grid.header.is_empty() {
            csv.push_str(
                &header_strs
                    .iter()
                    .map(|h| Self::escape_csv(h))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            csv.push('\n');
        } else {
            for row in &grid.header {
                csv.push_str(
                    &row.iter()
                        .map(|h| Self::escape_csv(h))
                        .collect::<Vec<_>>()
                        .join(","),
                );
                csv.push('\n');
            }
        }

        // Data rows.
        for row in &grid.rows {
            let padded: Vec<String> = (0..grid.column_count)
                .map(|i| Self::escape_csv(row.get(i).map(|s| s.as_str()).unwrap_or("")))
                .collect();
            csv.push_str(&padded.join(","));
            csv.push('\n');
        }

        TableRepresentation::Csv {
            header: header_strs,
            rows: grid.rows.clone(),
            caption: caption.map(|s| s.to_string()),
            csv_text: csv,
        }
    }

    /// Convert grid to plain text (tab-separated) format.
    pub fn to_plain_text(grid: &TableGrid, caption: Option<&str>) -> TableRepresentation {
        let mut text = String::new();

        for row in &grid.header {
            text.push_str(&row.join("\t"));
            text.push('\n');
        }
        for row in &grid.rows {
            text.push_str(&row.join("\t"));
            text.push('\n');
        }

        TableRepresentation::PlainText {
            text,
            caption: caption.map(|s| s.to_string()),
        }
    }

    /// Escape a cell value for safe inclusion in a Markdown table.
    ///
    /// Neutralizes pipe characters, newlines, HTML tags, and Markdown link injection.
    pub fn escape_pipe(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace("](", "]\\(")
            .replace('|', "\\|")
            .replace('\n', " ")
    }

    /// Escape a cell value for CSV output.
    ///
    /// Fields containing a comma, double-quote, or newline are wrapped in double quotes.
    /// Existing double-quote characters are escaped by doubling them.
    pub fn escape_csv(s: &str) -> String {
        if s.contains(',') || s.contains('"') || s.contains('\n') {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a small 2-column, 2-row grid for reuse across tests.
    fn sample_grid() -> TableGrid {
        TableGrid {
            header: vec![vec!["Name".into(), "Score".into()]],
            rows: vec![
                vec!["Alice".into(), "95".into()],
                vec!["Bob".into(), "87".into()],
            ],
            column_count: 2,
            has_multi_header: false,
            header_rows: vec![],
        }
    }

    #[test]
    fn test_markdown_output() {
        let grid = sample_grid();
        if let TableRepresentation::Markdown { markdown_text, .. } =
            TableTextConverter::to_markdown(&grid, None)
        {
            assert!(
                markdown_text.contains("| Name | Score |"),
                "header row not found in:\n{markdown_text}"
            );
            assert!(
                markdown_text.contains("| --- | --- |"),
                "separator row not found in:\n{markdown_text}"
            );
            assert!(
                markdown_text.contains("| Alice | 95 |"),
                "first data row not found in:\n{markdown_text}"
            );
            assert!(
                markdown_text.contains("| Bob | 87 |"),
                "second data row not found in:\n{markdown_text}"
            );
        } else {
            panic!("expected TableRepresentation::Markdown");
        }
    }

    #[test]
    fn test_csv_output() {
        let grid = sample_grid();
        if let TableRepresentation::Csv { csv_text, .. } = TableTextConverter::to_csv(&grid, None) {
            assert!(
                csv_text.contains("Name,Score"),
                "CSV header not found in:\n{csv_text}"
            );
            assert!(
                csv_text.contains("Alice,95"),
                "CSV row 1 not found in:\n{csv_text}"
            );
            assert!(
                csv_text.contains("Bob,87"),
                "CSV row 2 not found in:\n{csv_text}"
            );
        } else {
            panic!("expected TableRepresentation::Csv");
        }
    }

    #[test]
    fn test_plain_text_output() {
        let grid = sample_grid();
        if let TableRepresentation::PlainText { text, .. } =
            TableTextConverter::to_plain_text(&grid, None)
        {
            assert!(
                text.contains("Name\tScore"),
                "plain text header not found in:\n{text}"
            );
            assert!(
                text.contains("Alice\t95"),
                "plain text row 1 not found in:\n{text}"
            );
        } else {
            panic!("expected TableRepresentation::PlainText");
        }
    }

    #[test]
    fn test_pipe_escape() {
        assert_eq!(TableTextConverter::escape_pipe("a|b"), "a\\|b");
        assert_eq!(TableTextConverter::escape_pipe("no pipes"), "no pipes");
        // Newlines in cells are replaced with a space.
        assert_eq!(
            TableTextConverter::escape_pipe("line1\nline2"),
            "line1 line2"
        );
    }

    #[test]
    fn test_escape_pipe_neutralizes_html() {
        assert_eq!(
            TableTextConverter::escape_pipe("<script>alert(1)</script>"),
            "&lt;script&gt;alert(1)&lt;/script&gt;"
        );
    }

    #[test]
    fn test_escape_pipe_prevents_link_injection() {
        assert_eq!(
            TableTextConverter::escape_pipe("[click](http://evil.com)"),
            "[click]\\(http://evil.com)"
        );
    }

    #[test]
    fn test_csv_escape_with_comma() {
        assert_eq!(TableTextConverter::escape_csv("a,b"), "\"a,b\"");
    }

    #[test]
    fn test_csv_escape_with_quote() {
        assert_eq!(
            TableTextConverter::escape_csv("say \"hi\""),
            "\"say \"\"hi\"\"\""
        );
    }

    #[test]
    fn test_csv_no_escape_needed() {
        assert_eq!(TableTextConverter::escape_csv("plain text"), "plain text");
    }

    #[test]
    fn test_markdown_with_caption() {
        let grid = sample_grid();
        if let TableRepresentation::Markdown { caption, .. } =
            TableTextConverter::to_markdown(&grid, Some("Table 1: Example"))
        {
            assert_eq!(caption, Some("Table 1: Example".to_string()));
        } else {
            panic!("expected TableRepresentation::Markdown");
        }
    }

    #[test]
    fn test_markdown_pipe_in_cell_is_escaped() {
        let grid = TableGrid {
            header: vec![vec!["A|B".into(), "C".into()]],
            rows: vec![vec!["x|y".into(), "z".into()]],
            column_count: 2,
            has_multi_header: false,
            header_rows: vec![],
        };
        if let TableRepresentation::Markdown { markdown_text, .. } =
            TableTextConverter::to_markdown(&grid, None)
        {
            assert!(
                markdown_text.contains("A\\|B"),
                "pipe in header cell should be escaped:\n{markdown_text}"
            );
            assert!(
                markdown_text.contains("x\\|y"),
                "pipe in data cell should be escaped:\n{markdown_text}"
            );
        } else {
            panic!("expected TableRepresentation::Markdown");
        }
    }

    #[test]
    fn test_empty_grid_generates_placeholder_headers() {
        let grid = TableGrid {
            header: vec![],
            rows: vec![vec!["val1".into(), "val2".into()]],
            column_count: 2,
            has_multi_header: false,
            header_rows: vec![],
        };
        if let TableRepresentation::Markdown { markdown_text, .. } =
            TableTextConverter::to_markdown(&grid, None)
        {
            assert!(
                markdown_text.contains("| Col1 | Col2 |"),
                "placeholder headers not found:\n{markdown_text}"
            );
        } else {
            panic!("expected TableRepresentation::Markdown");
        }
    }

    /// P2-8: a 2-row header (grouped label + sub-headers) must not be
    /// flattened to only the last row in Markdown output — the upper row
    /// should still appear somewhere in `markdown_text`, and `header_rows`
    /// should carry every row un-flattened.
    #[test]
    fn multi_header_not_flattened_markdown() {
        let grid = TableGrid {
            header: vec![
                vec!["Metrics".into(), "Metrics".into(), "Other".into()],
                vec!["Accuracy".into(), "Precision".into(), "X".into()],
            ],
            rows: vec![vec!["0.9".into(), "0.8".into(), "1".into()]],
            column_count: 3,
            has_multi_header: true,
            header_rows: vec![],
        };
        if let TableRepresentation::Markdown {
            markdown_text,
            header_rows,
            ..
        } = TableTextConverter::to_markdown(&grid, None)
        {
            assert!(
                markdown_text.contains("Metrics"),
                "upper header text should be preserved:\n{markdown_text}"
            );
            assert!(
                markdown_text.contains("| Accuracy | Precision | X |"),
                "last header row should become the Markdown column labels:\n{markdown_text}"
            );
            assert_eq!(header_rows.len(), 2, "both header rows should be carried");
            assert_eq!(
                header_rows[0],
                vec![
                    "Metrics".to_string(),
                    "Metrics".to_string(),
                    "Other".to_string()
                ]
            );
        } else {
            panic!("expected TableRepresentation::Markdown");
        }
    }

    /// P2-8: CSV output emits every header row as its own leading line,
    /// rather than flattening to the last row only.
    #[test]
    fn multi_header_preserved_csv() {
        let grid = TableGrid {
            header: vec![
                vec!["Group".into(), "Group".into()],
                vec!["A".into(), "B".into()],
            ],
            rows: vec![vec!["1".into(), "2".into()]],
            column_count: 2,
            has_multi_header: true,
            header_rows: vec![],
        };
        if let TableRepresentation::Csv { csv_text, .. } = TableTextConverter::to_csv(&grid, None) {
            let lines: Vec<&str> = csv_text.lines().collect();
            assert_eq!(lines.len(), 3, "2 header rows + 1 data row:\n{csv_text}");
            assert_eq!(lines[0], "Group,Group");
            assert_eq!(lines[1], "A,B");
            assert_eq!(lines[2], "1,2");
        } else {
            panic!("expected TableRepresentation::Csv");
        }
    }
}
