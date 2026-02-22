//! Generates lightweight section table-of-contents entries from a PaperDocument.

use serde::{Deserialize, Serialize};

use crate::types::{PaperDocument, Section};

/// Lightweight section metadata — the "table of contents" entry.
///
/// Does not contain block text; only summary statistics for quick inspection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionEntry {
    /// Position in the top-level sections list (or within parent's children list).
    pub index: usize,
    /// Unique string path: numbering string ("II.", "3.1") or clean header text.
    pub path: String,
    /// Clean header text (numbering stripped).
    pub header: String,
    /// Raw header text as it appeared in the PDF (e.g. "II. KNOWLEDGE GRAPHS").
    pub header_raw: String,
    /// Hierarchy level: 1 = top-level section.
    pub level: u8,
    /// Page range (first_page, last_page) — zero-based page numbers.
    pub page_range: (u32, u32),
    /// Estimated token count for the section text (~4 ASCII chars per token).
    pub estimated_tokens: usize,
    /// True if this section contains any figures.
    pub has_figures: bool,
    /// Number of figures in this section.
    pub figure_count: usize,
    /// True if this section contains any tables.
    pub has_tables: bool,
    /// Number of tables in this section.
    pub table_count: usize,
    /// Child section entries.
    pub children: Vec<SectionEntry>,
}

impl SectionEntry {
    /// Format for CLI display: `[L1] INTRODUCTION  p.1-2  ~1200 tokens`.
    pub fn display_line(&self) -> String {
        let indent = "  ".repeat(self.level.saturating_sub(1) as usize);
        let fig = if self.figure_count > 0 {
            format!("  fig:{}", self.figure_count)
        } else {
            String::new()
        };
        let tab = if self.table_count > 0 {
            format!("  tab:{}", self.table_count)
        } else {
            String::new()
        };
        format!(
            "{}[{}] {}  p.{}-{}  ~{} tokens{}{}",
            indent,
            self.level,
            self.header,
            self.page_range.0,
            self.page_range.1,
            self.estimated_tokens,
            fig,
            tab,
        )
    }
}

/// Generates `SectionEntry` records from a `PaperDocument`.
pub struct TocGenerator;

impl TocGenerator {
    /// Generate a full table of contents from the document's section tree.
    pub fn generate(doc: &PaperDocument) -> Vec<SectionEntry> {
        doc.sections
            .iter()
            .enumerate()
            .map(|(i, s)| Self::section_to_entry(s, i))
            .collect()
    }

    fn section_to_entry(section: &Section, index: usize) -> SectionEntry {
        let text = section.full_text();
        let estimated_tokens = Self::estimate_tokens(&text);

        let path = section
            .header
            .as_ref()
            .map(|h| h.numbering.clone().unwrap_or_else(|| h.clean_text.clone()))
            .unwrap_or_else(|| format!("section_{index}"));

        SectionEntry {
            index,
            path,
            header: section.header_text(),
            header_raw: section
                .header
                .as_ref()
                .map(|h| h.text.clone())
                .unwrap_or_default(),
            level: section.level,
            page_range: section.page_range,
            estimated_tokens,
            has_figures: !section.figures.is_empty(),
            figure_count: section.figures.len(),
            has_tables: !section.tables.is_empty(),
            table_count: section.tables.len(),
            children: section
                .children
                .iter()
                .enumerate()
                .map(|(i, c)| Self::section_to_entry(c, i))
                .collect(),
        }
    }

    /// Estimate token count: ~4 ASCII chars/token, ~1.5 non-ASCII chars/token.
    pub fn estimate_tokens(text: &str) -> usize {
        let ascii = text.chars().filter(|c| c.is_ascii()).count();
        let non_ascii = text.chars().filter(|c| !c.is_ascii()).count();
        ascii / 4 + (non_ascii as f64 / 1.5) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DocumentMetadata, PaperDocument, Rect, Section, SectionHeader};

    fn make_doc(sections: Vec<Section>) -> PaperDocument {
        PaperDocument {
            paper_id: "test".to_string(),
            source_file: std::path::PathBuf::from("test.pdf"),
            metadata: DocumentMetadata::default(),
            sections,
            all_figures: vec![],
            all_tables: vec![],
        }
    }

    fn make_section(header: &str, level: u8, page: u32) -> Section {
        Section {
            header: Some(SectionHeader {
                text: header.to_string(),
                clean_text: header.to_string(),
                level,
                numbering: None,
                page,
                bbox: Rect::new(72.0, 700.0, 540.0, 690.0),
                block_index: 0,
            }),
            level,
            blocks: vec![],
            figures: vec![],
            tables: vec![],
            children: vec![],
            page_range: (page, page),
        }
    }

    #[test]
    fn toc_entry_count_matches_sections() {
        let doc = make_doc(vec![
            make_section("INTRODUCTION", 1, 0),
            make_section("METHODS", 1, 1),
        ]);
        let toc = TocGenerator::generate(&doc);
        assert_eq!(toc.len(), 2);
        assert_eq!(toc[0].header, "INTRODUCTION");
        assert_eq!(toc[1].header, "METHODS");
    }

    #[test]
    fn toc_entry_has_correct_level() {
        let doc = make_doc(vec![make_section("RESULTS", 1, 2)]);
        let toc = TocGenerator::generate(&doc);
        assert_eq!(toc[0].level, 1);
    }

    #[test]
    fn estimate_tokens_ascii() {
        // 40 ASCII chars / 4 = 10 tokens
        let tokens = TocGenerator::estimate_tokens("This is a test string with 40 chars!!!!!");
        assert!(tokens > 0);
    }
}
