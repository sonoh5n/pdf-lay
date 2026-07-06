//! Content-only projection of a [`PaperDocument`] for lightweight JSON output.
//!
//! [`JsonGenerator::generate`](super::json::JsonGenerator::generate) serializes
//! the *entire* [`PaperDocument`] tree, including every [`TextBlock`](crate::types::TextBlock)
//! bounding box, [`TextSpan`](crate::types::TextSpan) font name/size, and line
//! geometry. That is the right shape for tooling that needs exact layout
//! (debugging, re-rendering), but it is far heavier than an LLM needs and its
//! `block.text` is raw, unconverted text (no math, no table Markdown).
//!
//! This module defines a parallel, `Serialize`-only-in-spirit (also
//! `Deserialize`, since the cost is negligible) intermediate representation —
//! [`ContentDocument`] — that keeps only what an LLM/RAG consumer cares about:
//! section headers, a breadcrumb path, math-converted body text (produced by
//! [`render_core::render_section_content`]), and a light summary of each
//! figure/table. No `bbox`, no font metadata, no per-line/per-span arrays.

use crate::config::MathConfig;
use crate::output::render_core::{self, EscapeMode, RenderOptions};
use crate::types::{FigureInfo, PaperDocument, Section, TableInfo, TableRepresentation};
use serde::{Deserialize, Serialize};

/// Content-only projection of a [`PaperDocument`].
///
/// Carries document-level metadata plus the top-level [`ContentSection`]
/// tree. Contains no geometry (`bbox`) or font metadata anywhere in the tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentDocument {
    /// Unique identifier for this paper (mirrors `PaperDocument::paper_id`).
    pub paper_id: String,
    /// Document title, if extractable.
    pub title: Option<String>,
    /// List of author names.
    pub authors: Vec<String>,
    /// DOI string, if present.
    pub doi: Option<String>,
    /// Total number of pages.
    pub pages: u32,
    /// Top-level sections (children are nested inside each `ContentSection`).
    pub sections: Vec<ContentSection>,
}

/// Header info for a [`ContentSection`], stripped of `bbox`/`block_index`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentHeader {
    /// Raw header text as it appears in the PDF (e.g. "II. KNOWLEDGE GRAPHS").
    pub text: String,
    /// Header text with numbering removed (e.g. "KNOWLEDGE GRAPHS").
    pub clean_text: String,
    /// The number prefix if present (e.g. "II.", "3.1").
    pub numbering: Option<String>,
}

/// One section in the content-only tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentSection {
    /// This section's own header, or `None` for the headerless preamble.
    pub header: Option<ContentHeader>,
    /// Hierarchy level: 1 = top-level, 2 = subsection, 3 = subsubsection.
    pub level: u8,
    /// Clean heading text of every ancestor section, root-to-parent order.
    /// Headerless ancestors contribute no entry (same algorithm as
    /// `Chunker::build_context_prefix`, P2-2).
    pub breadcrumb: Vec<String>,
    /// This section's own body text, rendered via
    /// [`render_core::render_section_content`] (math-converted, `Plain`
    /// escape mode). Does **not** include figures/tables — those are
    /// interleaved in `full_generate`/Markdown/LLM-text, but here they are
    /// carried as their own structured fields below instead, so no content is
    /// silently dropped.
    pub text: String,
    /// (first_page, last_page) — zero-based page numbers covered.
    pub page_range: (u32, u32),
    /// Figures belonging directly to this section.
    pub figures: Vec<ContentFigure>,
    /// Tables belonging directly to this section.
    pub tables: Vec<ContentTable>,
    /// Child sections (subsections), recursively projected.
    pub children: Vec<ContentSection>,
}

/// Lightweight figure summary: identity, caption, and image filename — no
/// bounding boxes, pixel dimensions, or raw on-disk paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentFigure {
    /// Identifier string such as "Fig. 1" or "Figure 3".
    pub figure_id: String,
    /// Full caption text.
    pub caption: String,
    /// Image file name only (e.g. `"p000_img000.png"`), never the raw on-disk
    /// path (which may be absolute) — mirrors `render_core::write_figure`'s
    /// path handling.
    pub image_path: String,
    /// Zero-based page index where the figure appears.
    pub page: u32,
}

