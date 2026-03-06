# Task 18: pdf-lay Public Crate + Pipeline Finalization

## Overview

Wire together the public `pdf-lay` crate (re-export facade) and finalize the pipeline so that
the full end-to-end flow works:

```
pdf_lay::analyze_pdf(path, &config)
  -> AnalysisResult { document: PaperDocument, warnings }
  -> doc.toc()                          // Vec<SectionEntry>
  -> doc.select_sections(&["METHODS"])  // SectionSelector
  -> selector.to_markdown(&config)      // String
```

This task also ensures every module declared in `lib.rs` actually exists with a non-stub
implementation so that `cargo build -p pdf-lay-core` passes without any `todo!()` panics in
the non-test surface.

## Related Information
- **Plan**: `/Users/sonokawa/.claude/plans/fuzzy-drifting-summit.md` (Task 18)
- **Design doc**: `docs/arch/02_DESIGN.md` § 5.1 build
- **Overview**: `docs/develop/overview.md`
- **Dependencies**: Tasks 13, 16, 17 must all be completed first

## Files to Create

- [ ] `crates/pdf-lay/src/lib.rs` — public re-export crate (replace existing stub)

## Files to Modify

- [ ] `crates/pdf-lay-core/src/lib.rs` — verify all modules declared, finalize exports
- [ ] `crates/pdf-lay-core/src/pipeline.rs` — ensure `analyze_pdf` returns `AnalysisResult` with the `document` field (not raw `PaperDocument`)
- [ ] `crates/pdf-lay-core/src/selector/selector.rs` — ensure `total_estimated_tokens()` calls `Chunker::estimate_tokens` not a stub

## Implementation Steps

### Step 1: Finalize `crates/pdf-lay-core/src/lib.rs`

The final `lib.rs` should declare all modules. Verify it matches exactly:

```rust
#![warn(missing_docs)]

//! Core library for PDF layout analysis.

pub mod config;
pub mod error;
pub mod extract;
pub mod figure;
pub mod layout;
pub mod output;
pub mod selector;
pub mod structure;
pub mod types;

pub(crate) mod pipeline;

#[cfg(test)]
pub mod test_helpers;

pub use config::{
    CaptionStyle, ChunkConfig, Config, FigureTextFormat, LlmTextConfig, MarkdownConfig,
    MathConfig, MathRepresentationPreference, SplitStrategy, TableConfig,
};
pub use error::{AnalysisResult, PdfLayError, PdfLayWarning};
pub use pipeline::{analyze_pdf, analyze_pdf_bytes};
pub use selector::{LlmTextGenerator, SectionSelector, TocGenerator, SectionEntry};
pub use types::{
    BlockType, Chunk, DocumentMetadata, FigureInfo, ImageFormat, ImageInfo, InsertionPoint,
    PaperDocument, Rect, Section, SectionHeader, TextBlock, TextLine, TextSpan,
    TableInfo, TableRepresentation,
};
```

If any of these items do not yet exist because their task is incomplete, add a note but do NOT
skip the declaration — instead add a stub module/type so compilation succeeds.

### Step 2: `crates/pdf-lay/src/lib.rs` — Public Facade Crate

The `pdf-lay` crate is a thin re-export wrapper over `pdf-lay-core`:

```rust
//! `pdf-lay`: PDF Layout Analysis for Academic Papers.
//!
//! This crate re-exports the public API of `pdf-lay-core`.

pub use pdf_lay_core::{
    // Config types
    CaptionStyle,
    ChunkConfig,
    Config,
    FigureTextFormat,
    LlmTextConfig,
    MarkdownConfig,
    MathConfig,
    MathRepresentationPreference,
    SplitStrategy,
    TableConfig,

    // Error types
    AnalysisResult,
    PdfLayError,
    PdfLayWarning,

    // Pipeline
    analyze_pdf,
    analyze_pdf_bytes,

    // Selector
    LlmTextGenerator,
    SectionEntry,
    SectionSelector,
    TocGenerator,

    // Document types
    BlockType,
    Chunk,
    DocumentMetadata,
    FigureInfo,
    ImageFormat,
    ImageInfo,
    InsertionPoint,
    PaperDocument,
    Rect,
    Section,
    SectionHeader,
    TableInfo,
    TableRepresentation,
    TextBlock,
    TextLine,
    TextSpan,
};
```

### Step 3: Verify `crates/pdf-lay/Cargo.toml`

Confirm that the `pdf-lay` crate depends on `pdf-lay-core`:

```toml
[package]
name = "pdf-lay"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
pdf-lay-core = { path = "../pdf-lay-core" }
```

### Step 4: `SectionSelector::total_estimated_tokens()` — fix if stubbed

In `selector/selector.rs`, `total_estimated_tokens()` must call `Chunker::estimate_tokens`:

```rust
use crate::output::chunker::Chunker;

impl<'a> SectionSelector<'a> {
    /// Total estimated token count for all selected sections.
    pub fn total_estimated_tokens(&self) -> usize {
        self.selected.iter()
            .map(|s| Chunker::estimate_tokens(&s.full_text()))
            .sum()
    }
}
```

### Step 5: End-to-End Compilation Check

Run these commands to verify the build is clean:

```bash
cargo build -p pdf-lay-core
cargo build -p pdf-lay
```

### Step 6: E2E Smoke Test (compile-only, no PDF required)

Add to `tests/integration/e2e_api.rs` (create the file):

```rust
//! Tests that the public API compiles and basic type relationships hold.

use pdf_lay::{Config, MarkdownConfig, LlmTextConfig, SplitStrategy, ChunkConfig};

#[test]
fn config_defaults_compile() {
    let config = Config::default();
    assert!(!config.extract_images || config.extract_images); // always passes, just checks compile
}

#[test]
fn markdown_config_compiles() {
    let config = MarkdownConfig {
        image_base_path: "./images".to_string(),
        include_page_numbers: false,
        heading_offset: 1,
        include_metadata_header: false,
        table_as_image: false,
        figure_caption_style: pdf_lay::CaptionStyle::Italic,
    };
    assert_eq!(config.heading_offset, 1);
}

#[test]
fn chunk_config_defaults_compile() {
    let config = ChunkConfig {
        max_tokens: 4000,
        overlap_tokens: 200,
        split_strategy: SplitStrategy::SectionBoundary,
        include_section_context: true,
    };
    assert_eq!(config.max_tokens, 4000);
}
```

Add the integration test registration to `crates/pdf-lay/Cargo.toml`:

```toml
[[test]]
name = "e2e_api"
path = "../../tests/integration/e2e_api.rs"
```

Or place it under `crates/pdf-lay/tests/e2e_api.rs` (adjust path accordingly).

## Acceptance Criteria

- [ ] `cargo build -p pdf-lay-core` succeeds with zero errors
- [ ] `cargo build -p pdf-lay` succeeds with zero errors
- [ ] `cargo test -p pdf-lay-core` passes all existing tests
- [ ] `cargo test -p pdf-lay` passes the E2E API compile tests
- [ ] `SectionSelector::total_estimated_tokens()` does not call `todo!()`
- [ ] `analyze_pdf()` signature matches `fn(path: &Path, config: &Config) -> Result<AnalysisResult, PdfLayError>`
- [ ] `cargo clippy -p pdf-lay-core -p pdf-lay -- -D warnings` passes

## Dependencies

- Task 13 (pipeline), Task 16 (MarkdownGenerator), Task 17 (JsonGenerator + Chunker)
  must all be completed first.

## Commit Message

```
feat(pdf-lay): wire public re-export crate and verify full pipeline compiles end-to-end
```
