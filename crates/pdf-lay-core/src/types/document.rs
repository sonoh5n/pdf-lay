//! Top-level document types: images, figures, tables, the paper document itself.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::geometry::Rect;

/// A single image extracted from the PDF.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageInfo {
    /// Path where the image was saved on disk.
    pub path: PathBuf,
    /// Zero-based page index where the image appears.
    pub page: u32,
    /// Bounding box as returned by pdf_oxide (may be in a different scale).
    pub raw_bbox: Rect,
    /// Bounding box normalized to the same coordinate space as text spans.
    pub normalized_bbox: Rect,
    /// Image width in pixels.
    pub width_px: u32,
    /// Image height in pixels.
    pub height_px: u32,
    /// File format of the saved image.
    pub format: ImageFormat,
}

/// Supported image formats for extracted images.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImageFormat {
    /// PNG format.
    Png,
    /// JPEG format.
    Jpeg,
    /// Any other format (stores the format name string).
    Other(String),
}

/// A figure (image + caption + metadata).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FigureInfo {
    /// Identifier string such as "Fig. 1" or "Figure 3".
    pub figure_id: String,
    /// Numeric figure number if present.
    pub figure_number: Option<u32>,
    /// Full caption text.
    pub caption_text: String,
    /// The extracted image associated with this figure.
    pub image: ImageInfo,
    /// Surrounding body text (~500 chars) for context.
    pub context_text: String,
    /// Where in the output stream this figure should be inserted.
    pub insertion_point: InsertionPoint,
}

/// Where in the output stream a figure or table should be inserted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsertionPoint {
    /// Zero-based page index.
    pub page: u32,
    /// Insert after this `TextBlock::global_index`, or at top if `None`.
    pub after_block_index: Option<usize>,
    /// Y-coordinate of the insertion position in PDF coordinates.
    pub y_position: f64,
}

/// A table with its representation (Markdown, CSV, or plain text).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableInfo {
    /// Identifier string such as "Table 1" or "Tab. 2".
    pub table_id: String,
    /// Numeric table number if present.
    pub table_number: Option<u32>,
    /// Table caption text, if any.
    pub caption: Option<String>,
    /// The textual representation of the table contents.
    pub representation: TableRepresentation,
    /// Where in the output stream this table should be inserted.
    pub insertion_point: InsertionPoint,
    /// Zero-based page index where the table appears.
    pub page: u32,
}

/// The textual representation of a table (three quality tiers).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TableRepresentation {
    /// Full Markdown table (best, line-based detection succeeded).
    Markdown {
        /// Header cells.
        header: Vec<String>,
        /// Data rows, each a vector of cell strings.
        rows: Vec<Vec<String>>,
        /// Table caption, if any.
        caption: Option<String>,
        /// The complete Markdown table string.
        markdown_text: String,
    },
    /// CSV-style representation (text-alignment detection).
    Csv {
        /// Header cells.
        header: Vec<String>,
        /// Data rows, each a vector of cell strings.
        rows: Vec<Vec<String>>,
        /// Table caption, if any.
        caption: Option<String>,
        /// The complete CSV string.
        csv_text: String,
    },
    /// Plain text fallback (always works, lowest fidelity).
    PlainText {
        /// Raw text content of the table area.
        text: String,
        /// Table caption, if any.
        caption: Option<String>,
    },
}

/// Document metadata extracted from the PDF.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DocumentMetadata {
    /// Document title, if extractable.
    pub title: Option<String>,
    /// List of author names.
    pub authors: Vec<String>,
    /// DOI string, if present.
    pub doi: Option<String>,
    /// Total number of pages.
    pub pages: u32,
}

/// The top-level result of analyzing a single PDF.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperDocument {
    /// Unique identifier for this paper (e.g., filename stem or DOI-derived).
    pub paper_id: String,
    /// Path to the source PDF file.
    pub source_file: PathBuf,
    /// Extracted document metadata.
    pub metadata: DocumentMetadata,
    /// Hierarchical sections (top-level only; children are nested inside each `Section`).
    pub sections: Vec<super::text::Section>,
    /// All figures (flat list — also accessible through `Section::figures`).
    pub all_figures: Vec<FigureInfo>,
    /// All tables (flat list).
    pub all_tables: Vec<TableInfo>,
}

impl FigureInfo {
    /// Returns the description portion of the caption, stripping the "Fig. N:" prefix.
    pub fn caption_description(&self) -> &str {
        let text = self.caption_text.trim();
        if let Some(colon_pos) = text.find(':') {
            text[colon_pos + 1..].trim()
        } else if let Some(dot_pos) = text.find('.') {
            // Handle "Fig. 1 Description" (no colon).
            let after_dot = text[dot_pos + 1..].trim();
            if after_dot.starts_with(|c: char| c.is_ascii_digit()) {
                // Second number after dot, skip to next space.
                after_dot
                    .find(' ')
                    .map(|i| after_dot[i..].trim())
                    .unwrap_or(text)
            } else {
                after_dot
            }
        } else {
            text
        }
    }
}

impl PaperDocument {
    /// Estimated total text size in bytes (used for `String::with_capacity`).
    pub fn estimated_text_size(&self) -> usize {
        self.sections
            .iter()
            .map(|s| s.full_text().len())
            .sum::<usize>()
            + 1024
    }
}

/// A single chunk suitable for an LLM context window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    /// Sequential chunk index within this document.
    pub chunk_id: usize,
    /// Paper identifier matching `PaperDocument::paper_id`.
    pub paper_id: String,
    /// Header text of the containing section.
    pub section: String,
    /// (first_page, last_page) — zero-based page numbers covered by this chunk.
    pub page_range: (u32, u32),
    /// The text content of this chunk.
    pub text: String,
    /// Figures whose insertion points fall within this chunk.
    pub figures: Vec<FigureInfo>,
    /// Tables whose insertion points fall within this chunk.
    pub tables: Vec<TableInfo>,
    /// Estimated token count (approximation: len / 4 for English text).
    pub estimated_tokens: usize,
    /// True if this chunk continues in the next chunk.
    pub has_continuation: bool,
}
