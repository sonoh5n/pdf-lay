# Task 02: Common Type Definitions

## Overview

Define all shared data types used across the entire pipeline. This task creates the `types/`,
`error.rs`, and `config.rs` modules in `pdf-lay-core`. Every subsequent task depends on
these types, so they must be complete and correct before parallel work can begin.

This is the most critical task for correctness — the coordinate system (PDF default: lower-left
origin, Y-up, points), the `Rect` invariant (`top > bottom`), and the `FontInfo` heuristics
establish the contract all other modules rely on.

## Related Information
- **Plan**: `/Users/sonokawa/.claude/plans/fuzzy-drifting-summit.md` (Task 2)
- **Design doc**: `docs/arch/02_DESIGN.md` § 2.1 types
- **Overview**: `docs/develop/overview.md`
- **Dependencies**: Task 01 must be completed first

## Files to Create

- [ ] `crates/pdf-lay-core/src/types/mod.rs`
- [ ] `crates/pdf-lay-core/src/types/geometry.rs`
- [ ] `crates/pdf-lay-core/src/types/text.rs`
- [ ] `crates/pdf-lay-core/src/types/document.rs`
- [ ] `crates/pdf-lay-core/src/types/path.rs`
- [ ] `crates/pdf-lay-core/src/error.rs`
- [ ] `crates/pdf-lay-core/src/config.rs`

## Files to Modify

- [ ] `crates/pdf-lay-core/src/lib.rs` — add module declarations

## Implementation Steps

### Step 1: `types/geometry.rs`

```rust
//! Geometric primitives used throughout the pipeline.
//!
//! **Coordinate system**: PDF default — origin at lower-left, Y-axis pointing up, unit = points.
//! Invariant: `Rect::top > Rect::bottom` always holds.

use serde::{Deserialize, Serialize};

/// Bounding box in PDF coordinate space (lower-left origin, Y-up).
///
/// Invariant: `top > bottom`, `right > left`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub left: f64,
    /// Y-coordinate of the upper edge (larger Y value).
    pub top: f64,
    pub right: f64,
    /// Y-coordinate of the lower edge (smaller Y value).
    pub bottom: f64,
}

impl Rect {
    /// Create a new `Rect`. Panics in debug if invariant is violated.
    pub fn new(left: f64, top: f64, right: f64, bottom: f64) -> Self {
        debug_assert!(top >= bottom, "Rect: top ({top}) must be >= bottom ({bottom})");
        debug_assert!(right >= left, "Rect: right ({right}) must be >= left ({left})");
        Self { left, top, right, bottom }
    }

    /// Horizontal span in points.
    pub fn width(&self) -> f64 {
        self.right - self.left
    }

    /// Vertical span in points.
    pub fn height(&self) -> f64 {
        self.top - self.bottom
    }

    /// Horizontal center.
    pub fn center_x(&self) -> f64 {
        (self.left + self.right) / 2.0
    }

    /// Vertical center.
    pub fn center_y(&self) -> f64 {
        (self.top + self.bottom) / 2.0
    }

    /// Vertical gap between `self` and `other`.
    ///
    /// Positive when there is a gap (self is above other or vice versa).
    /// Zero or negative when the rects overlap vertically.
    pub fn vertical_gap(&self, other: &Rect) -> f64 {
        if self.bottom > other.top {
            // self is entirely above other
            self.bottom - other.top
        } else if other.bottom > self.top {
            // other is entirely above self
            other.bottom - self.top
        } else {
            // they overlap — gap is 0 (or negative to indicate overlap amount)
            let overlap = self.top.min(other.top) - self.bottom.max(other.bottom);
            -overlap
        }
    }

    /// Smallest bounding box containing both `self` and `other`.
    pub fn union(&self, other: &Rect) -> Rect {
        Rect {
            left: self.left.min(other.left),
            top: self.top.max(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.min(other.bottom),
        }
    }

    /// Returns true if this rect overlaps with `other` in both X and Y.
    pub fn overlaps(&self, other: &Rect) -> bool {
        self.left < other.right
            && self.right > other.left
            && self.bottom < other.top
            && self.top > other.bottom
    }
}

/// Page dimensions in points.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageDimensions {
    pub page_number: u32,
    /// Page width in points (e.g. 612 for US Letter).
    pub width: f64,
    /// Page height in points (e.g. 792 for US Letter).
    pub height: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_width_height() {
        let r = Rect::new(10.0, 30.0, 50.0, 10.0);
        assert_eq!(r.width(), 40.0);
        assert_eq!(r.height(), 20.0);
    }

    #[test]
    fn rect_center() {
        let r = Rect::new(0.0, 20.0, 20.0, 0.0);
        assert_eq!(r.center_x(), 10.0);
        assert_eq!(r.center_y(), 10.0);
    }

    #[test]
    fn rect_vertical_gap_above() {
        // r1 is above r2 with a 5pt gap
        let r1 = Rect::new(0.0, 30.0, 10.0, 20.0);
        let r2 = Rect::new(0.0, 15.0, 10.0, 0.0);
        assert_eq!(r1.vertical_gap(&r2), 5.0);
    }

    #[test]
    fn rect_vertical_gap_overlap() {
        let r1 = Rect::new(0.0, 20.0, 10.0, 10.0);
        let r2 = Rect::new(0.0, 15.0, 10.0, 5.0);
        assert!(r1.vertical_gap(&r2) < 0.0);
    }

    #[test]
    fn rect_union() {
        let r1 = Rect::new(0.0, 20.0, 10.0, 10.0);
        let r2 = Rect::new(5.0, 30.0, 20.0, 5.0);
        let u = r1.union(&r2);
        assert_eq!(u, Rect::new(0.0, 30.0, 20.0, 5.0));
    }
}
```

