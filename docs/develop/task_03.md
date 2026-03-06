# Task 03: PdfReader (pdf_oxide Wrapper)

## Overview

Implement `PdfReader` — the single point of contact with the `pdf_oxide` crate.
All other modules use `TextSpan`, `ImageInfo`, and `PathObject` from `types/`;
none of them import from `pdf_oxide` directly.

**Note on pdf_oxide API**: Before writing the mapping code, verify the actual field names
returned by `doc.extract_spans(page)` on a real PDF (see "Implementation Steps" §1 below).
The plan notes this as low-risk but requires empirical confirmation.

## Related Information
- **Plan**: `/Users/sonokawa/.claude/plans/fuzzy-drifting-summit.md` (Task 3)
- **Design doc**: `docs/arch/02_DESIGN.md` § 2.2 extract
- **Overview**: `docs/develop/overview.md`
- **Dependencies**: Task 02 must be completed first (types)

## Files to Create

- [ ] `crates/pdf-lay-core/src/extract/mod.rs`
- [ ] `crates/pdf-lay-core/src/extract/pdf_reader.rs`

## Files to Modify

- [ ] `crates/pdf-lay-core/Cargo.toml` — add `pdf_oxide` dependency
- [ ] `crates/pdf-lay-core/src/lib.rs` — uncomment `pub mod extract;`

## Implementation Steps

### Step 1: Add pdf_oxide to Cargo.toml

First, check the exact API by running a small exploration binary or test:

```toml
# In workspace Cargo.toml [workspace.dependencies]:
pdf_oxide = { git = "https://github.com/yfedoseev/pdf_oxide", version = "0.2" }

# In crates/pdf-lay-core/Cargo.toml [dependencies]:
pdf_oxide.workspace = true
```

Then verify the span fields with:
```rust
// Temporary exploration in a test (delete after confirming fields):
#[test]
fn explore_pdf_oxide_api() {
    // This test requires a real PDF in tests/fixtures/
    // Run: cargo test -p pdf-lay-core explore_pdf_oxide -- --ignored
    let doc = pdf_oxide::PdfDocument::open("tests/fixtures/sample.pdf").unwrap();
    let spans = doc.extract_spans(0).unwrap();
    for s in spans.iter().take(3) {
        eprintln!("{s:#?}");
    }
}
```

Map observed fields to our `TextSpan`. Expected mapping (adjust based on actual output):
- `span.text` or `span.content` → `TextSpan::text`
- `span.font` or `span.font_name` → `TextSpan::font_name`
- `span.size` or `span.font_size` → `TextSpan::font_size`
- `span.bbox` or `span.rect` → `TextSpan::bbox` (as `Rect`)

### Step 2: `extract/mod.rs`

```rust
//! PDF extraction layer — the only module that imports from `pdf_oxide`.
//!
//! Other modules must not import `pdf_oxide` directly.

mod pdf_reader;

pub use pdf_reader::PdfReader;
```

### Step 3: `extract/pdf_reader.rs`

