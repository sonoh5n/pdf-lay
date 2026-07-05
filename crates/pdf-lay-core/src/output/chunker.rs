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
                let sub = self.split_section_text(
                    &section_text,
                    "",
                    section.header_text(),
                    section.page_range,
                    section,
                    &mut chunk_id,
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
            self.chunk_section_recursive(section, &doc.paper_id, &mut chunk_id, &mut chunks);
        }

        chunks
    }

    fn chunk_section_recursive(
        &self,
        section: &Section,
        paper_id: &str,
        chunk_id: &mut usize,
        out: &mut Vec<Chunk>,
    ) {
        let opts = self.render_opts();
        let section_text = render_core::render_section_content(section, &opts);
        let estimated_tokens = Self::estimate_tokens(&section_text);

        if estimated_tokens <= self.config.max_tokens {
            out.push(Chunk {
                chunk_id: *chunk_id,
                paper_id: paper_id.to_string(),
                section: section.header_text(),
                page_range: section.page_range,
                text: section_text,
                figures: section.figures.clone(),
                tables: section.tables.clone(),
                estimated_tokens,
                has_continuation: false,
            });
            *chunk_id += 1;
        } else {
            // Section too large: split by paragraph.
            let sub = self.split_section_text(
                &section_text,
                paper_id,
                section.header_text(),
                section.page_range,
                section,
                chunk_id,
            );
            out.extend(sub);
        }

        // Recurse into children.
        for child in &section.children {
            self.chunk_section_recursive(child, paper_id, chunk_id, out);
        }
    }

    fn split_section_text(
        &self,
        text: &str,
        paper_id: &str,
        section_name: String,
        page_range: (u32, u32),
        section: &Section,
        chunk_id: &mut usize,
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
                // Flush current chunk.
                chunks.push(Chunk {
                    chunk_id: *chunk_id,
                    paper_id: paper_id.to_string(),
                    section: section_name.clone(),
                    page_range,
                    text: current_text.trim().to_string(),
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
                    estimated_tokens: current_tokens,
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
            chunks.push(Chunk {
                chunk_id: *chunk_id,
                paper_id: paper_id.to_string(),
                section: section_name,
                page_range,
                text: current_text.trim().to_string(),
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
                estimated_tokens: current_tokens,
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

        self.split_section_text(
            &all_text,
            &doc.paper_id,
            String::new(),
            (0, doc.metadata.pages.saturating_sub(1)),
            &empty_section,
            &mut 0,
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
}
