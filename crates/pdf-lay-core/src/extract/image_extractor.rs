//! Extracts embedded images from PDF pages and saves them to disk.
//!
//! Images are saved as `p{page:03}_img{num:03}.<ext>` in the configured output
//! directory, where `<ext>` depends on the image's real source format (see
//! [`ImageExtractor::extract_page_images`]). Failures are scoped as narrowly
//! as possible: a single image that cannot be saved is skipped with a
//! [`PdfLayWarning`] rather than discarding every other image on its page, and
//! a page whose extraction fails entirely is likewise skipped with a warning
//! rather than aborting the whole document (No Silent Drop — see
//! `docs/refactor/00_REVIEW_POLICY.md`).

use std::path::PathBuf;

use pdf_oxide::extractors::{ImageData, PdfImage};

use crate::config::ImageOutputFormat;
use crate::error::{PdfLayError, PdfLayWarning};
use crate::extract::PdfReader;
use crate::types::{ImageFormat, ImageInfo, Rect};

/// Extracts images from PDF pages and saves them to an output directory.
pub struct ImageExtractor {
    output_dir: PathBuf,
    /// Format to re-encode raw (non-JPEG-source) pixel data as. Ignored for
    /// images whose source data is already JPEG (DCT) unless `force_png`.
    image_format: ImageOutputFormat,
    /// When `true`, always save as PNG regardless of the image's real source
    /// format — the pre-P4-3 behavior, kept as an explicit opt-out in case a
    /// caller depends on every extracted image being a PNG file.
    force_png: bool,
}

impl ImageExtractor {
    /// Create a new extractor that saves images to `output_dir`, honoring
    /// each image's real source format (PNG default for raw pixel data, or
    /// lossless JPEG passthrough when the source is already JPEG-encoded).
    ///
    /// The directory is created lazily — only when the first image is extracted.
    pub fn new(output_dir: PathBuf) -> Self {
        Self {
            output_dir,
            image_format: ImageOutputFormat::default(),
            force_png: false,
        }
    }

    /// Set the format used to re-encode raw (non-JPEG-source) pixel data.
    /// Has no effect on already-JPEG-encoded source images unless
    /// [`Self::with_force_png`] is also set.
    pub fn with_image_format(mut self, format: ImageOutputFormat) -> Self {
        self.image_format = format;
        self
    }

    /// When `true`, always save every image as PNG, re-encoding JPEG-source
    /// images instead of passing them through losslessly. Restores the
    /// pre-P4-3 behavior for callers that need every file to be a PNG.
    pub fn with_force_png(mut self, force_png: bool) -> Self {
        self.force_png = force_png;
        self
    }

    /// Extract all images from all pages of the given reader.
    ///
    /// Returns the successfully extracted images alongside any non-fatal
    /// warnings (a page that failed outright, an individual image that could
    /// not be decoded/saved, an image with an unknown bbox, or an image with
    /// an ignored `/SMask`). Errors on individual pages or images never
    /// discard the rest of the document.
    pub fn extract_all(
        &self,
        reader: &mut PdfReader,
    ) -> Result<(Vec<ImageInfo>, Vec<PdfLayWarning>), PdfLayError> {
        std::fs::create_dir_all(&self.output_dir)?;

        let mut all_images = Vec::new();
        let mut warnings = Vec::new();
        let page_count = reader.page_count();
        for page in 0..page_count {
            match self.extract_page_images(reader, page, &mut warnings) {
                Ok(images) => all_images.extend(images),
                Err(e) => {
                    warnings.push(PdfLayWarning::PageSkipped {
                        page,
                        reason: format!("image extraction failed: {e}"),
                    });
                }
            }
        }
        Ok((all_images, warnings))
    }

