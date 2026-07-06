//! The single "block → rich text" rendering layer shared by all LLM-facing
//! outputs (Markdown, LLM text, chunk).
//!
//! Before this module existed, `output/markdown.rs` and `selector/llm_text.rs`
//! each carried a near-identical private function that walked a
//! [`TextBlock`]'s lines, substituted detected math spans with formatted math
//! notation, and left everything else untouched or HTML/Markdown-escaped. The
//! chunker did not go through either path at all — it used
//! [`Section::full_text`](crate::types::Section::full_text), a raw
//! concatenation of `block.text` with no math conversion, no table markdown,
//! and no figure placeholders.
//!
//! `render_core` collapses the two near-duplicate implementations into one,
//! parameterized by [`EscapeMode`] (the only real difference between the
//! Markdown and LLM-text variants), and exposes a section-level entry point,
//! [`render_section_content`], so the chunker can produce the same
//! high-fidelity text as markdown/llm_text instead of raw block text.

use std::collections::VecDeque;

use crate::config::{FigureTextFormat, MathConfig};
use crate::math::{MathContext, MathConverter, MathDetector, MathFormatter};
use crate::types::{BlockType, FigureInfo, Section, TableInfo, TableRepresentation, TextBlock};

/// How non-math text is sanitized when rendered.
///
/// Markdown output must neutralize HTML tags and Markdown link injection
/// coming from PDF-derived text (see [`escape_for_markdown_text`]). LLM text
/// and chunk output are consumed as plain text by a model, not rendered as
/// Markdown/HTML, so no escaping is applied there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EscapeMode {
    /// Escape HTML tags (`<`/`>`) and Markdown link injection (`](`).
    Markdown,
    /// Leave text unmodified.
    Plain,
}

/// Rendering options supplied by each output (markdown / llm_text / chunk)
/// when converting a [`Section`] to rich text via [`render_section_content`].
///
/// Not `serde`-derived: this is an execution-time view assembled from each
/// output's own `*Config` (e.g. `ChunkConfig`, `MarkdownConfig`), not a
/// persisted configuration struct itself.
pub(crate) struct RenderOptions<'a> {
    /// Math configuration to use for detection/conversion. `None` disables
    /// math conversion entirely (blocks are emitted as raw/escaped text).
    pub math_config: Option<&'a MathConfig>,
    /// Escaping policy applied to non-math text.
    pub escape: EscapeMode,
    /// Whether to emit the section's own heading line before its body.
    pub include_headers: bool,
    /// Whether to interleave figures at their `insertion_point`.
    pub include_figures: bool,
    /// Whether to interleave tables at their `insertion_point`.
    pub include_tables: bool,
    /// How figures are rendered when `include_figures` is true.
    pub figure_format: FigureTextFormat,
    /// Base path prepended to figure image filenames (empty = filename only).
    pub image_base: String,
}

/// Convert a single block's text to rich text, substituting contiguous math
/// spans with formatted math notation.
///
/// This is the single implementation replacing the formerly-duplicated
/// `markdown::convert_block_text_with_math` and
/// `llm_text::convert_block_text_for_llm`. When `detector`/`converter`/
/// `math_config` are all `Some`, math regions are detected per line and
/// converted via `converter`, then formatted with
/// [`MathFormatter::format_for_markdown`] or [`MathFormatter::format_for_llm`]
/// depending on `escape`. Non-math text is sanitized according to `escape`.
///
/// When math conversion is disabled (any of the three is `None`), the block's
/// raw `text` is returned, sanitized according to `escape` — matching the
/// pre-render-core fallback behavior of both call sites.
pub(crate) fn render_block(
    block: &TextBlock,
    detector: Option<&MathDetector>,
    converter: Option<&MathConverter>,
    math_config: Option<&MathConfig>,
    escape: EscapeMode,
) -> String {
    match (detector, converter, math_config) {
        (Some(detector), Some(converter), Some(config)) => {
            render_block_with_math(block, detector, converter, config, escape)
        }
        _ => sanitize(&block.text, escape),
    }
}

