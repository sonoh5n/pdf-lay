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

    /// Scan a page's `/Resources /XObject` dictionary directly (not the
    /// content stream) for Image XObject features that `pdf_oxide::extract_images`
    /// does not surface on its own.
    ///
    /// `pdf_oxide::PdfDocument::extract_images` silently omits any image it
    /// fails to decode from its `Ok(Vec<PdfImage>)` result — an XObject whose
    /// `/Filter` it cannot decode (e.g. `JPXDecode`, which pdf_oxide 0.3.8 has
    /// no decoder for — see `docs/refactor/phase4_findings.md` P4-1) never
    /// appears as an `Err`, it just vanishes with no signal at all. Likewise,
    /// `PdfImage` never exposes whether the image had an `/SMask` (pdf_oxide
    /// does not apply the soft mask, so a transparent image may render with a
    /// solid background — again with no indication in the returned
    /// `PdfImage`). This method reads the dictionary itself (the same
    /// non-rendering dictionary-walk pattern as [`Self::page_media_box`]) so
    /// `ImageExtractor` can at least warn when either gap is present on a
    /// page, even though it cannot recover the specific lost image.
    ///
    /// Best-effort: only inspects Image XObjects referenced directly from the
    /// page's own `/Resources` dictionary. Does not recurse into Form
    /// XObjects or parse inline images (`BI`/`ID`/`EI`) in the content
    /// stream — see `phase4_findings.md` P4-3 for what was and was not
    /// verified. Returns all-`false` hints (rather than an error) when the
    /// page/resources/XObject dictionary cannot be read, since this is a
    /// best-effort diagnostic, not a required extraction step.
    pub(super) fn image_xobject_hints(&mut self, page: u32) -> ImageXObjectHints {
        let mut hints = ImageXObjectHints::default();

        let Ok(page_obj) = self.inner.get_page_for_debug(page as usize) else {
            return hints;
        };
        let Some(page_dict) = page_obj.as_dict() else {
            return hints;
        };
        let Some(resources) = Self::resolve_dict_entry(&mut self.inner, page_dict, "Resources")
        else {
            return hints;
        };
        let Some(res_dict) = resources.as_dict() else {
            return hints;
        };
        let Some(xobjects) = Self::resolve_dict_entry(&mut self.inner, res_dict, "XObject") else {
            return hints;
        };
        let Some(xobj_dict) = xobjects.as_dict() else {
            return hints;
        };

        for entry in xobj_dict.values() {
            let resolved = if let Some(r) = entry.as_reference() {
                match self.inner.load_object(r) {
                    Ok(o) => o,
                    Err(_) => continue,
                }
            } else {
                entry.clone()
            };
            let Some(dict) = resolved.as_dict() else {
                continue;
            };
            if dict.get("Subtype").and_then(|o| o.as_name()) != Some("Image") {
                continue;
            }
            if dict.contains_key("SMask") {
                hints.has_smask = true;
            }
            if let Some(filter) = dict.get("Filter") {
                let is_jpx = filter.as_name() == Some("JPXDecode")
                    || filter
                        .as_array()
                        .map(|arr| arr.iter().any(|o| o.as_name() == Some("JPXDecode")))
                        .unwrap_or(false);
                if is_jpx {
                    hints.has_unsupported_filter = true;
                }
            }
        }

        hints
    }

    /// Resolve a dictionary entry that may be a direct object or an indirect
    /// reference, returning the loaded `Object` in either case.
    fn resolve_dict_entry(
        inner: &mut PdfDocument,
        dict: &std::collections::HashMap<String, Object>,
        key: &str,
    ) -> Option<Object> {
        let entry = dict.get(key)?;
        if let Some(r) = entry.as_reference() {
            inner.load_object(r).ok()
        } else {
            Some(entry.clone())
        }
    }
}