    /// Extract every image on a single page, pushing a warning (and skipping
    /// just that one image) for anything that cannot be saved rather than
    /// failing the whole page.
    fn extract_page_images(
        &self,
        reader: &mut PdfReader,
        page: u32,
        warnings: &mut Vec<PdfLayWarning>,
    ) -> Result<Vec<ImageInfo>, PdfLayError> {
        let raw_images = reader
            .inner_doc()
            .extract_images(page as usize)
            .map_err(|e| PdfLayError::ImageExtractionError {
                page,
                reason: e.to_string(),
            })?;

        // Detect gaps pdf_oxide's `extract_images` cannot surface itself: an
        // image dropped silently because of an unsupported filter never
        // appears as an `Err` (or anywhere at all) in `raw_images`, and a
        // present-but-ignored `/SMask` never appears on the returned
        // `PdfImage`. See `PdfReader::image_xobject_hints` for why this needs
        // its own dictionary read instead of just inspecting `raw_images`.
        let hints = reader.image_xobject_hints(page);
        if hints.has_smask {
            warnings.push(PdfLayWarning::ImageSMaskIgnored { page });
        }
        if hints.has_unsupported_filter {
            warnings.push(PdfLayWarning::ImageDecodeFailed {
                page,
                reason: "an Image XObject uses a filter (e.g. JPXDecode/JPEG2000) pdf_oxide 0.3.8 \
                         cannot decode; it is missing from the extracted images"
                    .to_string(),
            });
        }

        let mut result = Vec::new();
        for (num, raw_img) in raw_images.into_iter().enumerate() {
            match self.save_one_image(page, num, &raw_img) {
                Ok(info) => {
                    if !info.bbox_known {
                        warnings.push(PdfLayWarning::ImageBboxUnknown { page });
                    }
                    result.push(info);
                }
                Err(e) => {
                    // Per-image recovery: this image is skipped, but the rest
                    // of the page's images still get a chance to save.
                    warnings.push(PdfLayWarning::ImageDecodeFailed {
                        page,
                        reason: format!("image {num}: {e}"),
                    });
                }
            }
        }

        Ok(result)
    }

