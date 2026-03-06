# Task 15: LlmTextGenerator

## Overview

Implement `LlmTextGenerator` which converts selected sections into a clean, LLM-optimized
plain text string. Tables are inlined as Markdown or CSV text, figures are represented as
`[IMAGE: Fig. 1 path/to/img.png]` placeholders (or other formats per config), and
section headers are included as `## HeaderText`.

Body text blocks are included; `Caption`, `PageNumber`, `RunningHeader`, `RunningFooter`
blocks are excluded. Child sections are recursively appended.

## Related Information
- **Plan**: `/Users/sonokawa/.claude/plans/fuzzy-drifting-summit.md` (Task 15)
- **Design doc**: `docs/arch/02_DESIGN.md` § 2.7 selector — LlmTextGenerator
- **Spec**: `docs/arch/01_SPECIFICATION.md` § 2.14 F-019
- **Overview**: `docs/develop/overview.md`
- **Dependencies**: Task 14 (SectionSelector) must be completed first

## Files to Create

- [ ] `crates/pdf-lay-core/src/selector/llm_text.rs`

## Files to Modify

- [ ] `crates/pdf-lay-core/src/selector/mod.rs` — uncomment `pub use llm_text::LlmTextGenerator`
- [ ] `crates/pdf-lay-core/src/selector/selector.rs` — add `to_llm_text` method to `SectionSelector`

## Implementation Steps

### Step 1: `selector/llm_text.rs`

```rust
//! Generates LLM-optimized plain text from selected sections.

use crate::config::{FigureTextFormat, LlmTextConfig};
use crate::types::{BlockType, FigureInfo, Section, TableRepresentation};

/// Generates LLM-ready text from a slice of sections.
pub struct LlmTextGenerator {
    config: LlmTextConfig,
}

impl LlmTextGenerator {
    pub fn new(config: LlmTextConfig) -> Self {
        Self { config }
    }

    /// Generate text for a slice of sections (top-level; children are appended recursively).
    pub fn generate(&self, sections: &[&Section]) -> String {
        let mut output = String::new();
        for section in sections {
            self.write_section(&mut output, section);
        }
        output
    }

    fn write_section(&self, out: &mut String, section: &Section) {
        // Section header.
        if self.config.include_section_headers {
            if let Some(header) = &section.header {
                let hashes = "#".repeat(section.level as usize + 1);
                out.push_str(&format!("{} {}\n\n", hashes, header.clean_text));
            }
        }

        // Body blocks.
        for block in &section.blocks {
            match block.block_type {
                BlockType::BodyText | BlockType::Abstract | BlockType::ListItem => {
                    out.push_str(&block.text);
                    out.push_str("\n\n");
                }
                BlockType::Caption
                | BlockType::PageNumber
                | BlockType::RunningHeader
                | BlockType::RunningFooter => {
                    // Skip non-content blocks.
                }
                _ => {
                    // Include other types (Title, Footnote, etc.) by default.
                    out.push_str(&block.text);
                    out.push_str("\n\n");
                }
            }
        }

        // Tables (inline text representation).
        if self.config.include_tables {
            for table in &section.tables {
                if let Some(caption) = &table.caption {
                    out.push_str(&format!("**{}**\n\n", caption));
                }
                match &table.representation {
                    TableRepresentation::Markdown { markdown_text, .. } => {
                        out.push_str(markdown_text);
                    }
                    TableRepresentation::Csv { csv_text, .. } => {
                        out.push_str(csv_text);
                    }
                    TableRepresentation::PlainText { text, .. } => {
                        out.push_str(text);
                    }
                }
                out.push_str("\n\n");
            }
        }

        // Figures.
        if self.config.include_figures {
            for fig in &section.figures {
                self.write_figure(out, fig);
            }
        }

        // Child sections (recursive).
        let child_refs: Vec<&Section> = section.children.iter().collect();
        for child in child_refs {
            self.write_section(out, child);
        }
    }

    fn write_figure(&self, out: &mut String, fig: &FigureInfo) {
        let path_str = fig.image.path.display().to_string();
        match self.config.figure_format {
            FigureTextFormat::Placeholder => {
                out.push_str(&format!("[IMAGE: {} {}]\n\n", fig.figure_id, path_str));
            }
            FigureTextFormat::MarkdownLink => {
                out.push_str(&format!("![{}]({})\n\n", fig.figure_id, path_str));
            }
            FigureTextFormat::CaptionOnly => {
                out.push_str(&format!("[{}]\n\n", fig.caption_text));
            }
            FigureTextFormat::Omit => {
                // Do not include.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FigureTextFormat, LlmTextConfig, MathRepresentationPreference};
    use crate::types::{BlockType, FigureInfo, ImageFormat, ImageInfo, InsertionPoint, Rect, Section, SectionHeader, TextBlock, TextLine};
    use std::path::PathBuf;

    fn default_config() -> LlmTextConfig {
        LlmTextConfig {
            include_figures: true,
            include_tables: true,
            include_section_headers: true,
            math_representation: MathRepresentationPreference::Auto,
            figure_format: FigureTextFormat::Placeholder,
        }
    }

    fn make_body_block(text: &str) -> TextBlock {
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
            blocks: vec![make_body_block(text)],
            figures: vec![],
            tables: vec![],
            children: vec![],
            page_range: (0, 0),
        }
    }

    #[test]
    fn header_included_in_output() {
        let gen = LlmTextGenerator::new(default_config());
        let section = make_section("INTRODUCTION", "Body text here.", 1);
        let output = gen.generate(&[&section]);
        assert!(output.contains("INTRODUCTION"));
        assert!(output.contains("Body text here."));
    }

    #[test]
    fn header_omitted_when_disabled() {
        let mut config = default_config();
        config.include_section_headers = false;
        let gen = LlmTextGenerator::new(config);
        let section = make_section("INTRODUCTION", "Body text.", 1);
        let output = gen.generate(&[&section]);
        assert!(!output.contains("INTRODUCTION"));
        assert!(output.contains("Body text."));
    }

    #[test]
    fn page_numbers_excluded() {
        let gen = LlmTextGenerator::new(default_config());
        let mut section = make_section("INTRO", "Normal text.", 1);
        let mut pn = make_body_block("5");
        pn.block_type = BlockType::PageNumber;
        section.blocks.push(pn);
        let output = gen.generate(&[&section]);
        // "5" as standalone page number should not appear
        assert!(!output.trim_end().ends_with("5"));
    }

    #[test]
    fn figure_placeholder_format() {
        let gen = LlmTextGenerator::new(default_config());
        let mut section = make_section("SEC", "Text.", 1);
        section.figures.push(FigureInfo {
            figure_id: "Fig. 1".to_string(),
            figure_number: Some(1),
            caption_text: "Fig. 1: A figure.".to_string(),
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
        let output = gen.generate(&[&section]);
        assert!(output.contains("[IMAGE: Fig. 1"));
    }

    #[test]
    fn figure_omit_format() {
        let mut config = default_config();
        config.figure_format = FigureTextFormat::Omit;
        let gen = LlmTextGenerator::new(config);
        let mut section = make_section("SEC", "Text.", 1);
        section.figures.push(FigureInfo {
            figure_id: "Fig. 1".to_string(),
            figure_number: Some(1),
            caption_text: "Caption.".to_string(),
            image: ImageInfo {
                path: PathBuf::from("img.png"),
                page: 0,
                raw_bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                normalized_bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                width_px: 0,
                height_px: 0,
                format: ImageFormat::Png,
            },
            context_text: String::new(),
            insertion_point: InsertionPoint { page: 0, after_block_index: None, y_position: 0.0 },
        });
        let output = gen.generate(&[&section]);
        assert!(!output.contains("IMAGE"));
    }

    #[test]
    fn child_sections_recursed() {
        let gen = LlmTextGenerator::new(default_config());
        let mut parent = make_section("PARENT", "Parent text.", 1);
        parent.children.push(make_section("Child", "Child text.", 2));
        let output = gen.generate(&[&parent]);
        assert!(output.contains("Parent text."));
        assert!(output.contains("Child text."));
        assert!(output.contains("Child"));
    }
}
```