/// Hints about a page's Image XObjects gathered by [`PdfReader::image_xobject_hints`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct ImageXObjectHints {
    /// At least one Image XObject on the page has an `/SMask` entry.
    /// pdf_oxide does not apply soft masks, so the extracted raster may be
    /// missing the intended transparency/alpha.
    pub(super) has_smask: bool,
    /// At least one Image XObject on the page uses a `/Filter` pdf_oxide
    /// cannot decode (currently: `JPXDecode`/JPEG2000). That image is silently
    /// absent from `extract_images`'s result — this is the only way pdf-lay
    /// can detect that a page had an image it lost.
    pub(super) has_unsupported_filter: bool,
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

    // ---- P4-3 investigation: synthetic-PDF observation of pdf_oxide 0.3.8's
    // inline-image / Form-XObject / SMask / JPX handling ----
    //
    // See docs/refactor/phase4_findings.md's "P4-3" section for the write-up
    // these tests support. Builds on the same `TestPdfBuilder` used by P4-1.

    /// A page with a single Image XObject (drawn via `Do`) whose stream has
    /// `/Filter /DCTDecode`. `DctDecoder` (pdf_oxide `src/decoders/dct.rs`) is
    /// a pure byte passthrough — it does not validate JPEG magic bytes — so
    /// arbitrary bytes are sufficient to observe that pdf_oxide tags the
    /// resulting `PdfImage` as `ImageData::Jpeg` end-to-end through a real PDF
    /// (not just via a hand-built `PdfImage`, which `image_extractor.rs`'s own
    /// unit tests already cover for the save/format-selection logic).
    fn build_dct_image_pdf() -> Vec<u8> {
        let mut b = TestPdfBuilder::new();
        let catalog = b.add_obj(b"<< /Type /Catalog /Pages 2 0 R >>");
        assert_eq!(catalog, 1);
        b.add_obj(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        b.add_obj(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
              /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>",
        );
        b.add_stream_obj("", b"q 100 0 0 100 50 50 cm /Im0 Do Q");
        // Not a real decodable JPEG — DctDecoder passes bytes through
        // unchanged, and this test only checks that pdf_oxide *tags* the
        // image as `ImageData::Jpeg`, not that it can be decoded/rendered.
        let dummy_jpeg = b"not-a-real-jpeg-but-DCTDecode-is-a-byte-passthrough";
        b.add_stream_obj(
            "/Type /XObject /Subtype /Image /Width 4 /Height 4 \
             /BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter /DCTDecode",
            dummy_jpeg,
        );
        b.finish(1)
    }

    #[test]
    fn dct_filtered_xobject_image_is_tagged_as_jpeg_source() {
        let pdf = build_dct_image_pdf();
        let mut reader = PdfReader::from_bytes(&pdf).expect("DCT PDF should open");
        let images = reader
            .inner_doc()
            .extract_images(0)
            .expect("extract_images should succeed");
        assert_eq!(images.len(), 1);
        assert!(
            matches!(images[0].data(), pdf_oxide::extractors::ImageData::Jpeg(_)),
            "an Image XObject with /Filter /DCTDecode must be tagged ImageData::Jpeg"
        );
    }

    #[test]
    fn dct_filtered_xobject_image_saves_as_jpg_through_image_extractor() {
        // End-to-end: pdf-lay's own `ImageExtractor` (not just raw pdf_oxide)
        // must save this as `.jpg` losslessly (no re-encode), per P4-3's
        // format-honoring behavior.
        use crate::extract::ImageExtractor;
        let pdf = build_dct_image_pdf();
        let mut reader = PdfReader::from_bytes(&pdf).expect("DCT PDF should open");
        let tmp = tempfile::TempDir::new().unwrap();
        let extractor = ImageExtractor::new(tmp.path().to_path_buf());
        let (images, warnings) = extractor
            .extract_all(&mut reader)
            .expect("extraction should succeed");
        assert_eq!(images.len(), 1);
        assert!(matches!(images[0].format, crate::types::ImageFormat::Jpeg));
        let path = images[0].path.as_ref().unwrap();
        assert_eq!(path.extension().unwrap(), "jpg");
        assert!(warnings.is_empty(), "a clean DCT image should not warn");
    }

    /// 2x2 DeviceGray inline image (unfiltered, raw pixel bytes), in the
    /// dictionary shape the PDF spec actually requires: no `/Subtype` key
    /// inside the `BI`/`ID` dictionary (ISO 32000-1:2008 §8.9.7 — the
    /// abbreviated keys are `W`/`H`/`CS`/`BPC`/…; `Subtype` is XObject
    /// vocabulary, not inline-image vocabulary, and the `BI` operator itself
    /// already says "this is an image"). Real PDF producers do not emit
    /// `/Subtype` here.
    fn build_spec_conformant_inline_image_pdf() -> Vec<u8> {
        let mut b = TestPdfBuilder::new();
        let catalog = b.add_obj(b"<< /Type /Catalog /Pages 2 0 R >>");
        assert_eq!(catalog, 1);
        b.add_obj(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        b.add_obj(b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> /Contents 4 0 R >>");
        let content =
            b"q 100 0 0 100 50 50 cm BI /W 2 /H 2 /CS /DeviceGray /BPC 8 ID \xFF\x00\xFF\x00 EI Q";
        b.add_stream_obj("", content);
        b.finish(1)
    }

    /// Same image, but with a non-conformant `/Subtype /Image` key added
    /// inside the `BI` dictionary (real producers do not write this, but the
    /// PDF spec does not forbid extra keys either).
    fn build_inline_image_pdf_with_redundant_subtype() -> Vec<u8> {
        let mut b = TestPdfBuilder::new();
        let catalog = b.add_obj(b"<< /Type /Catalog /Pages 2 0 R >>");
        assert_eq!(catalog, 1);
        b.add_obj(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        b.add_obj(b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> /Contents 4 0 R >>");
        let content = b"q 100 0 0 100 50 50 cm BI /Subtype /Image /W 2 /H 2 /CS /DeviceGray /BPC 8 ID \xFF\x00\xFF\x00 EI Q";
        b.add_stream_obj("", content);
        b.finish(1)
    }

    #[test]
    fn spec_conformant_inline_image_is_silently_not_extracted() {
        // KEY P4-3 FINDING (see docs/refactor/phase4_findings.md): pdf_oxide
        // 0.3.8's `extract_image_from_inline` (`document.rs:5791`) reuses
        // `extract_image_from_xobject`, which unconditionally requires a
        // `/Subtype /Image` key (`extractors/images.rs` — "XObject missing
        // /Subtype" otherwise). Inline image dictionaries never carry
        // `/Subtype` in spec-conformant PDFs (confirmed by hand — see the
        // sibling test below, which adds a non-conformant `/Subtype /Image`
        // key and DOES get an image back). The failure inside
        // `extract_image_from_inline` is swallowed by `extract_images`'s
        // `if let Ok(image) = ... { images.push(image) }` (document.rs:5527),
        // so a page with a perfectly ordinary inline image silently yields
        // zero images for it — no error, no warning, indistinguishable from a
        // page with no inline image at all. P4-1's source-reading claim
        // ("inline images: 対応") is true only in the narrow, non-standard
        // case; it does not hold for real-world inline images. Deferred (per
        // the design's explicit "do not hand-roll a content-stream image
        // parser" instruction) rather than worked around here.
        let pdf = build_spec_conformant_inline_image_pdf();
        let mut reader = PdfReader::from_bytes(&pdf).expect("inline-image PDF should open");
        let images = reader
            .inner_doc()
            .extract_images(0)
            .expect("extract_images must not error even though the inline image is lost");
        assert!(
            images.is_empty(),
            "a spec-conformant inline image (no /Subtype key) is currently NOT extracted by \
             pdf_oxide 0.3.8 — see the finding note above; if this starts passing, pdf_oxide has \
             fixed the gap and P4-3's inline-image deferral should be revisited"
        );
    }

    #[test]
    fn inline_image_is_extracted_only_with_a_non_conformant_subtype_key() {
        // Companion to the finding above: isolates exactly what makes
        // `extract_image_from_inline` succeed (a redundant `/Subtype /Image`
        // key no real producer writes), so the gap is pinned down precisely
        // rather than asserted as "inline images don't work at all".
        let pdf = build_inline_image_pdf_with_redundant_subtype();
        let mut reader = PdfReader::from_bytes(&pdf).expect("inline-image PDF should open");
        let images = reader
            .inner_doc()
            .extract_images(0)
            .expect("extract_images should succeed");
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].width(), 2);
        assert_eq!(images[0].height(), 2);
    }

    /// A page whose content stream draws a Form XObject (`/Fm0 Do`), and the
    /// Form's *own* `/Resources` contains an Image XObject drawn via its own
    /// nested `Do` operator — the minimal shape needed to observe whether
    /// pdf_oxide's image extraction recurses into Form XObjects.
    fn build_form_xobject_with_image_pdf() -> Vec<u8> {
        let mut b = TestPdfBuilder::new();
        let catalog = b.add_obj(b"<< /Type /Catalog /Pages 2 0 R >>");
        assert_eq!(catalog, 1);
        b.add_obj(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        b.add_obj(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
              /Resources << /XObject << /Fm0 5 0 R >> >> /Contents 4 0 R >>",
        );
        b.add_stream_obj("", b"q 1 0 0 1 0 0 cm /Fm0 Do Q");
        // Form XObject: its content stream draws Im0 (object 6), declared in
        // the Form's *own* /Resources (not the page's).
        b.add_stream_obj(
            "/Type /XObject /Subtype /Form /BBox [0 0 200 200] \
             /Resources << /XObject << /Im0 6 0 R >> >>",
            b"q 100 0 0 100 10 10 cm /Im0 Do Q",
        );
        let img_data = [0xFFu8, 0x00, 0xFF, 0x00];
        b.add_stream_obj(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 \
             /BitsPerComponent 8 /ColorSpace /DeviceGray",
            &img_data,
        );
        b.finish(1)
    }

    #[test]
    fn image_inside_form_xobject_is_extracted_recursively_by_pdf_oxide() {
        // P4-3 kickoff verification (per phase4_findings.md P4-1 §6 "条件付き
        // GO"): confirms pdf_oxide 0.3.8 actually recurses into a Form
        // XObject and finds the image nested in the *Form's own* /Resources
        // (P4-1 only verified this by reading the `document.rs` source, not
        // by running a synthetic PDF through it).
        let pdf = build_form_xobject_with_image_pdf();
        let mut reader = PdfReader::from_bytes(&pdf).expect("Form XObject PDF should open");
        let images = reader
            .inner_doc()
            .extract_images(0)
            .expect("extract_images should succeed");
        assert_eq!(
            images.len(),
            1,
            "an image nested inside a Form XObject's own /Resources should be found"
        );
    }

    /// An Image XObject with `/SMask 6 0 R` pointing to a second (grayscale)
    /// Image XObject used as its soft mask.
    fn build_image_with_smask_pdf() -> Vec<u8> {
        let mut b = TestPdfBuilder::new();
        let catalog = b.add_obj(b"<< /Type /Catalog /Pages 2 0 R >>");
        assert_eq!(catalog, 1);
        b.add_obj(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        b.add_obj(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
              /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>",
        );
        b.add_stream_obj("", b"q 100 0 0 100 50 50 cm /Im0 Do Q");
        let img_data = [
            0x10u8, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x90, 0xA0, 0xB0, 0xC0,
        ];
        b.add_stream_obj(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 \
             /BitsPerComponent 8 /ColorSpace /DeviceRGB /SMask 6 0 R",
            &img_data,
        );
        let mask_data = [0xFFu8, 0x80, 0x40, 0x00];
        b.add_stream_obj(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 \
             /BitsPerComponent 8 /ColorSpace /DeviceGray",
            &mask_data,
        );
        b.finish(1)
    }

    #[test]
    fn image_xobject_hints_detects_smask() {
        // P4-1 established (by reading source) that pdf_oxide never applies
        // or surfaces `/SMask` on the returned `PdfImage`. This confirms, via
        // a synthetic PDF, that `image_xobject_hints` (the dictionary-level
        // workaround added in P4-3) detects its *presence* even though
        // pdf_oxide's own `PdfImage` says nothing about it.
        let pdf = build_image_with_smask_pdf();
        let mut reader = PdfReader::from_bytes(&pdf).expect("SMask PDF should open");

        // Confirm the base image itself still extracts fine (SMask presence
        // does not break extraction, it is just silently unapplied).
        let images = reader
            .inner_doc()
            .extract_images(0)
            .expect("extract_images should succeed");
        assert_eq!(images.len(), 1);

        let hints = reader.image_xobject_hints(0);
        assert!(hints.has_smask, "the SMask entry must be detected");
        assert!(!hints.has_unsupported_filter);
    }

    /// An Image XObject whose sole filter is `/JPXDecode` (JPEG2000), which
    /// pdf_oxide 0.3.8 has no decoder for (confirmed by P4-1: no `jpx`/
    /// `jpeg2000` module under `src/decoders/`).
    fn build_jpx_image_pdf() -> Vec<u8> {
        let mut b = TestPdfBuilder::new();
        let catalog = b.add_obj(b"<< /Type /Catalog /Pages 2 0 R >>");
        assert_eq!(catalog, 1);
        b.add_obj(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        b.add_obj(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
              /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>",
        );
        b.add_stream_obj("", b"q 100 0 0 100 50 50 cm /Im0 Do Q");
        // Not real JPEG2000 codestream data — only the /Filter tag matters
        // for this test (pdf_oxide has no JPX decoder to invoke regardless).
        let jpx_data = b"not-a-real-jpx-codestream";
        b.add_stream_obj(
            "/Type /XObject /Subtype /Image /Width 4 /Height 4 \
             /BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter /JPXDecode",
            jpx_data,
        );
        b.finish(1)
    }

    #[test]
    fn jpx_filtered_image_is_silently_absent_from_extract_images_with_no_error() {
        // KEY P4-3 FINDING: pdf_oxide's `extract_images` does not return an
        // `Err` for a page containing an undecodable image — the image is
        // just missing from the `Ok(Vec<PdfImage>)`, indistinguishable from a
        // page that legitimately has zero images. This is *why*
        // `image_xobject_hints` has to read the Resources/XObject dictionary
        // itself instead of reacting to an error from `extract_images`.
        let pdf = build_jpx_image_pdf();
        let mut reader = PdfReader::from_bytes(&pdf).expect("JPX PDF should open");
        let images = reader
            .inner_doc()
            .extract_images(0)
            .expect("extract_images must not error even though the image is undecodable");
        assert!(
            images.is_empty(),
            "the JPX-filtered image must be silently absent, not present or an Err"
        );
    }

    #[test]
    fn image_xobject_hints_detects_unsupported_jpx_filter() {
        let pdf = build_jpx_image_pdf();
        let mut reader = PdfReader::from_bytes(&pdf).expect("JPX PDF should open");
        let hints = reader.image_xobject_hints(0);
        assert!(
            hints.has_unsupported_filter,
            "a /Filter /JPXDecode Image XObject must be flagged as unsupported"
        );
        assert!(!hints.has_smask);
    }

    #[test]
    fn image_xobject_hints_is_all_false_for_a_plain_image() {
        let pdf = build_image_only_pdf();
        let mut reader = PdfReader::from_bytes(&pdf).expect("plain image PDF should open");
        let hints = reader.image_xobject_hints(0);
        assert!(!hints.has_smask);
        assert!(!hints.has_unsupported_filter);
    }
}
