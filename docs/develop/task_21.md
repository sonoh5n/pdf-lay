# Task 21: Integration Tests + Phase 1 Cleanup

## Overview

This is the final Phase 1 task. It has two goals:

1. **Integration tests** with real PDFs. Write integration tests in `tests/integration/` that
   run the full pipeline on real PDF files placed in `tests/fixtures/`. Tests are marked
   `#[ignore]` so they are skipped in CI unless the fixtures are present. The tests verify
   section count, page count, figure detection, and that output formats (Markdown, JSON, chunks)
   are non-empty and well-formed.

2. **Phase 1 cleanup**. Remove all remaining `todo!()` panics from non-test code paths, ensure
   `cargo clippy -p pdf-lay-core -p pdf-lay -p pdf-lay-cli -- -D warnings` passes, and ensure
   `cargo fmt --check` passes with no diffs.

## Related Information
- **Plan**: `/Users/sonokawa/.claude/plans/fuzzy-drifting-summit.md` (Task 21)
- **Design doc**: `docs/arch/02_DESIGN.md` — overall
- **Overview**: `docs/develop/overview.md`
- **Dependencies**: All Tasks 01-20 must be completed first

## Files to Create

- [ ] `tests/integration/mod.rs` (or `tests/integration.rs` at workspace root)
- [ ] `tests/integration/smoke_test.rs`
- [ ] `tests/fixtures/README.md` (explains what PDFs to place here)

## Files to Modify

- [ ] Any file containing `todo!()` outside of `#[cfg(test)]` blocks
- [ ] `Cargo.toml` (workspace root) — add `[[test]]` entry if needed

## Implementation Steps

### Step 1: Create `tests/fixtures/README.md`

```markdown
# Test Fixtures

Place PDF files here for integration testing.

Required files:
- `sample_ieee_twocol.pdf` — IEEE-style two-column paper (at least 5 pages)
- `sample_single_col.pdf` — Single-column paper (at least 3 pages)

These files are NOT committed to the repository. Download them manually:
1. Any IEEE conference paper (e.g., from IEEE Xplore)
2. Any arXiv preprint in single-column format

The integration tests are marked `#[ignore]` and will be skipped if the files
are not present. They run on CI only when `RUN_INTEGRATION_TESTS=1` is set.
```

### Step 2: `tests/integration/smoke_test.rs`

```rust
//! Integration smoke tests: verify the pipeline runs end-to-end on real PDFs.
//!
//! All tests are marked `#[ignore]` — they only run when:
//!   cargo test -- --ignored
//! or:
//!   RUN_INTEGRATION_TESTS=1 cargo test

use std::path::Path;
use pdf_lay::{analyze_pdf, Config, SplitStrategy, ChunkConfig};
use pdf_lay::selector::{TocGenerator, SectionSelector};
use pdf_lay::config::{LlmTextConfig, FigureTextFormat, MathRepresentationPreference, MarkdownConfig, CaptionStyle};

const IEEE_TWO_COL: &str = "tests/fixtures/sample_ieee_twocol.pdf";
const SINGLE_COL: &str = "tests/fixtures/sample_single_col.pdf";

// ---- Helpers ----

fn default_config() -> Config {
    Config {
        extract_images: false, // skip image extraction for speed in smoke tests
        ..Default::default()
    }
}

fn config_with_images() -> Config {
    Config {
        extract_images: true,
        image_output_dir: std::path::PathBuf::from("tests/fixtures/images"),
        ..Default::default()
    }
}

// ---- Tests: IEEE two-column paper ----

#[test]
#[ignore = "requires tests/fixtures/sample_ieee_twocol.pdf"]
fn ieee_paper_has_sections() {
    let result = analyze_pdf(Path::new(IEEE_TWO_COL), &default_config())
        .expect("Analysis should succeed on a valid PDF");

    let doc = &result.document;
    assert!(doc.metadata.pages > 0, "Should have at least one page");
    assert!(
        !doc.sections.is_empty(),
        "IEEE papers should produce at least one section"
    );

    println!("Pages: {}", doc.metadata.pages);
    println!("Sections: {}", doc.sections.len());
    println!("Warnings: {}", result.warnings.len());
}

#[test]
#[ignore = "requires tests/fixtures/sample_ieee_twocol.pdf"]
fn ieee_paper_toc_is_non_empty() {
    let result = analyze_pdf(Path::new(IEEE_TWO_COL), &default_config()).unwrap();
    let toc = TocGenerator::generate(&result.document);

    assert!(!toc.is_empty(), "TOC should not be empty");

    for entry in &toc {
        println!(
            "[L{}] {} (p.{}-{}, ~{} tokens)",
            entry.level, entry.header,
            entry.page_range.0 + 1, entry.page_range.1 + 1,
            entry.estimated_tokens
        );
    }
}