### Step 2: `types/text.rs`

```rust
//! Text-related types: font information, spans, lines, blocks, sections.

use serde::{Deserialize, Serialize};
use super::geometry::Rect;

/// Font metadata for a text span.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontInfo {
    pub name: String,
    pub size: f64,
    pub is_bold: bool,
    pub is_italic: bool,
}

impl FontInfo {
    /// Heuristic bold detection from font name string.
    ///
    /// Checks for substrings: "bold", "-bd", "heavy", "black" (case-insensitive).
    pub fn detect_bold(font_name: &str) -> bool {
        let lower = font_name.to_lowercase();
        lower.contains("bold")
            || lower.contains("-bd")
            || lower.contains("heavy")
            || lower.contains("black")
    }

    /// Heuristic italic detection from font name string.
    ///
    /// Checks for substrings: "italic", "oblique", "-it" (case-insensitive).
    pub fn detect_italic(font_name: &str) -> bool {
        let lower = font_name.to_lowercase();
        lower.contains("italic") || lower.contains("oblique") || lower.contains("-it")
    }
}

/// Minimum text unit extracted from PDF — a run of characters sharing the same font.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextSpan {
    pub text: String,
    pub font_name: String,
    pub font_size: f64,
    pub is_bold: bool,
    pub is_italic: bool,
    pub bbox: Rect,
    /// Zero-based page index.
    pub page: u32,
}

/// A logical line of text reconstructed from multiple `TextSpan`s.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextLine {
    pub spans: Vec<TextSpan>,
    /// Joined text with inter-span spaces inserted where appropriate.
    pub text: String,
    pub bbox: Rect,
    /// Zero-based page index.
    pub page: u32,
    /// Y-coordinate of the baseline (typically `bbox.bottom`).
    pub baseline_y: f64,
    /// Font size of the dominant span(s) in this line.
    pub primary_font_size: f64,
    /// Font name of the dominant span.
    pub primary_font_name: String,
    /// True if the majority of characters are bold.
    pub is_bold: bool,
}

/// A logical paragraph or block of text (one or more lines).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextBlock {
    /// Sequential index assigned during block grouping (used for cross-referencing).
    pub global_index: usize,
    pub lines: Vec<TextLine>,
    pub text: String,
    pub bbox: Rect,
    /// Zero-based page index of the first line.
    pub page: u32,
    /// Which column this block belongs to (0 = left/only column).
    pub column_index: usize,
    pub block_type: BlockType,
}

impl TextBlock {
    /// Returns the primary font size of this block (from first line, or 0.0 if empty).
    pub fn primary_font_size(&self) -> f64 {
        self.lines.first().map(|l| l.primary_font_size).unwrap_or(0.0)
    }

    /// Returns true if this block is predominantly bold.
    pub fn is_bold(&self) -> bool {
        self.lines.first().map(|l| l.is_bold).unwrap_or(false)
    }
}

/// Semantic classification of a `TextBlock`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BlockType {
    Title,
    Abstract,
    SectionHeader,
    SubsectionHeader,
    #[default]
    BodyText,
    Caption,
    ListItem,
    Equation,
    Footnote,
    PageNumber,
    RunningHeader,
    RunningFooter,
    Reference,
    Unknown,
}

/// Extracted section with its content and child sub-sections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    pub header: Option<SectionHeader>,
    pub level: u8,
    pub blocks: Vec<TextBlock>,
    pub figures: Vec<super::document::FigureInfo>,
    pub tables: Vec<super::document::TableInfo>,
    pub children: Vec<Section>,
    /// (first_page, last_page) — zero-based page numbers.
    pub page_range: (u32, u32),
}

impl Section {
    /// Returns the clean header text, or an empty string for headerless sections.
    pub fn header_text(&self) -> String {
        self.header.as_ref().map(|h| h.clean_text.clone()).unwrap_or_default()
    }

    /// Concatenates all body text in this section (excluding captions, page numbers, etc.).
    pub fn full_text(&self) -> String {
        self.blocks
            .iter()
            .filter(|b| {
                !matches!(
                    b.block_type,
                    BlockType::Caption
                        | BlockType::PageNumber
                        | BlockType::RunningHeader
                        | BlockType::RunningFooter
                )
            })
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// A detected section header with level and numbering information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionHeader {
    /// Raw text as it appears in the PDF (e.g. "II. KNOWLEDGE GRAPHS").
    pub text: String,
    /// Header text with numbering removed (e.g. "KNOWLEDGE GRAPHS").
    pub clean_text: String,
    /// Hierarchy level: 1 = top-level, 2 = subsection, 3 = subsubsection.
    pub level: u8,
    /// The number prefix if present (e.g. "II.", "3.1").
    pub numbering: Option<String>,
    /// Page where this header appears.
    pub page: u32,
    pub bbox: Rect,
    /// Index into the flat `TextBlock` array.
    pub block_index: usize,
}
```

