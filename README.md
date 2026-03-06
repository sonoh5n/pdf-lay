# pdf-lay

`pdf-lay` is a Rust-first PDF layout analysis library for academic papers.
It extracts text, sections, figures, tables, and metadata into a structured
intermediate representation suitable for LLM pipelines.

This repository provides:

- Rust library crate: `pdf-lay`
- CLI binary: `pdf-lay`
- Python extension module (PyO3): `pdflay`

## Workspace Layout

```text
crates/pdf-lay-core     internal core crate
crates/pdf-lay          public Rust API (re-export facade)
crates/pdf-lay-cli      CLI binary
crates/pdflay-python    Python bindings (maturin)
tests/                  integration tests and fixtures
```

## Prerequisites

- Rust toolchain (edition 2024; rustc 1.75+)
- Python 3.9+
- `uv` (recommended for `maturin`) or `maturin` installed directly

## Build and Test

```bash
# Build all crates
cargo build

# Rust tests
cargo test
cargo test -p pdf-lay-core
cargo test -p pdf-lay

# Lint/format
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

Note: some integration tests require local fixture PDFs in `tests/fixtures/`.

## Rust Crate Usage (`pdf-lay`)

### Add Dependency

From another local project:

```toml
[dependencies]
pdf-lay = { path = "/absolute/path/to/pdf-lay/crates/pdf-lay" }
```

### Minimal Example

```rust
use pdf_lay::{analyze_pdf, Config, MarkdownConfig, MarkdownGenerator, TocGenerator};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config {
        extract_images: true,
        image_output_dir: "./images".into(),
        ..Default::default()
    };

    let result = analyze_pdf(Path::new("paper.pdf"), &config)?;
    let doc = &result.document;

    println!("pages: {}", doc.metadata.pages);
    println!("top-level sections: {}", TocGenerator::generate(doc).len());

    let md = MarkdownGenerator::new(MarkdownConfig::default()).generate(doc);
    println!("{}", md);
    Ok(())
}
```

### Useful APIs

- `analyze_pdf(path, &Config) -> Result<AnalysisResult, PdfLayError>`
- `PaperDocument::toc()`
- `PaperDocument::select_sections(&["METHODS", "RESULTS"])`
- `SectionSelector::to_markdown(...)`
- `SectionSelector::to_llm_text(...)`
- `SectionSelector::to_chunks(...)`

## CLI Usage (`pdf-lay`)

### Run Without Installing

```bash
cargo run -p pdf-lay-cli -- --help
```

### Commands

```bash
# Print table of contents
cargo run -p pdf-lay-cli -- toc /path/to/paper.pdf

# TOC entries that contain figures
cargo run -p pdf-lay-cli -- toc /path/to/paper.pdf --figures-only

# Convert whole PDF to Markdown (stdout)
cargo run -p pdf-lay-cli -- markdown /path/to/paper.pdf

# Convert selected sections and write file
cargo run -p pdf-lay-cli -- markdown /path/to/paper.pdf \
  --section METHODS \
  --section RESULTS \
  --image-dir ./images \
  --image-base ./images \
  --output paper.md
```

### Build Installable Binary

```bash
cargo build -p pdf-lay-cli --release
./target/release/pdf-lay --help
```

## Python Usage (`pdflay`)

The Python package is built as a wheel via `maturin`.

### Build Wheel

```bash
uvx maturin build --release -m crates/pdflay-python/Cargo.toml
```

Wheel output:

```text
target/wheels/pdflay-<version>-cp39-abi3-<platform>.whl
```

### Install Wheel

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install target/wheels/pdflay-*.whl
```

### Quick Python Example

```python
import pdflay

doc = pdflay.analyze(
    "paper.pdf",
    image_dir="./images",
    extract_images=True,
    detect_tables=True,
)

print(doc.paper_id, doc.pages)
print(len(doc.toc()))

md = doc.to_markdown(image_base_path="./images")
print(md[:500])

sel = doc.select_sections(["methods", "results"])
print(sel.total_estimated_tokens())
print(sel.to_llm_text())

chunks = doc.to_chunks(max_tokens=2000, overlap=200, strategy="section")
print(len(chunks))
```

### Python API Highlights

- `pdflay.analyze(path, image_dir="./images", extract_images=True, detect_tables=True)`
- `PyPaperDocument.to_markdown(...)`
- `PyPaperDocument.to_json()`
- `PyPaperDocument.to_chunks(...)`
- `PyPaperDocument.toc()`
- `PyPaperDocument.select_sections(...)`
- `PySectionSelector.to_markdown()/to_json()/to_llm_text()/to_chunks()`

## End-to-End Local Verification

```bash
# Rust library and integration (fixture-dependent tests are ignored by default)
cargo test -p pdf-lay

# CLI
cargo run -p pdf-lay-cli -- --help

# Python wheel build + local install test
uvx maturin build --release -m crates/pdflay-python/Cargo.toml
python3 -m venv /tmp/pdflay_venv
source /tmp/pdflay_venv/bin/activate
pip install target/wheels/pdflay-*.whl
python -c "import pdflay; print(hasattr(pdflay, 'analyze'))"
```

## Notes

- `pdf-lay-core` is internal (`publish = false`).
- `pdf-lay` is the public Rust API crate.
- Python distribution is wheel-based from this repository; publishing to PyPI
  is a separate release step.
