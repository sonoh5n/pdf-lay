# Task 05: ImageExtractor + CoordinateNormalizer

## Overview

Implement image extraction from PDFs and coordinate normalization. Images are saved as
`p{page:03}_img{num:03}.png` in the configured output directory. The `CoordinateNormalizer`
resolves the scale difference between image bounding boxes (as reported by pdf_oxide) and
the text coordinate space.

If coordinate estimation fails, a warning is emitted and `scale_factor = 1.0` is used as
a safe fallback so analysis can continue.

## Related Information
- **Plan**: `/Users/sonokawa/.claude/plans/fuzzy-drifting-summit.md` (Task 5)
- **Design doc**: `docs/arch/02_DESIGN.md` § 2.2 extract (ImageExtractor, CoordinateNormalizer)
- **Overview**: `docs/develop/overview.md`
- **Dependencies**: Task 02 (types) must be completed; can run in parallel with Tasks 03-04

## Files to Create

- [ ] `crates/pdf-lay-core/src/extract/image_extractor.rs`
- [ ] `crates/pdf-lay-core/src/extract/coordinate.rs`

## Files to Modify

- [ ] `crates/pdf-lay-core/src/extract/mod.rs` — add pub use statements

## Implementation Steps

### Step 1: `extract/coordinate.rs`

```rust
//! Coordinate normalization between image bounding boxes and text coordinate space.
//!
//! pdf_oxide may report image bounding boxes in a different scale than text spans
//! (e.g., 1/1000 scale, pixel-based, etc.). This module estimates and applies the
//! scale factor to make image and text coordinates comparable.

use crate::types::{ImageInfo, PageDimensions, Rect, TextLine};
use crate::error::PdfLayWarning;

/// Resolves the scale difference between raw image bboxes and text coordinates.
#[derive(Debug, Clone)]
pub struct CoordinateNormalizer {
    /// Multiplication factor: normalized_coord = raw_coord * scale_factor.
    pub scale_factor: f64,
}

impl CoordinateNormalizer {
    /// Create a normalizer with a known scale factor.
    pub fn with_scale(scale_factor: f64) -> Self {
        Self { scale_factor }
    }

    /// Estimate the scale factor from images and text on the same page.
    ///
    /// Strategy (tried in order):
    /// 1. If any image's raw bbox width is close to the page width
    ///    (within 5%) → assume scale = 1.0.
    /// 2. Try scale = page_width / max_image_raw_width.
    /// 3. Try known fixed scales: 1/72, 1/96, 1/1000.
    /// 4. Fallback: scale = 1.0 with a warning.
    ///
    /// Returns `(normalizer, Option<warning>)`.
    pub fn estimate(
        images: &[ImageInfo],
        _text_lines: &[TextLine],
        page_dims: &PageDimensions,
    ) -> (Self, Option<PdfLayWarning>) {
        if images.is_empty() {
            return (Self::with_scale(1.0), None);
        }

        let page_width = page_dims.width;

        // Method 1: Check if raw bbox is already in page-point coordinates
        let max_raw_width = images.iter()
            .map(|img| img.raw_bbox.width())
            .fold(0.0_f64, f64::max);

        if max_raw_width > 0.0 {
            let ratio = max_raw_width / page_width;

            // If ratio is between 0.8 and 1.2 → already in point coordinates
            if (0.8..=1.2).contains(&ratio) {
                return (Self::with_scale(1.0), None);
            }

            // Method 2: Direct ratio
            if ratio > 1.0 {
                // Image coords are larger — need to scale down
                let factor = page_width / max_raw_width;
                // Sanity check: factor should be in (0.001, 10.0)
                if (0.001..10.0).contains(&factor) {
                    return (Self::with_scale(factor), None);
                }
            }
        }

        // Method 3: Try known PDF-to-screen scale factors
        let known_scales = [1.0_f64, 1.0 / 72.0, 72.0, 1.0 / 96.0, 96.0, 0.001, 1000.0];
        for &scale in &known_scales {
            let test_width = max_raw_width * scale;
            if (test_width - page_width).abs() < page_width * 0.2 {
                return (Self::with_scale(scale), None);
            }
        }

        // Fallback: use 1.0 and emit a warning
        let warning = PdfLayWarning::CoordinateFallback {
            page: page_dims.page_number,
            scale_used: 1.0,
        };
        (Self::with_scale(1.0), Some(warning))
    }

    /// Apply the scale factor to a raw bounding box.
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

    fn make_image_with_raw_bbox(raw: Rect) -> ImageInfo {
        use std::path::PathBuf;
        use crate::types::{ImageFormat};
        ImageInfo {
            path: PathBuf::from("test.png"),
            page: 0,
            raw_bbox: raw.clone(),
            normalized_bbox: raw,
            width_px: 100,
            height_px: 100,
            format: ImageFormat::Png,
        }
    }

    fn make_page(width: f64, height: f64) -> PageDimensions {
        PageDimensions { page_number: 0, width, height }
    }

    #[test]
    fn scale_1_when_raw_matches_page() {
        // Image raw bbox width ≈ page width → scale = 1.0
        let img = make_image_with_raw_bbox(Rect::new(0.0, 792.0, 612.0, 0.0));
        let page = make_page(612.0, 792.0);
        let (norm, warn) = CoordinateNormalizer::estimate(&[img], &[], &page);
        assert!((norm.scale_factor - 1.0).abs() < 0.01);
        assert!(warn.is_none());
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
        assert!(warn.is_none()); // No images → no warning needed
    }
}
```