### Step 2: Add `to_llm_text` to `SectionSelector`

In `selector/selector.rs`, add:

```rust
// Add import at top:
use crate::selector::llm_text::LlmTextGenerator;
use crate::config::LlmTextConfig;

// Add method to SectionSelector<'a>:
impl<'a> SectionSelector<'a> {
    // ... (existing methods) ...

    /// Generate LLM-optimized text for the selected sections.
    pub fn to_llm_text(&self, config: &LlmTextConfig) -> String {
        LlmTextGenerator::new(config.clone()).generate(&self.selected)
    }
}
```

## Acceptance Criteria

- [ ] All unit tests pass: `cargo test -p pdf-lay-core -- selector::llm_text`
  - `header_included_in_output`
  - `header_omitted_when_disabled`
  - `page_numbers_excluded`
  - `figure_placeholder_format`
  - `figure_omit_format`
  - `child_sections_recursed`
- [ ] `SectionSelector::to_llm_text` is callable and returns non-empty string for non-empty sections
- [ ] Tables are inline-rendered per `TableRepresentation` variant
- [ ] Figure format is configurable: Placeholder / MarkdownLink / CaptionOnly / Omit
- [ ] Child sections are recursively included
- [ ] `cargo clippy -p pdf-lay-core -- -D warnings` passes

## Dependencies

- Task 14 (SectionSelector + SectionEntry) must be completed first.

## Commit Message

```
feat(selector): add LlmTextGenerator producing LLM-ready plain text with inline tables and figure placeholders
```
