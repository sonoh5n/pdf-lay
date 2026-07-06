//! Multi-mode section selector providing borrowed views over a PaperDocument.

use serde::{Deserialize, Serialize};

use crate::config::{ChunkConfig, LlmTextConfig, MarkdownConfig};
use crate::output::{Chunker, JsonGenerator, MarkdownGenerator};
use crate::selector::llm_text::LlmTextGenerator;
use crate::selector::toc::{SectionEntry, TocGenerator};
use crate::types::{Chunk, PaperDocument, Section};

/// How [`SectionSelector::by_names_with`] compares a query name against a
/// section's header text.
///
/// `Substring` is the legacy, unbounded `contains` behavior kept as the
/// default for backward compatibility (see `docs/refactor/phase1_sections.md`
/// P1-7); it is prone to over-firing (e.g. `"in"` matching `INTRODUCTION`).
/// The other modes are additive and opt-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MatchMode {
    /// Legacy unbounded case-insensitive substring match (the historical
    /// `by_names` behavior). Kept as the default for backward compatibility.
    #[default]
    Substring,
    /// Case-insensitive full-string match only (no substring matching).
    Exact,
    /// Case-insensitive match at a Unicode word boundary, so a short query
    /// like `"method"` does not match inside `"METHODOLOGY"`.
    WordBoundary,
    /// Match after normalizing both the query and the header text (trim,
    /// full-width-to-half-width folding, uppercase). Intended for CJK
    /// headers where case/width variants should be treated as equal.
    Normalized,
}

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
    /// If a parent section matches, its children are automatically included
    /// (subtree "swallow"). This is equivalent to
    /// `by_names_with(doc, names, MatchMode::Substring, true)` and is kept
    /// unchanged for backward compatibility; use [`Self::by_names_with`] to
    /// pick a different match mode or to disable subtree swallowing.
    pub fn by_names(doc: &'a PaperDocument, names: &[&str]) -> Self {
        Self::by_names_with(doc, names, MatchMode::Substring, true)
    }

    /// Select sections by header name with an explicit [`MatchMode`] and
    /// subtree-inclusion policy.
    ///
    /// When `include_subtree` is `true`, a matching parent section is
    /// returned and its children are *not* independently walked (they are
    /// still reachable through `Section::children` on the returned parent,
    /// but they are not added as separate entries) — this is the legacy
    /// "swallow" behavior. When `false`, children are always walked
    /// regardless of whether the parent matched, so a child can be selected
    /// on its own and a matching parent no longer implicitly swallows it.
    pub fn by_names_with(
        doc: &'a PaperDocument,
        names: &[&str],
        mode: MatchMode,
        include_subtree: bool,
    ) -> Self {
        let selected = Self::collect_by_names(&doc.sections, names, mode, include_subtree);
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

    fn collect_by_names(
        sections: &'a [Section],
        names: &[&str],
        mode: MatchMode,
        include_subtree: bool,
    ) -> Vec<&'a Section> {
        let mut result = Vec::new();
        for section in sections {
            let header_upper = section.header_text().to_uppercase();
            let clean_upper = section
                .header
                .as_ref()
                .map(|h| h.clean_text.to_uppercase())
                .unwrap_or_default();

            let matched = names
                .iter()
                .any(|name| Self::name_matches(&header_upper, &clean_upper, name, mode));

            if matched {
                // Include the section (children are included via Section::children).
                result.push(section);
            }
            if !matched || !include_subtree {
                // Recurse into children: either the parent didn't match, or
                // subtree-swallow is disabled and children must be walked
                // (and can match) independently of the parent.
                result.extend(Self::collect_by_names(
                    &section.children,
                    names,
                    mode,
                    include_subtree,
                ));
            }
        }
        result
    }

    /// Whether `name` matches a section's header text under the given
    /// [`MatchMode`]. `header_upper`/`clean_upper` are already
    /// uppercased (see `collect_by_names`).
    fn name_matches(header_upper: &str, clean_upper: &str, name: &str, mode: MatchMode) -> bool {
        match mode {
            MatchMode::Substring => {
                let upper_name = name.to_uppercase();
                header_upper == upper_name
                    || clean_upper == upper_name
                    || header_upper.contains(&upper_name)
                    || clean_upper.contains(&upper_name)
            }
            MatchMode::Exact => {
                let upper_name = name.to_uppercase();
                header_upper == upper_name || clean_upper == upper_name
            }
            MatchMode::Normalized => {
                let norm_name = normalize(name);
                normalize(header_upper) == norm_name || normalize(clean_upper) == norm_name
            }
            MatchMode::WordBoundary => {
                word_boundary_matches(header_upper, name)
                    || word_boundary_matches(clean_upper, name)
            }
        }
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

/// Normalize a string for [`MatchMode::Normalized`] comparisons: trim, fold
/// full-width ASCII to half-width, then uppercase (a no-op for CJK scripts).
///
/// This mirrors `structure::header_detector::normalize` (added in P1-5) but
/// is kept as an independent copy rather than a cross-module `pub(crate)`
/// export: `structure/` is owned by a concurrently-in-progress task (P1-6)
/// and this selector module has no other reason to depend on it. If the two
/// ever drift, keeping them in sync is a small follow-up.
fn normalize(s: &str) -> String {
    s.trim()
        .chars()
        .map(|c| {
            if ('\u{FF01}'..='\u{FF5E}').contains(&c) {
                char::from_u32(c as u32 - 0xFEE0).unwrap_or(c)
            } else {
                c
            }
        })
        .collect::<String>()
        .to_uppercase()
}

/// Whether `name` occurs in `haystack` at a Unicode word boundary, so a
/// short query like `"method"` does not match inside `"METHODOLOGY"` (S7).
///
/// CJK scripts have no inter-word spaces, so `\b` there only fires at the
/// string's own edges or at a transition to/from non-word punctuation —
/// in practice this degenerates to "exact match, or delimited by a
/// non-word character", which is the safety property CJK headers need too.
fn word_boundary_matches(haystack: &str, name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return false;
    }
    let pattern = format!(r"(?i)\b{}\b", regex::escape(trimmed));
    match regex::Regex::new(&pattern) {
        Ok(re) => re.is_match(haystack),
        // Escaped patterns should always compile; fall back to an exact
        // comparison rather than panicking if one somehow doesn't.
        Err(_) => haystack.to_uppercase() == trimmed.to_uppercase(),
    }
}

