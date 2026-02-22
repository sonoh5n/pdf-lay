//! Generates Markdown output from a PaperDocument or selected sections.

use std::collections::VecDeque;

use crate::config::{CaptionStyle, MarkdownConfig};
use crate::types::{BlockType, FigureInfo, PaperDocument, Section, TableInfo, TableRepresentation};

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
            let raw_level = (header.level as usize) + (self.config.heading_offset as usize);
            let level = raw_level.clamp(1, 6);
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
        // Construct image path relative to image_base_path.
        let filename = fig
            .image
            .path
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
}
