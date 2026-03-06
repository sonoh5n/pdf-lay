# Task 16: MarkdownGenerator

## Overview

Implement the output module's `MarkdownGenerator` which converts a `PaperDocument` into a
Markdown string. Figures are emitted as `![fig_id](path)` with captions, tables are emitted
inline at their insertion point, section headers are emitted with configurable heading offset,
and an optional YAML front matter header can be prepended.

Also wire `SectionSelector::to_markdown()` to call `MarkdownGenerator::generate_for_sections()`.

## Related Information
- **Plan**: `/Users/sonokawa/.claude/plans/fuzzy-drifting-summit.md` (Task 16)
- **Design doc**: `docs/arch/02_DESIGN.md` § 2.6 output — MarkdownGenerator
- **Overview**: `docs/develop/overview.md`
- **Dependencies**: Task 13 (Pipeline Integration) and Task 14 (SectionSelector) must be completed first

## Files to Create

- [ ] `crates/pdf-lay-core/src/output/mod.rs`
- [ ] `crates/pdf-lay-core/src/output/markdown.rs`

## Files to Modify

- [ ] `crates/pdf-lay-core/src/lib.rs` — add `pub mod output;`
- [ ] `crates/pdf-lay-core/src/selector/selector.rs` — wire `to_markdown()` to call `MarkdownGenerator::generate_for_sections()`

## Implementation Steps

### Step 1: `output/mod.rs`

```rust
//! Output generation: Markdown, JSON, chunking.

mod markdown;
mod json;
mod chunker;

pub use markdown::MarkdownGenerator;
pub use json::JsonGenerator;
pub use chunker::Chunker;
```

Note: `json.rs` and `chunker.rs` will be stub files that panic with `todo!()` in this task;
they will be fully implemented in Task 17.

Stub for `output/json.rs`:
```rust
//! JSON output generation (implemented in Task 17).

use crate::types::PaperDocument;

pub struct JsonGenerator;

impl JsonGenerator {
    pub fn generate(_doc: &PaperDocument) -> Result<String, serde_json::Error> {
        todo!("JsonGenerator implemented in Task 17")
    }
}
```

Stub for `output/chunker.rs`:
```rust
//! Document chunker (implemented in Task 17).

use crate::types::{Chunk, PaperDocument, Section};
use crate::config::ChunkConfig;

pub struct Chunker {
    pub config: ChunkConfig,
}

impl Chunker {
    pub fn new(config: ChunkConfig) -> Self {
        Self { config }
    }

    pub fn chunk(&self, _doc: &PaperDocument) -> Vec<Chunk> {
        todo!("Chunker implemented in Task 17")
    }

    pub fn chunk_sections(&self, _sections: &[&Section]) -> Vec<Chunk> {
        todo!("Chunker implemented in Task 17")
    }

    /// Token estimation: ASCII ~4 chars/token, non-ASCII ~1.5 chars/token.
    pub fn estimate_tokens(text: &str) -> usize {
        let ascii_chars = text.chars().filter(|c| c.is_ascii()).count();
        let non_ascii_chars = text.chars().filter(|c| !c.is_ascii()).count();
        ascii_chars / 4 + (non_ascii_chars as f64 / 1.5) as usize
    }
}
```

### Step 2: `output/markdown.rs`

