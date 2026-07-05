//! Splits a PaperDocument into Chunks for LLM consumption.

use crate::config::{ChunkConfig, FigureTextFormat, SplitStrategy};
use crate::output::render_core::{self, EscapeMode, RenderOptions};
use crate::types::{Chunk, PaperDocument, Section};

/// Splits a [`PaperDocument`] into [`Chunk`] records for LLM consumption.
pub struct Chunker {
    /// The configuration controlling chunk sizes and split strategy.
    pub config: ChunkConfig,
}

impl Chunker {
    /// Create a new chunker with the given configuration.
    pub fn new(config: ChunkConfig) -> Self {
        Self { config }
    }

    /// Build the render-core options used to turn a [`Section`] into chunk
    /// body text: math conversion follows `self.config.math_config` (`None`
    /// disables it, preserving the pre-render-core behavior of chunk text
    /// carrying unconverted math glyphs), section headers are left out (the
    /// breadcrumb/heading prefix is a chunker-level concern, not render-core's),
    /// and figures/tables are interleaved at their insertion point so chunk
    /// text reaches the same fidelity as markdown/llm_text output.
    fn render_opts(&self) -> RenderOptions<'_> {
        RenderOptions {
            math_config: self.config.math_config.as_ref(),
            escape: EscapeMode::Plain,
            include_headers: false,
            include_figures: true,
            include_tables: true,
            figure_format: FigureTextFormat::Placeholder,
            image_base: String::new(),
        }
    }

    /// Chunk an entire document.
    pub fn chunk(&self, doc: &PaperDocument) -> Vec<Chunk> {
        match self.config.split_strategy {
            SplitStrategy::SectionBoundary => self.chunk_by_section(doc),
            SplitStrategy::TokenCount => self.chunk_by_tokens(doc),
            SplitStrategy::Paragraph => self.chunk_by_paragraph(doc),
        }
    }

    /// Chunk a slice of pre-selected sections.
    pub fn chunk_sections(&self, sections: &[&Section]) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut chunk_id = 0;
        let opts = self.render_opts();

        for section in sections {
            let section_text = render_core::render_section_content(section, &opts);
            let estimated_tokens = Self::estimate_tokens(&section_text);

            if estimated_tokens <= self.config.max_tokens {
                chunks.push(Chunk {
                    chunk_id,
                    paper_id: String::new(),
                    section: section.header_text(),
                    page_range: section.page_range,
                    text: section_text,
                    figures: section.figures.clone(),
                    tables: section.tables.clone(),
                    estimated_tokens,
                    has_continuation: false,
                });
                chunk_id += 1;
            } else {
                // Not tracked in a hierarchy here (a flat, pre-selected slice
                // of sections with no parent chain available), so no
                // breadcrumb prefix is built for this path — see
                // `chunk_section_recursive` for the SectionBoundary strategy,
                // which is where `include_section_context` applies (P2-2).
                let sub = self.split_section_text(
                    &section_text,
                    "",
                    section.header_text(),
                    section.page_range,
                    section,
                    &mut chunk_id,
                    "",
                );
                chunks.extend(sub);
            }
        }

        chunks
    }

    // ---- Strategy: SectionBoundary ----

    fn chunk_by_section(&self, doc: &PaperDocument) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut chunk_id = 0;

        for section in &doc.sections {
            self.chunk_section_recursive(section, &doc.paper_id, &[], &mut chunk_id, &mut chunks);
        }

        chunks
    }

    /// Recursively chunk `section` and its children, prefixing each chunk's
    /// text with a `[Context: A > B > C]` breadcrumb plus the section's own
    /// heading line when `include_section_context` is enabled.
    ///
    /// `breadcrumb` holds the clean heading text of every ancestor section
    /// (root-to-parent order); it is empty for top-level sections.
    fn chunk_section_recursive(
        &self,
        section: &Section,
        paper_id: &str,
        breadcrumb: &[&str],
        chunk_id: &mut usize,
        out: &mut Vec<Chunk>,
    ) {
        let opts = self.render_opts();
        let section_text = render_core::render_section_content(section, &opts);
        let own = section.header_text();
        let prefix = if self.config.include_section_context {
            Self::build_context_prefix(breadcrumb, &own)
        } else {
            String::new()
        };
        let prefixed_text = format!("{prefix}{section_text}");
        let estimated_tokens = Self::estimate_tokens(&prefixed_text);

        if estimated_tokens <= self.config.max_tokens {
            out.push(Chunk {
                chunk_id: *chunk_id,
                paper_id: paper_id.to_string(),
                section: own.clone(),
                page_range: section.page_range,
                text: prefixed_text,
                figures: section.figures.clone(),
                tables: section.tables.clone(),
                estimated_tokens,
                has_continuation: false,
            });
            *chunk_id += 1;
        } else {
            // Section too large: split by paragraph, carrying the same
            // breadcrumb/heading prefix onto every resulting sub-chunk.
            let sub = self.split_section_text(
                &section_text,
                paper_id,
                own.clone(),
                section.page_range,
                section,
                chunk_id,
                &prefix,
            );
            out.extend(sub);
        }

        // Recurse into children, extending the breadcrumb with this
        // section's own heading (skipped when headerless, so headerless
        // sections don't leave an empty path segment for their descendants).
        let mut child_breadcrumb = breadcrumb.to_vec();
        if !own.is_empty() {
            child_breadcrumb.push(own.as_str());
        }
        for child in &section.children {
            self.chunk_section_recursive(child, paper_id, &child_breadcrumb, chunk_id, out);
        }
    }

    /// Build the `[Context: ...]` breadcrumb + heading prefix prepended to a
    /// chunk's text when `include_section_context` is enabled.
    ///
    /// `ancestors` are the clean heading text of enclosing sections
    /// (root-to-parent order); `own` is the current section's own clean
    /// heading (empty for headerless sections). Headerless entries (empty
    /// strings) are dropped from the path so they never produce an empty
    /// `" > "`-joined segment. Returns an empty string when the resulting
    /// path is empty (an entirely headerless section with no headed
    /// ancestors) — no prefix is emitted in that case.
    fn build_context_prefix(ancestors: &[&str], own: &str) -> String {
        let mut path: Vec<&str> = ancestors
            .iter()
            .copied()
            .filter(|s| !s.is_empty())
            .collect();
        if !own.is_empty() {
            path.push(own);
        }
        if path.is_empty() {
            return String::new();
        }
        let joined = path.join(" > ");
        if own.is_empty() {
            format!("[Context: {joined}]\n\n")
        } else {
            format!("[Context: {joined}]\n# {own}\n\n")
        }
    }

    // `prefix` (P2-2's breadcrumb/heading text) pushed this past clippy's
    // 7-argument default; a dedicated params struct would be overkill for a
    // single private helper with three call sites.
    #[allow(clippy::too_many_arguments)]
    fn split_section_text(
        &self,
        text: &str,
        paper_id: &str,
        section_name: String,
        page_range: (u32, u32),
        section: &Section,
        chunk_id: &mut usize,
        prefix: &str,
    ) -> Vec<Chunk> {
        let paragraphs: Vec<&str> = text
            .split("\n\n")
            .filter(|p| !p.trim().is_empty())
            .collect();
        let mut chunks = Vec::new();
        let mut current_text = String::new();
        let mut current_tokens = 0usize;
        let mut is_first = true;

        for para in &paragraphs {
            let para_tokens = Self::estimate_tokens(para);

            if current_tokens + para_tokens > self.config.max_tokens && !current_text.is_empty() {
                // Flush current chunk. Every sub-chunk (first and
                // continuation alike) carries the same context prefix, so
                // positional information survives the split (P2-2).
                let prefixed_text = format!("{prefix}{}", current_text.trim());
                chunks.push(Chunk {
                    chunk_id: *chunk_id,
                    paper_id: paper_id.to_string(),
                    section: section_name.clone(),
                    page_range,
                    estimated_tokens: Self::estimate_tokens(&prefixed_text),
                    text: prefixed_text,
                    figures: if is_first {
                        section.figures.clone()
                    } else {
                        vec![]
                    },
                    tables: if is_first {
                        section.tables.clone()
                    } else {
                        vec![]
                    },
                    has_continuation: true,
                });
                *chunk_id += 1;
                is_first = false;

                // Add overlap from end of previous chunk.
                current_text = self.extract_overlap(&current_text);
                current_tokens = Self::estimate_tokens(&current_text);
            }

            if !current_text.is_empty() {
                current_text.push_str("\n\n");
            }
            current_text.push_str(para);
            current_tokens += para_tokens;
        }

        // Final chunk.
        if !current_text.is_empty() {
            let prefixed_text = format!("{prefix}{}", current_text.trim());
            chunks.push(Chunk {
                chunk_id: *chunk_id,
                paper_id: paper_id.to_string(),
                section: section_name,
                page_range,
                estimated_tokens: Self::estimate_tokens(&prefixed_text),
                text: prefixed_text,
                figures: if is_first {
                    section.figures.clone()
                } else {
                    vec![]
                },
                tables: if is_first {
                    section.tables.clone()
                } else {
                    vec![]
                },
                has_continuation: false,
            });
            *chunk_id += 1;
        }

        // Ensure the last chunk always has has_continuation = false.
        if let Some(last) = chunks.last_mut() {
            last.has_continuation = false;
        }

        chunks
    }

    fn extract_overlap(&self, text: &str) -> String {
        if self.config.overlap_tokens == 0 {
            return String::new();
        }
        // Take characters from the end of text proportional to overlap_tokens.
        let target_chars = self.config.overlap_tokens * 4; // approximate
        let char_count = text.chars().count();
        if char_count <= target_chars {
            text.to_string()
        } else {
            text.chars().skip(char_count - target_chars).collect()
        }
    }

    // ---- Strategy: TokenCount ----

    fn chunk_by_tokens(&self, doc: &PaperDocument) -> Vec<Chunk> {
        // Concatenate all section rich text (render-core: math-converted,
        // with inline table markdown and figure placeholders) then split
        // mechanically. Section attribution for these chunks is not yet
        // preserved (see P2-4); this task only fixes the text fidelity.
        let opts = self.render_opts();
        let all_text: String = doc
            .sections
            .iter()
            .map(|s| render_core::render_section_content(s, &opts))
            .collect::<Vec<_>>()
            .join("\n\n");

        if all_text.is_empty() {
            return vec![];
        }

        // Guard against max_tokens == 0 which would cause an infinite loop.
        let effective_max_tokens = self.config.max_tokens.max(1);

        let mut chunks = Vec::new();
        let mut chunk_id = 0;
        let chars: Vec<char> = all_text.chars().collect();
        let max_chars = effective_max_tokens * 4;
        let overlap_chars = self.config.overlap_tokens * 4;
        let mut start = 0;

        while start < chars.len() {
            let end = (start + max_chars).min(chars.len());
            let text: String = chars[start..end].iter().collect();
            let estimated_tokens = Self::estimate_tokens(&text);

            chunks.push(Chunk {
                chunk_id,
                paper_id: doc.paper_id.clone(),
                section: String::new(),
                page_range: (0, doc.metadata.pages.saturating_sub(1)),
                text,
                figures: vec![],
                tables: vec![],
                estimated_tokens,
                has_continuation: end < chars.len(),
            });
            chunk_id += 1;

            if end >= chars.len() {
                break;
            }
            // Ensure forward progress: advance by at least 1 character.
            let advance = max_chars.saturating_sub(overlap_chars).max(1);
            start += advance;
        }

        chunks
    }

    // ---- Strategy: Paragraph ----

    fn chunk_by_paragraph(&self, doc: &PaperDocument) -> Vec<Chunk> {
        let opts = self.render_opts();
        let all_text: String = doc
            .sections
            .iter()
            .map(|s| render_core::render_section_content(s, &opts))
            .collect::<Vec<_>>()
            .join("\n\n");

        let empty_section = Section {
            header: None,
            level: 1,
            blocks: vec![],
            figures: vec![],
            tables: vec![],
            children: vec![],
            page_range: (0, doc.metadata.pages.saturating_sub(1)),
        };

        // No per-chunk section attribution exists yet for this strategy (see
        // P2-4), so there is no section path to build a breadcrumb from; no
        // context prefix is applied here (non-scope for P2-2).
        self.split_section_text(
            &all_text,
            &doc.paper_id,
            String::new(),
            (0, doc.metadata.pages.saturating_sub(1)),
            &empty_section,
            &mut 0,
            "",
        )
    }

    // ---- Token estimation ----

    /// Token count estimate: ASCII ~4 chars/token, non-ASCII ~1.5 chars/token.
    pub fn estimate_tokens(text: &str) -> usize {
        let ascii_chars = text.chars().filter(|c| c.is_ascii()).count();
        let non_ascii_chars = text.chars().filter(|c| !c.is_ascii()).count();
        ascii_chars / 4 + (non_ascii_chars as f64 / 1.5) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ChunkConfig, SplitStrategy};
    use crate::types::{BlockType, DocumentMetadata, PaperDocument, Rect, Section, TextBlock};
    use std::path::PathBuf;

    fn make_doc_with_sections(sections: Vec<Section>) -> PaperDocument {
        PaperDocument {
            paper_id: "test".to_string(),
            source_file: PathBuf::from("test.pdf"),
            metadata: DocumentMetadata {
                pages: sections.len() as u32,
                ..Default::default()
            },
            all_figures: vec![],
            all_tables: vec![],
            sections,
        }
    }

    fn make_section(header: &str, text: &str, level: u8) -> Section {
        use crate::types::SectionHeader;
        Section {
            header: Some(SectionHeader {
                text: header.to_string(),
                clean_text: header.to_string(),
                level,
                numbering: None,
                page: 0,
                bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                block_index: 0,
            }),
            level,
            blocks: vec![TextBlock {
                global_index: 0,
                lines: vec![],
                text: text.to_string(),
                bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                page: 0,
                column_index: 0,
                block_type: BlockType::BodyText,
            }],
            figures: vec![],
            tables: vec![],
            children: vec![],
            page_range: (0, 0),
        }
    }

    fn default_config() -> ChunkConfig {
        ChunkConfig {
            max_tokens: 4000,
            overlap_tokens: 200,
            split_strategy: SplitStrategy::SectionBoundary,
            include_section_context: true,
            math_config: None,
        }
    }

    #[test]
    fn small_section_becomes_one_chunk() {
        let config = default_config();
        let chunker = Chunker::new(config);
        let doc = make_doc_with_sections(vec![make_section("INTRO", "Short intro text.", 1)]);
        let chunks = chunker.chunk(&doc);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].section, "INTRO");
        assert!(chunks[0].text.contains("Short intro text."));
    }

    #[test]
    fn multiple_sections_produce_multiple_chunks() {
        let config = default_config();
        let chunker = Chunker::new(config);
        let doc = make_doc_with_sections(vec![
            make_section("INTRO", "Introduction text.", 1),
            make_section("METHODS", "Methods text.", 1),
        ]);
        let chunks = chunker.chunk(&doc);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].section, "INTRO");
        assert_eq!(chunks[1].section, "METHODS");
    }

    #[test]
    fn chunk_ids_are_sequential() {
        let config = default_config();
        let chunker = Chunker::new(config);
        let doc = make_doc_with_sections(vec![
            make_section("SEC1", "Text 1.", 1),
            make_section("SEC2", "Text 2.", 1),
            make_section("SEC3", "Text 3.", 1),
        ]);
        let chunks = chunker.chunk(&doc);
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_id, i, "Chunk IDs should be 0, 1, 2...");
        }
    }

    #[test]
    fn token_estimation_for_ascii() {
        // 100 ASCII chars / 4 = 25 tokens.
        let text = "a".repeat(100);
        assert_eq!(Chunker::estimate_tokens(&text), 25);
    }

    #[test]
    fn last_chunk_has_continuation_false() {
        let config = default_config();
        let chunker = Chunker::new(config);
        let doc = make_doc_with_sections(vec![make_section("SEC", "Some text.", 1)]);
        let chunks = chunker.chunk(&doc);
        assert!(!chunks.last().unwrap().has_continuation);
    }

    #[test]
    fn extract_overlap_handles_non_ascii() {
        // Verify extract_overlap works with multibyte UTF-8 characters.
        let config = ChunkConfig {
            max_tokens: 10,
            overlap_tokens: 2, // 2 * 4 = 8 target chars
            split_strategy: SplitStrategy::SectionBoundary,
            include_section_context: true,
            math_config: None,
        };
        let chunker = Chunker::new(config);
        // 10 Japanese characters (3 bytes each in UTF-8).
        let text = "あいうえおかきくけこ";
        let overlap = chunker.extract_overlap(text);
        // Should take last 8 chars: "うえおかきくけこ"
        assert_eq!(overlap, "うえおかきくけこ");
    }

    #[test]
    fn token_count_strategy_zero_max_tokens_does_not_loop() {
        let config = ChunkConfig {
            max_tokens: 0,
            overlap_tokens: 0,
            split_strategy: SplitStrategy::TokenCount,
            include_section_context: true,
            math_config: None,
        };
        let chunker = Chunker::new(config);
        let doc = make_doc_with_sections(vec![make_section("SEC", "Hello world.", 1)]);
        // Should complete without infinite loop (max_tokens clamped to 1).
        let chunks = chunker.chunk(&doc);
        assert!(!chunks.is_empty());
    }

    // ---- render-core integration (P2-1): chunk.text must carry the same
    // fidelity as markdown/llm_text output instead of raw full_text(). ----

    /// A section whose single block is a math-font line (CMMI10 "α", all-math
    /// → Display context), so math conversion is exercised end-to-end.
    fn make_math_section(math_text: &str, font_name: &str) -> Section {
        use crate::types::{SectionHeader, TextLine, TextSpan};

        let span = TextSpan {
            text: math_text.to_string(),
            font_name: font_name.to_string(),
            font_size: 10.0,
            is_bold: false,
            is_italic: true,
            bbox: Rect::new(100.0, 700.0, 150.0, 690.0),
            page: 0,
        };
        let line = TextLine {
            text: math_text.to_string(),
            spans: vec![span],
            bbox: Rect::new(100.0, 700.0, 150.0, 690.0),
            page: 0,
            baseline_y: 690.0,
            primary_font_size: 10.0,
            primary_font_name: font_name.to_string(),
            is_bold: false,
        };
        let block = TextBlock {
            global_index: 0,
            lines: vec![line],
            text: math_text.to_string(),
            bbox: Rect::new(100.0, 700.0, 150.0, 690.0),
            page: 0,
            column_index: 0,
            block_type: BlockType::BodyText,
        };
        Section {
            header: Some(SectionHeader {
                text: "SEC".to_string(),
                clean_text: "SEC".to_string(),
                level: 1,
                numbering: None,
                page: 0,
                bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                block_index: 0,
            }),
            level: 1,
            blocks: vec![block],
            figures: vec![],
            tables: vec![],
            children: vec![],
            page_range: (0, 0),
        }
    }

    #[test]
    fn chunk_text_contains_converted_math() {
        use crate::config::{MathConfig, MathRepresentationPreference};

        let config = ChunkConfig {
            math_config: Some(MathConfig {
                representation: MathRepresentationPreference::LaTeX,
                ..MathConfig::default()
            }),
            ..default_config()
        };
        let chunker = Chunker::new(config);
        let doc = make_doc_with_sections(vec![make_math_section("α", "CMMI10")]);
        let chunks = chunker.chunk(&doc);

        assert_eq!(chunks.len(), 1);
        assert!(
            chunks[0].text.contains("\\alpha"),
            "expected converted math '\\alpha' in chunk text, got: {}",
            chunks[0].text
        );
        assert!(
            !chunks[0].text.contains('α'),
            "raw math glyph should have been converted, got: {}",
            chunks[0].text
        );
    }

    #[test]
    fn chunk_text_contains_table_markdown() {
        use crate::types::{InsertionPoint, TableInfo, TableRepresentation};

        let mut section = make_section("SEC", "Body text.", 1);
        section.tables.push(TableInfo {
            table_id: "Table 1".to_string(),
            table_number: Some(1),
            caption: None,
            representation: TableRepresentation::Markdown {
                header: vec!["A".to_string(), "B".to_string()],
                rows: vec![vec!["1".to_string(), "2".to_string()]],
                caption: None,
                markdown_text: "| A | B |\n| --- | --- |\n| 1 | 2 |\n".to_string(),
            },
            insertion_point: InsertionPoint {
                page: 0,
                after_block_index: None,
                y_position: 0.0,
            },
            page: 0,
        });
        let config = default_config();
        let chunker = Chunker::new(config);
        let doc = make_doc_with_sections(vec![section]);
        let chunks = chunker.chunk(&doc);

        assert_eq!(chunks.len(), 1);
        assert!(
            chunks[0].text.contains("| --- |"),
            "expected table markdown in chunk text, got: {}",
            chunks[0].text
        );
    }

    #[test]
    fn chunk_text_contains_figure_placeholder() {
        use crate::types::{FigureInfo, ImageFormat, ImageInfo, InsertionPoint};
        use std::path::PathBuf;

        let mut section = make_section("SEC", "Body text.", 1);
        section.figures.push(FigureInfo {
            figure_id: "Fig. 1".to_string(),
            figure_number: Some(1),
            caption_text: "Fig. 1: A diagram.".to_string(),
            image: ImageInfo {
                path: PathBuf::from("images/p000_img000.png"),
                page: 0,
                raw_bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                normalized_bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                width_px: 10,
                height_px: 10,
                format: ImageFormat::Png,
            },
            context_text: String::new(),
            insertion_point: InsertionPoint {
                page: 0,
                after_block_index: Some(0),
                y_position: 0.0,
            },
        });
        let config = default_config();
        let chunker = Chunker::new(config);
        let doc = make_doc_with_sections(vec![section]);
        let chunks = chunker.chunk(&doc);

        assert_eq!(chunks.len(), 1);
        assert!(
            chunks[0].text.contains("[IMAGE:"),
            "expected figure placeholder in chunk text, got: {}",
            chunks[0].text
        );
    }

    // ---- include_section_context (P2-2): breadcrumb + heading prefix ----

    #[test]
    fn breadcrumb_prefix_for_nested_section() {
        let config = default_config(); // include_section_context: true
        let chunker = Chunker::new(config);

        let mut parent = make_section("METHODS", "Parent body text.", 1);
        parent
            .children
            .push(make_section("Data Collection", "Child body text.", 2));

        let doc = make_doc_with_sections(vec![parent]);
        let chunks = chunker.chunk(&doc);

        assert_eq!(chunks.len(), 2, "expected one chunk per section");
        assert!(
            chunks[0]
                .text
                .starts_with("[Context: METHODS]\n# METHODS\n\n"),
            "top-level chunk should carry its own heading as the sole breadcrumb entry: {}",
            chunks[0].text
        );
        assert!(
            chunks[1]
                .text
                .starts_with("[Context: METHODS > Data Collection]\n# Data Collection\n\n"),
            "nested chunk should show the full ancestor path: {}",
            chunks[1].text
        );
    }

    #[test]
    fn no_prefix_when_context_disabled() {
        let config = ChunkConfig {
            include_section_context: false,
            ..default_config()
        };
        let chunker = Chunker::new(config);

        let mut parent = make_section("METHODS", "Parent body text.", 1);
        parent
            .children
            .push(make_section("Data Collection", "Child body text.", 2));

        let doc = make_doc_with_sections(vec![parent]);
        let chunks = chunker.chunk(&doc);

        assert_eq!(chunks.len(), 2);
        for chunk in &chunks {
            assert!(
                !chunk.text.contains("[Context:"),
                "prefix must be absent when include_section_context is false: {}",
                chunk.text
            );
        }
        // Disabling the flag reproduces the plain (unprefixed) body text.
        assert!(chunks[0].text.starts_with("Parent body text."));
        assert!(chunks[1].text.starts_with("Child body text."));
    }

    #[test]
    fn headerless_section_no_empty_breadcrumb() {
        let config = default_config();
        let chunker = Chunker::new(config);

        let section = Section {
            header: None,
            level: 1,
            blocks: vec![TextBlock {
                global_index: 0,
                lines: vec![],
                text: "Preamble text.".to_string(),
                bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                page: 0,
                column_index: 0,
                block_type: BlockType::BodyText,
            }],
            figures: vec![],
            tables: vec![],
            children: vec![],
            page_range: (0, 0),
        };
        let doc = make_doc_with_sections(vec![section]);
        let chunks = chunker.chunk(&doc);

        assert_eq!(chunks.len(), 1);
        assert!(
            !chunks[0].text.contains("[Context:"),
            "an entirely headerless section (no ancestors either) must have no breadcrumb: {}",
            chunks[0].text
        );
        assert!(
            !chunks[0].text.contains(" >  >") && !chunks[0].text.contains("Context: ]"),
            "must not produce an empty or doubly-separated breadcrumb: {}",
            chunks[0].text
        );
        assert!(chunks[0].text.contains("Preamble text."));
    }

    #[test]
    fn split_chunks_all_carry_prefix() {
        use crate::types::SectionHeader;

        // Small max_tokens forces `split_section_text` to break the section's
        // three paragraphs into multiple sub-chunks.
        let config = ChunkConfig {
            max_tokens: 15,
            overlap_tokens: 0,
            split_strategy: SplitStrategy::SectionBoundary,
            include_section_context: true,
            math_config: None,
        };
        let chunker = Chunker::new(config);

        let make_block = |idx: usize, text: &str| TextBlock {
            global_index: idx,
            lines: vec![],
            text: text.to_string(),
            bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
            page: 0,
            column_index: 0,
            block_type: BlockType::BodyText,
        };
        let section = Section {
            header: Some(SectionHeader {
                text: "BIG".to_string(),
                clean_text: "BIG".to_string(),
                level: 1,
                numbering: None,
                page: 0,
                bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                block_index: 0,
            }),
            level: 1,
            blocks: vec![
                make_block(0, "Paragraph one padding text goes here now."),
                make_block(1, "Paragraph two padding text goes here now."),
                make_block(2, "Paragraph three padding text goes here."),
            ],
            figures: vec![],
            tables: vec![],
            children: vec![],
            page_range: (0, 0),
        };
        let doc = make_doc_with_sections(vec![section]);
        let chunks = chunker.chunk(&doc);

        assert!(
            chunks.len() >= 2,
            "expected the oversized section to split into multiple sub-chunks, got {}",
            chunks.len()
        );
        for chunk in &chunks {
            assert!(
                chunk.text.starts_with("[Context: BIG]\n# BIG\n\n"),
                "every sub-chunk must carry the same section prefix: {}",
                chunk.text
            );
        }
    }
}