#[test]
#[ignore = "requires tests/fixtures/sample_ieee_twocol.pdf"]
fn ieee_paper_markdown_output_non_empty() {
    let result = analyze_pdf(Path::new(IEEE_TWO_COL), &default_config()).unwrap();
    let config = MarkdownConfig {
        image_base_path: "./images".to_string(),
        include_page_numbers: false,
        heading_offset: 1,
        include_metadata_header: false,
        table_as_image: false,
        figure_caption_style: CaptionStyle::Italic,
    };
    let md = pdf_lay::output::markdown::MarkdownGenerator::new(config)
        .generate(&result.document);

    assert!(!md.is_empty(), "Markdown output should not be empty");
    // Should contain at least one heading.
    assert!(md.contains("##"), "Markdown should contain at least one heading");
    println!("Markdown length: {} chars", md.len());
}

#[test]
#[ignore = "requires tests/fixtures/sample_ieee_twocol.pdf"]
fn ieee_paper_llm_text_non_empty() {
    let result = analyze_pdf(Path::new(IEEE_TWO_COL), &default_config()).unwrap();
    let config = LlmTextConfig {
        include_figures: true,
        include_tables: true,
        include_section_headers: true,
        figure_format: FigureTextFormat::Placeholder,
        math_representation: MathRepresentationPreference::Auto,
    };

    // Select all sections.
    let all_sections: Vec<&pdf_lay::Section> = result.document.sections.iter().collect();
    let text = pdf_lay::selector::LlmTextGenerator::new(config).generate(&all_sections);

    assert!(!text.is_empty(), "LLM text output should not be empty");
    println!("LLM text length: {} chars", text.len());
}

#[test]
#[ignore = "requires tests/fixtures/sample_ieee_twocol.pdf"]
fn ieee_paper_json_output_is_valid() {
    let result = analyze_pdf(Path::new(IEEE_TWO_COL), &default_config()).unwrap();
    let json = serde_json::to_string_pretty(&result.document)
        .expect("Serialization should not fail");

    assert!(!json.is_empty());
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("Output should be valid JSON");
    assert!(parsed.is_object(), "JSON root should be an object");
    assert!(parsed["sections"].is_array(), "sections should be a JSON array");
}

#[test]
#[ignore = "requires tests/fixtures/sample_ieee_twocol.pdf"]
fn ieee_paper_chunking_produces_chunks() {
    let result = analyze_pdf(Path::new(IEEE_TWO_COL), &default_config()).unwrap();
    let config = ChunkConfig {
        max_tokens: 2000,
        overlap_tokens: 100,
        split_strategy: SplitStrategy::SectionBoundary,
        include_section_context: true,
    };
    let chunks = pdf_lay::output::chunker::Chunker::new(config).chunk(&result.document);

    assert!(!chunks.is_empty(), "Should produce at least one chunk");
    // All chunk IDs should be sequential.
    for (i, chunk) in chunks.iter().enumerate() {
        assert_eq!(chunk.chunk_id, i, "Chunk IDs should be sequential");
    }
    // Last chunk should not have has_continuation.
    assert!(!chunks.last().unwrap().has_continuation);
    println!("Chunks: {}", chunks.len());
}

#[test]
#[ignore = "requires tests/fixtures/sample_ieee_twocol.pdf"]
fn ieee_paper_section_selector_by_name() {
    let result = analyze_pdf(Path::new(IEEE_TWO_COL), &default_config()).unwrap();
    let doc = &result.document;

    // Try to select INTRODUCTION — most IEEE papers have one.
    let selector = SectionSelector::by_names(doc, &["introduction"]);
    let text = selector.to_llm_text(&LlmTextConfig {
        include_figures: false,
        include_tables: false,
        include_section_headers: true,
        figure_format: FigureTextFormat::Omit,
        math_representation: MathRepresentationPreference::Auto,
    });

    // If introduction was found, text should be non-empty.
    if !selector.sections().is_empty() {
        assert!(!text.is_empty(), "Selected introduction text should not be empty");
        println!("Introduction text length: {} chars", text.len());
    } else {
        println!("NOTE: No INTRODUCTION section found (paper may use different naming)");
    }
}

#[test]
#[ignore = "requires tests/fixtures/sample_ieee_twocol.pdf"]
fn ieee_paper_no_panic_on_warnings() {
    // Just verify the pipeline completes without panicking and warnings are non-fatal.
    let result = analyze_pdf(Path::new(IEEE_TWO_COL), &default_config()).unwrap();
    // Any number of warnings is acceptable.
    for w in &result.warnings {
        println!("[warning] {:?}", w);
    }
    // The document should still be valid.
    assert!(!result.document.sections.is_empty() || result.document.metadata.pages > 0);
}

// ---- Tests: single-column paper ----