```rust
//! Generates Markdown output from a PaperDocument or selected sections.

use std::collections::VecDeque;
use crate::config::{CaptionStyle, MarkdownConfig};
use crate::types::{BlockType, FigureInfo, PaperDocument, Section, TableInfo};

/// Generates Markdown from a PaperDocument.
pub struct MarkdownGenerator {
    config: MarkdownConfig,
}

impl MarkdownGenerator {
    pub fn new(config: MarkdownConfig) -> Self {
        Self { config }
    }

    /// Generate Markdown for an entire document.
    pub fn generate(&self, doc: &PaperDocument) -> String {
        let mut md = String::with_capacity(doc.estimated_text_size());

        if self.config.include_metadata_header {
            md.push_str(&self.generate_front_matter(&doc.metadata));
        }

        for section in &doc.sections {
            self.write_section(&mut md, section);
        }

        md
    }

    /// Generate Markdown for a slice of selected sections (used by SectionSelector).
    pub fn generate_for_sections(&self, sections: &[&Section]) -> String {
        let mut md = String::new();
        for section in sections {
            self.write_section(&mut md, section);
        }
        md
    }

    fn generate_front_matter(&self, metadata: &crate::types::DocumentMetadata) -> String {
        let mut fm = String::from("---\n");
        if let Some(title) = &metadata.title {
            fm.push_str(&format!("title: \"{}\"\n", title.replace('"', "\\\"")));
        }
        if !metadata.authors.is_empty() {
            fm.push_str("authors:\n");
            for author in &metadata.authors {
                fm.push_str(&format!("  - \"{}\"\n", author.replace('"', "\\\"")));
            }
        }
        fm.push_str("---\n\n");
        fm
    }

    fn write_section(&self, md: &mut String, section: &Section) {
        // Section header.
        if let Some(header) = &section.header {
            let level = (header.level as u8 + self.config.heading_offset) as usize;
            let level = level.max(1).min(6);
            let prefix = "#".repeat(level);
            md.push_str(&format!("{} {}\n\n", prefix, header.clean_text));
        }

        // Optional page number comment.
        if self.config.include_page_numbers {
            md.push_str(&format!("<!-- page {} -->\n\n", section.page_range.0));
        }

        // Iterate blocks; insert figures/tables at their insertion_point.
        let mut figure_queue: VecDeque<&FigureInfo> = section.figures.iter().collect();
        let mut table_queue: VecDeque<&TableInfo> = section.tables.iter().collect();

        for block in &section.blocks {
            // Skip non-content blocks.
            match block.block_type {
                BlockType::Caption
                | BlockType::PageNumber
                | BlockType::RunningHeader
                | BlockType::RunningFooter => continue,
                _ => {
                    md.push_str(&block.text);
                    md.push_str("\n\n");
                }
            }

            // Emit figures whose insertion_point falls after this block.
            while let Some(fig) = figure_queue.front() {
                if fig.insertion_point.after_block_index == Some(block.global_index) {
                    self.write_figure(md, fig);
                    figure_queue.pop_front();
                } else {
                    break;
                }
            }

            // Emit tables whose insertion_point falls after this block.
            while let Some(table) = table_queue.front() {
                if table.insertion_point.after_block_index == Some(block.global_index) {
                    self.write_table(md, table);
                    table_queue.pop_front();
                } else {
                    break;
                }
            }
        }

        // Flush remaining figures/tables (no specific insertion point).
        while let Some(fig) = figure_queue.pop_front() {
            self.write_figure(md, fig);
        }
        while let Some(table) = table_queue.pop_front() {
            self.write_table(md, table);
        }

        // Recurse into child sections.
        for child in &section.children {
            self.write_section(md, child);
        }
    }

    fn write_figure(&self, md: &mut String, fig: &FigureInfo) {
        // Construct image path relative to image_base_path.
        let filename = fig.image.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| fig.image.path.display().to_string());
        let path = if self.config.image_base_path.is_empty() {
            filename
        } else {
            format!("{}/{}", self.config.image_base_path, filename)
        };

        md.push_str(&format!("![{}]({})\n\n", fig.figure_id, path));

        match self.config.figure_caption_style {
            CaptionStyle::Italic => {
                md.push_str(&format!("*{}*\n\n", fig.caption_text));
            }
            CaptionStyle::Bold => {
                // Bold the figure_id prefix, remainder as plain text.
                let description = fig.caption_description();
                md.push_str(&format!("**{}** {}\n\n", fig.figure_id, description));
            }
            CaptionStyle::PlainText => {
                md.push_str(&format!("{}\n\n", fig.caption_text));
            }
        }
    }

    fn write_table(&self, md: &mut String, table: &TableInfo) {
        use crate::types::TableRepresentation;

        if let Some(caption) = &table.caption {
            md.push_str(&format!("**{}**\n\n", caption));
        }

        match &table.representation {
            TableRepresentation::Markdown { markdown_text, .. } => {
                md.push_str(markdown_text);
            }
            TableRepresentation::Csv { csv_text, .. } => {
                md.push_str("```csv\n");
                md.push_str(csv_text);
                md.push_str("```");
            }
            TableRepresentation::PlainText { text, .. } => {
                md.push_str("```\n");
                md.push_str(text);
                md.push_str("```");
            }
        }
        md.push_str("\n\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CaptionStyle, MarkdownConfig};
    use crate::types::{
        BlockType, DocumentMetadata, FigureInfo, ImageFormat, ImageInfo, InsertionPoint,
        PaperDocument, Rect, Section, SectionHeader, TextBlock,
    };
    use std::path::PathBuf;

    fn default_config() -> MarkdownConfig {
        MarkdownConfig {
            image_base_path: "./images".to_string(),
            include_page_numbers: false,
            heading_offset: 1,
            include_metadata_header: false,
            table_as_image: false,
            figure_caption_style: CaptionStyle::Italic,
        }
    }

    fn make_block(text: &str) -> TextBlock {
        TextBlock {
            global_index: 0,
            lines: vec![],
            text: text.to_string(),
            bbox: Rect::new(72.0, 700.0, 540.0, 690.0),
            page: 0,
            column_index: 0,
            block_type: BlockType::BodyText,
        }
    }

    fn make_section(header: &str, text: &str, level: u8) -> Section {
        Section {
            header: Some(SectionHeader {
                text: header.to_string(),
                clean_text: header.to_string(),
                level,
                numbering: None,
                page: 0,
                bbox: Rect::new(72.0, 700.0, 540.0, 690.0),
                block_index: 0,
            }),
            level,
            blocks: vec![make_block(text)],
            figures: vec![],
            tables: vec![],
            children: vec![],
            page_range: (0, 0),
        }
    }

    #[test]
    fn section_header_uses_heading_offset() {
        let gen = MarkdownGenerator::new(default_config());
        let section = make_section("INTRODUCTION", "Body text.", 1);
        let output = gen.generate_for_sections(&[&section]);
        // level 1 + offset 1 = ## (h2)
        assert!(output.contains("## INTRODUCTION"), "Expected '## INTRODUCTION' in:\n{}", output);
    }

    #[test]
    fn body_text_included() {
        let gen = MarkdownGenerator::new(default_config());
        let section = make_section("SEC", "Some body text.", 1);
        let output = gen.generate_for_sections(&[&section]);
        assert!(output.contains("Some body text."));
    }

    #[test]
    fn page_number_block_excluded() {
        let gen = MarkdownGenerator::new(default_config());
        let mut section = make_section("SEC", "Body.", 1);
        let mut pn = make_block("42");
        pn.block_type = BlockType::PageNumber;
        section.blocks.push(pn);
        let output = gen.generate_for_sections(&[&section]);
        // "42" as standalone page number block should not appear
        assert!(!output.trim_end().ends_with("42"), "Page number should be excluded");
    }

    #[test]
    fn figure_written_with_italic_caption() {
        let gen = MarkdownGenerator::new(default_config());
        let mut section = make_section("SEC", "Text.", 1);
        section.figures.push(FigureInfo {
            figure_id: "Fig. 1".to_string(),
            figure_number: Some(1),
            caption_text: "Fig. 1: A diagram.".to_string(),
            image: ImageInfo {
                path: PathBuf::from("images/p000_img000.png"),
                page: 0,
                raw_bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                normalized_bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                width_px: 100,
                height_px: 100,
                format: ImageFormat::Png,
            },
            context_text: String::new(),
            insertion_point: InsertionPoint { page: 0, after_block_index: None, y_position: 0.0 },
        });
        let output = gen.generate_for_sections(&[&section]);
        assert!(output.contains("![Fig. 1]"), "Should contain image link");
        assert!(output.contains("*Fig. 1: A diagram.*"), "Should contain italic caption");
    }

    #[test]
    fn yaml_front_matter_emitted_when_enabled() {
        let mut config = default_config();
        config.include_metadata_header = true;
        let gen = MarkdownGenerator::new(config);
        let doc = PaperDocument {
            paper_id: "test".to_string(),
            source_file: PathBuf::from("test.pdf"),
            metadata: DocumentMetadata {
                pages: 1,
                title: Some("My Paper".to_string()),
                authors: vec!["Author A".to_string()],
                ..Default::default()
            },
            sections: vec![],
            all_figures: vec![],
            all_tables: vec![],
        };
        let output = gen.generate(&doc);
        assert!(output.starts_with("---\n"), "Should start with YAML front matter");
        assert!(output.contains("title:"), "Should contain title field");
    }

    #[test]
    fn child_sections_recursed() {
        let gen = MarkdownGenerator::new(default_config());
        let mut parent = make_section("PARENT", "Parent text.", 1);
        parent.children.push(make_section("Child", "Child text.", 2));
        let output = gen.generate_for_sections(&[&parent]);
        assert!(output.contains("Parent text."));
        assert!(output.contains("Child text."));
        // Child at level 2 + offset 1 = ### (h3)
        assert!(output.contains("### Child"), "Expected '### Child' in:\n{}", output);
    }
}
```

Note: `FigureInfo::caption_description()` is a helper method that should be added to `types/document.rs` in Task 02. If it was not added there, add it now:

```rust
// In types/document.rs, inside impl FigureInfo:
/// Returns the description portion of the caption, stripping the "Fig. N:" prefix.
pub fn caption_description(&self) -> &str {
    // Strip "Fig. N:" or "Fig. N." prefix from caption_text.
    let text = self.caption_text.trim();
    if let Some(colon_pos) = text.find(':') {
        text[colon_pos + 1..].trim()
    } else if let Some(dot_pos) = text.find('.') {
        // Handle "Fig. 1 Description" (no colon).
        let after_dot = text[dot_pos + 1..].trim();
        if after_dot.starts_with(|c: char| c.is_ascii_digit()) {
            // Second number after dot, skip to next space.
            after_dot.find(' ').map(|i| after_dot[i..].trim()).unwrap_or(text)
        } else {
            after_dot
        }
    } else {
        text
    }
}
```

### Step 3: Wire `to_markdown` in `selector/selector.rs`

In `selector/selector.rs`, add the import and method (should already be stubbed from Task 14,
but finalize it now):

```rust
use crate::output::markdown::MarkdownGenerator;
use crate::config::MarkdownConfig;