### Step 3: `types/document.rs`

```rust
//! Top-level document types: images, figures, tables, the paper document itself.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use super::geometry::Rect;

/// A single image extracted from the PDF.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageInfo {
    /// Path where the image was saved on disk.
    pub path: PathBuf,
    pub page: u32,
    /// Bounding box as returned by pdf_oxide (may be in a different scale).
    pub raw_bbox: Rect,
    /// Bounding box normalized to the same coordinate space as text spans.
    pub normalized_bbox: Rect,
    pub width_px: u32,
    pub height_px: u32,
    pub format: ImageFormat,
}

/// Supported image formats for extracted images.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Other(String),
}

/// A figure (image + caption + metadata).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FigureInfo {
    /// Identifier string such as "Fig. 1" or "Figure 3".
    pub figure_id: String,
    pub figure_number: Option<u32>,
    pub caption_text: String,
    pub image: ImageInfo,
    /// Surrounding body text (~500 chars) for context.
    pub context_text: String,
    pub insertion_point: InsertionPoint,
}

/// Where in the output stream a figure or table should be inserted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsertionPoint {
    pub page: u32,
    /// Insert after this `TextBlock::global_index`, or at top if `None`.
    pub after_block_index: Option<usize>,
    pub y_position: f64,
}

/// A table with its representation (Markdown, CSV, or plain text).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableInfo {
    pub table_id: String,
    pub table_number: Option<u32>,
    pub caption: Option<String>,
    pub representation: TableRepresentation,
    pub insertion_point: InsertionPoint,
    pub page: u32,
}

/// The textual representation of a table (three quality tiers).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TableRepresentation {
    /// Full Markdown table (best, line-based detection succeeded).
    Markdown {
        header: Vec<String>,
        rows: Vec<Vec<String>>,
        caption: Option<String>,
        markdown_text: String,
    },
    /// CSV-style representation (text-alignment detection).
    Csv {
        header: Vec<String>,
        rows: Vec<Vec<String>>,
        caption: Option<String>,
        csv_text: String,
    },
    /// Plain text fallback (always works, lowest fidelity).
    PlainText {
        text: String,
        caption: Option<String>,
    },
}

/// Document metadata extracted from the PDF.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub doi: Option<String>,
    pub pages: u32,
}

/// The top-level result of analyzing a single PDF.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperDocument {
    pub paper_id: String,
    pub source_file: PathBuf,
    pub metadata: DocumentMetadata,
    pub sections: Vec<super::text::Section>,
    /// All figures (flat list — also accessible through `Section::figures`).
    pub all_figures: Vec<FigureInfo>,
    /// All tables (flat list).
    pub all_tables: Vec<TableInfo>,
}

impl PaperDocument {
    /// Estimated total text size in bytes (used for `String::with_capacity`).
    pub fn estimated_text_size(&self) -> usize {
        self.sections.iter().map(|s| s.full_text().len()).sum::<usize>() + 1024
    }
}

/// A single chunk suitable for an LLM context window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub chunk_id: usize,
    pub paper_id: String,
    /// Header text of the containing section.
    pub section: String,
    pub page_range: (u32, u32),
    pub text: String,
    pub figures: Vec<FigureInfo>,
    pub tables: Vec<TableInfo>,
    pub estimated_tokens: usize,
    /// True if this chunk continues in the next chunk.
    pub has_continuation: bool,
}
```

