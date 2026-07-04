//! The main analysis pipeline: Extract → Layout → Structure → Figure → Output.

use std::path::Path;

use crate::error::Coverage;
use crate::{
    config::Config,
    error::{AnalysisResult, PdfLayError, PdfLayWarning},
    extract::{CoordinateNormalizer, ImageExtractor, PdfReader, SpanBuilder},
    figure::{CaptionDetector, CaptionInfo, CaptionType, ImageMatcher},
    layout::{ColumnDetector, LineReconstructor},
    structure::{BlockClassifier, BlockGrouper, HeaderDetector, MetadataExtractor, SectionBuilder},
    table::{GridBuilder, TableDetector, TableRegion, TableTextConverter},
    types::{BlockType, InsertionPoint, PaperDocument, Section, TableInfo, TextBlock},
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

    // ---- Resource limit checks ----

    let file_size = std::fs::metadata(path)
        .map_err(|_| PdfLayError::FileNotFound(path.to_path_buf()))?
        .len();
    if file_size > config.resource_limits.max_file_size {
        return Err(PdfLayError::ResourceLimitExceeded {
            limit: format!(
                "max file size {} bytes",
                config.resource_limits.max_file_size
            ),
            actual: format!("{file_size} bytes"),
        });
    }

    let mut reader = PdfReader::open(path)?;
    let page_count = reader.page_count();

    if page_count > config.resource_limits.max_pages {
        return Err(PdfLayError::ResourceLimitExceeded {
            limit: format!("max pages {}", config.resource_limits.max_pages),
            actual: format!("{page_count} pages"),
        });
    }

    // Extract all text spans.
    let raw_spans = reader.extract_all_text_spans()?;
    let spans = SpanBuilder::new().merge(raw_spans);

    // Coverage baseline: total characters extracted from the PDF.
    let extracted_chars: usize = spans.iter().map(|s| s.text.chars().count()).sum();

    // Collect page dimensions. Prefer the real MediaBox; when it cannot be read,
    // derive the page extent from that page's spans so no on-page text falls
    // outside the layout bounds (No Silent Drop), and only then fall back to a
    // Letter-size default. Both fallbacks are reported as warnings.
    let mut page_dims_list = Vec::new();
    for page in 0..page_count {
        let dims = match reader.page_media_box(page) {
            Some((width, height, _rotation)) => crate::types::PageDimensions {
                page_number: page,
                width,
                height,
            },
            None => {
                let (mut width, mut height) = (0.0_f64, 0.0_f64);
                for s in spans.iter().filter(|s| s.page == page) {
                    width = width.max(s.bbox.right);
                    height = height.max(s.bbox.top);
                }
                if width > 0.0 && height > 0.0 {
                    warnings.push(PdfLayWarning::PageDimensionsFallback {
                        page,
                        method: "span-bbox",
                    });
                    crate::types::PageDimensions {
                        page_number: page,
                        width,
                        height,
                    }
                } else {
                    warnings.push(PdfLayWarning::PageDimensionsFallback {
                        page,
                        method: "letter-default",
                    });
                    crate::types::PageDimensions {
                        page_number: page,
                        width: 612.0,
                        height: 792.0,
                    }
                }
            }
        };
        page_dims_list.push(dims);
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

    let classifier = BlockClassifier::from_blocks(&blocks)
        .with_limits(config.caption_max_chars, config.running_header_max_chars);
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

    // ---- Phase 5.5: Table Detection ----
    // Must happen before Section Assembly because SectionBuilder::build consumes `blocks`.

    let tables = if config.detect_tables {
        let paths = reader.extract_all_paths()?;
        let table_detector = TableDetector::new(config.table_config.clone());
        let table_captions: Vec<&CaptionInfo> = captions
            .iter()
            .filter(|c| c.caption_type == CaptionType::Table)
            .collect();
        let regions = table_detector.detect(&blocks, &paths, &table_captions);

        let mut table_infos = Vec::new();
        for region in &regions {
            let grid = GridBuilder::build(
                &region.block_indices,
                &blocks,
                &region.bbox,
                region.has_rules,
            );
            let repr = TableTextConverter::to_markdown(
                &grid,
                region.caption.as_ref().map(|c| c.full_text.as_str()),
            );

            let table_number = region.caption.as_ref().and_then(|c| c.number);
            let table_id = format!("Table {}", table_number.unwrap_or(0));

            table_infos.push(TableInfo {
                table_id,
                table_number,
                caption: region.caption.as_ref().map(|c| c.full_text.clone()),
                representation: repr,
                insertion_point: determine_table_insertion(region, &blocks),
                page: region.page,
            });
        }
        table_infos
    } else {
        vec![]
    };

    // ---- Metadata Extraction ----
    // Must happen before Section Assembly because SectionBuilder::build consumes `blocks`.

    let metadata = MetadataExtractor::extract(&blocks, page_count);

    // ---- Phase 6: Section Assembly ----

    // Coverage: count blocks that will be dropped from body text by the
    // renderer (they are represented elsewhere or intentionally excluded).
    let dropped_blocks = blocks
        .iter()
        .filter(|b| {
            matches!(
                b.block_type,
                BlockType::Caption
                    | BlockType::PageNumber
                    | BlockType::RunningHeader
                    | BlockType::RunningFooter
            )
        })
        .count();

    let sections = SectionBuilder::build(blocks, &headers, figures, tables, &layouts);

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
        metadata,
        sections,
        all_figures,
        all_tables,
    };

    // Coverage: characters that reached the output (section body + headers).
    let emitted_chars = emitted_char_count(&document.sections);
    let ratio = if extracted_chars == 0 {
        1.0
    } else {
        (emitted_chars as f64 / extracted_chars as f64).clamp(0.0, 1.0)
    };
    if ratio < config.min_coverage_ratio {
        warnings.push(PdfLayWarning::LowCoverage { ratio });
    }
    let coverage = Coverage {
        extracted_chars,
        emitted_chars,
        dropped_blocks,
        ratio,
    };

    Ok(AnalysisResult {
        document,
        warnings,
        coverage,
    })
}

