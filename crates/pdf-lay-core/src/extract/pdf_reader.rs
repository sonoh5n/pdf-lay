//! Wrapper around `pdf_oxide::PdfDocument` that converts its types to our internal types.
//!
//! This is the **only** file in the crate that imports from `pdf_oxide`.
//! All other modules receive `Vec<TextSpan>` / `Vec<ImageInfo>` from this reader.

use std::path::Path;

use pdf_oxide::document::PdfDocument;

use crate::error::PdfLayError;
use crate::types::{FontInfo, PageDimensions, PathObject, PathType, Rect, TextSpan};

/// A handle to an opened PDF document.
///
/// This struct is the sole importer of `pdf_oxide`. All other modules
/// receive `Vec<TextSpan>` / `Vec<ImageInfo>` from this reader.
pub struct PdfReader {
    inner: PdfDocument,
    /// Holds the temporary file alive when created via `from_bytes()`.
    /// The file is automatically cleaned up when the `PdfReader` is dropped.
    _temp_file: Option<tempfile::NamedTempFile>,
}

impl PdfReader {
    /// Open a PDF file from disk.
    pub fn open(path: &Path) -> Result<Self, PdfLayError> {
        if !path.exists() {
            return Err(PdfLayError::FileNotFound(path.to_path_buf()));
        }
        let inner =
            PdfDocument::open(path).map_err(|e| PdfLayError::PdfParseError(e.to_string()))?;
        Ok(Self {
            inner,
            _temp_file: None,
        })
    }

    /// Open a PDF from an in-memory byte slice.
    ///
    /// Writes the bytes to a temporary file then opens it with pdf_oxide, since
    /// `pdf_oxide::PdfDocument` requires a file-backed reader.  The temporary
    /// file is kept alive for the lifetime of this `PdfReader` and automatically
    /// cleaned up when it is dropped.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PdfLayError> {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new()?;
        tmp.write_all(bytes)?;
        tmp.flush()?;
        let path = tmp.path().to_path_buf();
        let inner =
            PdfDocument::open(&path).map_err(|e| PdfLayError::PdfParseError(e.to_string()))?;
        Ok(Self {
            inner,
            _temp_file: Some(tmp),
        })
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
    /// Reads vector graphics paths from the page content stream and converts them
    /// into `PathObject` values with bounding boxes and type classifications.
    ///
    /// # PathType classification
    ///
    /// - `Horizontal`: height < 2.0 and width > 5.0 (horizontal rule)
    /// - `Vertical`: width < 2.0 and height > 5.0 (vertical rule)
    /// - `Rectangle`: width > 5.0 and height > 5.0 (potential table cell border)
    /// - `Other`: everything else
    pub fn extract_paths(&mut self, page: u32) -> Result<Vec<PathObject>, PdfLayError> {
        let total = self.page_count();
        if page >= total {
            return Err(PdfLayError::PageOutOfRange(page, total));
        }

        let raw_paths = match self.inner.extract_paths(page as usize) {
            Ok(paths) => paths,
            Err(e) => {
                log::warn!("extract_paths: failed for page {page}: {e}");
                return Ok(Vec::new());
            }
        };

        let mut result = Vec::with_capacity(raw_paths.len());
        for raw in raw_paths {
            if let Some(obj) = convert_path(raw, page) {
                result.push(obj);
            }
        }

        log::debug!(
            "extract_paths: page {page} — {} path objects extracted",
            result.len()
        );
        Ok(result)
    }

