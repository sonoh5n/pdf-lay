//! Generates Markdown output from a PaperDocument or selected sections.

use std::collections::VecDeque;
use std::path::Path;

use crate::config::{CaptionStyle, MarkdownConfig};
use crate::math::{MathConverter, MathDetector};
use crate::output::render_core::{self, EscapeMode, escape_for_markdown_text};
use crate::types::{BlockType, FigureInfo, PaperDocument, Section, TableInfo, TableRepresentation};

/// Escape a string for use inside a double-quoted YAML value.
///
/// Handles `"`, `\`, and control characters that could break YAML parsing.
fn escape_for_yaml_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

/// Build a Markdown image link path pointing at `filename` inside `image_dir`,
/// expressed relative to `output_dir` (the directory of the output `.md` file).
///
/// Uses forward slashes for portability. Falls back to `./filename` when a
/// relative path cannot be computed.
fn relative_image_path(image_dir: &Path, output_dir: &Path, filename: &str) -> String {
    match pathdiff::diff_paths(image_dir, output_dir) {
        Some(rel) => {
            let joined = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            if joined.is_empty() {
                format!("./{filename}")
            } else {
                format!("{joined}/{filename}")
            }
        }
        None => format!("./{filename}"),
    }
}

/// Generates Markdown from a [`PaperDocument`].
pub struct MarkdownGenerator {
    config: MarkdownConfig,
}

impl MarkdownGenerator {
    /// Create a new generator with the given configuration.
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

