//! Generates LLM-optimized plain text from selected sections.

use crate::config::{FigureTextFormat, LlmTextConfig, MathConfig, MathRepresentationPreference};
use crate::math::{MathConverter, MathDetector};
use crate::output::render_core::{self, EscapeMode};
use crate::types::{BlockType, FigureInfo, Section, TableRepresentation};

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
        // Section header.
        if self.config.include_section_headers
            && let Some(header) = &section.header
        {
            let hashes = "#".repeat(section.level as usize + 1);
            out.push_str(&format!("{} {}\n\n", hashes, header.clean_text));
        }

        // Prepare math detector/converter once per section (from LlmTextConfig).
        let math_config = math_config_from_llm(&self.config);
        let math_components = math_config.as_ref().map(|mc| {
            (
                MathDetector::new(mc.clone()),
                MathConverter::new(mc.clone()),
            )
        });

        // Body blocks. Delegates to render-core's single block→rich-text
        // implementation (shared with markdown/chunker).
        for block in &section.blocks {
            match block.block_type {
                BlockType::BodyText | BlockType::Abstract | BlockType::ListItem => {
                    let (detector, converter) = match &math_components {
                        Some((d, c)) => (Some(d), Some(c)),
                        None => (None, None),
                    };
                    let text = render_core::render_block(
                        block,
                        detector,
                        converter,
                        math_config.as_ref(),
                        EscapeMode::Plain,
                    );
                    out.push_str(&text);
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
                    let (detector, converter) = match &math_components {
                        Some((d, c)) => (Some(d), Some(c)),
                        None => (None, None),
                    };
                    let text = render_core::render_block(
                        block,
                        detector,
                        converter,
                        math_config.as_ref(),
                        EscapeMode::Plain,
                    );
                    out.push_str(&text);
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
        for child in &section.children {
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
    use crate::types::{
        BlockType, FigureInfo, ImageFormat, ImageInfo, InsertionPoint, Rect, Section,
        SectionHeader, TextBlock,
    };
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
