# Task 14: TocGenerator + SectionSelector

## Overview

Implement the selector module: `TocGenerator` converts `PaperDocument::sections` into
lightweight `SectionEntry` records (with estimated token counts, figure/table counts, etc.),
and `SectionSelector` provides five selection modes: by name, by index, by level, by page
range, and by predicate.

`SectionSelector` is a borrowed view (`'a` lifetime over `PaperDocument`) to avoid cloning
the full document. For PyO3 compatibility (Task 19), it also exposes `selected_indices()`.

## Related Information
- **Plan**: `/Users/sonokawa/.claude/plans/fuzzy-drifting-summit.md` (Task 14)
- **Design doc**: `docs/arch/02_DESIGN.md` § 2.7 selector
- **Spec**: `docs/arch/01_SPECIFICATION.md` § 2.14 F-019
- **Overview**: `docs/develop/overview.md`
- **Dependencies**: Task 13 (pipeline) must be completed first

## Files to Create

- [ ] `crates/pdf-lay-core/src/selector/mod.rs`
- [ ] `crates/pdf-lay-core/src/selector/toc.rs`
- [ ] `crates/pdf-lay-core/src/selector/selector.rs`

## Files to Modify

- [ ] `crates/pdf-lay-core/src/lib.rs` — uncomment `pub mod selector;`

## Implementation Steps

### Step 1: `selector/mod.rs`

```rust
//! Section selection layer: TOC generation and selective section output.

mod toc;
mod selector;
mod llm_text;   // Task 15

pub use toc::{SectionEntry, TocGenerator};
pub use selector::SectionSelector;
// pub use llm_text::LlmTextGenerator;  // uncommented in Task 15
```

### Step 2: `selector/toc.rs`

```rust
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
    pub page_range: (u32, u32),
    /// Estimated token count for the section text (~4 ASCII chars per token).
    pub estimated_tokens: usize,
    pub has_figures: bool,
    pub figure_count: usize,
    pub has_tables: bool,
    pub table_count: usize,
    /// Child section entries.
    pub children: Vec<SectionEntry>,
}

impl SectionEntry {
    /// Format for CLI display: `[L1] INTRODUCTION  p.1-2  ~1200 tokens`.
    pub fn display_line(&self) -> String {
        let indent = "  ".repeat((self.level.saturating_sub(1)) as usize);
        let fig = if self.figure_count > 0 { format!("  fig:{}", self.figure_count) } else { String::new() };
        let tab = if self.table_count > 0 { format!("  tab:{}", self.table_count) } else { String::new() };
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
            .map(|h| {
                h.numbering
                    .clone()
                    .unwrap_or_else(|| h.clean_text.clone())
            })
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
    use crate::types::{DocumentMetadata, PaperDocument, Section, SectionHeader, Rect};

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
```

### Step 3: `selector/selector.rs`

```rust
//! Multi-mode section selector providing borrowed views over a PaperDocument.

use crate::types::{PaperDocument, Section};
use crate::selector::toc::{SectionEntry, TocGenerator};

/// A borrowed selection of sections from a `PaperDocument`.
///
/// The `'a` lifetime ties this selector to the document it was created from.
pub struct SectionSelector<'a> {
    doc: &'a PaperDocument,
    selected: Vec<&'a Section>,
}

impl<'a> SectionSelector<'a> {
    // ---- constructors ----

    /// Select sections by header name (partial match, case-insensitive).
    ///
    /// If a parent section matches, its children are automatically included.
    pub fn by_names(doc: &'a PaperDocument, names: &[&str]) -> Self {
        let selected = Self::collect_by_names(&doc.sections, names);
        Self { doc, selected }
    }

    /// Select sections by flat index into the full section tree.
    pub fn by_indices(doc: &'a PaperDocument, indices: &[usize]) -> Self {
        let flat = Self::flatten(&doc.sections);
        let selected = indices
            .iter()
            .filter_map(|&i| flat.get(i).copied())
            .collect();
        Self { doc, selected }
    }