### Step 2: `extract/image_extractor.rs`

```rust
//! Extracts embedded images from PDF pages and saves them to disk.

use std::path::{Path, PathBuf};
use crate::error::PdfLayError;
use crate::types::{ImageFormat, ImageInfo, Rect};

/// Extracts images from PDF pages and saves them to an output directory.
pub struct ImageExtractor {
    output_dir: PathBuf,
}

impl ImageExtractor {
    /// Create a new extractor that saves images to `output_dir`.
    ///
    /// The directory is created if it does not exist.
    pub fn new(output_dir: PathBuf) -> Self {
        Self { output_dir }
    }

    /// Extract all images from all pages of the given reader.
    ///
    /// Images are saved as `p{page:03}_img{num:03}.png`.
    /// Errors on individual pages are logged as warnings and skipped.
    pub fn extract_all(
        &self,
        reader: &crate::extract::PdfReader,
    ) -> Result<Vec<ImageInfo>, PdfLayError> {
        // Ensure output directory exists
        std::fs::create_dir_all(&self.output_dir)?;

        let mut all_images = Vec::new();
        for page in 0..reader.page_count() {
            match self.extract_page_images(reader, page) {
                Ok(images) => all_images.extend(images),
                Err(e) => {
                    log::warn!("Image extraction skipped for page {page}: {e}");
                }
            }
        }
        Ok(all_images)
    }

    fn extract_page_images(
        &self,
        reader: &crate::extract::PdfReader,
        page: u32,
    ) -> Result<Vec<ImageInfo>, PdfLayError> {
        // Retrieve raw images from pdf_oxide.
        // Adjust method name based on actual pdf_oxide API:
        let raw_images = reader.inner_doc().extract_images(page as usize)
            .map_err(|e| PdfLayError::ImageExtractionError {
                page,
                reason: e.to_string(),
            })?;

        let mut result = Vec::new();
        for (num, raw_img) in raw_images.into_iter().enumerate() {
            let filename = format!("p{page:03}_img{num:03}.png");
            let path = self.output_dir.join(&filename);

            // Decode and save the image using the `image` crate.
            // pdf_oxide should return image bytes; decode format from headers.
            self.save_image_bytes(&raw_img.data, &path)?;

            // Get pixel dimensions
            let dimensions = image::image_dimensions(&path)?;

            // Build raw bbox from pdf_oxide's reported coordinates.
            // Coordinate normalization happens in the pipeline (Task 13).
            let raw_bbox = Rect::new(
                raw_img.bbox.x_min,
                raw_img.bbox.y_max,
                raw_img.bbox.x_max,
                raw_img.bbox.y_min,
            );

            result.push(ImageInfo {
                path,
                page,
                raw_bbox: raw_bbox.clone(),
                normalized_bbox: raw_bbox, // Will be updated by CoordinateNormalizer
                width_px: dimensions.0,
                height_px: dimensions.1,
                format: ImageFormat::Png,
            });
        }

        Ok(result)
    }

    fn save_image_bytes(&self, data: &[u8], path: &Path) -> Result<(), PdfLayError> {
        // Try to decode as a known format, then save as PNG.
        let img = image::load_from_memory(data)?;
        img.save(path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_extractor() {
        let extractor = ImageExtractor::new(PathBuf::from("/tmp/test_images"));
        assert_eq!(extractor.output_dir, PathBuf::from("/tmp/test_images"));
    }
}
```

### Step 3: Update `extract/mod.rs`

```rust
//! PDF extraction layer — the only module that imports from `pdf_oxide`.

mod coordinate;
mod image_extractor;
mod pdf_reader;
mod span_builder;

pub use coordinate::CoordinateNormalizer;
pub use image_extractor::ImageExtractor;
pub use pdf_reader::PdfReader;
pub use span_builder::SpanBuilder;
```

**Note on `PdfReader::inner_doc()`**: The `ImageExtractor` needs access to the underlying
pdf_oxide document handle. Add a package-private accessor to `PdfReader`:

```rust
// In pdf_reader.rs:
/// Returns a reference to the underlying pdf_oxide document.
/// Only for use within the `extract` module.
pub(super) fn inner_doc(&self) -> &pdf_oxide::PdfDocument {
    &self.inner
}
```

## Acceptance Criteria

- [ ] `cargo test -p pdf-lay-core -- extract::coordinate` all pass:
  - `scale_1_when_raw_matches_page`
  - `normalize_applies_scale`
  - `fallback_when_no_images`
- [ ] `CoordinateNormalizer::normalize` correctly multiplies all four Rect fields by scale_factor
- [ ] `ImageExtractor::new` creates the struct without error
- [ ] `cargo clippy -p pdf-lay-core -- -D warnings` passes
- [ ] When pdf_oxide does not support image extraction, `extract_all` returns `Ok(vec![])` without panicking (add a stub in `extract_page_images` if needed)
- [ ] Image files are named correctly: `p000_img000.png`, `p001_img000.png` etc.

## Dependencies

- Task 02 (types) must be completed first.
- Task 03 (PdfReader) should be completed first (for `inner_doc()` access pattern).
- This task can run in parallel with Tasks 03-04 if the `inner_doc()` accessor is added as a stub.

## Commit Message

```
feat(extract): add ImageExtractor and CoordinateNormalizer with fallback handling
```