/// Core math-aware rendering, shared by both escape modes.
fn render_block_with_math(
    block: &TextBlock,
    detector: &MathDetector,
    converter: &MathConverter,
    config: &MathConfig,
    escape: EscapeMode,
) -> String {
    let mut result = String::new();

    for (line_idx, line) in block.lines.iter().enumerate() {
        if line_idx > 0 {
            result.push('\n');
        }

        let math_regions = detector.detect_in_line(line);

        if math_regions.is_empty() {
            // No math in this line — concatenate span texts, sanitized.
            let line_text: String = line.spans.iter().map(|s| s.text.as_str()).collect();
            result.push_str(&sanitize(&line_text, escape));
        } else {
            // Rebuild the line, substituting math regions with formatted math.
            let mut span_idx = 0usize;

            for region in &math_regions {
                // Output non-math spans that precede this region, sanitized.
                while span_idx < line.spans.len() {
                    let span = &line.spans[span_idx];
                    let is_region_start = region
                        .spans
                        .first()
                        .is_some_and(|rs| rs.bbox.left == span.bbox.left && rs.text == span.text);
                    if is_region_start {
                        break;
                    }
                    result.push_str(&sanitize(&span.text, escape));
                    span_idx += 1;
                }

                // Convert and format the math region (NOT sanitized — math
                // content must preserve LaTeX operators like < > &).
                let converted = converter.convert(&region.text, &region.spans);
                let is_display = region.context == MathContext::Display;
                let formatted = match escape {
                    EscapeMode::Markdown => MathFormatter::format_for_markdown(
                        &converted,
                        is_display,
                        region.equation_number.as_deref(),
                        config,
                    ),
                    EscapeMode::Plain => MathFormatter::format_for_llm(
                        &converted,
                        is_display,
                        region.equation_number.as_deref(),
                        config,
                    ),
                };
                result.push_str(&formatted);

                span_idx += region.spans.len();
            }

            // Output any remaining non-math spans after the last region.
            while span_idx < line.spans.len() {
                result.push_str(&sanitize(&line.spans[span_idx].text, escape));
                span_idx += 1;
            }
        }
    }

    // Fall back to block.text if lines are empty (defensive).
    if result.is_empty() && !block.text.is_empty() {
        return sanitize(&block.text, escape);
    }

    result
}

/// Sanitize a text fragment according to the given [`EscapeMode`].
fn sanitize(s: &str, escape: EscapeMode) -> String {
    match escape {
        EscapeMode::Markdown => escape_for_markdown_text(s),
        EscapeMode::Plain => s.to_string(),
    }
}

/// Escape a string for safe inclusion in Markdown body text or headings.
///
/// Neutralizes HTML tags (`<` / `>`) and Markdown link injection (`](`).
///
/// Shared by `output::markdown` and `render_core` itself; moved here from
/// `output::markdown` so both the Markdown and future render-core callers use
/// the exact same sanitization.
pub(crate) fn escape_for_markdown_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace("](", "]\\(")
}

/// Render a section's *own* body (blocks, figures, tables) to rich text.
///
/// Does **not** recurse into `section.children` — callers control recursion
/// (the chunker walks the section tree per-chunk; `markdown`/`llm_text`
/// recurse inside their own `write_section`).
///
/// Blocks classified as [`BlockType::Caption`], [`BlockType::PageNumber`],
/// [`BlockType::RunningHeader`], or [`BlockType::RunningFooter`] contribute no
/// body text — this mirrors the drop rule already applied by
/// `Section::full_text`, `markdown::write_section`, and
/// `llm_text::write_section`. No new drop path is introduced: every block
/// skipped here was already excluded from body text in all three outputs, and
/// any figure/table anchored to a skipped block is still emitted via the
/// insertion-point interleave below.
pub(crate) fn render_section_content(section: &Section, opts: &RenderOptions) -> String {
    let mut out = String::new();

    if opts.include_headers
        && let Some(header) = &section.header
    {
        out.push_str(&sanitize(&header.clean_text, opts.escape));
        out.push_str("\n\n");
    }

    let math_components = opts.math_config.map(|mc| {
        (
            MathDetector::new(mc.clone()),
            MathConverter::new(mc.clone()),
        )
    });

    let mut figure_queue: VecDeque<&FigureInfo> = section.figures.iter().collect();
    let mut table_queue: VecDeque<&TableInfo> = section.tables.iter().collect();

    for block in &section.blocks {
        let emit_body = !matches!(
            block.block_type,
            BlockType::Caption
                | BlockType::PageNumber
                | BlockType::RunningHeader
                | BlockType::RunningFooter
        );

        if emit_body {
            let (detector, converter) = match &math_components {
                Some((d, c)) => (Some(d), Some(c)),
                None => (None, None),
            };
            let text = render_block(block, detector, converter, opts.math_config, opts.escape);
            out.push_str(&text);
            out.push_str("\n\n");
        }

        if opts.include_figures {
            while let Some(fig) = figure_queue.front() {
                if fig.insertion_point.after_block_index == Some(block.global_index) {
                    write_figure(&mut out, fig, opts);
                    figure_queue.pop_front();
                } else {
                    break;
                }
            }
        }

        if opts.include_tables {
            while let Some(table) = table_queue.front() {
                if table.insertion_point.after_block_index == Some(block.global_index) {
                    write_table(&mut out, table);
                    table_queue.pop_front();
                } else {
                    break;
                }
            }
        }
    }

    // Flush remaining figures/tables whose insertion point matched no block
    // (or matched a block anchor after the last block was processed).
    if opts.include_figures {
        while let Some(fig) = figure_queue.pop_front() {
            write_figure(&mut out, fig, opts);
        }
    }
    if opts.include_tables {
        while let Some(table) = table_queue.pop_front() {
            write_table(&mut out, table);
        }
    }

    out.trim_end().to_string()
}

