# pdflay

`pdflay` is the Python package for `pdf-lay`, a Rust-powered PDF layout
analysis library for academic papers.

It extracts structured content from PDFs into representations that are easier
to use in LLM and RAG pipelines, including:

- sections and table of contents
- figures and captions
- tables
- Markdown and JSON output
- chunked text for downstream indexing

## Installation

```bash
pip install pdflay
```

## Quick Example

```python
import pdflay

doc = pdflay.analyze(
    "paper.pdf",
    image_dir="./images",
    extract_images=True,
    detect_tables=True,
)

print(doc.toc())
print(doc.to_markdown(image_base_path="./images")[:500])
```

## Project

- Repository: https://github.com/sonoh5n/pdf-lay
- Issues: https://github.com/sonoh5n/pdf-lay/issues
