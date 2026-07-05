//! Text-related types: font information, spans, lines, blocks, sections.

use serde::{Deserialize, Serialize};

use super::geometry::Rect;

/// Font metadata for a text span.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontInfo {
    /// Full font name as embedded in the PDF (may be obfuscated, e.g. "CMMI10", "F2").
    pub name: String,
    /// Font size in points.
    pub size: f64,
    /// Whether this font is bold (from heuristic detection).
    pub is_bold: bool,
    /// Whether this font is italic (from heuristic detection).
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
    /// The text content of this span.
    pub text: String,
    /// Full font name as embedded in the PDF.
    pub font_name: String,
    /// Font size in points.
    pub font_size: f64,
    /// Whether this span uses a bold font (from heuristic detection).
    pub is_bold: bool,
    /// Whether this span uses an italic font (from heuristic detection).
    pub is_italic: bool,
    /// Bounding box in PDF coordinates.
    pub bbox: Rect,
    /// Zero-based page index.
    pub page: u32,
}

/// A logical line of text reconstructed from multiple `TextSpan`s.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextLine {
    /// Individual spans that make up this line.
    pub spans: Vec<TextSpan>,
    /// Joined text with inter-span spaces inserted where appropriate.
    pub text: String,
    /// Bounding box enclosing all spans.
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
    /// Lines that make up this block.
    pub lines: Vec<TextLine>,
    /// Full text of the block (lines joined with newlines).
    pub text: String,
    /// Bounding box enclosing all lines.
    pub bbox: Rect,
    /// Zero-based page index of the first line.
    pub page: u32,
    /// Which column this block belongs to (0 = left/only column).
    pub column_index: usize,
    /// Semantic classification of this block.
    pub block_type: BlockType,
}

impl TextBlock {
    /// Returns the primary font size of this block (from first line, or 0.0 if empty).
    pub fn primary_font_size(&self) -> f64 {
        self.lines
            .first()
            .map(|l| l.primary_font_size)
            .unwrap_or(0.0)
    }

    /// Returns true if this block is predominantly bold.
    pub fn is_bold(&self) -> bool {
        self.lines.first().map(|l| l.is_bold).unwrap_or(false)
    }
}

/// Semantic classification of a `TextBlock`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BlockType {
    /// Document title.
    Title,
    /// Abstract section.
    Abstract,
    /// Top-level section header.
    SectionHeader,
    /// Subsection or lower-level header.
    SubsectionHeader,
    /// Regular body text.
    #[default]
    BodyText,
    /// Figure or table caption.
    Caption,
    /// List item.
    ListItem,
    /// Mathematical equation.
    Equation,
    /// Footnote text.
    Footnote,
    /// Page number.
    PageNumber,
    /// Running header (appears at the top of each page).
    RunningHeader,
    /// Running footer (appears at the bottom of each page).
    RunningFooter,
    /// Bibliographic reference.
    Reference,
    /// Block could not be classified.
    Unknown,
}

/// Extracted section with its content and child sub-sections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    /// Optional header for this section (None for the preamble before the first header).
    pub header: Option<SectionHeader>,
    /// Hierarchy level: 1 = top-level, 2 = subsection, 3 = subsubsection.
    pub level: u8,
    /// Text blocks that belong directly to this section.
    pub blocks: Vec<TextBlock>,
    /// Figures in this section.
    pub figures: Vec<super::document::FigureInfo>,
    /// Tables in this section.
    pub tables: Vec<super::document::TableInfo>,
    /// Child sections (subsections).
    pub children: Vec<Section>,
    /// (first_page, last_page) — zero-based page numbers.
    pub page_range: (u32, u32),
}

impl Section {
    /// Returns the clean header text, or an empty string for headerless sections.
    pub fn header_text(&self) -> String {
        self.header
            .as_ref()
            .map(|h| h.clean_text.clone())
            .unwrap_or_default()
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
    /// Page where this header appears (zero-based).
    pub page: u32,
    /// Bounding box of the header block.
    pub bbox: Rect,
    /// `global_index` of the header's anchor block (a stable id that matches
    /// `TextBlock.global_index`, not a slice position).
    pub block_index: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_info_detect_bold_positive() {
        assert!(FontInfo::detect_bold("HelveticaBold"));
        assert!(FontInfo::detect_bold("Arial-BD"));
        assert!(FontInfo::detect_bold("GothamHeavy"));
        assert!(FontInfo::detect_bold("FuturaBlack"));
    }

    #[test]
    fn font_info_detect_bold_negative() {
        assert!(!FontInfo::detect_bold("Helvetica"));
        assert!(!FontInfo::detect_bold("TimesNewRoman"));
        assert!(!FontInfo::detect_bold("Arial"));
    }

    #[test]
    fn font_info_detect_italic_positive() {
        assert!(FontInfo::detect_italic("TimesItalic"));
        assert!(FontInfo::detect_italic("HelveticaOblique"));
        assert!(FontInfo::detect_italic("Times-IT"));
    }

    #[test]
    fn font_info_detect_italic_negative() {
        assert!(!FontInfo::detect_italic("Times"));
        assert!(!FontInfo::detect_italic("Helvetica"));
    }

    #[test]
    fn block_type_default() {
        assert_eq!(BlockType::default(), BlockType::BodyText);
    }
}