    /// Select all sections at the specified level.
    pub fn by_level(doc: &'a PaperDocument, level: u8) -> Self {
        let selected = Self::flatten(&doc.sections)
            .into_iter()
            .filter(|s| s.level == level)
            .collect();
        Self { doc, selected }
    }

    /// Select sections whose page_range overlaps [start, end].
    pub fn by_pages(doc: &'a PaperDocument, start: u32, end: u32) -> Self {
        let selected = Self::flatten(&doc.sections)
            .into_iter()
            .filter(|s| s.page_range.0 <= end && s.page_range.1 >= start)
            .collect();
        Self { doc, selected }
    }

    /// Select sections using a predicate on `SectionEntry` metadata.
    pub fn by_predicate<F>(doc: &'a PaperDocument, pred: F) -> Self
    where
        F: Fn(&SectionEntry) -> bool,
    {
        let toc = TocGenerator::generate(doc);
        let flat_sections = Self::flatten(&doc.sections);
        let flat_toc = Self::flatten_entries(&toc);

        let selected = flat_sections
            .into_iter()
            .zip(flat_toc.iter())
            .filter(|(_, entry)| pred(entry))
            .map(|(section, _)| section)
            .collect();

        Self { doc, selected }
    }

    // ---- output accessors ----

    /// Reference to the selected sections slice.
    pub fn sections(&self) -> &[&Section] {
        &self.selected
    }

    /// Sum of estimated token counts across all selected sections.
    pub fn total_estimated_tokens(&self) -> usize {
        self.selected
            .iter()
            .map(|s| TocGenerator::estimate_tokens(&s.full_text()))
            .sum()
    }

    /// Flat indices into the document's section tree (used by PyO3 bindings).
    pub fn selected_indices(&self) -> Vec<usize> {
        let flat = Self::flatten(&self.doc.sections);
        self.selected
            .iter()
            .filter_map(|sel| flat.iter().position(|&s| std::ptr::eq(s, *sel)))
            .collect()
    }

    // ---- private helpers ----

    fn collect_by_names(sections: &'a [Section], names: &[&str]) -> Vec<&'a Section> {
        let mut result = Vec::new();
        for section in sections {
            let header_upper = section.header_text().to_uppercase();
            let clean_upper = section
                .header
                .as_ref()
                .map(|h| h.clean_text.to_uppercase())
                .unwrap_or_default();

            let matches = names.iter().any(|name| {
                let upper_name = name.to_uppercase();
                header_upper == upper_name
                    || clean_upper == upper_name
                    || header_upper.contains(&upper_name)
                    || clean_upper.contains(&upper_name)
            });

            if matches {
                // Include the section (children are included via Section::children).
                result.push(section);
            } else {
                // Recurse into children.
                result.extend(Self::collect_by_names(&section.children, names));
            }
        }
        result
    }

    fn flatten(sections: &'a [Section]) -> Vec<&'a Section> {
        let mut result = Vec::new();
        for section in sections {
            result.push(section);
            result.extend(Self::flatten(&section.children));
        }
        result
    }

    fn flatten_entries(entries: &[SectionEntry]) -> Vec<&SectionEntry> {
        let mut result = Vec::new();
        for entry in entries {
            result.push(entry);
            result.extend(Self::flatten_entries(&entry.children));
        }
        result
    }
}

// Add to PaperDocument impl in types/document.rs or in this file as extension:
impl PaperDocument {
    /// Generate the table of contents.
    pub fn toc(&self) -> Vec<SectionEntry> {
        TocGenerator::generate(self)
    }

    /// Select sections by header name (partial match, case-insensitive).
    pub fn select_sections<'a>(&'a self, names: &[&str]) -> SectionSelector<'a> {
        SectionSelector::by_names(self, names)
    }

    /// Select sections by flat index.
    pub fn select_sections_by_index<'a>(&'a self, indices: &[usize]) -> SectionSelector<'a> {
        SectionSelector::by_indices(self, indices)
    }

    /// Select sections by level.
    pub fn select_sections_by_level<'a>(&'a self, level: u8) -> SectionSelector<'a> {
        SectionSelector::by_level(self, level)
    }

    /// Select sections overlapping a page range.
    pub fn select_sections_by_pages<'a>(&'a self, start: u32, end: u32) -> SectionSelector<'a> {
        SectionSelector::by_pages(self, start, end)
    }

    /// Select sections using a predicate function.
    pub fn select_sections_where<'a, F>(&'a self, pred: F) -> SectionSelector<'a>
    where
        F: Fn(&SectionEntry) -> bool,
    {
        SectionSelector::by_predicate(self, pred)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DocumentMetadata, PaperDocument, Rect, Section, SectionHeader};

    fn make_section_with_children(header: &str, level: u8, page: u32, children: Vec<Section>) -> Section {
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
            children,
            page_range: (page, page),
        }
    }

    fn make_doc() -> PaperDocument {
        PaperDocument {
            paper_id: "test".to_string(),
            source_file: std::path::PathBuf::from("test.pdf"),
            metadata: DocumentMetadata::default(),
            sections: vec![
                make_section_with_children("INTRODUCTION", 1, 0, vec![]),
                make_section_with_children("METHODS", 1, 1, vec![
                    make_section_with_children("Data Collection", 2, 1, vec![]),
                ]),
                make_section_with_children("RESULTS", 1, 3, vec![]),
            ],
            all_figures: vec![],
            all_tables: vec![],
        }
    }

    #[test]
    fn select_by_name_partial_match() {
        let doc = make_doc();
        let sel = doc.select_sections(&["RESULT"]);
        assert_eq!(sel.sections().len(), 1);
        assert_eq!(sel.sections()[0].header_text(), "RESULTS");
    }

    #[test]
    fn select_by_level() {
        let doc = make_doc();
        let sel = doc.select_sections_by_level(1);
        assert_eq!(sel.sections().len(), 3); // INTRODUCTION, METHODS, RESULTS
    }

    #[test]
    fn select_by_index() {
        let doc = make_doc();
        let sel = doc.select_sections_by_index(&[0, 2]);
        // Flat order: [INTRODUCTION(0), METHODS(1), Data Collection(2), RESULTS(3)]
        // indices 0 and 2 → INTRODUCTION and Data Collection
        assert_eq!(sel.sections().len(), 2);
    }

    #[test]
    fn select_by_pages() {
        let doc = make_doc();
        let sel = doc.select_sections_by_pages(1, 2);
        // METHODS (p.1) and Data Collection (p.1) should match.
        assert!(!sel.sections().is_empty());
    }

    #[test]
    fn select_by_predicate() {
        let doc = make_doc();
        let sel = doc.select_sections_where(|entry| entry.level == 2);
        assert_eq!(sel.sections().len(), 1);
        assert_eq!(sel.sections()[0].header_text(), "Data Collection");
    }

    #[test]
    fn total_estimated_tokens_sums() {
        let doc = make_doc();
        let sel = doc.select_sections_by_level(1);
        let total = sel.total_estimated_tokens();
        // All sections have empty blocks → 0 tokens each
        assert_eq!(total, 0);
    }
}
```

## Acceptance Criteria

- [ ] All unit tests pass: `cargo test -p pdf-lay-core -- selector`
  - TocGenerator: `toc_entry_count_matches_sections`, `toc_entry_has_correct_level`, `estimate_tokens_ascii`
  - SectionSelector: `select_by_name_partial_match`, `select_by_level`, `select_by_index`, `select_by_pages`, `select_by_predicate`, `total_estimated_tokens_sums`
- [ ] `SectionEntry::display_line()` produces a formatted string suitable for CLI output
- [ ] `SectionSelector::by_names` is case-insensitive and partial-matches
- [ ] `SectionSelector::selected_indices()` returns correct flat indices (used by PyO3)
- [ ] `PaperDocument::toc()` and all `select_sections_*` methods are accessible
- [ ] `cargo clippy -p pdf-lay-core -- -D warnings` passes

## Dependencies

- Task 13 (pipeline + PaperDocument assembly) must be completed first.

## Commit Message

```
feat(selector): add TocGenerator and SectionSelector with 5 selection modes
```