### Step 4: `types/path.rs`

```rust
//! Path objects extracted from PDF (used for table rule detection in Phase 2).

use serde::{Deserialize, Serialize};
use super::geometry::Rect;

/// A line segment or path from the PDF page content stream.
///
/// Used in Phase 2 (table detection) to detect horizontal/vertical rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathObject {
    pub bbox: Rect,
    pub page: u32,
    pub path_type: PathType,
    /// Line width in points.
    pub line_width: f64,
}

/// Classification of a PDF path for table detection purposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PathType {
    /// A horizontal line (height < 2pt).
    Horizontal,
    /// A vertical line (width < 2pt).
    Vertical,
    /// A rectangle (potential table cell border).
    Rectangle,
    /// Any other path shape.
    Other,
}
```

### Step 5: `types/mod.rs`

```rust
//! Shared types used across all pipeline modules.

pub mod document;
pub mod geometry;
pub mod path;
pub mod text;

// Convenience re-exports so callers can write `use crate::types::Rect` etc.
pub use document::{
    Chunk, DocumentMetadata, FigureInfo, ImageFormat, ImageInfo, InsertionPoint,
    PaperDocument, TableInfo, TableRepresentation,
};
pub use geometry::{PageDimensions, Rect};
pub use path::{PathObject, PathType};
pub use text::{
    BlockType, FontInfo, Section, SectionHeader, TextBlock, TextLine, TextSpan,
};
```

### Step 6: `error.rs`

```rust
//! Error and warning types for the pdf-lay pipeline.

use std::path::PathBuf;
use thiserror::Error;

/// All errors that can occur during PDF analysis.
#[derive(Debug, Error)]
pub enum PdfLayError {
    #[error("PDF file not found: {0}")]
    FileNotFound(PathBuf),

    #[error("Failed to parse PDF: {0}")]
    PdfParseError(String),

    #[error("Page {0} out of range (total pages: {1})")]
    PageOutOfRange(u32, u32),

    #[error("Image extraction failed on page {page}: {reason}")]
    ImageExtractionError { page: u32, reason: String },

    #[error("Coordinate normalization failed: scale factor could not be determined")]
    CoordinateNormalizationError,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Image processing error: {0}")]
    ImageError(#[from] image::ImageError),

    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),
}

/// Non-fatal issues that allow analysis to continue.
///
/// Accumulated in `AnalysisResult::warnings` rather than returned as `Err`.
#[derive(Debug, Clone)]
pub enum PdfLayWarning {
    /// A caption was detected but no nearby image could be matched.
    UnmatchedCaption { caption: String, page: u32 },
    /// An image was found but no caption could be matched to it.
    UnmatchedImage { image_path: String, page: u32 },
    /// Coordinate normalization fell back to a default scale factor.
    CoordinateFallback { page: u32, scale_used: f64 },
    /// An entire page was skipped due to an extraction error.
    PageSkipped { page: u32, reason: String },
}

/// The result of a full PDF analysis, including any non-fatal warnings.
#[derive(Debug)]
pub struct AnalysisResult {
    pub document: crate::types::PaperDocument,
    pub warnings: Vec<PdfLayWarning>,
}
```