/// Render a figure placeholder/link per `opts.figure_format`, using
/// `opts.image_base` + the image filename (never the raw on-disk path).
///
/// A vector figure (`fig.image.path.is_none()` — a caption matched to a
/// cluster of vector-graphic paths rather than a raster image, see
/// [`crate::figure::VectorFigureClusterer`]) has no file to link to. Rather
/// than fabricate a path, every format except `Omit` falls back to stating
/// plainly that no image was extracted (No Silent Drop: the figure is still
/// reported, just honestly).
fn write_figure(out: &mut String, fig: &FigureInfo, opts: &RenderOptions) {
    if matches!(opts.figure_format, FigureTextFormat::Omit) {
        return;
    }

    let Some(filename) = fig.image.filename() else {
        out.push_str(&format!(
            "[{} (vector figure, no extracted image): {}]\n\n",
            fig.figure_id, fig.caption_text
        ));
        return;
    };
    let path = if opts.image_base.is_empty() {
        filename
    } else {
        format!("{}/{}", opts.image_base, filename)
    };

    match opts.figure_format {
        FigureTextFormat::Placeholder => {
            out.push_str(&format!("[IMAGE: {} {}]\n\n", fig.figure_id, path));
        }
        FigureTextFormat::MarkdownLink => {
            out.push_str(&format!("![{}]({})\n\n", fig.figure_id, path));
        }
        FigureTextFormat::CaptionOnly => {
            out.push_str(&format!("[{}]\n\n", fig.caption_text));
        }
        FigureTextFormat::Omit => {
            // Unreachable in practice (handled by the early return above);
            // kept as a no-op arm for exhaustiveness instead of panicking.
        }
    }
}