/// Recursively sum the characters that reach the output: each section's body
/// text plus its header text.
fn emitted_char_count(sections: &[Section]) -> usize {
    sections
        .iter()
        .map(|s| {
            let header_chars = s
                .header
                .as_ref()
                .map(|h| h.clean_text.chars().count())
                .unwrap_or(0);
            header_chars + s.full_text().chars().count() + emitted_char_count(&s.children)
        })
        .sum()
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

/// Determine the insertion point for a table region.
///
/// Uses the last block index in the region to compute the position after which the table
/// should be inserted in the output stream.
fn determine_table_insertion(region: &TableRegion, _blocks: &[TextBlock]) -> InsertionPoint {
    if let Some(&last_idx) = region.block_indices.last() {
        InsertionPoint {
            page: region.page,
            after_block_index: Some(last_idx),
            y_position: region.bbox.bottom,
        }
    } else {
        InsertionPoint {
            page: region.page,
            after_block_index: None,
            y_position: region.bbox.bottom,
        }
    }
}

/// Analyze PDF from bytes (for use by Python bindings or in-memory workflows).
///
/// Writes the bytes to a temporary file then delegates to [`analyze_pdf`].
pub fn analyze_pdf_bytes(bytes: &[u8], config: &Config) -> Result<AnalysisResult, PdfLayError> {
    let byte_len = bytes.len() as u64;
    if byte_len > config.resource_limits.max_file_size {
        return Err(PdfLayError::ResourceLimitExceeded {
            limit: format!(
                "max file size {} bytes",
                config.resource_limits.max_file_size
            ),
            actual: format!("{byte_len} bytes"),
        });
    }

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
    fn emitted_char_count_sums_headers_and_body() {
        use crate::types::{Rect, Section, SectionHeader};

        let block = TextBlock {
            global_index: 0,
            lines: vec![],
            text: "hello".to_string(), // 5 chars
            bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
            page: 0,
            column_index: 0,
            block_type: BlockType::BodyText,
        };
        let section = Section {
            header: Some(SectionHeader {
                text: "1 Intro".to_string(),
                clean_text: "Intro".to_string(), // 5 chars
                level: 1,
                numbering: None,
                page: 0,
                bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                block_index: 0,
            }),
            level: 1,
            blocks: vec![block],
            figures: vec![],
            tables: vec![],
            children: vec![],
            page_range: (0, 0),
        };
        // header "Intro" (5) + body "hello" (5) = 10.
        assert_eq!(emitted_char_count(&[section]), 10);
    }

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

    #[test]
    fn analyze_pdf_bytes_rejects_oversized_input() {
        use crate::config::ResourceLimits;

        let config = Config {
            resource_limits: ResourceLimits {
                max_file_size: 10, // 10 bytes
                max_pages: 2000,
            },
            ..Default::default()
        };
        let big_bytes = vec![0u8; 100];
        let result = analyze_pdf_bytes(&big_bytes, &config);
        assert!(
            matches!(result, Err(PdfLayError::ResourceLimitExceeded { .. })),
            "Expected ResourceLimitExceeded, got: {result:?}"
        );
    }
}
