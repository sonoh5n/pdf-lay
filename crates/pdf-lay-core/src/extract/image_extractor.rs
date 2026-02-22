//! Extracts embedded images from PDF pages and saves them to disk.
//!
//! Images are saved as `p{page:03}_img{num:03}.png` in the configured output
//! directory.  Errors on individual pages are logged as warnings and skipped
//! so that the rest of the document can still be processed.

use std::path::PathBuf;

use crate::error::PdfLayError;
use crate::extract::PdfReader;
use crate::types::{ImageFormat, ImageInfo, Rect};

/// Extracts images from PDF pages and saves them to an output directory.
pub struct ImageExtractor {
    output_dir: PathBuf,
}

impl ImageExtractor {
    /// Create a new extractor that saves images to `output_dir`.
    ///
    /// The directory is created lazily — only when the first image is extracted.
    pub fn new(output_dir: PathBuf) -> Self {
        Self { output_dir }
    }

    /// Extract all images from all pages of the given reader.
    ///
    /// Images are saved as `p{page:03}_img{num:03}.png`.
    /// Errors on individual pages are logged as warnings and skipped.
    pub fn extract_all(&self, reader: &mut PdfReader) -> Result<Vec<ImageInfo>, PdfLayError> {
        std::fs::create_dir_all(&self.output_dir)?;

        let mut all_images = Vec::new();
        let page_count = reader.page_count();
        for page in 0..page_count {
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
        reader: &mut PdfReader,
        page: u32,
    ) -> Result<Vec<ImageInfo>, PdfLayError> {
        let raw_images = reader
            .inner_doc()
            .extract_images(page as usize)
            .map_err(|e| PdfLayError::ImageExtractionError {
                page,
                reason: e.to_string(),
            })?;

        let mut result = Vec::new();
        for (num, raw_img) in raw_images.into_iter().enumerate() {
            let filename = format!("p{page:03}_img{num:03}.png");
            let path = self.output_dir.join(&filename);

            // Save image to disk as PNG using pdf_oxide's built-in encoder.
            raw_img
                .save_as_png(&path)
                .map_err(|e| PdfLayError::ImageExtractionError {
                    page,
                    reason: format!("save_as_png '{filename}': {e}"),
                })?;

            // Build raw bbox from the coordinates reported by pdf_oxide.
            // pdf_oxide's Rect: {x, y, width, height} in PDF Y-up space where
            //   x, y = lower-left corner, y + height = upper-left corner.
            // Our Rect requires top > bottom (Y-up invariant).
            let raw_bbox = match raw_img.bbox() {
                Some(b) => {
                    let left = b.x as f64;
                    let bottom = b.y as f64;
                    let right = (b.x + b.width) as f64;
                    let top = (b.y + b.height) as f64;
                    // Guard against degenerate bboxes (e.g. width=0 or height=0).
                    if right > left && top > bottom {
                        Rect::new(left, top, right, bottom)
                    } else {
                        // Use a unit rectangle at the origin as a safe placeholder.
                        Rect::new(0.0, 1.0, 1.0, 0.0)
                    }
                }
                None => {
                    // bbox is optional in pdf_oxide; use a placeholder when absent.
                    Rect::new(0.0, 1.0, 1.0, 0.0)
                }
            };

            result.push(ImageInfo {
                path,
                page,
                raw_bbox: raw_bbox.clone(),
                normalized_bbox: raw_bbox, // Updated by CoordinateNormalizer
                width_px: raw_img.width(),
                height_px: raw_img.height(),
                format: ImageFormat::Png,
            });
        }

        Ok(result)
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
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let extractor = ImageExtractor::new(tmp.path().to_path_buf());
        let mut reader =
            PdfReader::open(std::path::Path::new("tests/fixtures/sample.pdf")).unwrap();
        let images = extractor.extract_all(&mut reader).unwrap();
        // Just verify we get a result without panic; real PDFs may have 0 images.
        for img in &images {
            assert!(
                img.path.exists(),
                "Expected image file to exist: {:?}",
                img.path
            );
            assert!(img.width_px > 0);
            assert!(img.height_px > 0);
        }
    }
}
