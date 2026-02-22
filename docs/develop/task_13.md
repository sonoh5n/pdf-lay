# Task 13: Pipeline Integration

## Overview

Wire together all previous modules into a single `analyze_pdf()` function that accepts a path
and `Config`, runs the full Extract → Layout → Structure → Figure pipeline, and returns an
`AnalysisResult` containing the `PaperDocument` and any accumulated warnings.

Also create `test_helpers.rs` with `make_span`, `make_line`, `make_block` etc. so unit tests
in all modules can be written consistently.

The pipeline flow:
```
PdfReader::open()
  → extract_all_text_spans() + SpanBuilder::merge()
  → LineReconstructor::reconstruct()
  → ColumnDetector::detect() per page
  → BlockGrouper::group()
  → BlockClassifier::from_blocks().classify_all()
  → HeaderDetector::detect()
  → [parallel] ImageExtractor::extract_all() if config.extract_images
  → [parallel] CaptionDetector::detect()
  → CoordinateNormalizer::estimate() per page
  → ImageMatcher::match_all()
  → SectionBuilder::build()
  → PaperDocument assembly
```

## Related Information
- **Plan**: `/Users/sonokawa/.claude/plans/fuzzy-drifting-summit.md` (Task 13)
- **Design doc**: `docs/arch/02_DESIGN.md` § 1.3 data flow
- **Overview**: `docs/develop/overview.md`
- **Dependencies**: Tasks 05, 11, and 12 must all be completed first

## Files to Create

- [ ] `crates/pdf-lay-core/src/pipeline.rs`
- [ ] `crates/pdf-lay-core/src/test_helpers.rs`

## Files to Modify

- [ ] `crates/pdf-lay-core/src/lib.rs` — add `pub(crate) mod pipeline; pub use pipeline::analyze_pdf;`

## Implementation Steps

### Step 1: `test_helpers.rs`

```rust
//! Test helpers for building synthetic TextSpan, TextLine, TextBlock etc.
//!
//! Used throughout unit tests across all modules.
//! Only compiled under `#[cfg(test)]`.

#![cfg(test)]

use crate::types::{BlockType, Rect, TextBlock, TextLine, TextSpan};

/// Build a minimal TextSpan for use in tests.
pub fn make_span(text: &str, left: f64, top: f64, font_size: f64) -> TextSpan {
    TextSpan {
        text: text.to_string(),
        font_name: "Regular".to_string(),
        font_size,
        is_bold: false,
        is_italic: false,
        bbox: Rect::new(
            left,
            top,
            left + text.len() as f64 * font_size * 0.5,
            top - font_size,
        ),
        page: 0,
    }
}

/// Build a bold TextSpan.
pub fn make_bold_span(text: &str, left: f64, top: f64, font_size: f64) -> TextSpan {
    let mut s = make_span(text, left, top, font_size);
    s.is_bold = true;
    s.font_name = "Bold".to_string();
    s
}

/// Build a minimal TextLine.
pub fn make_line(text: &str, left: f64, top: f64, font_size: f64, page: u32) -> TextLine {
    let span = {
        let mut s = make_span(text, left, top, font_size);
        s.page = page;
        s
    };
    let bbox = span.bbox.clone();
    TextLine {
        spans: vec![span],
        text: text.to_string(),
        bbox,
        page,
        baseline_y: top - font_size,
        primary_font_size: font_size,
        primary_font_name: "Regular".to_string(),
        is_bold: false,
    }
}

/// Build a bold TextLine.
pub fn make_bold_line(text: &str, left: f64, top: f64, font_size: f64, page: u32) -> TextLine {
    let mut l = make_line(text, left, top, font_size, page);
    l.is_bold = true;
    l.spans.iter_mut().for_each(|s| s.is_bold = true);
    l
}

/// Build a TextBlock from a single line.
pub fn make_block_from_line(line: TextLine, global_index: usize) -> TextBlock {
    let bbox = line.bbox.clone();
    let page = line.page;
    let text = line.text.clone();
    TextBlock {
        global_index,
        lines: vec![line],
        text,
        bbox,
        page,
        column_index: 0,
        block_type: BlockType::BodyText,
    }
}
```

### Step 2: `pipeline.rs`

```rust
//! The main analysis pipeline: Extract → Layout → Structure → Figure → Output.

