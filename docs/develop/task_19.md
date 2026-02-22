# Task 19: PyO3 Bindings

## Overview

Implement the Python bindings crate (`pdflay-python`) using PyO3 0.23+. This exposes the
full analysis pipeline as a Python extension module named `pdflay`.

Key Python classes:
- `pdflay.analyze(path, image_dir, extract_images, detect_tables)` — top-level function
- `PyPaperDocument` — wraps `PaperDocument`, exposes `.metadata`, `.sections`, `.figures`,
  `.to_markdown()`, `.to_json()`, `.to_chunks()`, `.toc()`, `.select_sections()`,
  `.select_sections_by_index()`, `.select_sections_by_level()`, `.select_sections_by_pages()`
- `PySection` — wraps `Section`, exposes `.header`, `.level`, `.text`, `.page_range`, `.figures`, `.children`
- `PySectionEntry` — wraps `SectionEntry`, exposes all metadata fields + `.__repr__()`
- `PySectionSelector` — stores `(doc: PaperDocument, selected_indices: Vec<usize>)` and exposes
  `.to_markdown()`, `.to_json()`, `.to_llm_text()`, `.to_chunks()`, `.total_estimated_tokens()`
- `PyChunk` — wraps `Chunk`, exposes all fields as properties
- `PyFigureInfo` — wraps `FigureInfo`, exposes `.figure_id`, `.figure_number`, `.caption_text`, `.image_path`

**Critical requirement**: ALL `#[pyclass]` structs MUST derive or implement `Clone`.
`PySectionSelector` stores owned data (`PaperDocument`) — not references — to avoid lifetime
issues with PyO3.

## Related Information
- **Plan**: `/Users/sonokawa/.claude/plans/fuzzy-drifting-summit.md` (Task 19)
- **Design doc**: `docs/arch/02_DESIGN.md` § 2.10 PyO3 bindings
- **Overview**: `docs/develop/overview.md`
- **Dependencies**: Task 18 (public crate finalization) must be completed first

## Files to Modify

- [ ] `crates/pdflay-python/src/lib.rs` — replace stub with full implementation

## Implementation Steps

### Step 1: Verify `crates/pdflay-python/Cargo.toml`

```toml
[package]
name = "pdflay-python"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
name = "pdflay"
crate-type = ["cdylib"]

[dependencies]
pdf-lay-core = { path = "../pdf-lay-core" }
pyo3 = { workspace = true, features = ["extension-module"] }

[features]
extension-module = ["pyo3/extension-module"]
```

### Step 2: Full `crates/pdflay-python/src/lib.rs`

