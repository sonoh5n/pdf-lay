//! Splits a PaperDocument into Chunks for LLM consumption.

use crate::config::{ChunkConfig, SplitStrategy};
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

        for section in sections {
            let section_text = section.full_text();
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
                    &section.full_text(),
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
        let section_text = section.full_text();
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
                &section.full_text(),
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
        // Concatenate all section text then split mechanically.
        let all_text: String = doc
            .sections
            .iter()
            .map(|s| s.full_text())
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
        let all_text: String = doc
            .sections
            .iter()
            .map(|s| s.full_text())
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
        };
        let chunker = Chunker::new(config);
        let doc = make_doc_with_sections(vec![make_section("SEC", "Hello world.", 1)]);
        // Should complete without infinite loop (max_tokens clamped to 1).
        let chunks = chunker.chunk(&doc);
        assert!(!chunks.is_empty());
    }
}
