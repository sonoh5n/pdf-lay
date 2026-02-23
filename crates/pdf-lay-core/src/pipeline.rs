//! The main analysis pipeline: Extract → Layout → Structure → Figure → Output.

use std::path::Path;

use crate::{
    config::Config,
    error::{AnalysisResult, PdfLayError, PdfLayWarning},
    extract::{CoordinateNormalizer, ImageExtractor, PdfReader, SpanBuilder},
    figure::{CaptionDetector, CaptionType, ImageMatcher},
    layout::{ColumnDetector, LineReconstructor},
    structure::{BlockClassifier, BlockGrouper, HeaderDetector, SectionBuilder},
    types::{DocumentMetadata, PaperDocument},
};

/// Analyze a PDF file and return a structured `PaperDocument`.
///
/// This function runs the complete pipeline:
/// 1. Extract text spans and (optionally) images.
/// 2. Reconstruct lines and detect column layout.
/// 3. Group blocks, classify types, detect headers.
/// 4. Match captions to images.
/// 5. Build section hierarchy.
///
/// Non-fatal issues are reported as `AnalysisResult::warnings`.
pub fn analyze_pdf(path: &Path, config: &Config) -> Result<AnalysisResult, PdfLayError> {
    let mut warnings: Vec<PdfLayWarning> = Vec::new();

    // ---- Phase 1: Extract ----

    let mut reader = PdfReader::open(path)?;
    let page_count = reader.page_count();

    // Extract all text spans.
    let raw_spans = reader.extract_all_text_spans()?;
    let spans = SpanBuilder::new().merge(raw_spans);

    // Collect page dimensions.
    let mut page_dims_list = Vec::new();
    for page in 0..page_count {
        match reader.page_dimensions(page) {
            Ok(dims) => page_dims_list.push(dims),
            Err(e) => {
                warnings.push(PdfLayWarning::PageSkipped {
                    page,
                    reason: e.to_string(),
                });
            }
        }
    }

    // Extract images (optional).
    let mut images = Vec::new();
    if config.extract_images {
        let extractor = ImageExtractor::new(config.image_output_dir.clone());
        match extractor.extract_all(&mut reader) {
            Ok(imgs) => images = imgs,
            Err(e) => {
                warnings.push(PdfLayWarning::PageSkipped {
                    page: 0,
                    reason: format!("image extraction failed: {e}"),
                });
            }
        }
    }

    // ---- Phase 2: Layout ----

    let line_reconstructor = LineReconstructor::new();
    let lines = line_reconstructor.reconstruct(&spans);

    let column_detector = ColumnDetector::new().with_bin_width(config.column_detection_bin_width);
    let layouts: Vec<_> = page_dims_list
        .iter()
        .map(|dims| column_detector.detect(&lines, dims))
        .collect();

    // ---- Phase 3: Coordinate Normalization ----

    // Estimate a per-page normalizer from all images on that page, then apply
    // the normalizer to each image's raw bbox. We do the estimation once per
    // page (not once per image) so the scale factor is stable.
    let unique_pages: Vec<u32> = {
        let mut pages: Vec<u32> = images.iter().map(|i| i.page).collect();
        pages.sort_unstable();
        pages.dedup();
        pages
    };

    for page_num in unique_pages {
        let page_dims = match page_dims_list.iter().find(|d| d.page_number == page_num) {
            Some(d) => d,
            None => continue,
        };
        let page_lines: Vec<crate::types::TextLine> = lines
            .iter()
            .filter(|l| l.page == page_num)
            .cloned()
            .collect();
        // Collect a read-only snapshot of images on this page for estimation.
        let page_images_snapshot: Vec<_> = images
            .iter()
            .filter(|i| i.page == page_num)
            .cloned()
            .collect();

        let (norm, warn) =
            CoordinateNormalizer::estimate(&page_images_snapshot, &page_lines, page_dims);
        if let Some(w) = warn {
            warnings.push(w);
        }

        // Apply the normalizer to every image on this page.
        for img in images.iter_mut().filter(|i| i.page == page_num) {
            img.normalized_bbox = norm.normalize(&img.raw_bbox);
        }
    }

    // ---- Phase 4: Structure ----

    let mut blocks = BlockGrouper::new()
        .with_gap_multiplier(config.block_gap_multiplier)
        .group(&lines, &layouts);

    let classifier = BlockClassifier::from_blocks(&blocks);
    classifier.classify_all(&mut blocks);

    let headers = HeaderDetector::with_config(
        classifier.body_font_size,
        config.header_detection.min_score,
        config.header_detection.max_chars,
        config.header_detection.max_lines,
    )
    .detect(&blocks);

    // ---- Phase 5: Figure Matching ----

    let caption_detector = CaptionDetector::new();
    let captions = caption_detector.detect(&blocks);

    let image_matcher = ImageMatcher::new().with_max_gap(config.caption_max_gap_pt);
    let figures = image_matcher.match_all(&captions, &images, &blocks);

    // Warn about unmatched figure captions.
    let matched_caption_texts: std::collections::HashSet<String> =
        figures.iter().map(|f| f.caption_text.clone()).collect();

    for caption in &captions {
        if caption.caption_type == CaptionType::Figure
            && !matched_caption_texts.contains(&caption.full_text)
        {
            warnings.push(PdfLayWarning::UnmatchedCaption {
                caption: caption.full_text.clone(),
                page: caption.page,
            });
        }
    }

    // ---- Phase 6: Section Assembly ----

    let sections = SectionBuilder::build(blocks, &headers, figures, vec![], &layouts);

    // ---- Assembly ----

    let paper_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let all_figures: Vec<_> = collect_all_figures(&sections);

    let all_tables: Vec<_> = collect_all_tables(&sections);

    let document = PaperDocument {
        paper_id,
        source_file: path.to_path_buf(),
        metadata: DocumentMetadata {
            pages: page_count,
            ..Default::default()
        },
        sections,
        all_figures,
        all_tables,
    };

    Ok(AnalysisResult { document, warnings })
}