impl PaperDocument {
    /// Generate the table of contents.
    pub fn toc(&self) -> Vec<SectionEntry> {
        TocGenerator::generate(self)
    }

    /// Select sections by header name (partial match, case-insensitive).
    ///
    /// Equivalent to `select_sections_with(names, MatchMode::Substring,
    /// true)`. Kept unchanged for backward compatibility; use
    /// [`Self::select_sections_with`] for other match modes or to disable
    /// subtree swallowing.
    pub fn select_sections<'a>(&'a self, names: &[&str]) -> SectionSelector<'a> {
        SectionSelector::by_names(self, names)
    }

    /// Select sections by header name with an explicit [`MatchMode`] and
    /// subtree-inclusion policy. See [`SectionSelector::by_names_with`].
    pub fn select_sections_with<'a>(
        &'a self,
        names: &[&str],
        mode: MatchMode,
        include_subtree: bool,
    ) -> SectionSelector<'a> {
        SectionSelector::by_names_with(self, names, mode, include_subtree)
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
    use super::{MatchMode, SectionSelector};
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

    // ---- P1-7: match modes and subtree-swallow control ----

    #[test]
    fn legacy_by_names_unchanged() {
        // `by_names` / `select_sections` must behave exactly as before:
        // unbounded substring match with subtree swallow.
        let doc = make_doc();
        let sel = doc.select_sections(&["RESULT"]);
        assert_eq!(sel.sections().len(), 1);
        assert_eq!(sel.sections()[0].header_text(), "RESULTS");

        let sel2 = SectionSelector::by_names_with(&doc, &["RESULT"], MatchMode::Substring, true);
        assert_eq!(sel2.sections().len(), 1);
        assert_eq!(sel2.sections()[0].header_text(), "RESULTS");
    }

    #[test]
    fn word_boundary_no_substring_overfire() {
        let doc = PaperDocument {
            paper_id: "test".to_string(),
            source_file: std::path::PathBuf::from("test.pdf"),
            metadata: DocumentMetadata::default(),
            sections: vec![make_section_with_children("METHODOLOGY", 1, 0, vec![])],
            all_figures: vec![],
            all_tables: vec![],
        };

        // "method" must not match inside "METHODOLOGY" under word-boundary mode.
        let sel = SectionSelector::by_names_with(&doc, &["method"], MatchMode::WordBoundary, true);
        assert!(sel.sections().is_empty());

        // The full word does match.
        let sel2 =
            SectionSelector::by_names_with(&doc, &["methodology"], MatchMode::WordBoundary, true);
        assert_eq!(sel2.sections().len(), 1);

        // Legacy Substring mode does over-fire on the same query (sanity check
        // that the new mode is actually doing something different).
        let sel3 = SectionSelector::by_names_with(&doc, &["method"], MatchMode::Substring, true);
        assert_eq!(sel3.sections().len(), 1);
    }

    #[test]
    fn exact_mode_requires_full_match() {
        let doc = make_doc();

        // "RESULT" is a substring of "RESULTS" but not an exact match.
        let sel = SectionSelector::by_names_with(&doc, &["RESULT"], MatchMode::Exact, true);
        assert!(sel.sections().is_empty());

        let sel2 = SectionSelector::by_names_with(&doc, &["RESULTS"], MatchMode::Exact, true);
        assert_eq!(sel2.sections().len(), 1);
        assert_eq!(sel2.sections()[0].header_text(), "RESULTS");
    }

    #[test]
    fn normalized_mode_fullwidth() {
        let doc = PaperDocument {
            paper_id: "test".to_string(),
            source_file: std::path::PathBuf::from("test.pdf"),
            metadata: DocumentMetadata::default(),
            sections: vec![make_section_with_children("ＲＥＳＵＬＴＳ", 1, 0, vec![])],
            all_figures: vec![],
            all_tables: vec![],
        };

        let sel = SectionSelector::by_names_with(&doc, &["RESULTS"], MatchMode::Normalized, true);
        assert_eq!(sel.sections().len(), 1);

        // Exact mode (no normalization) must not match the full-width variant.
        let sel2 = SectionSelector::by_names_with(&doc, &["RESULTS"], MatchMode::Exact, true);
        assert!(sel2.sections().is_empty());
    }

    #[test]
    fn select_child_only_without_subtree() {
        let doc = make_doc();
        let sel =
            SectionSelector::by_names_with(&doc, &["Data Collection"], MatchMode::Substring, false);
        assert_eq!(sel.sections().len(), 1);
        assert_eq!(sel.sections()[0].header_text(), "Data Collection");
    }

    #[test]
    fn select_parent_without_swallowing_children() {
        let doc = make_doc();
        let names = ["METHODS", "Data Collection"];

        // include_subtree=true (legacy): parent match stops recursion, so the
        // child is not returned as a separate entry even though its name also
        // appears in `names`.
        let swallowed = SectionSelector::by_names_with(&doc, &names, MatchMode::Exact, true);
        assert_eq!(swallowed.sections().len(), 1);
        assert_eq!(swallowed.sections()[0].header_text(), "METHODS");

        // include_subtree=false: children are always walked independently, so
        // both the matching parent and the matching child come back.
        let not_swallowed = SectionSelector::by_names_with(&doc, &names, MatchMode::Exact, false);
        assert_eq!(not_swallowed.sections().len(), 2);
        let texts: Vec<_> = not_swallowed
            .sections()
            .iter()
            .map(|s| s.header_text())
            .collect();
        assert!(texts.contains(&"METHODS".to_string()));
        assert!(texts.contains(&"Data Collection".to_string()));
    }

    #[test]
    fn select_sections_with_exposes_match_mode_on_document() {
        // Verifies the `PaperDocument::select_sections_with` caller surface.
        let doc = make_doc();
        let sel = doc.select_sections_with(&["method"], MatchMode::WordBoundary, true);
        assert!(sel.sections().is_empty());
        let sel2 = doc.select_sections_with(&["METHODS"], MatchMode::Exact, true);
        assert_eq!(sel2.sections().len(), 1);
    }
}