```rust
//! Wrapper around `pdf_oxide::PdfDocument` that converts its types to our internal types.

use std::path::Path;
use crate::error::PdfLayError;
use crate::types::{FontInfo, PageDimensions, PathObject, Rect, TextSpan};

/// A handle to an opened PDF document.
///
/// This struct is the sole importer of `pdf_oxide`. All other modules
/// receive `Vec<TextSpan>` / `Vec<ImageInfo>` from this reader.
pub struct PdfReader {
    // Store the pdf_oxide document handle. The exact type name must be
    // confirmed against the actual pdf_oxide API.
    inner: pdf_oxide::PdfDocument,
}

impl PdfReader {
    /// Open a PDF file from disk.
    pub fn open(path: &Path) -> Result<Self, PdfLayError> {
        if !path.exists() {
            return Err(PdfLayError::FileNotFound(path.to_path_buf()));
        }
        let inner = pdf_oxide::PdfDocument::open(path)
            .map_err(|e| PdfLayError::PdfParseError(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Open a PDF from an in-memory byte slice.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PdfLayError> {
        // Adjust method name based on actual pdf_oxide API:
        let inner = pdf_oxide::PdfDocument::from_bytes(bytes)
            .map_err(|e| PdfLayError::PdfParseError(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Number of pages in the document (0-based indexing used throughout).
    pub fn page_count(&self) -> u32 {
        // Adjust: pdf_oxide may return usize; cast as needed
        self.inner.page_count() as u32
    }

    /// Dimensions of the specified page in points.
    pub fn page_dimensions(&self, page: u32) -> Result<PageDimensions, PdfLayError> {
        self.check_page(page)?;
        // Adjust field names based on pdf_oxide API:
        let (width, height) = self.inner.page_size(page as usize)
            .ok_or_else(|| PdfLayError::PageOutOfRange(page, self.page_count()))?;
        Ok(PageDimensions {
            page_number: page,
            width,
            height,
        })
    }

    /// Extract text spans from a single page.
    ///
    /// Converts pdf_oxide's span representation into our `TextSpan` type.
    /// Font bold/italic detection uses `FontInfo::detect_bold/detect_italic` heuristics.
    pub fn extract_text_spans(&self, page: u32) -> Result<Vec<TextSpan>, PdfLayError> {
        self.check_page(page)?;

        // Adjust method name: pdf_oxide may use `extract_spans`, `text_spans`, etc.
        let raw_spans = self.inner.extract_spans(page as usize)
            .map_err(|e| PdfLayError::PdfParseError(format!("page {page}: {e}")))?;

        let spans = raw_spans
            .into_iter()
            .filter_map(|s| self.convert_span(s, page))
            .collect();

        Ok(spans)
    }

    /// Extract text spans from all pages.
    pub fn extract_all_text_spans(&self) -> Result<Vec<TextSpan>, PdfLayError> {
        let mut all = Vec::new();
        for page in 0..self.page_count() {
            match self.extract_text_spans(page) {
                Ok(spans) => all.extend(spans),
                Err(e) => {
                    log::warn!("Skipping page {page} due to extraction error: {e}");
                }
            }
        }
        Ok(all)
    }

    /// Extract path objects (lines and rectangles) for table rule detection (Phase 2).
    ///
    /// Returns an empty vec if pdf_oxide does not support path extraction.
    pub fn extract_paths(&self, page: u32) -> Result<Vec<PathObject>, PdfLayError> {
        self.check_page(page)?;
        // Phase 2: implement when pdf_oxide path API is confirmed.
        // For now return empty to allow pipeline to compile.
        log::debug!("extract_paths: stub — returning empty for page {page}");
        Ok(Vec::new())
    }

    // ---- private helpers ----

    fn check_page(&self, page: u32) -> Result<(), PdfLayError> {
        let total = self.page_count();
        if page >= total {
            Err(PdfLayError::PageOutOfRange(page, total))
        } else {
            Ok(())
        }
    }

    /// Convert a single pdf_oxide span to our `TextSpan`.
    ///
    /// Returns `None` for empty text spans (which pdf_oxide may produce for
    /// whitespace-only glyph sequences).
    fn convert_span(&self, raw: pdf_oxide::TextSpan, page: u32) -> Option<TextSpan> {
        // Adjust field accesses based on confirmed pdf_oxide API:
        let text = raw.text.trim().to_string();
        if text.is_empty() {
            return None;
        }

        let font_name = raw.font_name.clone();
        let font_size = raw.font_size;

        // pdf_oxide bbox: confirm axis direction. PDF default is Y-up (lower-left origin).
        // If pdf_oxide returns Y-down coordinates, flip here:
        //   top    = page_height - raw.bbox.min_y
        //   bottom = page_height - raw.bbox.max_y
        // Otherwise use directly:
        let bbox = Rect::new(
            raw.bbox.x_min,  // left
            raw.bbox.y_max,  // top  (larger Y in PDF space)
            raw.bbox.x_max,  // right
            raw.bbox.y_min,  // bottom (smaller Y in PDF space)
        );

        Some(TextSpan {
            text,
            is_bold: FontInfo::detect_bold(&font_name),
            is_italic: FontInfo::detect_italic(&font_name),
            font_name,
            font_size,
            bbox,
            page,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_nonexistent_returns_error() {
        let result = PdfReader::open(Path::new("/nonexistent/path/to/file.pdf"));
        assert!(matches!(result, Err(PdfLayError::FileNotFound(_))));
    }

    // Integration test — requires a real PDF fixture.
    // Run with: cargo test -p pdf-lay-core -- --ignored
    #[test]
    #[ignore = "requires tests/fixtures/sample.pdf"]
    fn extract_spans_from_real_pdf() {
        let reader = PdfReader::open(Path::new("tests/fixtures/sample.pdf")).unwrap();
        assert!(reader.page_count() > 0);
        let spans = reader.extract_text_spans(0).unwrap();
        assert!(!spans.is_empty(), "Expected non-empty spans from page 0");
        // Spot-check first span
        let s = &spans[0];
        assert!(!s.text.is_empty());
        assert!(s.font_size > 0.0);
        assert!(s.bbox.width() > 0.0);
        assert!(s.bbox.height() > 0.0);
    }
}
```

### Step 4: Update `lib.rs`

Uncomment `pub mod extract;` in `crates/pdf-lay-core/src/lib.rs`.

## Acceptance Criteria

- [ ] `cargo build -p pdf-lay-core` succeeds after adding pdf_oxide dependency
- [ ] `cargo clippy -p pdf-lay-core -- -D warnings` passes
- [ ] `PdfReader::open("/nonexistent/file.pdf")` returns `Err(PdfLayError::FileNotFound(_))`
- [ ] `PdfReader::page_count()` returns the correct page count for a test PDF
- [ ] `PdfReader::extract_text_spans(page)` returns non-empty spans with valid coordinates
- [ ] No `pdf_oxide` types leak outside `extract/` — verify with `cargo check` that `layout/` etc. compile without `pdf_oxide` in scope
- [ ] All extracted `TextSpan::bbox` values satisfy `top > bottom` and `right > left`

## Dependencies

- Task 02 (types) must be completed first.

## Commit Message

```
feat(extract): add PdfReader wrapping pdf_oxide with TextSpan conversion
```
