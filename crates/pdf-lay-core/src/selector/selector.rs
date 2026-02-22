//! Multi-mode section selector providing borrowed views over a PaperDocument.

use crate::config::{ChunkConfig, LlmTextConfig, MarkdownConfig};
use crate::output::{Chunker, JsonGenerator, MarkdownGenerator};
use crate::selector::llm_text::LlmTextGenerator;
use crate::selector::toc::{SectionEntry, TocGenerator};
use crate::types::{Chunk, PaperDocument, Section};

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
            .map(|s| Chunker::estimate_tokens(&s.full_text()))
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

    /// Generate LLM-optimized text for the selected sections.
    pub fn to_llm_text(&self, config: &LlmTextConfig) -> String {
        LlmTextGenerator::new(config.clone()).generate(&self.selected)
    }

    /// Generate Markdown for the selected sections.
    pub fn to_markdown(&self, config: &MarkdownConfig) -> String {
        MarkdownGenerator::new(config.clone()).generate_for_sections(&self.selected)
    }

    /// Serialize selected sections to pretty-printed JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        JsonGenerator::generate_sections(&self.selected)
    }

    /// Split selected sections into chunks for LLM consumption.
    pub fn to_chunks(&self, config: &ChunkConfig) -> Vec<Chunk> {
        Chunker::new(config.clone()).chunk_sections(&self.selected)
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
    use crate::types::{DocumentMetadata, PaperDocument, Rect, Section, SectionHeader};

    fn make_section_with_children(
        header: &str,
        level: u8,
        page: u32,
        children: Vec<Section>,
    ) -> Section {
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
                make_section_with_children(
                    "METHODS",
                    1,
                    1,
                    vec![make_section_with_children("Data Collection", 2, 1, vec![])],
                ),
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