use std::path::Path;
use crate::{
    config::Config,
    error::{AnalysisResult, PdfLayError, PdfLayWarning},
    extract::{CoordinateNormalizer, ImageExtractor, PdfReader, SpanBuilder},
    figure::{CaptionDetector, ImageMatcher},
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

    let reader = PdfReader::open(path)?;
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
        match extractor.extract_all(&reader) {
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

    let column_detector = ColumnDetector::new();
    let layouts: Vec<_> = page_dims_list
        .iter()
        .map(|dims| column_detector.detect(&lines, dims))
        .collect();

    // ---- Phase 3: Coordinate Normalization ----

    // Normalize image coordinates per page.
    let page_lines_for_page = |page: u32| -> Vec<&crate::types::TextLine> {
        lines.iter().filter(|l| l.page == page).collect()
    };
    for img in &mut images {
        let page_dims = page_dims_list.iter().find(|d| d.page_number == img.page);
        if let Some(dims) = page_dims {
            let page_lines: Vec<_> = page_lines_for_page(img.page);
            let (norm, warn) = CoordinateNormalizer::estimate(&[img.clone()], &page_lines, dims);
            if let Some(w) = warn {
                warnings.push(w);
            }
            img.normalized_bbox = norm.normalize(&img.raw_bbox);
        }
    }

    // ---- Phase 4: Structure ----

    let mut blocks = BlockGrouper::new().group(&lines, &layouts);

    let classifier = BlockClassifier::from_blocks(&blocks);
    classifier.classify_all(&mut blocks);

    let headers = HeaderDetector::new(classifier.body_font_size).detect(&blocks);

    // ---- Phase 5: Figure Matching ----

    let caption_detector = CaptionDetector::new();
    let captions = caption_detector.detect(&blocks);

    let image_matcher = ImageMatcher::new()
        .with_max_gap(config.caption_max_gap_pt);
    let figures = image_matcher.match_all(&captions, &images, &blocks);

    // Warn about unmatched captions.
    let matched_captions: std::collections::HashSet<usize> = figures
        .iter()
        .filter_map(|f| {
            captions.iter().enumerate().find_map(|(i, c)| {
                if c.full_text == f.caption_text { Some(i) } else { None }
            })
        })
        .collect();
    for (i, caption) in captions.iter().enumerate() {
        use crate::figure::CaptionType;
        if caption.caption_type == CaptionType::Figure && !matched_captions.contains(&i) {
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

    let all_figures: Vec<_> = sections
        .iter()
        .flat_map(|s| s.figures.iter().cloned())
        .collect();

    let document = PaperDocument {
        paper_id,
        source_file: path.to_path_buf(),
        metadata: DocumentMetadata {
            pages: page_count,
            ..Default::default()
        },
        sections,
        all_figures,
        all_tables: Vec::new(),
    };

    Ok(AnalysisResult { document, warnings })
}

/// Analyze PDF from bytes (for use by Python bindings).
pub fn analyze_pdf_bytes(bytes: &[u8], config: &Config) -> Result<AnalysisResult, PdfLayError> {
    // Write to a temp file and delegate to analyze_pdf.
    // (Alternatively: PdfReader::from_bytes if supported.)
    use std::io::Write;
    let tmp = tempfile::NamedTempFile::new()?;
    tmp.as_file().write_all(bytes)?;
    analyze_pdf(tmp.path(), config)
}
```

### Step 3: Update `lib.rs`

```rust
#![warn(missing_docs)]

pub mod config;
pub mod error;
pub mod extract;
pub mod figure;
pub mod layout;
pub mod output;     // Task 16-17
pub mod selector;   // Task 14-15
pub mod structure;
pub mod types;

pub(crate) mod pipeline;

#[cfg(test)]
pub mod test_helpers;

pub use error::{AnalysisResult, PdfLayError, PdfLayWarning};
pub use pipeline::{analyze_pdf, analyze_pdf_bytes};
```

## Smoke Test (after implementation)

Add an integration test in `tests/integration/smoke_test.rs`:

```rust
//! Smoke test: verify the pipeline runs end-to-end on a sample PDF.

#[test]
#[ignore = "requires tests/fixtures/sample.pdf"]
fn smoke_test_ieee_paper() {
    use pdf_lay_core::{analyze_pdf, config::Config};
    use std::path::Path;

    let config = Config {
        extract_images: false, // skip images for speed
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
```

## Acceptance Criteria

- [ ] `cargo build -p pdf-lay-core` succeeds with all modules wired in
- [ ] `cargo test -p pdf-lay-core` passes (all existing unit tests)
- [ ] Smoke test passes when a real PDF is placed in `tests/fixtures/`
- [ ] `AnalysisResult::warnings` captures unmatched captions and coord fallbacks without panicking
- [ ] `analyze_pdf` returns `Err(PdfLayError::FileNotFound)` for nonexistent paths
- [ ] `test_helpers` functions produce valid types (verified by existing unit tests using them)
- [ ] `cargo clippy -p pdf-lay-core -- -D warnings` passes

## Dependencies

- Task 05 (ImageExtractor + CoordinateNormalizer) must be completed.
- Task 11 (SectionBuilder + ReadingOrderSorter) must be completed.
- Task 12 (CaptionDetector + ImageMatcher) must be completed.

## Commit Message

```
feat(pipeline): wire Extract→Layout→Structure→Figure pipeline into analyze_pdf()
```