```rust
//! Python bindings for pdf-lay using PyO3.

use pyo3::prelude::*;
use std::path::{Path, PathBuf};

use pdf_lay_core::{
    analyze_pdf,
    config::{
        CaptionStyle, ChunkConfig, Config, FigureTextFormat, LlmTextConfig, MarkdownConfig,
        SplitStrategy,
    },
    output::{chunker::Chunker, markdown::MarkdownGenerator},
    selector::{SectionSelector, TocGenerator},
    types::{FigureInfo, PaperDocument, Section, SectionEntry},
    AnalysisResult,
};

// ---------------------------------------------------------------------------
// PyPaperDocument
// ---------------------------------------------------------------------------

#[pyclass]
#[derive(Clone)]
struct PyPaperDocument {
    inner: PaperDocument,
}

#[pymethods]
impl PyPaperDocument {
    #[getter]
    fn paper_id(&self) -> &str {
        &self.inner.paper_id
    }

    #[getter]
    fn pages(&self) -> u32 {
        self.inner.metadata.pages
    }

    #[getter]
    fn sections(&self) -> Vec<PySection> {
        self.inner.sections.iter()
            .map(|s| PySection { inner: s.clone() })
            .collect()
    }

    #[getter]
    fn figures(&self) -> Vec<PyFigureInfo> {
        self.inner.all_figures.iter()
            .map(|f| PyFigureInfo { inner: f.clone() })
            .collect()
    }

    /// Generate Markdown output.
    #[pyo3(signature = (image_base_path="./images", include_page_numbers=false, heading_offset=1))]
    fn to_markdown(
        &self,
        image_base_path: &str,
        include_page_numbers: bool,
        heading_offset: u8,
    ) -> String {
        let config = MarkdownConfig {
            image_base_path: image_base_path.to_string(),
            include_page_numbers,
            heading_offset,
            include_metadata_header: false,
            table_as_image: false,
            figure_caption_style: CaptionStyle::Italic,
        };
        MarkdownGenerator::new(config).generate(&self.inner)
    }

    /// Serialize to pretty-printed JSON.
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string_pretty(&self.inner)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
    }

    /// Split document into LLM-consumable chunks.
    #[pyo3(signature = (max_tokens=4000, overlap=200, strategy="section"))]
    fn to_chunks(&self, max_tokens: usize, overlap: usize, strategy: &str) -> Vec<PyChunk> {
        let split_strategy = match strategy {
            "token" => SplitStrategy::TokenCount,
            "paragraph" => SplitStrategy::Paragraph,
            _ => SplitStrategy::SectionBoundary,
        };
        let config = ChunkConfig {
            max_tokens,
            overlap_tokens: overlap,
            split_strategy,
            include_section_context: true,
        };
        Chunker::new(config).chunk(&self.inner)
            .into_iter()
            .map(|c| PyChunk { inner: c })
            .collect()
    }

    /// Return table of contents (section hierarchy metadata).
    fn toc(&self) -> Vec<PySectionEntry> {
        TocGenerator::generate(&self.inner)
            .into_iter()
            .map(|e| PySectionEntry { inner: e })
            .collect()
    }

    /// Select sections by header name (case-insensitive partial match).
    fn select_sections(&self, names: Vec<String>) -> PySectionSelector {
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let selector = SectionSelector::by_names(&self.inner, &name_refs);
        PySectionSelector {
            doc: self.inner.clone(),
            selected_indices: selector.selected_indices(),
        }
    }

    /// Select sections by flat index.
    fn select_sections_by_index(&self, indices: Vec<usize>) -> PySectionSelector {
        let selector = SectionSelector::by_indices(&self.inner, &indices);
        PySectionSelector {
            doc: self.inner.clone(),
            selected_indices: selector.selected_indices(),
        }
    }

    /// Select sections by heading level (1 = top-level).
    fn select_sections_by_level(&self, level: u8) -> PySectionSelector {
        let selector = SectionSelector::by_level(&self.inner, level);
        PySectionSelector {
            doc: self.inner.clone(),
            selected_indices: selector.selected_indices(),
        }
    }

    /// Select sections whose page range falls within [start, end].
    fn select_sections_by_pages(&self, start: u32, end: u32) -> PySectionSelector {
        let selector = SectionSelector::by_pages(&self.inner, start, end);
        PySectionSelector {
            doc: self.inner.clone(),
            selected_indices: selector.selected_indices(),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "PyPaperDocument(paper_id='{}', pages={}, sections={})",
            self.inner.paper_id,
            self.inner.metadata.pages,
            self.inner.sections.len()
        )
    }
}

// ---------------------------------------------------------------------------
// PySectionSelector
// ---------------------------------------------------------------------------

/// Holds an owned copy of the document plus selected section indices.
/// This avoids lifetime issues with PyO3 — no reference into `PaperDocument`.
#[pyclass]
#[derive(Clone)]
struct PySectionSelector {
    doc: PaperDocument,
    selected_indices: Vec<usize>,
}

impl PySectionSelector {
    fn rebuild_selector(&self) -> SectionSelector {
        SectionSelector::by_indices(&self.doc, &self.selected_indices)
    }
}

#[pymethods]
impl PySectionSelector {
    /// Number of selected sections.
    fn __len__(&self) -> usize {
        self.selected_indices.len()
    }

    /// Total estimated token count.
    fn total_estimated_tokens(&self) -> usize {
        self.rebuild_selector().total_estimated_tokens()
    }

    /// Markdown output for selected sections.
    #[pyo3(signature = (image_base_path="./images"))]
    fn to_markdown(&self, image_base_path: &str) -> String {
        let config = MarkdownConfig {
            image_base_path: image_base_path.to_string(),
            include_page_numbers: false,
            heading_offset: 1,
            include_metadata_header: false,
            table_as_image: false,
            figure_caption_style: CaptionStyle::Italic,
        };
        self.rebuild_selector().to_markdown(&config)
    }

    /// JSON output for selected sections.
    fn to_json(&self) -> PyResult<String> {
        self.rebuild_selector().to_json()
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
    }

    /// LLM-optimized plain text for selected sections.
    #[pyo3(signature = (include_figures=true, include_tables=true, figure_format="placeholder"))]
    fn to_llm_text(
        &self,
        include_figures: bool,
        include_tables: bool,
        figure_format: &str,
    ) -> String {
        let config = LlmTextConfig {
            include_figures,
            include_tables,
            include_section_headers: true,
            figure_format: match figure_format {
                "markdown" => FigureTextFormat::MarkdownLink,
                "caption" => FigureTextFormat::CaptionOnly,
                "omit" => FigureTextFormat::Omit,
                _ => FigureTextFormat::Placeholder,
            },
            math_representation: pdf_lay_core::config::MathRepresentationPreference::Auto,
        };
        self.rebuild_selector().to_llm_text(&config)
    }

    /// Split selected sections into chunks.
    #[pyo3(signature = (max_tokens=4000, overlap=200))]
    fn to_chunks(&self, max_tokens: usize, overlap: usize) -> Vec<PyChunk> {
        let config = ChunkConfig {
            max_tokens,
            overlap_tokens: overlap,
            split_strategy: SplitStrategy::SectionBoundary,
            include_section_context: true,
        };
        self.rebuild_selector().to_chunks(&config)
            .into_iter()
            .map(|c| PyChunk { inner: c })
            .collect()
    }

    fn __repr__(&self) -> String {
        format!("PySectionSelector(selected={})", self.selected_indices.len())
    }
}

// ---------------------------------------------------------------------------
// PySectionEntry
// ---------------------------------------------------------------------------

#[pyclass]
#[derive(Clone)]
struct PySectionEntry {
    inner: SectionEntry,
}

#[pymethods]
impl PySectionEntry {
    #[getter] fn index(&self) -> usize { self.inner.index }
    #[getter] fn header(&self) -> String { self.inner.header.clone() }
    #[getter] fn header_raw(&self) -> String { self.inner.header_raw.clone() }
    #[getter] fn path(&self) -> String { self.inner.path.clone() }
    #[getter] fn level(&self) -> u8 { self.inner.level }
    #[getter] fn page_start(&self) -> u32 { self.inner.page_range.0 }
    #[getter] fn page_end(&self) -> u32 { self.inner.page_range.1 }
    #[getter] fn estimated_tokens(&self) -> usize { self.inner.estimated_tokens }
    #[getter] fn has_figures(&self) -> bool { self.inner.has_figures }
    #[getter] fn figure_count(&self) -> usize { self.inner.figure_count }
    #[getter] fn has_tables(&self) -> bool { self.inner.has_tables }
    #[getter] fn table_count(&self) -> usize { self.inner.table_count }
    #[getter]
    fn children(&self) -> Vec<PySectionEntry> {
        self.inner.children.iter()
            .map(|c| PySectionEntry { inner: c.clone() })
            .collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "[L{}] {} (p.{}-{}, ~{} tokens, fig:{}, tab:{})",
            self.inner.level,
            self.inner.header,
            self.inner.page_range.0,
            self.inner.page_range.1,
            self.inner.estimated_tokens,
            self.inner.figure_count,
            self.inner.table_count,
        )
    }
}

// ---------------------------------------------------------------------------
// PySection
// ---------------------------------------------------------------------------

#[pyclass]
#[derive(Clone)]
struct PySection {
    inner: Section,
}

#[pymethods]
impl PySection {
    #[getter]
    fn header(&self) -> Option<String> {
        self.inner.header.as_ref().map(|h| h.clean_text.clone())
    }

    #[getter]
    fn header_raw(&self) -> Option<String> {
        self.inner.header.as_ref().map(|h| h.text.clone())
    }

    #[getter]
    fn level(&self) -> u8 { self.inner.level }

    #[getter]
    fn text(&self) -> String { self.inner.full_text() }

    #[getter]
    fn page_start(&self) -> u32 { self.inner.page_range.0 }

    #[getter]
    fn page_end(&self) -> u32 { self.inner.page_range.1 }

    #[getter]
    fn figures(&self) -> Vec<PyFigureInfo> {
        self.inner.figures.iter()
            .map(|f| PyFigureInfo { inner: f.clone() })
            .collect()
    }

    #[getter]
    fn children(&self) -> Vec<PySection> {
        self.inner.children.iter()
            .map(|s| PySection { inner: s.clone() })
            .collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "PySection(header='{}', level={}, blocks={})",
            self.inner.header_text(),
            self.inner.level,
            self.inner.blocks.len()
        )
    }
}

// ---------------------------------------------------------------------------
// PyFigureInfo
// ---------------------------------------------------------------------------

#[pyclass]
#[derive(Clone)]
struct PyFigureInfo {
    inner: FigureInfo,
}

#[pymethods]
impl PyFigureInfo {
    #[getter] fn figure_id(&self) -> &str { &self.inner.figure_id }
    #[getter] fn figure_number(&self) -> Option<u32> { self.inner.figure_number }
    #[getter] fn caption_text(&self) -> &str { &self.inner.caption_text }
    #[getter] fn image_path(&self) -> String { self.inner.image.path.display().to_string() }
    #[getter] fn page(&self) -> u32 { self.inner.image.page }

    fn __repr__(&self) -> String {
        format!("PyFigureInfo(id='{}', page={})", self.inner.figure_id, self.inner.image.page)
    }
}

// ---------------------------------------------------------------------------
// PyChunk
// ---------------------------------------------------------------------------

use pdf_lay_core::types::Chunk;

#[pyclass]
#[derive(Clone)]
struct PyChunk {
    inner: Chunk,
}

#[pymethods]
impl PyChunk {
    #[getter] fn chunk_id(&self) -> usize { self.inner.chunk_id }
    #[getter] fn paper_id(&self) -> &str { &self.inner.paper_id }
    #[getter] fn section(&self) -> &str { &self.inner.section }
    #[getter] fn text(&self) -> &str { &self.inner.text }
    #[getter] fn estimated_tokens(&self) -> usize { self.inner.estimated_tokens }
    #[getter] fn page_start(&self) -> u32 { self.inner.page_range.0 }
    #[getter] fn page_end(&self) -> u32 { self.inner.page_range.1 }
    #[getter] fn has_continuation(&self) -> bool { self.inner.has_continuation }

    fn __repr__(&self) -> String {
        format!(
            "PyChunk(id={}, section='{}', tokens={})",
            self.inner.chunk_id, self.inner.section, self.inner.estimated_tokens
        )
    }
}

// ---------------------------------------------------------------------------
// Top-level functions
// ---------------------------------------------------------------------------

/// Analyze a PDF file and return a PyPaperDocument.
///
/// Args:
///     path: Path to the PDF file.
///     image_dir: Output directory for extracted images (default: "./images").
///     extract_images: Whether to extract embedded images (default: True).
///     detect_tables: Whether to detect tables (default: True).
///
/// Returns:
///     PyPaperDocument with sections, figures, and metadata.
///
/// Raises:
///     RuntimeError: If the PDF cannot be read or parsed.
#[pyfunction]
#[pyo3(signature = (path, image_dir="./images", extract_images=true, detect_tables=true))]
fn analyze(
    path: &str,
    image_dir: &str,
    extract_images: bool,
    detect_tables: bool,
) -> PyResult<PyPaperDocument> {
    let config = Config {
        image_output_dir: PathBuf::from(image_dir),
        extract_images,
        detect_tables,
        ..Default::default()
    };
    let AnalysisResult { document, warnings } =
        analyze_pdf(Path::new(path), &config)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

    // Log warnings to stderr (non-fatal).
    for w in &warnings {
        eprintln!("[pdflay warning] {:?}", w);
    }

    Ok(PyPaperDocument { inner: document })
}

// ---------------------------------------------------------------------------
// Module registration
// ---------------------------------------------------------------------------

#[pymodule]
fn pdflay(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(analyze, m)?)?;
    m.add_class::<PyPaperDocument>()?;
    m.add_class::<PySection>()?;
    m.add_class::<PySectionEntry>()?;
    m.add_class::<PySectionSelector>()?;
    m.add_class::<PyFigureInfo>()?;
    m.add_class::<PyChunk>()?;
    Ok(())
}
```

