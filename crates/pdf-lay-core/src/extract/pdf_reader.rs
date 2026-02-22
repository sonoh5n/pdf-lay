//! Wrapper around `pdf_oxide::PdfDocument` that converts its types to our internal types.
//!
//! This is the **only** file in the crate that imports from `pdf_oxide`.
//! All other modules receive `Vec<TextSpan>` / `Vec<ImageInfo>` from this reader.

use std::path::Path;

use pdf_oxide::document::PdfDocument;

use crate::error::PdfLayError;
use crate::types::{FontInfo, PageDimensions, PathObject, Rect, TextSpan};

/// A handle to an opened PDF document.
///
/// This struct is the sole importer of `pdf_oxide`. All other modules
/// receive `Vec<TextSpan>` / `Vec<ImageInfo>` from this reader.
pub struct PdfReader {
    inner: PdfDocument,
}

impl PdfReader {
    /// Open a PDF file from disk.
    pub fn open(path: &Path) -> Result<Self, PdfLayError> {
        if !path.exists() {
            return Err(PdfLayError::FileNotFound(path.to_path_buf()));
        }
        let inner =
            PdfDocument::open(path).map_err(|e| PdfLayError::PdfParseError(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Open a PDF from an in-memory byte slice.
    ///
    /// Writes the bytes to a temporary file then opens it with pdf_oxide, since
    /// `pdf_oxide::PdfDocument` requires a file-backed reader.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PdfLayError> {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new()?;
        tmp.write_all(bytes)?;
        tmp.flush()?;
        let path = tmp.path().to_path_buf();
        let inner =
            PdfDocument::open(&path).map_err(|e| PdfLayError::PdfParseError(e.to_string()))?;
        // Keep the temp file alive by leaking the NamedTempFile — the OS will clean it up
        // when the process exits or when the file handle is eventually dropped.
        // For long-running processes a more careful approach (e.g., storing the handle in
        // PdfReader) would be preferable, but for now this is sufficient.
        let _ = tmp.keep();
        Ok(Self { inner })
    }

    /// Number of pages in the document (0-based indexing used throughout).
    pub fn page_count(&mut self) -> u32 {
        self.inner.page_count().unwrap_or(0) as u32
    }

    /// Dimensions of the specified page in points.
    ///
    /// Parses the page's MediaBox from the PDF dictionary.
    /// Falls back to US Letter size (612 × 792 pt) if the page cannot be found.
    pub fn page_dimensions(&mut self, page: u32) -> Result<PageDimensions, PdfLayError> {
        let total = self.page_count();
        if page >= total {
            return Err(PdfLayError::PageOutOfRange(page, total));
        }
        // pdf_oxide does not expose a non-rendering page-size API, so we derive
        // dimensions from the extracted spans' bounding boxes as a best-effort
        // heuristic.  A proper implementation would parse the /MediaBox array from
        // the page dictionary; that requires either the `rendering` feature or
        // duplicating the dictionary walking logic, both of which are out of scope
        // for Task 3.  Instead, use a fixed Letter default so callers can proceed.
        //
        // NOTE: This will be improved in a future task when page-geometry access is
        // available.
        Ok(PageDimensions {
            page_number: page,
            width: 612.0,
            height: 792.0,
        })
    }

    /// Extract text spans from a single page.
    ///
    /// Converts pdf_oxide's span representation into our `TextSpan` type.
    /// Font bold/italic detection uses `FontInfo::detect_bold/detect_italic` heuristics
    /// in addition to pdf_oxide's parsed `FontWeight`.
    pub fn extract_text_spans(&mut self, page: u32) -> Result<Vec<TextSpan>, PdfLayError> {
        let total = self.page_count();
        if page >= total {
            return Err(PdfLayError::PageOutOfRange(page, total));
        }

        let raw_spans = self
            .inner
            .extract_spans(page as usize)
            .map_err(|e| PdfLayError::PdfParseError(format!("page {page}: {e}")))?;

        let spans = raw_spans
            .into_iter()
            .filter_map(|s| convert_span(s, page))
            .collect();

        Ok(spans)
    }

    /// Extract text spans from all pages.
    ///
    /// Pages that fail to extract are logged as warnings and skipped so that
    /// the rest of the document can still be processed.
    pub fn extract_all_text_spans(&mut self) -> Result<Vec<TextSpan>, PdfLayError> {
        let mut all = Vec::new();
        let total = self.page_count();
        for page in 0..total {
            match self.inner.extract_spans(page as usize) {
                Ok(raw_spans) => {
                    all.extend(raw_spans.into_iter().filter_map(|s| convert_span(s, page)));
                }
                Err(e) => {
                    log::warn!("Skipping page {page} due to extraction error: {e}");
                }
            }
        }
        Ok(all)
    }

    /// Extract path objects (lines and rectangles) for table rule detection.
    ///
    /// Returns an empty `Vec` for now. Full implementation is Phase 2.
    pub fn extract_paths(&mut self, page: u32) -> Result<Vec<PathObject>, PdfLayError> {
        let total = self.page_count();
        if page >= total {
            return Err(PdfLayError::PageOutOfRange(page, total));
        }
        log::debug!("extract_paths: stub — returning empty for page {page}");
        Ok(Vec::new())
    }

    /// Returns a mutable reference to the underlying pdf_oxide document.
    ///
    /// Only for use within the `extract` module (e.g., by `ImageExtractor`).
    pub(super) fn inner_doc(&mut self) -> &mut PdfDocument {
        &mut self.inner
    }
}

// ---- Private conversion helpers ------------------------------------------------

/// Convert a single pdf_oxide `TextSpan` to our `TextSpan`.
///
/// Returns `None` for spans whose text is empty after trimming (pdf_oxide may
/// produce whitespace-only spans for inter-word spacing).
///
/// # Coordinate mapping
///
/// pdf_oxide stores bounding boxes in PDF native coordinates (Y-up, origin at
/// lower-left corner of the page).  Its `Rect` fields are:
///   - `x`      — left edge
///   - `y`      — lower edge (baseline / bottom of glyph box)
///   - `width`  — horizontal extent
///   - `height` — vertical extent (positive, going up)
///
/// Our `Rect` uses the same Y-up convention and requires `top > bottom`:
///   - `left`   = raw `x`
///   - `right`  = raw `x + width`
///   - `top`    = raw `y + height`  (upper edge, larger Y)
///   - `bottom` = raw `y`           (lower edge, smaller Y)
fn convert_span(raw: pdf_oxide::layout::TextSpan, page: u32) -> Option<TextSpan> {
    let text = raw.text.trim().to_string();
    if text.is_empty() {
        return None;
    }

    let font_name = raw.font_name.clone();
    let font_size = raw.font_size as f64;

    // Determine bold/italic from pdf_oxide's parsed FontWeight, plus name heuristics.
    let is_bold = raw.font_weight.is_bold() || FontInfo::detect_bold(&font_name);
    let is_italic = raw.is_italic || FontInfo::detect_italic(&font_name);

    // Map pdf_oxide's Rect (x, y, width, height in Y-up space) to our Rect.
    let ox = raw.bbox.x as f64;
    let oy = raw.bbox.y as f64;
    let ow = raw.bbox.width as f64;
    let oh = raw.bbox.height as f64;

    // Guard against degenerate bboxes (zero-area or inverted).
    if ow <= 0.0 || oh <= 0.0 {
        return None;
    }

    let bbox = Rect::new(
        ox,      // left
        oy + oh, // top  (upper Y in PDF Y-up space)
        ox + ow, // right
        oy,      // bottom (lower Y in PDF Y-up space)
    );

    Some(TextSpan {
        text,
        is_bold,
        is_italic,
        font_name,
        font_size,
        bbox,
        page,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_oxide::layout::FontWeight;

    #[test]
    fn open_nonexistent_returns_file_not_found() {
        let result = PdfReader::open(Path::new("/nonexistent/path/to/file.pdf"));
        assert!(
            matches!(result, Err(PdfLayError::FileNotFound(_))),
            "Expected FileNotFound error"
        );
    }

    #[test]
    fn convert_span_empty_text_returns_none() {
        let raw = pdf_oxide::layout::TextSpan {
            text: "   ".to_string(),
            bbox: pdf_oxide::geometry::Rect::new(0.0, 0.0, 10.0, 12.0),
            font_name: "Regular".to_string(),
            font_size: 12.0,
            font_weight: FontWeight::Normal,
            is_italic: false,
            color: pdf_oxide::layout::Color::new(0.0, 0.0, 0.0),
            mcid: None,
            sequence: 0,
            split_boundary_before: false,
            offset_semantic: false,
            char_spacing: 0.0,
            word_spacing: 0.0,
            horizontal_scaling: 100.0,
            primary_detected: false,
        };
        assert!(convert_span(raw, 0).is_none());
    }

    #[test]
    fn convert_span_maps_coordinates_correctly() {
        // Raw: x=10, y=100, width=50, height=12 (PDF Y-up)
        // Expected: left=10, right=60, bottom=100, top=112
        let raw = pdf_oxide::layout::TextSpan {
            text: "Hello".to_string(),
            bbox: pdf_oxide::geometry::Rect::new(10.0, 100.0, 50.0, 12.0),
            font_name: "Regular".to_string(),
            font_size: 12.0,
            font_weight: FontWeight::Normal,
            is_italic: false,
            color: pdf_oxide::layout::Color::new(0.0, 0.0, 0.0),
            mcid: None,
            sequence: 0,
            split_boundary_before: false,
            offset_semantic: false,
            char_spacing: 0.0,
            word_spacing: 0.0,
            horizontal_scaling: 100.0,
            primary_detected: false,
        };
        let span = convert_span(raw, 0).expect("Should produce a valid span");
        assert_eq!(span.text, "Hello");
        assert_eq!(span.bbox.left, 10.0);
        assert_eq!(span.bbox.right, 60.0);
        assert_eq!(span.bbox.bottom, 100.0);
        assert_eq!(span.bbox.top, 112.0);
        assert!(span.bbox.top > span.bbox.bottom, "top must exceed bottom");
    }

    #[test]
    fn convert_span_detects_bold_from_font_weight() {
        let raw = pdf_oxide::layout::TextSpan {
            text: "Bold text".to_string(),
            bbox: pdf_oxide::geometry::Rect::new(0.0, 0.0, 40.0, 12.0),
            font_name: "Arial".to_string(),
            font_size: 12.0,
            font_weight: FontWeight::Bold,
            is_italic: false,
            color: pdf_oxide::layout::Color::new(0.0, 0.0, 0.0),
            mcid: None,
            sequence: 0,
            split_boundary_before: false,
            offset_semantic: false,
            char_spacing: 0.0,
            word_spacing: 0.0,
            horizontal_scaling: 100.0,
            primary_detected: false,
        };
        let span = convert_span(raw, 0).unwrap();
        assert!(span.is_bold);
    }

    #[test]
    fn convert_span_zero_size_returns_none() {
        let raw = pdf_oxide::layout::TextSpan {
            text: "A".to_string(),
            bbox: pdf_oxide::geometry::Rect::new(0.0, 0.0, 0.0, 12.0),
            font_name: "Regular".to_string(),
            font_size: 12.0,
            font_weight: FontWeight::Normal,
            is_italic: false,
            color: pdf_oxide::layout::Color::new(0.0, 0.0, 0.0),
            mcid: None,
            sequence: 0,
            split_boundary_before: false,
            offset_semantic: false,
            char_spacing: 0.0,
            word_spacing: 0.0,
            horizontal_scaling: 100.0,
            primary_detected: false,
        };
        assert!(convert_span(raw, 0).is_none());
    }

    // Integration test — requires a real PDF fixture.
    #[test]
    #[ignore = "requires tests/fixtures/sample.pdf"]
    fn extract_spans_from_real_pdf() {
        let mut reader = PdfReader::open(Path::new("tests/fixtures/sample.pdf")).unwrap();
        assert!(reader.page_count() > 0);
        let spans = reader.extract_text_spans(0).unwrap();
        assert!(!spans.is_empty(), "Expected non-empty spans from page 0");
        let s = &spans[0];
        assert!(!s.text.is_empty());
        assert!(s.font_size > 0.0);
        assert!(s.bbox.width() > 0.0);
        assert!(s.bbox.height() > 0.0);
        assert!(s.bbox.top > s.bbox.bottom);
    }
}