### Step 7: `config.rs`

```rust
//! Configuration types for the analysis pipeline.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level configuration for `analyze_pdf`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Directory where extracted images are saved.
    pub image_output_dir: PathBuf,
    pub image_format: ImageOutputFormat,
    /// Whether to extract images at all (disable for text-only use cases).
    pub extract_images: bool,
    /// Whether to attempt table detection (Phase 2; stub in Phase 1).
    pub detect_tables: bool,
    pub table_config: TableConfig,
    pub math_config: MathConfig,
    /// Maximum vertical distance (points) between a caption and its image.
    pub caption_max_gap_pt: f64,
    /// Bin width (points) for the X-histogram in column detection.
    pub column_detection_bin_width: f64,
    /// Line-gap multiplier for block boundary detection.
    pub block_gap_multiplier: f64,
    pub header_detection: HeaderDetectionConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            image_output_dir: PathBuf::from("images"),
            image_format: ImageOutputFormat::Png,
            extract_images: true,
            detect_tables: false, // enabled in Phase 2
            table_config: TableConfig::default(),
            math_config: MathConfig::default(),
            caption_max_gap_pt: 50.0,
            column_detection_bin_width: 10.0,
            block_gap_multiplier: 1.8,
            header_detection: HeaderDetectionConfig::default(),
        }
    }
}

/// Image output format when saving extracted images.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum ImageOutputFormat {
    #[default]
    Png,
    Jpeg,
}

/// Configuration for Markdown output generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkdownConfig {
    /// Base path prepended to image paths in `![alt](path)`.
    pub image_base_path: String,
    pub include_page_numbers: bool,
    /// Added to the section level when generating `#` headers.
    /// Default 1 means level-1 sections become `##`.
    pub heading_offset: u8,
    pub include_metadata_header: bool,
    pub table_as_image: bool,
    pub figure_caption_style: CaptionStyle,
}

impl Default for MarkdownConfig {
    fn default() -> Self {
        Self {
            image_base_path: "./images".to_string(),
            include_page_numbers: false,
            heading_offset: 1,
            include_metadata_header: false,
            table_as_image: false,
            figure_caption_style: CaptionStyle::Italic,
        }
    }
}

/// How figure captions are rendered in Markdown output.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum CaptionStyle {
    #[default]
    Italic,     // *Fig. 1: ...*
    Bold,       // **Fig. 1:** ...
    PlainText,  // Fig. 1: ...
}

/// Configuration for table detection and rendering (Phase 2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableConfig {
    pub min_columns: usize,
    pub column_alignment_tolerance: f64,
    pub use_rule_detection: bool,
    pub use_text_alignment: bool,
}

impl Default for TableConfig {
    fn default() -> Self {
        Self {
            min_columns: 2,
            column_alignment_tolerance: 5.0,
            use_rule_detection: true,
            use_text_alignment: true,
        }
    }
}

/// Configuration for math detection and conversion (Phase 2/3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MathConfig {
    pub representation: MathRepresentationPreference,
    pub inline_delimiter: (String, String),
    pub display_delimiter: (String, String),
    /// Y-offset threshold for superscript/subscript detection (as ratio of font_size).
    pub superscript_y_threshold: f64,
    pub additional_math_fonts: Vec<String>,
}

