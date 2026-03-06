---
name: pdf
description: Analyze academic paper PDF to extract structured content (sections, figures, tables) using pdf-lay
argument-hint: "<pdf-path> [--format markdown|json|toc] [--section NAME]"
allowed-tools: Bash(pdf-lay *), Bash(cargo run *), Bash(python *), Read
---

# PDF Structure Analysis

Analyze an academic paper PDF using pdf-lay and return structured content.

## Usage

```
/pdf paper.pdf                          # Full Markdown output (default)
/pdf paper.pdf --format toc             # Table of contents with token estimates
/pdf paper.pdf --format json            # Full JSON output
/pdf paper.pdf --section "Introduction" # Extract specific section only
/pdf paper.pdf --section "Results" --section "Discussion"  # Multiple sections
```

## Instructions

### 1. Detect pdf-lay availability

Try in order, use the first that works:

```bash
# Option A: CLI binary in PATH
pdf-lay --version

# Option B: cargo run from pdf-lay repo
cargo run -p pdf-lay-cli -- --version

# Option C: Python package
python -c "import pdflay; print('ok')"
```

If none work, tell the user:
> pdf-lay is not installed. Install via:
> - CLI: `cargo install --path crates/pdf-lay-cli` (from pdf-lay repo)
> - Python: `pip install pdflay` or `cd crates/pdflay-python && maturin develop`

### 2. Run analysis based on format

**TOC** (table of contents):
```bash
pdf-lay toc "$PDF_PATH"
```

**Markdown** (default):
```bash
pdf-lay markdown "$PDF_PATH" --no-page-numbers
```

**Markdown with section filter**:
```bash
pdf-lay markdown "$PDF_PATH" --section "Introduction" --no-page-numbers
```

**JSON** (via Python):
```python
import pdflay
doc = pdflay.analyze("$PDF_PATH", extract_images=False)
print(doc.to_json())
```

### 3. Present results

- For TOC: display the full table with section names, levels, page ranges, and token estimates
- For Markdown: display the full output. If >300 lines, show the TOC first and ask which section to display
- For JSON: save to file and report the path, or show a summary
- Always note warnings emitted by pdf-lay (shown on stderr)

## Output Capabilities

| Format | Source | Content |
|--------|--------|---------|
| `toc` | CLI | Section hierarchy, page ranges, estimated tokens, figure/table counts |
| `markdown` | CLI | Full structured Markdown with headers, body, figures, tables |
| `json` | Python | Complete document structure as JSON |

## Section Selection

pdf-lay supports case-insensitive partial matching for section names:
- `"intro"` matches "Introduction", "1. Introduction", "I. INTRODUCTION"
- `"method"` matches "Methods", "Methodology", "II. METHODS"
- `"result"` matches "Results", "Results and Discussion"