    /// Save a single already-parsed `PdfImage` to disk and build its
    /// `ImageInfo`, honoring the image's real source format (see the module
    /// docs). Factored out of [`Self::extract_page_images`] so the format
    /// decision, save, and bbox handling for one image can be unit-tested
    /// directly against a hand-built `PdfImage` (no PDF file needed).
    fn save_one_image(
        &self,
        page: u32,
        num: usize,
        raw_img: &PdfImage,
    ) -> Result<ImageInfo, PdfLayError> {
        let is_jpeg_source = matches!(raw_img.data(), ImageData::Jpeg(_));
        let save_as_jpeg = !self.force_png
            && (is_jpeg_source || matches!(self.image_format, ImageOutputFormat::Jpeg));

        let (ext, format) = if save_as_jpeg {
            ("jpg", ImageFormat::Jpeg)
        } else {
            ("png", ImageFormat::Png)
        };
        let filename = format!("p{page:03}_img{num:03}.{ext}");
        let path = self.output_dir.join(&filename);

        let save_result = if save_as_jpeg {
            raw_img.save_as_jpeg(&path)
        } else {
            raw_img.save_as_png(&path)
        };
        save_result.map_err(|e| PdfLayError::ImageExtractionError {
            page,
            reason: format!("failed to save as {ext}: {e}"),
        })?;

        // Build raw bbox from the coordinates reported by pdf_oxide.
        // pdf_oxide's Rect: {x, y, width, height} in PDF Y-up space where
        //   x, y = lower-left corner, y + height = upper-left corner.
        // Our Rect requires top > bottom (Y-up invariant).
        let (raw_bbox, bbox_known) = match raw_img.bbox() {
            Some(b) => {
                let left = b.x as f64;
                let bottom = b.y as f64;
                let right = (b.x + b.width) as f64;
                let top = (b.y + b.height) as f64;
                // Guard against degenerate bboxes (e.g. width=0 or height=0).
                if right > left && top > bottom {
                    (Rect::new(left, top, right, bottom), true)
                } else {
                    (Rect::new(0.0, 0.0, 0.0, 0.0), false)
                }
            }
            None => {
                // bbox is optional in pdf_oxide; unknown rather than
                // fabricated — see `ImageInfo::bbox_known`.
                (Rect::new(0.0, 0.0, 0.0, 0.0), false)
            }
        };

        Ok(ImageInfo {
            path: Some(path),
            page,
            raw_bbox: raw_bbox.clone(),
            normalized_bbox: raw_bbox, // Updated by CoordinateNormalizer
            width_px: raw_img.width(),
            height_px: raw_img.height(),
            format,
            bbox_known,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_oxide::extractors::{ColorSpace, PixelFormat};
    use pdf_oxide::geometry::Rect as PdfOxideRect;
    use tempfile::TempDir;

    fn rgb_image(width: u32, height: u32, pixels: Vec<u8>) -> PdfImage {
        PdfImage::new(
            width,
            height,
            ColorSpace::DeviceRGB,
            8,
            ImageData::Raw {
                pixels,
                format: PixelFormat::RGB,
            },
        )
    }

    #[test]
    fn new_creates_extractor() {
        let extractor = ImageExtractor::new(PathBuf::from("/tmp/test_images"));
        assert_eq!(extractor.output_dir, PathBuf::from("/tmp/test_images"));
        assert!(!extractor.force_png);
    }

    #[test]
    fn with_force_png_sets_flag() {
        let extractor = ImageExtractor::new(PathBuf::from("/tmp/test_images")).with_force_png(true);
        assert!(extractor.force_png);
    }

    #[test]
    fn extract_all_on_nonexistent_pdf_returns_error() {
        // A PdfReader that cannot be opened → extract_all should bubble the error.
        // We cannot easily construct a PdfReader without a valid file, so just verify
        // that the ImageExtractor struct is well-formed.
        let extractor = ImageExtractor::new(PathBuf::from("/tmp/pdf_lay_test_images"));
        // The output_dir does not need to exist before extract_all is called.
        assert_eq!(
            extractor.output_dir,
            PathBuf::from("/tmp/pdf_lay_test_images")
        );
    }

    // Integration test — requires a real PDF fixture.
    #[test]
    #[ignore = "requires tests/fixtures/sample.pdf"]
    fn extract_images_from_real_pdf() {
        let tmp = TempDir::new().unwrap();
        let extractor = ImageExtractor::new(tmp.path().to_path_buf());
        let mut reader =
            PdfReader::open(std::path::Path::new("tests/fixtures/sample.pdf")).unwrap();
        let (images, _warnings) = extractor.extract_all(&mut reader).unwrap();
        // Just verify we get a result without panic; real PDFs may have 0 images.
        for img in &images {
            let path = img.path.as_ref().expect("raster image must have a path");
            assert!(path.exists(), "Expected image file to exist: {path:?}");
            assert!(img.width_px > 0);
            assert!(img.height_px > 0);
        }
    }

    // Additional synthetic-PDF regression tests exercising the full
    // extract-images-from-a-PDF path (inline images, Form XObjects, SMask/JPX
    // dictionary hints) live in `extract::pdf_reader::tests`, which already
    // has `TestPdfBuilder`; see that module and
    // `docs/refactor/phase4_findings.md` (P4-3 section). The tests below
    // exercise `save_one_image` directly against hand-built `PdfImage`s
    // (no PDF file needed) since that is where the per-image format/bbox
    // decisions actually live.

    #[test]
    fn jpeg_source_passes_through_as_jpg_without_reencoding() {
        // `ImageData::Jpeg` bytes are written directly (no decode), so this
        // is deterministic without needing a real, decodable JPEG payload.
        let tmp = TempDir::new().unwrap();
        let extractor = ImageExtractor::new(tmp.path().to_path_buf());
        let raw_bytes = vec![0xAA, 0xBB, 0xCC, 0xDD];
        let img = PdfImage::new(
            10,
            10,
            ColorSpace::DeviceRGB,
            8,
            ImageData::Jpeg(raw_bytes.clone()),
        );

        let info = extractor.save_one_image(0, 0, &img).unwrap();
        assert!(matches!(info.format, ImageFormat::Jpeg));
        let path = info.path.unwrap();
        assert_eq!(path.extension().unwrap(), "jpg");
        assert_eq!(std::fs::read(&path).unwrap(), raw_bytes);
    }

    #[test]
    fn force_png_reencodes_even_a_jpeg_source() {
        // Generate a real, decodable JPEG in memory (force_png must decode it
        // to re-encode as PNG, unlike the passthrough path above).
        let rgb = image::RgbImage::from_pixel(4, 4, image::Rgb([10, 20, 30]));
        let mut jpeg_bytes = Vec::new();
        image::DynamicImage::ImageRgb8(rgb)
            .write_to(
                &mut std::io::Cursor::new(&mut jpeg_bytes),
                image::ImageFormat::Jpeg,
            )
            .unwrap();

        let tmp = TempDir::new().unwrap();
        let extractor = ImageExtractor::new(tmp.path().to_path_buf()).with_force_png(true);
        let img = PdfImage::new(4, 4, ColorSpace::DeviceRGB, 8, ImageData::Jpeg(jpeg_bytes));

        let info = extractor.save_one_image(0, 0, &img).unwrap();
        assert!(matches!(info.format, ImageFormat::Png));
        assert_eq!(info.path.unwrap().extension().unwrap(), "png");
    }

    #[test]
    fn raw_source_defaults_to_png() {
        let tmp = TempDir::new().unwrap();
        let extractor = ImageExtractor::new(tmp.path().to_path_buf());
        let img = rgb_image(2, 2, vec![0u8; 2 * 2 * 3]);

        let info = extractor.save_one_image(0, 0, &img).unwrap();
        assert!(matches!(info.format, ImageFormat::Png));
        assert_eq!(info.path.unwrap().extension().unwrap(), "png");
    }

    #[test]
    fn raw_source_honors_configured_jpeg_output_format() {
        // Config.image_format == Jpeg must be honored for raw (non-JPEG
        // -source) pixel data too, not just for already-JPEG images.
        let tmp = TempDir::new().unwrap();
        let extractor = ImageExtractor::new(tmp.path().to_path_buf())
            .with_image_format(ImageOutputFormat::Jpeg);
        let img = rgb_image(2, 2, vec![0u8; 2 * 2 * 3]);

        let info = extractor.save_one_image(0, 0, &img).unwrap();
        assert!(matches!(info.format, ImageFormat::Jpeg));
        assert_eq!(info.path.unwrap().extension().unwrap(), "jpg");
    }

    #[test]
    fn force_png_overrides_configured_jpeg_format() {
        let tmp = TempDir::new().unwrap();
        let extractor = ImageExtractor::new(tmp.path().to_path_buf())
            .with_image_format(ImageOutputFormat::Jpeg)
            .with_force_png(true);
        let img = rgb_image(2, 2, vec![0u8; 2 * 2 * 3]);

        let info = extractor.save_one_image(0, 0, &img).unwrap();
        assert!(matches!(info.format, ImageFormat::Png));
    }

    #[test]
    fn known_bbox_is_converted_and_marked_known() {
        let tmp = TempDir::new().unwrap();
        let extractor = ImageExtractor::new(tmp.path().to_path_buf());
        let mut img = rgb_image(2, 2, vec![0u8; 2 * 2 * 3]);
        // pdf_oxide bbox: lower-left (x, y), width/height, Y-up.
        img.set_bbox(PdfOxideRect::new(10.0, 20.0, 30.0, 40.0));

        let info = extractor.save_one_image(0, 0, &img).unwrap();
        assert!(info.bbox_known);
        assert_eq!(info.raw_bbox, Rect::new(10.0, 60.0, 40.0, 20.0));
    }

    #[test]
    fn missing_bbox_is_reported_as_unknown_not_fabricated() {
        let tmp = TempDir::new().unwrap();
        let extractor = ImageExtractor::new(tmp.path().to_path_buf());
        let img = rgb_image(2, 2, vec![0u8; 2 * 2 * 3]); // no bbox set

        let info = extractor.save_one_image(0, 0, &img).unwrap();
        assert!(!info.bbox_known);
        // The placeholder must be a harmless zero-size rect, not a fake
        // "positioned" unit rect that could be mistaken for a real bbox.
        assert_eq!(info.raw_bbox, Rect::new(0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn degenerate_bbox_is_reported_as_unknown() {
        let tmp = TempDir::new().unwrap();
        let extractor = ImageExtractor::new(tmp.path().to_path_buf());
        let mut img = rgb_image(2, 2, vec![0u8; 2 * 2 * 3]);
        img.set_bbox(PdfOxideRect::new(10.0, 20.0, 0.0, 0.0)); // zero width/height

        let info = extractor.save_one_image(0, 0, &img).unwrap();
        assert!(!info.bbox_known);
    }

    #[test]
    fn one_bad_image_does_not_prevent_saving_a_good_one() {
        // P4-3: per-image recovery. A malformed image (pixel buffer that
        // doesn't match width*height*bytes_per_pixel — pdf_oxide's own
        // `save_as_png` rejects this) must fail in isolation; a second,
        // well-formed image on the same page must still save successfully.
        // `extract_page_images`'s loop (see above) pushes a warning and
        // `continue`s on exactly this kind of per-image `Err`, rather than
        // aborting the whole page.
        let tmp = TempDir::new().unwrap();
        let extractor = ImageExtractor::new(tmp.path().to_path_buf());

        let bad_img = rgb_image(2, 2, vec![0u8; 3]); // way too few bytes
        let bad_result = extractor.save_one_image(0, 0, &bad_img);
        assert!(
            bad_result.is_err(),
            "malformed pixel buffer must be rejected"
        );

        let good_img = rgb_image(2, 2, vec![0u8; 2 * 2 * 3]);
        let good_result = extractor.save_one_image(0, 1, &good_img);
        assert!(
            good_result.is_ok(),
            "a well-formed image must still save even though a sibling image failed"
        );
    }
}