/// Lightweight table summary: identity, caption, and a single textual
/// representation — no per-cell grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentTable {
    /// Identifier string such as "Table 1" or "Tab. 2".
    pub table_id: String,
    /// Table caption text, if any.
    pub caption: Option<String>,
    /// The table's textual representation (Markdown/CSV/plain text, in
    /// whichever tier `TableRepresentation` resolved to), the same text a
    /// reader would see inline in Markdown/LLM-text output.
    pub text: String,
    /// Zero-based page index where the table appears.
    pub page: u32,
}

/// Project a full [`PaperDocument`] into a [`ContentDocument`].
///
/// `opts` controls math conversion (`opts.math_config`); `include_headers`,
/// `include_figures`, and `include_tables` are ignored for the purposes of
/// this projection's own body text (headers and figures/tables are always
/// emitted as their own structured fields instead of interleaved text) — the
/// caller is expected to pass an `opts` built for that (see
/// [`JsonGenerator::generate_content_only`](super::json::JsonGenerator::generate_content_only)).
pub(crate) fn project_content(doc: &PaperDocument, opts: &RenderOptions) -> ContentDocument {
    let sections = doc
        .sections
        .iter()
        .map(|s| project_section(s, &[], opts))
        .collect();

    ContentDocument {
        paper_id: doc.paper_id.clone(),
        title: doc.metadata.title.clone(),
        authors: doc.metadata.authors.clone(),
        doi: doc.metadata.doi.clone(),
        pages: doc.metadata.pages,
        sections,
    }
}

/// Build the [`RenderOptions`] `generate_content_only` uses to render each
/// section's body text: `Plain` escaping (LLM-consumed text, not Markdown),
/// no header line (headers are emitted as [`ContentHeader`] instead), and no
/// figure/table interleaving (they are emitted as [`ContentFigure`]/
/// [`ContentTable`] instead) — only math conversion is controlled by the
/// caller-supplied `math_config`.
pub(crate) fn content_render_options(math_config: Option<&MathConfig>) -> RenderOptions<'_> {
    RenderOptions {
        math_config,
        escape: EscapeMode::Plain,
        include_headers: false,
        include_figures: false,
        include_tables: false,
        figure_format: crate::config::FigureTextFormat::Placeholder,
        image_base: String::new(),
    }
}

/// Recursively project one [`Section`] and its children.
///
/// `ancestors` holds the clean heading text of every enclosing section
/// (root-to-parent order), already filtered of headerless (empty) entries —
/// the same breadcrumb algorithm `Chunker::build_context_prefix` uses (P2-2),
/// kept independent here since the chunker's helper formats a `[Context: ...]`
/// prefix string rather than a `Vec<String>` field.
fn project_section(
    section: &Section,
    ancestors: &[String],
    opts: &RenderOptions,
) -> ContentSection {
    let text = render_core::render_section_content(section, opts);

    let header = section.header.as_ref().map(|h| ContentHeader {
        text: h.text.clone(),
        clean_text: h.clean_text.clone(),
        numbering: h.numbering.clone(),
    });

    let breadcrumb: Vec<String> = ancestors
        .iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect();

    let figures = section.figures.iter().map(project_figure).collect();
    let tables = section.tables.iter().map(project_table).collect();

    let own = section.header_text();
    let mut child_ancestors = breadcrumb.clone();
    if !own.is_empty() {
        child_ancestors.push(own);
    }
    let children = section
        .children
        .iter()
        .map(|c| project_section(c, &child_ancestors, opts))
        .collect();

    ContentSection {
        header,
        level: section.level,
        breadcrumb,
        text,
        page_range: section.page_range,
        figures,
        tables,
        children,
    }
}

