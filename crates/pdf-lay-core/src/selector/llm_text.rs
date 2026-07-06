//! Generates LLM-optimized plain text from selected sections.

use crate::config::{LlmTextConfig, MathConfig, MathRepresentationPreference};
use crate::output::render_core::{self, EscapeMode, RenderOptions};
use crate::types::Section;

/// Build a [`MathConfig`] from the representation preference stored in [`LlmTextConfig`].
///
/// Delimiter defaults are used since `LlmTextConfig` does not carry custom delimiters.
fn math_config_from_llm(config: &LlmTextConfig) -> Option<MathConfig> {
    match config.math_representation {
        MathRepresentationPreference::PlainText => None,
        ref pref => Some(MathConfig {
            representation: pref.clone(),
            ..MathConfig::default()
        }),
    }
}

/// Generates LLM-ready text from a slice of sections.
pub struct LlmTextGenerator {
    config: LlmTextConfig,
}

impl LlmTextGenerator {
    /// Create a new generator with the given configuration.
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
        // Section header. Kept as llm_text's own "#"-per-level heading style
        // (distinct from render-core's plain heading line), so this is
        // handled here rather than via `RenderOptions::include_headers`.
        if self.config.include_section_headers
            && let Some(header) = &section.header
        {
            let hashes = "#".repeat(section.level as usize + 1);
            out.push_str(&format!("{} {}\n\n", hashes, header.clean_text));
        }

        // Body blocks, figures, and tables. Delegates to render-core's
        // single section-body implementation (shared with markdown/chunker),
        // which interleaves figures/tables at their `insertion_point` instead
        // of draining them all at the section end, and builds figure links
        // from `image_base` + filename rather than the raw on-disk path.
        let math_config = math_config_from_llm(&self.config);
        let opts = RenderOptions {
            math_config: math_config.as_ref(),
            escape: EscapeMode::Plain,
            include_headers: false,
            include_figures: self.config.include_figures,
            include_tables: self.config.include_tables,
            figure_format: self.config.figure_format.clone(),
            image_base: self.config.image_base.clone(),
        };
        let body = render_core::render_section_content(section, &opts);
        if !body.is_empty() {
            out.push_str(&body);
            out.push_str("\n\n");
        }

        // Child sections (recursive).
        for child in &section.children {
            self.write_section(out, child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FigureTextFormat, LlmTextConfig, MathRepresentationPreference};
    use crate::types::{
        BlockType, FigureInfo, ImageFormat, ImageInfo, InsertionPoint, Rect, Section,
        SectionHeader, TableInfo, TableRepresentation, TextBlock,
    };
    use std::path::PathBuf;

    fn default_config() -> LlmTextConfig {
        LlmTextConfig {
            include_figures: true,
            include_tables: true,
            include_section_headers: true,
            math_representation: MathRepresentationPreference::Auto,
            figure_format: FigureTextFormat::Placeholder,
            image_base: String::new(),
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
        let generator = LlmTextGenerator::new(default_config());
        let section = make_section("INTRODUCTION", "Body text here.", 1);
        let output = generator.generate(&[&section]);
        assert!(output.contains("INTRODUCTION"));
        assert!(output.contains("Body text here."));
    }

    #[test]
    fn header_omitted_when_disabled() {
        let mut config = default_config();
        config.include_section_headers = false;
        let generator = LlmTextGenerator::new(config);
        let section = make_section("INTRODUCTION", "Body text.", 1);
        let output = generator.generate(&[&section]);
        assert!(!output.contains("INTRODUCTION"));
        assert!(output.contains("Body text."));
    }

    #[test]
    fn page_numbers_excluded() {
        let generator = LlmTextGenerator::new(default_config());
        let mut section = make_section("INTRO", "Normal text.", 1);
        let mut pn = make_body_block("5");
        pn.block_type = BlockType::PageNumber;
        section.blocks.push(pn);
        let output = generator.generate(&[&section]);
        // "5" as standalone page number should not appear
        assert!(!output.trim_end().ends_with("5"));
    }

    #[test]
    fn figure_placeholder_format() {
        let generator = LlmTextGenerator::new(default_config());
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
            insertion_point: InsertionPoint {
                page: 0,
                after_block_index: None,
                y_position: 0.0,
            },
        });
        let output = generator.generate(&[&section]);
        assert!(output.contains("[IMAGE: Fig. 1"));
    }