impl<'a> SectionSelector<'a> {
    // ... (existing methods) ...

    /// Generate Markdown for the selected sections.
    pub fn to_markdown(&self, config: &MarkdownConfig) -> String {
        MarkdownGenerator::new(config.clone()).generate_for_sections(&self.selected)
    }
}
```

### Step 4: Update `lib.rs`

Add to `crates/pdf-lay-core/src/lib.rs`:

```rust
pub mod output;
```

## Acceptance Criteria

- [ ] All unit tests pass: `cargo test -p pdf-lay-core -- output::markdown`
  - `section_header_uses_heading_offset`
  - `body_text_included`
  - `page_number_block_excluded`
  - `figure_written_with_italic_caption`
  - `yaml_front_matter_emitted_when_enabled`
  - `child_sections_recursed`
- [ ] `SectionSelector::to_markdown()` compiles and produces non-empty output for non-empty selections
- [ ] Figures are inserted at their `insertion_point.after_block_index` relative to blocks
- [ ] `CaptionStyle::Italic`, `Bold`, `PlainText` all produce distinct output
- [ ] `cargo clippy -p pdf-lay-core -- -D warnings` passes

## Dependencies

- Task 13 (pipeline integration, `lib.rs` module setup) must be completed first.
- Task 14 (SectionSelector) must be completed first.

## Commit Message

```
feat(output): add MarkdownGenerator with figure/table insertion and YAML front matter
```
