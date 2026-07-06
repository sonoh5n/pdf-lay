---
name: pdf
description: Extract structured content from an academic-paper PDF using the pdf-lay CLI (or its pdflay Python bindings). Use this when the user gives a path to a PDF and asks for its structure, a table of contents, a specific named section, or a Markdown/JSON conversion. The model derives the PDF path and the desired output from the request in natural language — there is no slash command and no shell-variable substitution.
allowed-tools: Bash(pdf-lay *), Bash(cargo run *), Bash(python3 *), Read
---

# PDF Structure Analysis

Analyze an academic-paper PDF with pdf-lay and return structured content
(table of contents, full Markdown, a selected section, or JSON).

## When to use

Invoke when the user supplies a PDF file path and wants any of:
- the section hierarchy / table of contents,
- the whole document as Markdown,
- one or more specific sections,
- a JSON dump of the document structure.

## How to derive arguments from the request

Read these from the user's natural-language message (nothing is substituted for you):
- **PDF path**: the file the user names. Substitute it literally into the commands below
  in place of `<PDF_PATH>`.
- **Desired output**: pick the matching command below. There is NO `--format` flag —
  the output kind is chosen by the pdf-lay *subcommand* (`toc`, `markdown`, or `json`).
  Do not pass `--format`.
- **Section filter**: if the user names sections, pass one `--section <NAME>` per section
  (the flag is repeatable; matching is case-insensitive partial match).

## Detect availability

Try in order; use the first that works (substitute the real path, do not run `$VAR`):
- `pdf-lay --version`  (CLI on PATH)
- `cargo run -p pdf-lay-cli -- --version`  (from a checkout of the pdf-lay repo)
- `python3 -c "import pdflay; print('ok')"`  (Python bindings)

If none work, tell the user to install pdf-lay:
> - CLI from source: `cargo install --path crates/pdf-lay-cli`
> - Python: `python3 -m venv .venv && source .venv/bin/activate && \
>   maturin develop -m crates/pdflay-python/Cargo.toml`
>   (there is no `pip install pdflay` — the package is not published on PyPI)

## Run analysis

Table of contents:
    pdf-lay toc <PDF_PATH>

Full Markdown (default):
    pdf-lay markdown <PDF_PATH> --no-page-numbers

Selected section(s):
    pdf-lay markdown <PDF_PATH> --section "Introduction" --no-page-numbers
    pdf-lay markdown <PDF_PATH> --section "Results" --section "Discussion" --no-page-numbers

Full JSON dump (complete document tree with geometry/font metadata):
    pdf-lay json <PDF_PATH>

Lightweight, LLM-facing JSON (no bbox/geometry, math-converted body text):
    pdf-lay json <PDF_PATH> --content-only

Equivalent via Python bindings, if the CLI is unavailable:
    python3 -c "import pdflay; print(pdflay.analyze('<PDF_PATH>', extract_images=False).to_json())"

## Present results

- TOC: show the table (level, header, page range, ~tokens, fig/tab counts).
- Markdown: show the output; if it exceeds ~300 lines, show the TOC first and ask which
  section to expand.
- JSON: save to a file and report the path, or summarize.
- Always surface warnings pdf-lay prints to stderr.

## Output capabilities

| Subcommand | Source | Content |
|------------|--------|---------|
| `toc` | CLI | Section hierarchy, page ranges, estimated tokens, figure/table counts |
| `markdown` | CLI | Full structured Markdown with headers, body, figures, tables |
| `json` | CLI | Full geometry-carrying document dump, or `--content-only` for a lightweight LLM-facing projection |

## Section name matching

pdf-lay matches section names case-insensitively by partial match:
- "intro"  → "Introduction", "1. Introduction", "I. INTRODUCTION"
- "method" → "Methods", "Methodology"
- "result" → "Results", "Results and Discussion"