    #[test]
    fn figure_omit_format() {
        let mut config = default_config();
        config.figure_format = FigureTextFormat::Omit;
        let generator = LlmTextGenerator::new(config);
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
            insertion_point: InsertionPoint {
                page: 0,
                after_block_index: None,
                y_position: 0.0,
            },
        });
        let output = generator.generate(&[&section]);
        assert!(!output.contains("IMAGE"));
    }

    #[test]
    fn figure_uses_image_base_not_raw_path() {
        let mut config = default_config();
        config.image_base = "./img".to_string();
        let generator = LlmTextGenerator::new(config);
        let mut section = make_section("SEC", "Text.", 1);
        section.figures.push(FigureInfo {
            figure_id: "Fig. 1".to_string(),
            figure_number: Some(1),
            caption_text: "Fig. 1: A figure.".to_string(),
            image: ImageInfo {
                path: PathBuf::from("/abs/images/p000_img000.png"),
                page: 0,
                raw_bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                normalized_bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                width_px: 100,
                height_px: 100,
                format: ImageFormat::Png,
            },
            context_text: String::new(),
            insertion_point: InsertionPoint {
                page: 0,
                after_block_index: None,
                y_position: 0.0,
            },
        });
        let output = generator.generate(&[&section]);
        assert!(
            output.contains("./img/p000_img000.png"),
            "expected image_base-prefixed filename in output:\n{output}"
        );
        assert!(
            !output.contains("/abs/images/"),
            "raw on-disk directory must not leak into output:\n{output}"
        );
    }

    #[test]
    fn figure_base_empty_is_filename_only() {
        // default_config() leaves image_base empty.
        let generator = LlmTextGenerator::new(default_config());
        let mut section = make_section("SEC", "Text.", 1);
        section.figures.push(FigureInfo {
            figure_id: "Fig. 1".to_string(),
            figure_number: Some(1),
            caption_text: "Fig. 1: A figure.".to_string(),
            image: ImageInfo {
                path: PathBuf::from("/abs/images/p000_img000.png"),
                page: 0,
                raw_bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                normalized_bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                width_px: 100,
                height_px: 100,
                format: ImageFormat::Png,
            },
            context_text: String::new(),
            insertion_point: InsertionPoint {
                page: 0,
                after_block_index: None,
                y_position: 0.0,
            },
        });
        let output = generator.generate(&[&section]);
        assert!(
            output.contains("[IMAGE: Fig. 1 p000_img000.png]"),
            "expected filename-only image path (no raw disk path):\n{output}"
        );
        assert!(!output.contains("/abs/"));
    }

    #[test]
    fn figure_inserted_at_insertion_point() {
        let generator = LlmTextGenerator::new(default_config());
        let mut first = make_body_block("BEFORE_MARKER");
        first.global_index = 0;
        let mut second = make_body_block("AFTER_MARKER");
        second.global_index = 1;
        let mut section = Section {
            header: None,
            level: 1,
            blocks: vec![first, second],
            figures: vec![],
            tables: vec![],
            children: vec![],
            page_range: (0, 0),
        };
        section.figures.push(FigureInfo {
            figure_id: "Fig. 1".to_string(),
            figure_number: Some(1),
            caption_text: "Fig. 1: A figure.".to_string(),
            image: ImageInfo {
                path: PathBuf::from("img.png"),
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

        let output = generator.generate(&[&section]);
        let before_pos = output.find("BEFORE_MARKER").expect("first block present");
        let image_pos = output.find("[IMAGE:").expect("figure placeholder present");
        let after_pos = output.find("AFTER_MARKER").expect("second block present");
        assert!(
            before_pos < image_pos && image_pos < after_pos,
            "figure anchored after block 0 must appear between the two blocks, not dumped at section end:\n{output}"
        );
    }

    #[test]
    fn table_inserted_at_insertion_point() {
        let generator = LlmTextGenerator::new(default_config());
        let mut first = make_body_block("BEFORE_MARKER");
        first.global_index = 0;
        let mut second = make_body_block("AFTER_MARKER");
        second.global_index = 1;
        let mut section = Section {
            header: None,
            level: 1,
            blocks: vec![first, second],
            figures: vec![],
            tables: vec![],
            children: vec![],
            page_range: (0, 0),
        };
        section.tables.push(TableInfo {
            table_id: "Table 1".to_string(),
            table_number: Some(1),
            caption: None,
            representation: TableRepresentation::PlainText {
                text: "TABLE_MARKER".to_string(),
                caption: None,
            },
            insertion_point: InsertionPoint {
                page: 0,
                after_block_index: Some(0),
                y_position: 0.0,
            },
            page: 0,
        });

        let output = generator.generate(&[&section]);
        let before_pos = output.find("BEFORE_MARKER").expect("first block present");
        let table_pos = output.find("TABLE_MARKER").expect("table text present");
        let after_pos = output.find("AFTER_MARKER").expect("second block present");
        assert!(
            before_pos < table_pos && table_pos < after_pos,
            "table anchored after block 0 must appear between the two blocks, not dumped at section end:\n{output}"
        );
    }

    #[test]
    fn child_sections_recursed() {
        let generator = LlmTextGenerator::new(default_config());
        let mut parent = make_section("PARENT", "Parent text.", 1);
        parent
            .children
            .push(make_section("Child", "Child text.", 2));
        let output = generator.generate(&[&parent]);
        assert!(output.contains("Parent text."));
        assert!(output.contains("Child text."));
        assert!(output.contains("Child"));
    }

    // ---- Math integration tests -------------------------------------------

    /// Build a TextBlock with a single math-font span (all-math line → Display context).
    fn make_math_body_block(math_text: &str, font_name: &str) -> TextBlock {
        use crate::types::{TextLine, TextSpan};

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
        TextBlock {
            global_index: 0,
            lines: vec![line],
            text: math_text.to_string(),
            bbox: Rect::new(100.0, 700.0, 150.0, 690.0),
            page: 0,
            column_index: 0,
            block_type: BlockType::BodyText,
        }
    }

    fn section_with_block(block: TextBlock) -> Section {
        Section {
            header: None,
            level: 1,
            blocks: vec![block],
            figures: vec![],
            tables: vec![],
            children: vec![],
            page_range: (0, 0),
        }
    }

    #[test]
    fn test_math_inline_in_llm_text_latex() {
        // With LaTeX representation, CM font α should appear as $\alpha$.
        let config = LlmTextConfig {
            math_representation: MathRepresentationPreference::LaTeX,
            ..default_config()
        };
        let generator = LlmTextGenerator::new(config);
        let block = make_math_body_block("α", "CMMI10");
        let section = section_with_block(block);
        let output = generator.generate(&[&section]);
        assert!(
            output.contains("\\alpha"),
            "Expected LaTeX \\alpha in LLM text output:\n{output}"
        );
    }

    #[test]
    fn test_math_display_in_llm_text() {
        // All-math line → Display context → $$ delimiters with LaTeX representation.
        let config = LlmTextConfig {
            math_representation: MathRepresentationPreference::LaTeX,
            ..default_config()
        };
        let generator = LlmTextGenerator::new(config);
        let block = make_math_body_block("α", "CMMI10");
        let section = section_with_block(block);
        let output = generator.generate(&[&section]);
        assert!(
            output.contains("$$"),
            "Expected display math '$$' in LLM text output:\n{output}"
        );
    }

    #[test]
    fn test_plain_text_representation_no_delimiters() {
        // PlainText representation → math_config_from_llm returns None → no delimiters.
        let config = LlmTextConfig {
            math_representation: MathRepresentationPreference::PlainText,
            ..default_config()
        };
        let generator = LlmTextGenerator::new(config);
        let block = make_math_body_block("α", "CMMI10");
        let section = section_with_block(block);
        let output = generator.generate(&[&section]);
        // PlainText maps to None in math_config_from_llm, so output is block.text unchanged.
        assert!(
            !output.contains("$"),
            "PlainText representation should not emit $ delimiters:\n{output}"
        );
    }
}
