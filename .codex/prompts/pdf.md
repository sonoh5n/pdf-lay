---
description: Analyze academic paper PDF to extract structured content using pdf-lay
argument-hint: "<pdf-path> [--format markdown|json|toc] [--section NAME]"
---

# PDF Structure Analysis

Analyze an academic paper PDF using pdf-lay and return structured content.

## Usage

```
/pdf paper.pdf                          # Full Markdown output (default)
/pdf paper.pdf --format toc             # Table of contents with token estimates
/pdf paper.pdf --format json            # Full JSON output
/pdf paper.pdf --section "Introduction" # Extract specific section only
```

## Detect pdf-lay availability

Try in order:
1. `pdf-lay --version` (CLI in PATH)
2. `cargo run -p pdf-lay-cli -- --version` (from pdf-lay repo)
3. `python -c "import pdflay"` (Python package)

## Run analysis

**TOC**: `pdf-lay toc "$1"`
**Markdown** (default): `pdf-lay markdown "$1" --no-page-numbers`
**Markdown + section**: `pdf-lay markdown "$1" --section "NAME" --no-page-numbers`
**JSON** (Python): `python -c "import pdflay; print(pdflay.analyze('$1', extract_images=False).to_json())"`

## Section name matching

pdf-lay supports case-insensitive partial matching:
- `"intro"` → "Introduction", "1. Introduction", "I. INTRODUCTION"
- `"method"` → "Methods", "Methodology"
- `"result"` → "Results", "Results and Discussion"

## Present results

- TOC: show section hierarchy with levels, page ranges, token counts
- Markdown: show full output; if >300 lines, show TOC first then ask which section
- JSON: save to file or show summary