/// Render a table's textual representation (Markdown/CSV/plain text tier).
fn write_table(out: &mut String, table: &TableInfo) {
    if let Some(caption) = &table.caption {
        out.push_str(caption);
        out.push_str("\n\n");
    }
    match &table.representation {
        TableRepresentation::Markdown { markdown_text, .. } => out.push_str(markdown_text),
        TableRepresentation::Csv { csv_text, .. } => out.push_str(csv_text),
        TableRepresentation::PlainText { text, .. } => out.push_str(text),
    }
    out.push_str("\n\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MathConfig, MathRepresentationPreference};
    use crate::types::{
        ImageFormat, ImageInfo, InsertionPoint, Rect, SectionHeader, TextLine, TextSpan,
    };
    use std::path::PathBuf;

    fn make_plain_block(text: &str) -> TextBlock {
        let span = TextSpan {
            text: text.to_string(),
            font_name: "TimesNewRoman".to_string(),
            font_size: 10.0,
            is_bold: false,
            is_italic: false,
            bbox: Rect::new(50.0, 700.0, 95.0, 690.0),
            page: 0,
        };
        let line = TextLine {
            text: text.to_string(),
            spans: vec![span],
            bbox: Rect::new(50.0, 700.0, 95.0, 690.0),
            page: 0,
            baseline_y: 690.0,
            primary_font_size: 10.0,
            primary_font_name: "TimesNewRoman".to_string(),
            is_bold: false,
        };
        TextBlock {
            global_index: 0,
            lines: vec![line],
            text: text.to_string(),
            bbox: Rect::new(50.0, 700.0, 95.0, 690.0),
            page: 0,
            column_index: 0,
            block_type: BlockType::BodyText,
        }
    }

    /// A block whose single line is entirely a math-font span (Display context).
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

    #[test]
    fn render_block_converts_math_latex() {
        let config = MathConfig {
            representation: MathRepresentationPreference::LaTeX,
            ..MathConfig::default()
        };
        let detector = MathDetector::new(config.clone());
        let converter = MathConverter::new(config.clone());
        let block = make_math_block("α", "CMMI10");

        let out = render_block(
            &block,
            Some(&detector),
            Some(&converter),
            Some(&config),
            EscapeMode::Plain,
        );
        assert!(out.contains("\\alpha"), "expected \\alpha in: {out}");
    }

    #[test]
    fn render_block_plain_does_not_escape() {
        let block = make_plain_block("Hello <b>world</b>");
        let out = render_block(&block, None, None, None, EscapeMode::Plain);
        assert!(
            out.contains("<b>"),
            "plain mode must not escape HTML: {out}"
        );
    }

    #[test]
    fn render_block_markdown_escapes_html() {
        let block = make_plain_block("Hello <b>world</b>");
        let out = render_block(&block, None, None, None, EscapeMode::Markdown);
        assert!(
            out.contains("&lt;b&gt;"),
            "markdown mode must escape HTML: {out}"
        );
        assert!(!out.contains("<b>"));
    }

    fn section_with_two_blocks_and_figure() -> Section {
        let first = make_plain_block("BEFORE_MARKER");
        let mut second = make_plain_block("AFTER_MARKER");
        second.global_index = 1;

        Section {
            header: None,
            level: 1,
            blocks: vec![first, second],
            figures: vec![FigureInfo {
                figure_id: "Fig. 1".to_string(),
                figure_number: Some(1),
                caption_text: "Fig. 1: X.".to_string(),
                image: ImageInfo {
                    path: Some(PathBuf::from("images/p000_img000.png")),
                    page: 0,
                    raw_bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                    normalized_bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                    width_px: 10,
                    height_px: 10,
                    format: ImageFormat::Png,
                    bbox_known: true,
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
        }
    }

    #[test]
    fn render_section_interleaves_figure_at_insertion_point() {
        let section = section_with_two_blocks_and_figure();
        let opts = RenderOptions {
            math_config: None,
            escape: EscapeMode::Plain,
            include_headers: false,
            include_figures: true,
            include_tables: true,
            figure_format: FigureTextFormat::Placeholder,
            image_base: String::new(),
        };
        let out = render_section_content(&section, &opts);

        let before_pos = out.find("BEFORE_MARKER").expect("first block present");
        let image_pos = out.find("[IMAGE:").expect("figure placeholder present");
        let after_pos = out.find("AFTER_MARKER").expect("second block present");
        assert!(
            before_pos < image_pos && image_pos < after_pos,
            "figure anchored after block 0 must appear between the two blocks:\n{out}"
        );
    }

    #[test]
    fn render_section_includes_header_when_enabled() {
        let mut section = section_with_two_blocks_and_figure();
        section.header = Some(SectionHeader {
            text: "1. INTRO".to_string(),
            clean_text: "INTRO".to_string(),
            level: 1,
            numbering: Some("1.".to_string()),
            page: 0,
            bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
            block_index: 0,
        });
        let opts = RenderOptions {
            math_config: None,
            escape: EscapeMode::Plain,
            include_headers: true,
            include_figures: false,
            include_tables: false,
            figure_format: FigureTextFormat::Placeholder,
            image_base: String::new(),
        };
        let out = render_section_content(&section, &opts);
        assert!(out.starts_with("INTRO"), "expected header first:\n{out}");
    }

    #[test]
    fn escape_markdown_text_neutralizes_html() {
        assert_eq!(
            escape_for_markdown_text("<script>alert(1)</script>"),
            "&lt;script&gt;alert(1)&lt;/script&gt;"
        );
    }
}