/// Project one [`FigureInfo`] to a [`ContentFigure`], reducing the image
/// reference to its filename (never the raw on-disk path, which may be
/// absolute) — same fallback as `render_core::write_figure` when the path has
/// no file name component.
fn project_figure(fig: &FigureInfo) -> ContentFigure {
    let image_path = fig
        .image
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| fig.image.path.display().to_string());

    ContentFigure {
        figure_id: fig.figure_id.clone(),
        caption: fig.caption_text.clone(),
        image_path,
        page: fig.image.page,
    }
}

/// Project one [`TableInfo`] to a [`ContentTable`], flattening whichever
/// [`TableRepresentation`] tier it resolved to into a single `text` field.
fn project_table(table: &TableInfo) -> ContentTable {
    let text = match &table.representation {
        TableRepresentation::Markdown { markdown_text, .. } => markdown_text.clone(),
        TableRepresentation::Csv { csv_text, .. } => csv_text.clone(),
        TableRepresentation::PlainText { text, .. } => text.clone(),
    };

    ContentTable {
        table_id: table.table_id.clone(),
        caption: table.caption.clone(),
        text,
        page: table.page,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MathConfig, MathRepresentationPreference};
    use crate::types::{
        BlockType, DocumentMetadata, ImageFormat, ImageInfo, InsertionPoint, Rect, SectionHeader,
        TextBlock, TextLine, TextSpan,
    };
    use std::path::PathBuf;

    fn make_body_block(text: &str) -> TextBlock {
        TextBlock {
            global_index: 0,
            lines: vec![],
            text: text.to_string(),
            bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
            page: 0,
            column_index: 0,
            block_type: BlockType::BodyText,
        }
    }

    fn make_math_block(math_text: &str, font_name: &str) -> TextBlock {
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

    fn make_section(header: &str, level: u8, text: &str) -> Section {
        Section {
            header: Some(SectionHeader {
                text: format!("{header} raw"),
                clean_text: header.to_string(),
                level,
                numbering: Some("1.".to_string()),
                page: 0,
                bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
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

    fn make_doc(sections: Vec<Section>) -> PaperDocument {
        PaperDocument {
            paper_id: "paper1".to_string(),
            source_file: PathBuf::from("paper1.pdf"),
            metadata: DocumentMetadata {
                title: Some("A Great Paper".to_string()),
                authors: vec!["A. Author".to_string()],
                doi: Some("10.1/xyz".to_string()),
                pages: 3,
            },
            sections,
            all_figures: vec![],
            all_tables: vec![],
        }
    }

    #[test]
    fn project_content_carries_document_metadata() {
        let doc = make_doc(vec![make_section("INTRO", 1, "Intro body.")]);
        let opts = content_render_options(None);
        let content = project_content(&doc, &opts);

        assert_eq!(content.paper_id, "paper1");
        assert_eq!(content.title.as_deref(), Some("A Great Paper"));
        assert_eq!(content.authors, vec!["A. Author".to_string()]);
        assert_eq!(content.doi.as_deref(), Some("10.1/xyz"));
        assert_eq!(content.pages, 3);
    }

    #[test]
    fn project_content_includes_header_and_body_text() {
        let doc = make_doc(vec![make_section("INTRODUCTION", 1, "Intro body text.")]);
        let opts = content_render_options(None);
        let content = project_content(&doc, &opts);

        assert_eq!(content.sections.len(), 1);
        let header = content.sections[0].header.as_ref().unwrap();
        assert_eq!(header.clean_text, "INTRODUCTION");
        assert_eq!(header.text, "INTRODUCTION raw");
        assert_eq!(header.numbering.as_deref(), Some("1."));
        assert!(content.sections[0].text.contains("Intro body text."));
    }

    #[test]
    fn project_content_builds_breadcrumb_for_nested_sections() {
        let mut parent = make_section("METHODS", 1, "Parent body.");
        parent
            .children
            .push(make_section("Data Collection", 2, "Child body."));
        let doc = make_doc(vec![parent]);

        let opts = content_render_options(None);
        let content = project_content(&doc, &opts);

        assert!(content.sections[0].breadcrumb.is_empty());
        let child = &content.sections[0].children[0];
        assert_eq!(child.breadcrumb, vec!["METHODS".to_string()]);
    }

    #[test]
    fn project_content_headerless_section_has_empty_breadcrumb_entry() {
        let mut section = make_section("SEC", 1, "Body.");
        section.header = None;
        let doc = make_doc(vec![section]);

        let opts = content_render_options(None);
        let content = project_content(&doc, &opts);

        assert!(content.sections[0].header.is_none());
        assert!(content.sections[0].breadcrumb.is_empty());
    }

    #[test]
    fn project_content_converts_math_when_config_supplied() {
        let mut section = make_section("SEC", 1, "unused");
        section.blocks = vec![make_math_block("α", "CMMI10")];
        let doc = make_doc(vec![section]);

        let math_config = MathConfig {
            representation: MathRepresentationPreference::LaTeX,
            ..MathConfig::default()
        };
        let opts = content_render_options(Some(&math_config));
        let content = project_content(&doc, &opts);

        assert!(
            content.sections[0].text.contains("\\alpha"),
            "expected converted math in: {}",
            content.sections[0].text
        );
    }

    #[test]
    fn project_content_no_math_config_leaves_raw_glyph() {
        let mut section = make_section("SEC", 1, "unused");
        section.blocks = vec![make_math_block("α", "CMMI10")];
        let doc = make_doc(vec![section]);

        let opts = content_render_options(None);
        let content = project_content(&doc, &opts);

        assert!(content.sections[0].text.contains('α'));
    }

    #[test]
    fn project_content_includes_figures_and_tables() {
        use crate::types::{TableInfo, TableRepresentation};

        let mut section = make_section("SEC", 1, "Body.");
        section.figures.push(FigureInfo {
            figure_id: "Fig. 1".to_string(),
            figure_number: Some(1),
            caption_text: "Fig. 1: A diagram.".to_string(),
            image: ImageInfo {
                path: PathBuf::from("/abs/images/p000_img000.png"),
                page: 2,
                raw_bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                normalized_bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                width_px: 10,
                height_px: 10,
                format: ImageFormat::Png,
            },
            context_text: String::new(),
            insertion_point: InsertionPoint {
                page: 2,
                after_block_index: Some(0),
                y_position: 0.0,
            },
        });
        section.tables.push(TableInfo {
            table_id: "Table 1".to_string(),
            table_number: Some(1),
            caption: Some("Table 1: Results.".to_string()),
            representation: TableRepresentation::Markdown {
                header: vec!["A".to_string()],
                rows: vec![vec!["1".to_string()]],
                caption: None,
                markdown_text: "| A |\n| --- |\n| 1 |\n".to_string(),
                header_rows: vec![],
            },
            insertion_point: InsertionPoint {
                page: 1,
                after_block_index: None,
                y_position: 0.0,
            },
            page: 1,
        });
        let doc = make_doc(vec![section]);

        let opts = content_render_options(None);
        let content = project_content(&doc, &opts);

        let figures = &content.sections[0].figures;
        assert_eq!(figures.len(), 1);
        assert_eq!(figures[0].figure_id, "Fig. 1");
        assert_eq!(figures[0].caption, "Fig. 1: A diagram.");
        assert_eq!(figures[0].image_path, "p000_img000.png");
        assert!(
            !figures[0].image_path.contains('/'),
            "must be basename only"
        );
        assert_eq!(figures[0].page, 2);

        let tables = &content.sections[0].tables;
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].table_id, "Table 1");
        assert_eq!(tables[0].caption.as_deref(), Some("Table 1: Results."));
        assert!(tables[0].text.contains("| --- |"));
        assert_eq!(tables[0].page, 1);
    }

    #[test]
    fn content_document_round_trips_through_serde() {
        let doc = make_doc(vec![make_section("INTRO", 1, "Intro body text.")]);
        let opts = content_render_options(None);
        let content = project_content(&doc, &opts);

        let json = serde_json::to_string(&content).expect("serialize");
        let parsed: ContentDocument = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(parsed.paper_id, content.paper_id);
        assert_eq!(parsed.sections.len(), content.sections.len());
        assert_eq!(parsed.sections[0].text, content.sections[0].text);
    }
}