#[test]
#[ignore = "requires tests/fixtures/sample_single_col.pdf"]
fn single_col_paper_has_sections() {
    let result = analyze_pdf(Path::new(SINGLE_COL), &default_config())
        .expect("Analysis should succeed");

    assert!(result.document.metadata.pages > 0);
    assert!(!result.document.sections.is_empty());
    println!("Single-col pages: {}", result.document.metadata.pages);
    println!("Single-col sections: {}", result.document.sections.len());
}

// ---- Error handling test ----

#[test]
fn nonexistent_pdf_returns_error() {
    use pdf_lay::PdfLayError;
    let result = analyze_pdf(Path::new("tests/fixtures/does_not_exist.pdf"), &default_config());
    assert!(result.is_err(), "Should return Err for nonexistent file");
    match result.unwrap_err() {
        PdfLayError::FileNotFound(_) | PdfLayError::IoError(_) => {}
        e => panic!("Expected FileNotFound or IoError, got: {:?}", e),
    }
}
```

### Step 3: Register integration tests in workspace `Cargo.toml`

In the workspace root `Cargo.toml`, add the integration test:

```toml
[[test]]
name = "integration"
path = "tests/integration/smoke_test.rs"
```

Or create `tests/integration.rs` that includes the module:

```rust
// tests/integration.rs
mod smoke_test;
```

And `tests/integration/mod.rs`:

```rust
// tests/integration/mod.rs  (if using module layout)
pub mod smoke_test;
```

The simplest approach is a single file at `tests/integration/smoke_test.rs` with the test
binary declared in the workspace root `Cargo.toml` as above.

**Note**: The test binary must depend on `pdf-lay` (the public crate):

```toml
[[test]]
name = "integration"
path = "tests/integration/smoke_test.rs"

[dev-dependencies]
pdf-lay = { path = "crates/pdf-lay" }
serde_json = { workspace = true }
```

### Step 4: Phase 1 Cleanup Checklist

Run each command and fix all warnings/errors before marking this task complete:

```bash
# 1. Find all todo!() in non-test code
grep -r "todo!()" crates/pdf-lay-core/src/ \
  crates/pdf-lay/src/ \
  crates/pdf-lay-cli/src/ \
  | grep -v "#\[cfg(test)\]"
# Expected: no matches

# 2. Find all unimplemented!() in non-test code
grep -r "unimplemented!()" crates/*/src/ | grep -v "#\[cfg(test)\]"
# Expected: no matches

# 3. Format check
cargo fmt --check
# Expected: exit 0 (no diffs)

# 4. Clippy for all crates
cargo clippy -p pdf-lay-core -p pdf-lay -p pdf-lay-cli -- -D warnings
# Expected: no warnings

# 5. All unit tests pass
cargo test -p pdf-lay-core -p pdf-lay -p pdf-lay-cli
# Expected: all pass

# 6. Integration tests compile (even if fixtures are missing)
cargo test --test integration -- --list
# Expected: lists test names without error
```

### Step 5: Fix any remaining `todo!()` occurrences

Common locations where stubs may remain:

- `output/chunker.rs`: `chunk_by_tokens` and `chunk_by_paragraph` (should be implemented in Task 17)
- `extract/pdf_reader.rs`: `extract_paths()` (intended as a stub for now — replace `todo!()` with
  `Ok(vec![])` so it compiles without panicking)
- Any `impl Default` that uses `todo!()`

For `extract_paths()` specifically, return an empty vec since path extraction is not part of
Phase 1 scope:

```rust
pub fn extract_paths(&self, _page: u32) -> Result<Vec<PathObject>, PdfLayError> {
    // Path extraction not implemented in Phase 1; returns empty.
    Ok(vec![])
}
```

### Step 6: Verify `cargo fmt` passes

Run `cargo fmt` (not `--check`) to auto-format, then commit:

```bash
cargo fmt
git diff  # Review diffs, then stage and commit
```

## Acceptance Criteria

- [ ] `grep -r "todo!()" crates/*/src/ | grep -v "#\[cfg(test)\]"` produces no output
- [ ] `cargo fmt --check` exits 0
- [ ] `cargo clippy -p pdf-lay-core -p pdf-lay -p pdf-lay-cli -- -D warnings` exits 0
- [ ] `cargo test -p pdf-lay-core -p pdf-lay -p pdf-lay-cli` — all tests pass
- [ ] `cargo test --test integration -- nonexistent_pdf_returns_error` passes without fixtures
- [ ] `cargo test --test integration -- --ignored` passes when fixture PDFs are placed in
  `tests/fixtures/` (at least `ieee_paper_has_sections` and `ieee_paper_markdown_output_non_empty`)
- [ ] `cargo build --workspace` succeeds
- [ ] `docs/develop/overview.md` progress log updated to reflect Phase 1 completion

## Dependencies

- All Tasks 01-20 must be completed first.

## Commit Message

```
test(integration): add smoke tests for full pipeline and complete Phase 1 cleanup
```