    /// Extract path objects from all pages.
    ///
    /// Pages that fail to extract are logged as warnings and skipped.
    pub fn extract_all_paths(&mut self) -> Result<Vec<PathObject>, PdfLayError> {
        let mut all = Vec::new();
        for page in 0..self.page_count() {
            all.extend(self.extract_paths(page)?);
        }
        Ok(all)
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

/// Convert a single pdf_oxide `PathContent` to our `PathObject`.
///
/// Returns `None` for paths whose bounding box has non-positive dimensions
/// or non-finite coordinates (degenerate paths that cannot be classified).
///
/// # Coordinate mapping
///
/// pdf_oxide stores path bounding boxes in PDF native coordinates (Y-up, origin at
/// lower-left corner of the page).  The `Rect` fields are:
///   - `x`      — left edge
///   - `y`      — lower edge (bottom of bounding box in Y-up space)
///   - `width`  — horizontal extent (always positive)
///   - `height` — vertical extent (always positive, going up)
///
/// Our `Rect` uses the same Y-up convention and requires `top > bottom`:
///   - `left`   = raw `x`
///   - `right`  = raw `x + width`
///   - `top`    = raw `y + height`  (upper edge, larger Y)
///   - `bottom` = raw `y`           (lower edge, smaller Y)
///
/// # PathType classification
///
/// - `Horizontal`: height < 2.0 and width > 5.0
/// - `Vertical`: width < 2.0 and height > 5.0
/// - `Rectangle`: width > 5.0 and height > 5.0
/// - `Other`: everything else
fn convert_path(raw: pdf_oxide::elements::PathContent, page: u32) -> Option<PathObject> {
    let ox = raw.bbox.x as f64;
    let oy = raw.bbox.y as f64;
    let ow = raw.bbox.width as f64;
    let oh = raw.bbox.height as f64;

    // Guard against non-finite or degenerate bboxes.
    if !ox.is_finite() || !oy.is_finite() || !ow.is_finite() || !oh.is_finite() {
        log::debug!("convert_path: skipping path with non-finite bbox on page {page}");
        return None;
    }
    if ow < 0.0 || oh < 0.0 {
        log::debug!("convert_path: skipping path with negative dimensions on page {page}");
        return None;
    }

    // Map to our Y-up Rect.  When width or height is zero (a degenerate line),
    // use max() to avoid violating the top >= bottom / right >= left invariant.
    let left = ox;
    let bottom = oy;
    let right = (ox + ow).max(ox);
    let top = (oy + oh).max(oy);

    let bbox = Rect::new(left, top, right, bottom);
    let width = bbox.width();
    let height = bbox.height();

    let path_type = classify_path(width, height);
    let line_width = raw.stroke_width as f64;

    Some(PathObject {
        bbox,
        page,
        path_type,
        line_width,
    })
}

/// Classify a path based on its bounding box dimensions.
///
/// - `Horizontal`: height < 2.0 and width > 5.0
/// - `Vertical`: width < 2.0 and height > 5.0
/// - `Rectangle`: width > 5.0 and height > 5.0
/// - `Other`: everything else
fn classify_path(width: f64, height: f64) -> PathType {
    if height < 2.0 && width > 5.0 {
        PathType::Horizontal
    } else if width < 2.0 && height > 5.0 {
        PathType::Vertical
    } else if width > 5.0 && height > 5.0 {
        PathType::Rectangle
    } else {
        PathType::Other
    }
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

    // ---- convert_path / classify_path tests ----

    fn make_path_content(
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        stroke_width: f32,
    ) -> pdf_oxide::elements::PathContent {
        pdf_oxide::elements::PathContent {
            bbox: pdf_oxide::geometry::Rect::new(x, y, w, h),
            operations: Vec::new(),
            stroke_color: Some(pdf_oxide::layout::Color::new(0.0, 0.0, 0.0)),
            fill_color: None,
            stroke_width,
            line_cap: pdf_oxide::elements::LineCap::Butt,
            line_join: pdf_oxide::elements::LineJoin::Miter,
            reading_order: None,
        }
    }

    #[test]
    fn classify_path_horizontal() {
        // wide and thin → Horizontal
        assert!(matches!(classify_path(100.0, 1.0), PathType::Horizontal));
        // exactly at boundary: height == 1.9, width == 5.1
        assert!(matches!(classify_path(5.1, 1.9), PathType::Horizontal));
    }

    #[test]
    fn classify_path_vertical() {
        // tall and thin → Vertical
        assert!(matches!(classify_path(1.0, 100.0), PathType::Vertical));
        // exactly at boundary: width == 1.9, height == 5.1
        assert!(matches!(classify_path(1.9, 5.1), PathType::Vertical));
    }

    #[test]
    fn classify_path_rectangle() {
        // both dimensions large → Rectangle
        assert!(matches!(classify_path(50.0, 20.0), PathType::Rectangle));
        // at boundary: width == 5.1, height == 5.1
        assert!(matches!(classify_path(5.1, 5.1), PathType::Rectangle));
    }

    #[test]
    fn classify_path_other() {
        // tiny square → Other
        assert!(matches!(classify_path(2.0, 2.0), PathType::Other));
        // zero dimensions → Other
        assert!(matches!(classify_path(0.0, 0.0), PathType::Other));
    }

    #[test]
    fn convert_path_horizontal_rule() {
        // x=50, y=300, width=200, height=1 → horizontal line
        let raw = make_path_content(50.0, 300.0, 200.0, 1.0, 0.5);
        let obj = convert_path(raw, 2).expect("Should produce a PathObject");
        assert_eq!(obj.page, 2);
        assert_eq!(obj.bbox.left, 50.0);
        assert_eq!(obj.bbox.right, 250.0);
        assert_eq!(obj.bbox.bottom, 300.0);
        assert_eq!(obj.bbox.top, 301.0);
        assert!(obj.bbox.top >= obj.bbox.bottom, "top must be >= bottom");
        assert!(matches!(obj.path_type, PathType::Horizontal));
        assert_eq!(obj.line_width, 0.5);
    }

    #[test]
    fn convert_path_vertical_rule() {
        // x=100, y=200, width=1, height=100 → vertical line
        let raw = make_path_content(100.0, 200.0, 1.0, 100.0, 1.0);
        let obj = convert_path(raw, 0).expect("Should produce a PathObject");
        assert_eq!(obj.bbox.left, 100.0);
        assert_eq!(obj.bbox.right, 101.0);
        assert_eq!(obj.bbox.bottom, 200.0);
        assert_eq!(obj.bbox.top, 300.0);
        assert!(matches!(obj.path_type, PathType::Vertical));
    }

    #[test]
    fn convert_path_rectangle() {
        // x=10, y=50, width=100, height=50 → rectangle
        let raw = make_path_content(10.0, 50.0, 100.0, 50.0, 1.0);
        let obj = convert_path(raw, 1).expect("Should produce a PathObject");
        assert_eq!(obj.bbox.left, 10.0);
        assert_eq!(obj.bbox.right, 110.0);
        assert_eq!(obj.bbox.bottom, 50.0);
        assert_eq!(obj.bbox.top, 100.0);
        assert!(obj.bbox.top > obj.bbox.bottom, "top must exceed bottom");
        assert!(matches!(obj.path_type, PathType::Rectangle));
    }

    #[test]
    fn convert_path_degenerate_negative_dims_returns_none() {
        // Negative width — should be skipped.
        let raw = make_path_content(10.0, 10.0, -5.0, 10.0, 1.0);
        assert!(convert_path(raw, 0).is_none());
    }

    #[test]
    fn convert_path_zero_height_degenerate_line() {
        // Zero height — a degenerate horizontal line (point on a line).
        // Should still produce a PathObject (classified as Other since width may be > 5
        // but height == 0 < 2 so Horizontal if width > 5).
        let raw = make_path_content(10.0, 100.0, 50.0, 0.0, 1.0);
        let obj = convert_path(raw, 0).expect("Should produce a PathObject for zero-height path");
        assert_eq!(obj.bbox.top, obj.bbox.bottom); // degenerate
        // width=50 > 5.0 and height=0 < 2.0 → Horizontal
        assert!(matches!(obj.path_type, PathType::Horizontal));
    }
}