    /// Generate Markdown for a slice of selected sections (used by `SectionSelector`).
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
            fm.push_str(&format!("title: \"{}\"\n", escape_for_yaml_value(title)));
        }
        if !metadata.authors.is_empty() {
            fm.push_str("authors:\n");
            for author in &metadata.authors {
                fm.push_str(&format!("  - \"{}\"\n", escape_for_yaml_value(author)));
            }
        }
        fm.push_str("---\n\n");
        fm
    }

    fn write_section(&self, md: &mut String, section: &Section) {
        // Section header.
        if let Some(header) = &section.header {
            let raw_level = (header.level as usize) + (self.config.heading_offset as usize);
            let level = raw_level.clamp(1, 6);
            let prefix = "#".repeat(level);
            md.push_str(&format!(
                "{} {}\n\n",
                prefix,
                escape_for_markdown_text(&header.clean_text)
            ));
        }

        // Optional page number comment.
        if self.config.include_page_numbers {
            md.push_str(&format!("<!-- page {} -->\n\n", section.page_range.0));
        }

        // Prepare math detector/converter if math_config is set.
        let math_components = self.config.math_config.as_ref().map(|mc| {
            (
                MathDetector::new(mc.clone()),
                MathConverter::new(mc.clone()),
            )
        });

        // Iterate blocks; insert figures/tables at their insertion_point.
        let mut figure_queue: VecDeque<&FigureInfo> = section.figures.iter().collect();
        let mut table_queue: VecDeque<&TableInfo> = section.tables.iter().collect();

        for block in &section.blocks {
            // Non-content blocks contribute no body text, but we must still run
            // the figure/table drain below (a figure anchored to such a block
            // would otherwise never be emitted inline and get flushed at the
            // section end).
            let emit_body = !matches!(
                block.block_type,
                BlockType::Caption
                    | BlockType::PageNumber
                    | BlockType::RunningHeader
                    | BlockType::RunningFooter
            );
            if emit_body {
                // Delegates to render-core's single block→rich-text
                // implementation (shared with llm_text/chunker). Escaping is
                // applied inside render_block to non-math spans only,
                // preserving math delimiters.
                let (detector, converter) = match &math_components {
                    Some((d, c)) => (Some(d), Some(c)),
                    None => (None, None),
                };
                let text = render_core::render_block(
                    block,
                    detector,
                    converter,
                    self.config.math_config.as_ref(),
                    EscapeMode::Markdown,
                );
                md.push_str(&text);
                md.push_str("\n\n");
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

        // Flush remaining figures/tables (no specific insertion point matched).
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
        let filename = fig
            .image
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| fig.image.path.display().to_string());
        // When both the on-disk image directory and the output directory are
        // known, write the link as a path relative to the output file so it
        // resolves correctly regardless of where the .md is written. Otherwise
        // fall back to prefixing image_base_path (the legacy behavior).
        let path = match (&self.config.image_dir, &self.config.output_dir) {
            (Some(image_dir), Some(output_dir)) => {
                relative_image_path(image_dir, output_dir, &filename)
            }
            _ if self.config.image_base_path.is_empty() => filename,
            _ => format!("{}/{}", self.config.image_base_path, filename),
        };

        let safe_id = escape_for_markdown_text(&fig.figure_id);
        md.push_str(&format!("![{}]({})\n\n", safe_id, path));

        match self.config.figure_caption_style {
            CaptionStyle::Italic => {
                let safe = escape_for_markdown_text(&fig.caption_text);
                md.push_str(&format!("*{safe}*\n\n"));
            }
            CaptionStyle::Bold => {
                // Bold the figure_id prefix, remainder as plain text.
                let safe_desc = escape_for_markdown_text(fig.caption_description());
                md.push_str(&format!("**{safe_id}** {safe_desc}\n\n"));
            }
            CaptionStyle::PlainText => {
                let safe = escape_for_markdown_text(&fig.caption_text);
                md.push_str(&format!("{safe}\n\n"));
            }
        }
    }

    fn write_table(&self, md: &mut String, table: &TableInfo) {
        if let Some(caption) = &table.caption {
            md.push_str(&format!("**{}**\n\n", escape_for_markdown_text(caption)));
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
    use crate::config::{CaptionStyle, MarkdownConfig, MathConfig, MathRepresentationPreference};
    use crate::types::{
        BlockType, DocumentMetadata, FigureInfo, ImageFormat, ImageInfo, InsertionPoint,
        PaperDocument, Rect, Section, SectionHeader, TextBlock, TextLine,
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
            math_config: None,
            image_dir: None,
            output_dir: None,
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
        let mdgen = MarkdownGenerator::new(default_config());
        let section = make_section("INTRODUCTION", "Body text.", 1);
        let output = mdgen.generate_for_sections(&[&section]);
        // level 1 + offset 1 = ## (h2)
        assert!(
            output.contains("## INTRODUCTION"),
            "Expected '## INTRODUCTION' in:\n{}",
            output
        );
    }

    #[test]
    fn body_text_included() {
        let mdgen = MarkdownGenerator::new(default_config());
        let section = make_section("SEC", "Some body text.", 1);
        let output = mdgen.generate_for_sections(&[&section]);
        assert!(output.contains("Some body text."));
    }

    #[test]
    fn page_number_block_excluded() {
        let mdgen = MarkdownGenerator::new(default_config());
        let mut section = make_section("SEC", "Body.", 1);
        let mut pn = make_block("42");
        pn.block_type = BlockType::PageNumber;
        section.blocks.push(pn);
        let output = mdgen.generate_for_sections(&[&section]);
        // "42" as standalone page number block should not appear
        assert!(
            !output.trim_end().ends_with("42"),
            "Page number should be excluded"
        );
    }

    #[test]
    fn figure_written_with_italic_caption() {
        let mdgen = MarkdownGenerator::new(default_config());
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
            insertion_point: InsertionPoint {
                page: 0,
                after_block_index: None,
                y_position: 0.0,
            },
        });
        let output = mdgen.generate_for_sections(&[&section]);
        assert!(output.contains("![Fig. 1]"), "Should contain image link");
        assert!(
            output.contains("*Fig. 1: A diagram.*"),
            "Should contain italic caption"
        );
    }

    fn section_with_one_figure() -> Section {
        Section {
            header: None,
            level: 1,
            blocks: vec![make_block("Body.")],
            figures: vec![FigureInfo {
                figure_id: "Fig. 1".to_string(),
                figure_number: Some(1),
                caption_text: "Fig. 1: X.".to_string(),
                image: ImageInfo {
                    path: PathBuf::from("out/images/p000_img000.png"),
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
                    after_block_index: None,
                    y_position: 0.0,
                },
            }],
            tables: vec![],
            children: vec![],
            page_range: (0, 0),
        }
    }

    #[test]
    fn figure_link_is_relative_to_output_dir() {
        // Images under out/images, output written to docs/paper.md → link must be
        // "../out/images/p000_img000.png".
        let mut config = default_config();
        config.image_dir = Some(PathBuf::from("out/images"));
        config.output_dir = Some(PathBuf::from("docs"));
        let mdgen = MarkdownGenerator::new(config);
        let out = mdgen.generate_for_sections(&[&section_with_one_figure()]);
        assert!(
            out.contains("![Fig. 1](../out/images/p000_img000.png)"),
            "expected relative link ../out/images/...:\n{out}"
        );
    }

    #[test]
    fn explicit_image_base_preserved_when_no_dirs() {
        // No image_dir/output_dir → legacy prefix behavior with image_base_path.
        let config = default_config(); // image_base_path = "./images", dirs None
        let mdgen = MarkdownGenerator::new(config);
        let out = mdgen.generate_for_sections(&[&section_with_one_figure()]);
        assert!(
            out.contains("![Fig. 1](./images/p000_img000.png)"),
            "expected legacy prefixed link ./images/...:\n{out}"
        );
    }

    #[test]
    fn figure_anchored_to_caption_block_emitted_inline() {
        // A figure whose insertion point is a Caption block must still be
        // emitted at that point (inline), not flushed to the section end.
        let mdgen = MarkdownGenerator::new(default_config());
        let caption = TextBlock {
            global_index: 0,
            lines: vec![],
            text: "Fig. 1: Cap".to_string(),
            bbox: Rect::new(72.0, 700.0, 540.0, 690.0),
            page: 0,
            column_index: 0,
            block_type: BlockType::Caption,
        };
        let body = TextBlock {
            global_index: 1,
            lines: vec![],
            text: "BODYTEXTMARKER".to_string(),
            bbox: Rect::new(72.0, 680.0, 540.0, 670.0),
            page: 0,
            column_index: 0,
            block_type: BlockType::BodyText,
        };
        let section = Section {
            header: None,
            level: 1,
            blocks: vec![caption, body],
            figures: vec![FigureInfo {
                figure_id: "Fig. 1".to_string(),
                figure_number: Some(1),
                caption_text: "Fig. 1: Cap".to_string(),
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
            }],
            tables: vec![],
            children: vec![],
            page_range: (0, 0),
        };
        let out = mdgen.generate_for_sections(&[&section]);
        let img_pos = out
            .find("![Fig. 1]")
            .expect("figure link should be present");
        let body_pos = out
            .find("BODYTEXTMARKER")
            .expect("body text should be present");
        assert!(
            img_pos < body_pos,
            "figure anchored to caption block (index 0) must appear before the body block (index 1):\n{out}"
        );
    }

    #[test]
    fn yaml_front_matter_emitted_when_enabled() {
        let mut config = default_config();
        config.include_metadata_header = true;
        let mdgen = MarkdownGenerator::new(config);
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
        let output = mdgen.generate(&doc);
        assert!(
            output.starts_with("---\n"),
            "Should start with YAML front matter"
        );
        assert!(output.contains("title:"), "Should contain title field");
    }

    #[test]
    fn child_sections_recursed() {
        let mdgen = MarkdownGenerator::new(default_config());
        let mut parent = make_section("PARENT", "Parent text.", 1);
        parent
            .children
            .push(make_section("Child", "Child text.", 2));
        let output = mdgen.generate_for_sections(&[&parent]);
        assert!(output.contains("Parent text."));
        assert!(output.contains("Child text."));
        // Child at level 2 + offset 1 = ### (h3)
        assert!(
            output.contains("### Child"),
            "Expected '### Child' in:\n{}",
            output
        );
    }

    // ---- Math integration tests -------------------------------------------

    /// Build a TextBlock that contains a single line with one CM-font (math) span.
    fn make_math_block(math_text: &str, font_name: &str) -> TextBlock {
        use crate::types::TextSpan;

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

    /// Build a TextBlock with a mixed line: one plain text span followed by one CM math span.
    fn make_mixed_block(plain_text: &str, math_text: &str, math_font: &str) -> TextBlock {
        use crate::types::TextSpan;

        let plain_span = TextSpan {
            text: plain_text.to_string(),
            font_name: "TimesNewRoman".to_string(),
            font_size: 10.0,
            is_bold: false,
            is_italic: false,
            bbox: Rect::new(50.0, 700.0, 95.0, 690.0),
            page: 0,
        };
        let math_span = TextSpan {
            text: math_text.to_string(),
            font_name: math_font.to_string(),
            font_size: 10.0,
            is_bold: false,
            is_italic: true,
            bbox: Rect::new(100.0, 700.0, 145.0, 690.0),
            page: 0,
        };
        let full_text = format!("{plain_text}{math_text}");
        let line = TextLine {
            text: full_text.clone(),
            spans: vec![plain_span, math_span],
            bbox: Rect::new(50.0, 700.0, 145.0, 690.0),
            page: 0,
            baseline_y: 690.0,
            primary_font_size: 10.0,
            primary_font_name: "TimesNewRoman".to_string(),
            is_bold: false,
        };
        TextBlock {
            global_index: 0,
            lines: vec![line],
            text: full_text,
            bbox: Rect::new(50.0, 700.0, 145.0, 690.0),
            page: 0,
            column_index: 0,
            block_type: BlockType::BodyText,
        }
    }

    /// A section that holds a pre-built block.
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
    fn test_math_inline_in_markdown() {
        // A block with a mixed line: plain text span + CM math font span.
        // The math span is Inline (not all-math), so the output should contain `$\alpha$`.
        let mut config = default_config();
        config.math_config = Some(MathConfig {
            representation: MathRepresentationPreference::LaTeX,
            ..MathConfig::default()
        });
        let mdgen = MarkdownGenerator::new(config);
        // Mixed block: "where " (plain) + "α" (CMMI10 math) → Inline context.
        let block = make_mixed_block("where ", "α", "CMMI10");
        let section = section_with_block(block);
        let output = mdgen.generate_for_sections(&[&section]);
        assert!(
            output.contains("$\\alpha$"),
            "Expected inline math '$\\alpha$' in:\n{output}"
        );
    }

    #[test]
    fn test_math_display_in_markdown() {
        // A block whose single line contains only CM math spans — the whole
        // line is classified as Display math, so the output should use `$$`.
        let mut config = default_config();
        config.math_config = Some(MathConfig {
            representation: MathRepresentationPreference::LaTeX,
            ..MathConfig::default()
        });
        let mdgen = MarkdownGenerator::new(config);
        // Pure-math block (all spans CM → Display context).
        let block = make_math_block("α", "CMMI10");
        let section = section_with_block(block);
        let output = mdgen.generate_for_sections(&[&section]);
        // All-math single-span line → Display → $$ delimiters.
        assert!(
            output.contains("$$"),
            "Expected display math '$$' delimiters in:\n{output}"
        );
    }

    #[test]
    fn test_no_math_passthrough() {
        // When math_config is None the block text should be passed through unchanged.
        let config = default_config(); // math_config is None
        let mdgen = MarkdownGenerator::new(config);
        let block = make_mixed_block("where ", "α", "TimesNewRoman");
        let expected_text = block.text.clone();
        let section = section_with_block(block);
        let output = mdgen.generate_for_sections(&[&section]);
        assert!(
            output.contains(&expected_text),
            "Expected plain text passthrough '{expected_text}' in:\n{output}"
        );
        // No math delimiters when math_config is None.
        assert!(
            !output.contains("$\\alpha$"),
            "Should not contain math delimiters when math_config is None"
        );
    }

    // ---- Sanitization tests ---------------------------------------------------

    #[test]
    fn escape_yaml_value_handles_special_chars() {
        assert_eq!(
            escape_for_yaml_value(r#"A "title" with \ and tabs	"#),
            r#"A \"title\" with \\ and tabs\t"#
        );
        assert_eq!(escape_for_yaml_value("line1\nline2"), "line1\\nline2");
    }

    #[test]
    fn escape_markdown_text_neutralizes_html() {
        assert_eq!(
            escape_for_markdown_text("<script>alert(1)</script>"),
            "&lt;script&gt;alert(1)&lt;/script&gt;"
        );
    }

    #[test]
    fn escape_markdown_text_prevents_link_injection() {
        assert_eq!(
            escape_for_markdown_text("click [here](http://evil.com)"),
            "click [here]\\(http://evil.com)"
        );
    }

    #[test]
    fn front_matter_escapes_yaml_injection() {
        let mut config = default_config();
        config.include_metadata_header = true;
        let mdgen = MarkdownGenerator::new(config);
        let doc = PaperDocument {
            paper_id: "test".to_string(),
            source_file: PathBuf::from("test.pdf"),
            metadata: DocumentMetadata {
                pages: 1,
                title: Some("evil\"\n  injected: true".to_string()),
                authors: vec![],
                ..Default::default()
            },
            sections: vec![],
            all_figures: vec![],
            all_tables: vec![],
        };
        let output = mdgen.generate(&doc);
        // The raw newline must be escaped so "injected: true" stays inside the
        // YAML string value rather than becoming a separate key.
        assert!(
            output.contains(r#"title: "evil\"#),
            "Quotes should be escaped in YAML value:\n{output}"
        );
        // The literal two-char sequence \n should appear instead of a real newline
        // between "evil..." and "injected".
        assert!(
            output.contains("\\n"),
            "Newlines should be escaped:\n{output}"
        );
        // The value must remain on one line (no raw newline inside the title field)
        for line in output.lines() {
            if line.trim_start().starts_with("title:") {
                assert!(
                    line.contains("injected: true"),
                    "Injected text should be contained within the title value, not a separate key:\n{output}"
                );
            }
        }
    }

    #[test]
    fn body_text_escapes_html_tags() {
        let mdgen = MarkdownGenerator::new(default_config());
        let section = make_section("SEC", "Hello <img src=x onerror=alert(1)>", 1);
        let output = mdgen.generate_for_sections(&[&section]);
        assert!(
            !output.contains("<img"),
            "HTML tags should be escaped:\n{output}"
        );
        assert!(output.contains("&lt;img"));
    }

    #[test]
    fn section_header_escapes_html() {
        let mdgen = MarkdownGenerator::new(default_config());
        let section = make_section("<script>alert(1)</script>", "Body.", 1);
        let output = mdgen.generate_for_sections(&[&section]);
        assert!(
            !output.contains("<script>"),
            "Script tags in headers should be escaped:\n{output}"
        );
    }

    #[test]
    fn escape_markdown_text_handles_ampersand() {
        assert_eq!(escape_for_markdown_text("A & B"), "A &amp; B");
    }

    #[test]
    fn math_delimiters_not_escaped_in_mixed_block() {
        // When math_config is enabled, the `$...$` delimiters and LaTeX
        // operators like `<` inside math must NOT be HTML-escaped.
        let mut config = default_config();
        config.math_config = Some(MathConfig {
            representation: MathRepresentationPreference::LaTeX,
            ..MathConfig::default()
        });
        let mdgen = MarkdownGenerator::new(config);
        // Mixed block: "where " (plain) + "α" (CMMI10 math) → Inline context.
        let block = make_mixed_block("where ", "α", "CMMI10");
        let section = section_with_block(block);
        let output = mdgen.generate_for_sections(&[&section]);
        // Math should remain intact — $ delimiters should not be mangled.
        assert!(
            output.contains("$\\alpha$"),
            "Math delimiters must not be escaped:\n{output}"
        );
        // But non-math text should still be there.
        assert!(
            output.contains("where "),
            "Non-math text should be preserved:\n{output}"
        );
    }

    #[test]
    fn html_in_non_math_span_escaped_with_math_enabled() {
        // Even with math enabled, non-math text must be sanitized.
        let mut config = default_config();
        config.math_config = Some(MathConfig {
            representation: MathRepresentationPreference::LaTeX,
            ..MathConfig::default()
        });
        let mdgen = MarkdownGenerator::new(config);
        let block = make_mixed_block("<script>evil</script> ", "α", "CMMI10");
        let section = section_with_block(block);
        let output = mdgen.generate_for_sections(&[&section]);
        assert!(
            !output.contains("<script>"),
            "HTML in non-math text must be escaped even with math enabled:\n{output}"
        );
        assert!(
            output.contains("&lt;script&gt;"),
            "HTML should be entity-escaped:\n{output}"
        );
        // Math part should still work.
        assert!(
            output.contains("$\\alpha$"),
            "Math should still be rendered:\n{output}"
        );
    }
}