/// Recursively collect all figures from sections and their children.
fn collect_all_figures(sections: &[crate::types::text::Section]) -> Vec<crate::types::FigureInfo> {
    let mut result = Vec::new();
    for section in sections {
        result.extend(section.figures.iter().cloned());
        result.extend(collect_all_figures(&section.children));
    }
    result
}

/// Recursively collect all tables from sections and their children.
fn collect_all_tables(sections: &[crate::types::text::Section]) -> Vec<crate::types::TableInfo> {
    let mut result = Vec::new();
    for section in sections {
        result.extend(section.tables.iter().cloned());
        result.extend(collect_all_tables(&section.children));
    }
    result
}

/// Analyze PDF from bytes (for use by Python bindings or in-memory workflows).
///
/// Writes the bytes to a temporary file then delegates to [`analyze_pdf`].
pub fn analyze_pdf_bytes(bytes: &[u8], config: &Config) -> Result<AnalysisResult, PdfLayError> {
    use std::io::Write as _;
    let mut tmp = tempfile::NamedTempFile::new()?;
    tmp.write_all(bytes)?;
    tmp.flush()?;
    analyze_pdf(tmp.path(), config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyze_pdf_returns_file_not_found_for_nonexistent_path() {
        let config = Config::default();
        let result = analyze_pdf(Path::new("/nonexistent/does/not/exist.pdf"), &config);
        assert!(
            matches!(result, Err(PdfLayError::FileNotFound(_))),
            "Expected FileNotFound, got: {result:?}"
        );
    }

    #[test]
    #[ignore = "requires tests/fixtures/sample.pdf"]
    fn smoke_test_ieee_paper() {
        let config = Config {
            extract_images: false,
            ..Default::default()
        };
        let result = analyze_pdf(Path::new("tests/fixtures/sample.pdf"), &config)
            .expect("analysis should succeed");
        let doc = result.document;
        assert!(doc.metadata.pages > 0);
        assert!(!doc.sections.is_empty(), "Expected at least one section");
        println!("Pages: {}", doc.metadata.pages);
        println!("Sections: {}", doc.sections.len());
        for s in &doc.sections {
            println!("  [L{}] {}", s.level, s.header_text());
        }
    }
}