### Step 3: Verify pyproject.toml

`crates/pdflay-python/pyproject.toml` should be:

```toml
[build-system]
requires = ["maturin>=1.0,<2.0"]
build-backend = "maturin"

[project]
name = "pdflay"
requires-python = ">=3.9"
description = "PDF Layout Analysis for Academic Papers"
license = { text = "MIT OR Apache-2.0" }
classifiers = [
    "Programming Language :: Rust",
    "Programming Language :: Python :: Implementation :: CPython",
    "Topic :: Text Processing :: General",
    "Topic :: Scientific/Engineering",
]

[tool.maturin]
features = ["pyo3/extension-module"]
module-name = "pdflay"
```

### Step 4: Development Build Verification

```bash
# From project root
cd crates/pdflay-python
maturin develop
python -c "import pdflay; print(dir(pdflay))"
# Expected: ['PyChunk', 'PyFigureInfo', 'PySectionEntry', 'PySectionSelector',
#            'PyPaperDocument', 'PySection', 'analyze', ...]
```

## Design Notes

### Why `PySectionSelector` stores owned `PaperDocument`

PyO3 does not support structs with non-`'static` lifetimes as `#[pyclass]`. The
`SectionSelector<'a>` borrows from `&'a PaperDocument`, which cannot be stored in a `#[pyclass]`.

