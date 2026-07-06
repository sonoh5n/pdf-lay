//! Wrapper around `pdf_oxide::PdfDocument` that converts its types to our internal types.
//!
//! This is the **only** file in the crate that imports from `pdf_oxide`.
//! All other modules receive `Vec<TextSpan>` / `Vec<ImageInfo>` from this reader.

use std::path::Path;

use pdf_oxide::document::PdfDocument;
use pdf_oxide::object::Object;

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

    /// Read the page's MediaBox (and rotation) directly from the page dictionary.
    ///
    /// Returns `(width, height, rotation_degrees)` in points, with `width` and
    /// `height` already swapped for 90°/270° page rotation. Returns `None` when
    /// the MediaBox cannot be read.
    ///
    /// This uses pdf_oxide's non-rendering page-dictionary accessor
    /// (`get_page_for_debug`), so it works **without** enabling the heavy
    /// `rendering` feature. Inherited MediaBox values are resolved by
    /// pdf_oxide's page-tree walk.
    pub fn page_media_box(&mut self, page: u32) -> Option<(f64, f64, i32)> {
        let page_obj = self.inner.get_page_for_debug(page as usize).ok()?;
        let dict = page_obj.as_dict()?;

        // MediaBox = [x0, y0, x1, y1]; entries may be Integer or Real.
        let arr = dict.get("MediaBox").and_then(|o| o.as_array())?;
        let num = |o: &Object| o.as_real().or_else(|| o.as_integer().map(|i| i as f64));
        let x0 = num(arr.first()?)?;
        let y0 = num(arr.get(1)?)?;
        let x1 = num(arr.get(2)?)?;
        let y1 = num(arr.get(3)?)?;
        let width = (x1 - x0).abs();
        let height = (y1 - y0).abs();
        if width <= 0.0 || height <= 0.0 {
            return None;
        }

        // /Rotate (optional): normalize to a positive 0/90/180/270 value.
        let rotation = dict
            .get("Rotate")
            .and_then(|o| o.as_integer())
            .map(|r| r.rem_euclid(360) as i32)
            .unwrap_or(0);

        // For 90°/270° the visible page is rotated a quarter turn, so the
        // effective width and height are swapped relative to the MediaBox.
        let (width, height) = if rotation == 90 || rotation == 270 {
            (height, width)
        } else {
            (width, height)
        };

        Some((width, height, rotation))
    }

    /// Dimensions of the specified page in points.
    ///
    /// Reads the page's real MediaBox via [`Self::page_media_box`]. When the
    /// MediaBox cannot be read, falls back to US Letter size (612 × 792 pt) so
    /// callers can proceed. Callers that have access to extracted spans should
    /// prefer a span-extent fallback (see the pipeline) over this Letter default.
    pub fn page_dimensions(&mut self, page: u32) -> Result<PageDimensions, PdfLayError> {
        let total = self.page_count();
        if page >= total {
            return Err(PdfLayError::PageOutOfRange(page, total));
        }
        let (width, height) = self
            .page_media_box(page)
            .map(|(w, h, _rot)| (w, h))
            .unwrap_or((612.0, 792.0));
        Ok(PageDimensions {
            page_number: page,
            width,
            height,
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

    /// Build a minimal single-page PDF with a given MediaBox and optional
    /// /Rotate, with a correct cross-reference table so pdf_oxide can open it.
    fn build_minimal_pdf(media_box: [i32; 4], rotate: Option<i32>) -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();
        let mut offsets: Vec<usize> = Vec::new();

        buf.extend_from_slice(b"%PDF-1.4\n");

        offsets.push(buf.len());
        buf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

        offsets.push(buf.len());
        buf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");

        offsets.push(buf.len());
        let rotate_str = rotate.map(|r| format!(" /Rotate {r}")).unwrap_or_default();
        let page = format!(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [{} {} {} {}]{} >>\nendobj\n",
            media_box[0], media_box[1], media_box[2], media_box[3], rotate_str
        );
        buf.extend_from_slice(page.as_bytes());

        let xref_offset = buf.len();
        let size = offsets.len() + 1; // + the free object 0
        buf.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
        buf.extend_from_slice(b"0000000000 65535 f \n");
        for off in &offsets {
            buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        buf.extend_from_slice(
            format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n")
                .as_bytes(),
        );

        buf
    }

    #[test]
    fn page_media_box_reads_real_dimensions() {
        // A4 in points: 595 × 842 (NOT the old hardcoded Letter 612 × 792).
        let pdf = build_minimal_pdf([0, 0, 595, 842], None);
        let mut reader = PdfReader::from_bytes(&pdf).expect("minimal PDF should open");
        let (w, h, rot) = reader
            .page_media_box(0)
            .expect("MediaBox should be readable");
        assert!((w - 595.0).abs() < 1.0, "width should be ~595, got {w}");
        assert!((h - 842.0).abs() < 1.0, "height should be ~842, got {h}");
        assert_eq!(rot, 0);
    }

    #[test]
    fn page_media_box_swaps_on_rotation() {
        let pdf = build_minimal_pdf([0, 0, 595, 842], Some(90));
        let mut reader = PdfReader::from_bytes(&pdf).expect("minimal PDF should open");
        let (w, h, rot) = reader
            .page_media_box(0)
            .expect("MediaBox should be readable");
        // 90° rotation swaps width and height.
        assert!(
            (w - 842.0).abs() < 1.0,
            "rotated width should be ~842, got {w}"
        );
        assert!(
            (h - 595.0).abs() < 1.0,
            "rotated height should be ~595, got {h}"
        );
        assert_eq!(rot, 90);
    }

    #[test]
    fn page_dimensions_uses_mediabox_not_letter_default() {
        let pdf = build_minimal_pdf([0, 0, 595, 842], None);
        let mut reader = PdfReader::from_bytes(&pdf).expect("minimal PDF should open");
        let dims = reader
            .page_dimensions(0)
            .expect("dimensions should be readable");
        assert!(
            dims.height > 800.0,
            "A4 height must be read from MediaBox (~842), got {} (regression to Letter 792?)",
            dims.height
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

    // ---- P4-1 investigation: synthetic-PDF observation of pdf_oxide 0.3.8 ----
    //
    // See docs/refactor/phase4_findings.md for the write-up these tests support.
    // The repo has no real Japanese/scanned/vertical-text PDF fixtures, so these
    // build minimal hand-crafted PDFs (xref-correct, following `build_minimal_pdf`
    // above) to observe pdf_oxide behavior that IS reachable without a real
    // embedded CJK font: ToUnicode-only CID text, an image-only ("scanned-like")
    // page, and page /Rotate handling. Building an actual vertical-writing-mode
    // rendering test is out of reach offline (would need real vertical metrics /
    // a real font), but Identity-V *decoding* can be probed the same way as
    // Identity-H because pdf_oxide treats both through the same CID→ToUnicode path.

    /// Minimal xref-correct multi-object PDF builder for the investigation tests
    /// below. Objects are added in call order (object numbers are 1-based);
    /// `finish()` emits the xref table + trailer. Mirrors `build_minimal_pdf`'s
    /// approach but supports multiple objects and stream objects with a
    /// `/Length` computed from the actual payload.
    struct TestPdfBuilder {
        buf: Vec<u8>,
        offsets: Vec<usize>,
    }

    impl TestPdfBuilder {
        fn new() -> Self {
            let mut buf = Vec::new();
            buf.extend_from_slice(b"%PDF-1.4\n");
            Self {
                buf,
                offsets: Vec::new(),
            }
        }

        fn next_obj_num(&self) -> usize {
            self.offsets.len() + 1
        }

        /// Add a non-stream object (dictionary/array body only, no `N 0 obj` wrapper).
        fn add_obj(&mut self, body: &[u8]) -> usize {
            let num = self.next_obj_num();
            self.offsets.push(self.buf.len());
            self.buf
                .extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            self.buf.extend_from_slice(body);
            self.buf.extend_from_slice(b"\nendobj\n");
            num
        }

        /// Add a stream object. `dict_body` is the dictionary entries (no
        /// surrounding `<<`/`>>`, no `/Length` — that is computed here).
        fn add_stream_obj(&mut self, dict_body: &str, data: &[u8]) -> usize {
            let num = self.next_obj_num();
            self.offsets.push(self.buf.len());
            self.buf
                .extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            self.buf.extend_from_slice(
                format!("<< {dict_body} /Length {} >>\nstream\n", data.len()).as_bytes(),
            );
            self.buf.extend_from_slice(data);
            self.buf.extend_from_slice(b"\nendstream\nendobj\n");
            num
        }

        fn finish(mut self, root_obj: usize) -> Vec<u8> {
            let xref_offset = self.buf.len();
            let size = self.offsets.len() + 1;
            self.buf
                .extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
            self.buf.extend_from_slice(b"0000000000 65535 f \n");
            for off in &self.offsets {
                self.buf
                    .extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
            }
            self.buf.extend_from_slice(
                format!(
                    "trailer\n<< /Size {size} /Root {root_obj} 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n"
                )
                .as_bytes(),
            );
            self.buf
        }
    }

    /// A page with a standard (non-embedded) Type1 font and one `Tj` text
    /// operator. Sanity-checks the harness itself (not a pdf_oxide limitation
    /// probe): confirms `TestPdfBuilder`-produced PDFs open and extract cleanly.
    fn build_text_sanity_pdf() -> Vec<u8> {
        let mut b = TestPdfBuilder::new();
        let catalog = b.add_obj(b"<< /Type /Catalog /Pages 2 0 R >>");
        assert_eq!(catalog, 1);
        b.add_obj(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        b.add_obj(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
              /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        );
        b.add_stream_obj("", b"BT /F1 12 Tf 72 700 Td (Hello World) Tj ET");
        b.add_obj(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
        b.finish(1)
    }

    /// A page with **no text operators**, only a single Image XObject drawn via
    /// `Do` — the minimal shape of a "scanned page" (no embedded text at all).
    /// 2x2 DeviceGray, 8 bits/component, unfiltered raw data (no JPEG/CCITT
    /// decoder needed).
    fn build_image_only_pdf() -> Vec<u8> {
        let mut b = TestPdfBuilder::new();
        let catalog = b.add_obj(b"<< /Type /Catalog /Pages 2 0 R >>");
        assert_eq!(catalog, 1);
        b.add_obj(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        b.add_obj(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
              /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>",
        );
        b.add_stream_obj("", b"q 500 0 0 700 50 50 cm /Im0 Do Q");
        let img_data = [0xFFu8, 0x00, 0xFF, 0x00];
        b.add_stream_obj(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 \
             /BitsPerComponent 8 /ColorSpace /DeviceGray",
            &img_data,
        );
        b.finish(1)
    }

    /// A page whose only text is two CID codes (`<0001>`, `<0002>`) run through
    /// a synthetic `/ToUnicode` CMap mapping them to U+65E5 ("日") and U+672C
    /// ("本"). The descendant `CIDFontType2` has **no embedded `FontFile2`** —
    /// this tests whether pdf_oxide can recover correct Unicode text from the
    /// ToUnicode CMap alone (as claimed in `phase4_extraction.md`'s capability
    /// table), without requiring a real embedded CJK font (infeasible offline).
    ///
    /// `writing_mode` selects `/Identity-H` or `/Identity-V` in `/Encoding` —
    /// used to compare horizontal vs. vertical decoding (see
    /// `identity_v_bbox_matches_identity_h_bbox_ignore` below).
    fn build_cjk_tounicode_pdf(writing_mode: &str) -> Vec<u8> {
        let mut b = TestPdfBuilder::new();
        let catalog = b.add_obj(b"<< /Type /Catalog /Pages 2 0 R >>");
        assert_eq!(catalog, 1);
        b.add_obj(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        b.add_obj(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
              /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        );
        b.add_stream_obj("", b"BT /F1 24 Tf 72 700 Td <00010002> Tj ET");
        let font_dict = format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /FakeCJK /Encoding /{writing_mode} \
             /DescendantFonts [6 0 R] /ToUnicode 8 0 R >>"
        );
        b.add_obj(font_dict.as_bytes());
        b.add_obj(
            b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /FakeCJK \
              /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
              /FontDescriptor 7 0 R /DW 1000 /CIDToGIDMap /Identity >>",
        );
        b.add_obj(
            b"<< /Type /FontDescriptor /FontName /FakeCJK /Flags 4 \
              /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 1000 /Descent 0 \
              /CapHeight 1000 /StemV 80 >>",
        );
        let cmap = "/CIDInit /ProcSet findresource begin\n\
                    12 dict begin\n\
                    begincmap\n\
                    1 begincodespacerange\n\
                    <0000> <FFFF>\n\
                    endcodespacerange\n\
                    2 beginbfchar\n\
                    <0001> <65E5>\n\
                    <0002> <672C>\n\
                    endbfchar\n\
                    endcmap\n\
                    CMapName currentdict /CMapName put\n\
                    end\nend\n";
        b.add_stream_obj("", cmap.as_bytes());
        b.finish(1)
    }

    /// A page with `/Rotate 90` and a single text span, used to check whether
    /// `extract_spans`'s bbox coordinates are adjusted for page rotation.
    fn build_rotated_text_pdf() -> Vec<u8> {
        let mut b = TestPdfBuilder::new();
        let catalog = b.add_obj(b"<< /Type /Catalog /Pages 2 0 R >>");
        assert_eq!(catalog, 1);
        b.add_obj(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        b.add_obj(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Rotate 90 \
              /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        );
        b.add_stream_obj("", b"BT /F1 12 Tf 72 700 Td (Rotated) Tj ET");
        b.add_obj(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
        b.finish(1)
    }

    #[test]
    fn text_sanity_pdf_extracts_via_pdf_lay_wrapper() {
        // Harness sanity check: TestPdfBuilder output opens and extracts through
        // pdf-lay's own PdfReader wrapper (not just raw pdf_oxide).
        let pdf = build_text_sanity_pdf();
        let mut reader = PdfReader::from_bytes(&pdf).expect("sanity PDF should open");
        let spans = reader
            .extract_text_spans(0)
            .expect("extract_text_spans should succeed");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "Hello World");
    }

    #[test]
    fn cjk_tounicode_only_text_decodes_through_pdf_lay_wrapper() {
        // Verifies (via pdf-lay's own PdfReader, not raw pdf_oxide) that CID
        // text with only a ToUnicode CMap — no embedded font program — decodes
        // to correct Unicode. Confirms the capability-table claim "ToUnicode
        // CMap: 対応" end-to-end through the pdf-lay wrapper.
        let pdf = build_cjk_tounicode_pdf("Identity-H");
        let mut reader = PdfReader::from_bytes(&pdf).expect("CJK PDF should open");
        let spans = reader
            .extract_text_spans(0)
            .expect("extract_text_spans should succeed");
        assert_eq!(spans.len(), 1, "expected a single merged CJK span");
        assert_eq!(spans[0].text, "日本");
        assert!(spans[0].bbox.width() > 0.0);
        assert!(spans[0].bbox.height() > 0.0);
    }

    #[test]
    fn identity_v_decodes_text_but_bbox_matches_identity_h_shape() {
        // OBSERVED pdf_oxide 0.3.8 LIMITATION (see phase4_findings.md item 3):
        // Identity-V (vertical writing mode) correctly decodes the same
        // Unicode text as Identity-H via the same ToUnicode/CID path, but the
        // returned bbox has the *same wide/short shape* as the horizontal
        // case — pdf_oxide does not transpose the span geometry for vertical
        // writing mode. `pdf_oxide::layout::TextSpan` has no rotation/writing
        // -mode field pdf-lay could use to detect this itself (per the
        // capability table). This test locks in the current (horizontal-
        // shaped) bbox so a future pdf_oxide upgrade that starts rotating
        // vertical spans will be caught by a test failure here, not silently.
        let h_pdf = build_cjk_tounicode_pdf("Identity-H");
        let v_pdf = build_cjk_tounicode_pdf("Identity-V");

        let mut h_reader = PdfReader::from_bytes(&h_pdf).expect("H PDF should open");
        let h_spans = h_reader.extract_text_spans(0).unwrap();
        let mut v_reader = PdfReader::from_bytes(&v_pdf).expect("V PDF should open");
        let v_spans = v_reader.extract_text_spans(0).unwrap();

        assert_eq!(h_spans.len(), 1);
        assert_eq!(v_spans.len(), 1);
        assert_eq!(
            v_spans[0].text, "日本",
            "Identity-V should still decode text"
        );
        // Same bbox width/height as Identity-H: wide (multiple wide CJK glyphs
        // side-by-side), not tall/narrow as true vertical layout would be.
        assert!(
            v_spans[0].bbox.width() > v_spans[0].bbox.height(),
            "Identity-V span bbox is horizontal-shaped (width {} > height {}), \
             confirming pdf_oxide does not lay out vertical writing mode",
            v_spans[0].bbox.width(),
            v_spans[0].bbox.height()
        );
        assert_eq!(h_spans[0].bbox.width(), v_spans[0].bbox.width());
        assert_eq!(h_spans[0].bbox.height(), v_spans[0].bbox.height());
    }

    #[test]
    fn image_only_page_yields_no_spans_and_no_panic() {
        // Regression guard: a page with zero text operators (the minimal shape
        // of a scanned/image-only page) must not panic anywhere in the
        // pdf_reader extraction path, and must yield zero spans (not garbage).
        // See phase4_findings.md item 5: pdf-lay currently reports NO warning
        // for this case at the pipeline level (tracked for P4-2), so this test
        // only asserts the pdf_reader-level contract (no panic, empty result).
        let pdf = build_image_only_pdf();
        let mut reader = PdfReader::from_bytes(&pdf).expect("image-only PDF should open");
        let spans = reader
            .extract_text_spans(0)
            .expect("extract_text_spans should succeed even with zero text");
        assert!(spans.is_empty(), "image-only page should yield zero spans");
    }

    #[test]
    fn image_only_page_image_is_still_extracted() {
        // Companion to the above: the page's image XObject IS visible via
        // pdf_oxide's extract_images (used by ImageExtractor), even though no
        // text spans exist. Confirms "no text" != "no content" for a
        // scanned-like page, which matters for the OCR-candidate heuristic
        // P4-2 will need (page has images but ~0 native text chars).
        let pdf = build_image_only_pdf();
        let mut reader = PdfReader::from_bytes(&pdf).expect("image-only PDF should open");
        let images = reader
            .inner_doc()
            .extract_images(0)
            .expect("extract_images should succeed");
        assert_eq!(
            images.len(),
            1,
            "the page's single image XObject should be found"
        );
    }

    #[test]
    fn rotated_page_span_bbox_is_not_adjusted_for_rotate_entry() {
        // OBSERVED pdf_oxide 0.3.8 LIMITATION (see phase4_findings.md item 4):
        // `page_media_box` correctly swaps width/height for a /Rotate 90 page
        // (P0-1 behavior), but `extract_spans`'s bbox coordinates are returned
        // in the page's *original* (pre-rotation) coordinate frame. A span
        // whose un-rotated Y-coordinate (700pt) fits inside the un-rotated
        // page height (792pt) therefore ends up *outside* the swapped
        // (rotated) page height (612pt) pdf-lay now reports for that page.
        // This is a real geometric mismatch a downstream layout step would
        // need to correct (out of scope for P4-1 — see findings for options).
        let pdf = build_rotated_text_pdf();
        let mut reader = PdfReader::from_bytes(&pdf).expect("rotated PDF should open");

        let (_w, swapped_height, rotation) = reader
            .page_media_box(0)
            .expect("MediaBox should be readable");
        assert_eq!(rotation, 90);
        assert!((swapped_height - 612.0).abs() < 1.0);

        let spans = reader.extract_text_spans(0).expect("spans should extract");
        assert_eq!(spans.len(), 1);
        // The span's bbox is still expressed in the *un-rotated* frame (top
        // near the original MediaBox's 792pt height), which exceeds the
        // rotated page's reported height (612pt) — the mismatch this test
        // documents.
        assert!(
            spans[0].bbox.top > swapped_height,
            "expected the un-rotated span bbox.top ({}) to exceed the \
             swapped rotated page height ({swapped_height}), demonstrating \
             the coordinate-frame mismatch",
            spans[0].bbox.top
        );
    }
}
