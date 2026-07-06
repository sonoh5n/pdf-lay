//! Coordinate normalization between image bounding boxes and text coordinate space.
//!
//! `pdf_oxide` may report image bounding boxes in a different scale than text spans
//! (e.g., 1/1000 scale, pixel-based, etc.).  This module estimates and applies the
//! scale factor to make image and text coordinates comparable.

use crate::error::PdfLayWarning;
use crate::types::{ImageInfo, PageDimensions, Rect, TextLine};

/// Resolves the scale difference between raw image bboxes and text coordinates.
///
/// Image bounding boxes from `pdf_oxide` may be in a different scale than the
/// text span coordinates.  `CoordinateNormalizer` estimates a scale factor and
/// provides a `normalize` method to apply it.
#[derive(Debug, Clone)]
pub struct CoordinateNormalizer {
    /// Multiplication factor: `normalized_coord = raw_coord * scale_factor`.
    pub scale_factor: f64,
}

impl CoordinateNormalizer {
    /// Create a normalizer with an explicit scale factor.
    pub fn with_scale(scale_factor: f64) -> Self {
        Self { scale_factor }
    }

    /// Estimate the scale factor from images and text on the same page.
    ///
    /// Strategy (tried in order):
    ///
    /// 1. If no images are present on the page → return scale = 1.0 (no-op).
    /// 2. If the widest image's raw bbox width is within 5 % of the page width
    ///    → assume coordinates are already in the same space (scale = 1.0).
    /// 3. Compute `page_width / max_raw_width` and validate it falls in (0.001, 10.0).
    /// 4. Try a set of known PDF-to-screen conversion factors (72, 96, 1000, etc.).
    /// 5. Fall back to scale = 1.0 and emit a `PdfLayWarning::CoordinateFallback`.
    ///
    /// Returns `(normalizer, Option<warning>)`.  The warning is `None` when the
    /// scale could be determined reliably.
    pub fn estimate(
        images: &[ImageInfo],
        _text_lines: &[TextLine],
        page_dims: &PageDimensions,
    ) -> (Self, Option<PdfLayWarning>) {
        // Images with an unknown bbox (P4-3: pdf_oxide reported none, or a
        // degenerate one) carry a zero-size placeholder rect that must not
        // feed into scale estimation — it is not a real measurement.
        let known_images: Vec<&ImageInfo> = images.iter().filter(|i| i.bbox_known).collect();
        if known_images.is_empty() {
            return (Self::with_scale(1.0), None);
        }

        let page_width = page_dims.width;

        let max_raw_width = known_images
            .iter()
            .map(|img| img.raw_bbox.width())
            .fold(0.0_f64, f64::max);

        if max_raw_width > 0.0 {
            let ratio = max_raw_width / page_width;

            // Strategy 2: already in point coordinates (within 20 %)
            if (0.8..=1.2).contains(&ratio) {
                return (Self::with_scale(1.0), None);
            }

            // Strategy 3: direct ratio-based factor
            if ratio > 1.0 {
                let factor = page_width / max_raw_width;
                if (0.001..10.0).contains(&factor) {
                    return (Self::with_scale(factor), None);
                }
            }
        }

        // Strategy 4: probe known PDF-to-screen scaling relationships
        let known_scales: &[f64] = &[1.0, 1.0 / 72.0, 72.0, 1.0 / 96.0, 96.0, 0.001, 1000.0];
        for &scale in known_scales {
            let test_width = max_raw_width * scale;
            if (test_width - page_width).abs() < page_width * 0.2 {
                return (Self::with_scale(scale), None);
            }
        }

        // Strategy 5: fallback with warning
        let warning = PdfLayWarning::CoordinateFallback {
            page: page_dims.page_number,
            scale_used: 1.0,
        };
        (Self::with_scale(1.0), Some(warning))
    }

    /// Apply the scale factor to a raw bounding box.
    ///
    /// All four coordinates (`left`, `top`, `right`, `bottom`) are multiplied
    /// by `scale_factor`.  The resulting `Rect` preserves the `top > bottom`
    /// invariant when `scale_factor > 0`.
    pub fn normalize(&self, raw_bbox: &Rect) -> Rect {
        Rect::new(
            raw_bbox.left * self.scale_factor,
            raw_bbox.top * self.scale_factor,
            raw_bbox.right * self.scale_factor,
            raw_bbox.bottom * self.scale_factor,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::types::ImageFormat;

    fn make_image_with_raw_bbox(raw: Rect) -> ImageInfo {
        ImageInfo {
            path: Some(PathBuf::from("test.png")),
            page: 0,
            raw_bbox: raw.clone(),
            normalized_bbox: raw,
            width_px: 100,
            height_px: 100,
            format: ImageFormat::Png,
            bbox_known: true,
        }
    }

    fn make_page(width: f64, height: f64) -> PageDimensions {
        PageDimensions {
            page_number: 0,
            width,
            height,
        }
    }

    #[test]
    fn scale_1_when_raw_matches_page() {
        // Image raw bbox width ≈ page width → scale = 1.0
        let img = make_image_with_raw_bbox(Rect::new(0.0, 792.0, 612.0, 0.0));
        let page = make_page(612.0, 792.0);
        let (norm, warn) = CoordinateNormalizer::estimate(&[img], &[], &page);
        assert!(
            (norm.scale_factor - 1.0).abs() < 0.01,
            "Expected scale ≈ 1.0, got {}",
            norm.scale_factor
        );
        assert!(warn.is_none(), "Expected no warning when scale matches");
    }

    #[test]
    fn normalize_applies_scale() {
        let norm = CoordinateNormalizer::with_scale(0.5);
        let raw = Rect::new(100.0, 200.0, 300.0, 50.0);
        let result = norm.normalize(&raw);
        assert_eq!(result.left, 50.0);
        assert_eq!(result.top, 100.0);
        assert_eq!(result.right, 150.0);
        assert_eq!(result.bottom, 25.0);
    }

    #[test]
    fn fallback_when_no_images() {
        let page = make_page(612.0, 792.0);
        let (norm, warn) = CoordinateNormalizer::estimate(&[], &[], &page);
        assert_eq!(norm.scale_factor, 1.0);
        assert!(warn.is_none(), "No images → no warning expected");
    }

    #[test]
    fn unknown_bbox_images_are_ignored_not_treated_as_a_scale_measurement() {
        // P4-3: an image with `bbox_known == false` carries a zero-size
        // placeholder rect. If it were treated like real data it would corrupt
        // (or, worse, spuriously trigger `CoordinateFallback` for) a page
        // whose only images have an unknown bbox.
        let mut img = make_image_with_raw_bbox(Rect::new(0.0, 0.0, 0.0, 0.0));
        img.bbox_known = false;
        let page = make_page(612.0, 792.0);
        let (norm, warn) = CoordinateNormalizer::estimate(&[img], &[], &page);
        assert_eq!(norm.scale_factor, 1.0);
        assert!(
            warn.is_none(),
            "an unknown-bbox-only page must not report a coordinate fallback"
        );
    }

    #[test]
    fn normalize_preserves_top_gt_bottom_invariant() {
        let norm = CoordinateNormalizer::with_scale(2.0);
        let raw = Rect::new(10.0, 50.0, 30.0, 20.0);
        let result = norm.normalize(&raw);
        assert!(
            result.top > result.bottom,
            "top ({}) must be > bottom ({})",
            result.top,
            result.bottom
        );
    }

    #[test]
    fn with_scale_stores_factor() {
        let norm = CoordinateNormalizer::with_scale(2.5);
        assert!((norm.scale_factor - 2.5).abs() < f64::EPSILON);
    }
}