The solution from the design doc: store `(doc: PaperDocument, selected_indices: Vec<usize>)`
as owned data, then reconstruct `SectionSelector::by_indices(&self.doc, &self.selected_indices)`
on each method call.

The `selected_indices()` method must exist on `SectionSelector` (implemented in Task 14):

```rust
// In selector/selector.rs:
pub fn selected_indices(&self) -> Vec<usize> {
    let flat = Self::flatten_sections(&self.doc.sections);
    self.selected.iter()
        .filter_map(|s| flat.iter().position(|f| std::ptr::eq(*f, *s)))
        .collect()
}
```

## Acceptance Criteria

- [ ] `cargo build -p pdflay-python` succeeds
- [ ] `maturin develop` succeeds (requires maturin installed: `pip install maturin`)
- [ ] `python -c "import pdflay; print(pdflay.__doc__)"` does not error
- [ ] `pdflay.analyze` function is importable
- [ ] All `#[pyclass]` types have `#[derive(Clone)]`
- [ ] `PySectionSelector` does not store any references — only owned `PaperDocument` + indices
- [ ] `cargo clippy -p pdflay-python -- -D warnings` passes

## Dependencies

- Task 18 (public crate + full pipeline) must be completed first.

## Commit Message

```
feat(python): add PyO3 bindings with PyPaperDocument, PySectionSelector, and pdflay.analyze()
```