impl Default for MathConfig {
    fn default() -> Self {
        Self {
            representation: MathRepresentationPreference::Auto,
            inline_delimiter: ("$".to_string(), "$".to_string()),
            display_delimiter: ("$$\n".to_string(), "\n$$".to_string()),
            superscript_y_threshold: 0.3,
            additional_math_fonts: Vec::new(),
        }
    }
}

/// Preferred output format for mathematical expressions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum MathRepresentationPreference {
    LaTeX,
    UnicodeMath,
    PlainText,
    #[default]
    Auto,
}

/// Configuration for section header detection scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderDetectionConfig {
    /// Minimum score for a block to be classified as a header.
    pub min_score: u32,
    /// Maximum character count for a header candidate.
    pub max_chars: usize,
    /// Maximum line count for a header candidate.
    pub max_lines: usize,
}

impl Default for HeaderDetectionConfig {
    fn default() -> Self {
        Self {
            min_score: 4,
            max_chars: 120,
            max_lines: 3,
        }
    }
}

/// Configuration for LLM text generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmTextConfig {
    pub include_figures: bool,
    pub include_tables: bool,
    pub include_section_headers: bool,
    pub math_representation: MathRepresentationPreference,
    pub figure_format: FigureTextFormat,
}

impl Default for LlmTextConfig {
    fn default() -> Self {
        Self {
            include_figures: true,
            include_tables: true,
            include_section_headers: true,
            math_representation: MathRepresentationPreference::Auto,
            figure_format: FigureTextFormat::Placeholder,
        }
    }
}

/// How figures are represented in LLM text output.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum FigureTextFormat {
    /// `[IMAGE: Fig. 1 path/to/img.png]`
    #[default]
    Placeholder,
    /// `![Fig. 1](path/to/img.png)`
    MarkdownLink,
    /// Caption text only, no path.
    CaptionOnly,
    /// Omit figures entirely.
    Omit,
}

/// Configuration for chunk splitting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkConfig {
    pub max_tokens: usize,
    pub overlap_tokens: usize,
    pub split_strategy: SplitStrategy,
    pub include_section_context: bool,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            max_tokens: 4000,
            overlap_tokens: 200,
            split_strategy: SplitStrategy::SectionBoundary,
            include_section_context: true,
        }
    }
}

/// Strategy for splitting sections into chunks.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum SplitStrategy {
    /// Split at section boundaries first (recommended).
    #[default]
    SectionBoundary,
    /// Split purely by token count.
    TokenCount,
    /// Split at paragraph boundaries.
    Paragraph,
}
```

### Step 8: Update `crates/pdf-lay-core/src/lib.rs`

```rust
//! pdf-lay-core: internal PDF layout analysis library.

#![warn(missing_docs)]

pub mod config;
pub mod error;
pub mod types;

// Modules added by subsequent tasks:
// pub mod extract;
// pub mod layout;
// pub mod structure;
// pub mod figure;
// pub mod selector;
// pub mod output;
// pub(crate) mod pipeline;

pub use error::{AnalysisResult, PdfLayError, PdfLayWarning};
```

## Acceptance Criteria

- [ ] `cargo build -p pdf-lay-core` succeeds
- [ ] `cargo clippy -p pdf-lay-core -- -D warnings` passes
- [ ] Unit tests in `geometry.rs` all pass: `cargo test -p pdf-lay-core -- types`
  - `rect_width_height`
  - `rect_center`
  - `rect_vertical_gap_above`
  - `rect_vertical_gap_overlap`
  - `rect_union`
- [ ] `FontInfo::detect_bold("HelveticaBold")` returns `true`
- [ ] `FontInfo::detect_bold("Helvetica")` returns `false`
- [ ] `FontInfo::detect_italic("TimesItalic")` returns `true`
- [ ] `Config::default()` constructs without panic and has correct default values:
  - `caption_max_gap_pt == 50.0`
  - `block_gap_multiplier == 1.8`
  - `column_detection_bin_width == 10.0`
- [ ] All types implement `Debug`, `Clone`, `Serialize`, `Deserialize`
- [ ] `BlockType` implements `Default` (defaults to `BodyText`)

## Dependencies

- Task 01 must be completed first.

## Commit Message

```
feat(types): add core type definitions — Rect, TextSpan, TextBlock, Section, Config, Error
```
