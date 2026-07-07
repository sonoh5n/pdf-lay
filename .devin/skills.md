# pdf-lay Skills for AI Agents

This document defines reusable skills for AI agents (Devin, Claude Code, Codex, etc.) to use pdf-lay for academic paper PDF analysis.

## Prerequisites

pdf-lay must be available via one of:
- **CLI**: `pdf-lay` binary in PATH
- **Cargo**: `cargo run -p pdf-lay-cli --` from the pdf-lay repository
- **Python**: `import pdflay` (install via `python3 -m venv .venv && source .venv/bin/activate && \
  maturin develop -m crates/pdflay-python/Cargo.toml`; `pdflay` is not published on PyPI, so
  `pip install pdflay` does not work)

---

## Skill: pdf — PDF Structure Analysis

**Trigger**: User provides a PDF file and wants to extract its structure.

### CLI Usage

```bash
# Table of contents (section hierarchy, page ranges, token estimates)
pdf-lay toc <pdf-path>

# Full Markdown output
pdf-lay markdown <pdf-path> --no-page-numbers

# Specific section(s) only (case-insensitive partial match)
pdf-lay markdown <pdf-path> --section "Introduction" --no-page-numbers
pdf-lay markdown <pdf-path> --section "Methods" --section "Results" --no-page-numbers
```

### Python Usage

```python
import pdflay

doc = pdflay.analyze("paper.pdf", extract_images=False)
print(doc.to_markdown())               # Markdown
print(doc.to_json())                    # JSON
for entry in doc.toc():                 # TOC entries
    print(f"[L{entry.level}] {entry.header}  ~{entry.estimated_tokens} tokens")
```

### JSON (CLI, no Python required)

```bash
pdf-lay json paper.pdf                    # Full geometry-carrying dump
pdf-lay json paper.pdf --content-only     # Lightweight, LLM-facing projection
```

---

## Skill: pdf-to-llm — PDF to LLM Chunks

**Trigger**: User needs PDF content chunked for RAG, LLM context windows, or embeddings.

### CLI Usage (JSONL chunks, no Python required)

```bash
pdf-lay chunks paper.pdf --max-tokens 4000 --overlap 200 --strategy section
pdf-lay chunks paper.pdf --section "Methods" --section "Results" -o chunks.jsonl
```

### CLI Usage (plain LLM text, single context-window injection)

```bash
pdf-lay llm-text paper.pdf --section "Methods" --section "Results"
```

### Python Usage (preferred for programmatic chunk objects)

```python
import pdflay

doc = pdflay.analyze("paper.pdf", extract_images=False)

# Chunk full document (section-based, 4000 tokens max, 200 overlap)
chunks = doc.to_chunks(max_tokens=4000, overlap=200, strategy="section")
for c in chunks:
    print(f"Chunk {c.chunk_id}: [{c.section}] {c.estimated_tokens} tokens, p.{c.page_start+1}-{c.page_end+1}")

# Chunk specific sections only
sel = doc.select_sections(["Methods", "Results", "Discussion"])
chunks = sel.to_chunks(max_tokens=4000, overlap=200)

# Single LLM-optimized text (for direct context window injection)
text = sel.to_llm_text(include_figures=True, include_tables=True)
```

### Parameters

| Parameter | Default | Options |
|-----------|---------|---------|
| max_tokens | 4000 | Any positive integer |
| overlap | 200 | Tokens shared between chunks |
| strategy | "section" | "section", "paragraph", "token" |

### CLI Fallback (Markdown, then chunk manually)

```bash
# Extract specific section as text (then chunk manually)
pdf-lay markdown paper.pdf --section "Methods" --no-page-numbers
```

---

## Skill: pdf-summary — Paper Summarization

**Trigger**: User wants a summary of an academic paper.

### Steps

1. Extract structure:
   ```bash
   pdf-lay toc paper.pdf
   ```

2. Extract key sections:
   ```bash
   pdf-lay markdown paper.pdf --section "Abstract" --no-page-numbers
   pdf-lay markdown paper.pdf --section "Introduction" --no-page-numbers
   pdf-lay markdown paper.pdf --section "Methods" --no-page-numbers
   pdf-lay markdown paper.pdf --section "Results" --no-page-numbers
   pdf-lay markdown paper.pdf --section "Conclusion" --no-page-numbers
   ```

   Or via Python:
   ```python
   import pdflay
   doc = pdflay.analyze("paper.pdf", extract_images=False)
   sel = doc.select_sections(["Abstract", "Introduction", "Methods", "Results", "Conclusion"])
   text = sel.to_llm_text()
   ```

3. Generate summary in this format:
   ```
   **Title**: [from content or metadata]
   **Authors**: [if available]

   ### Purpose
   [2-3 sentences on the problem addressed]

   ### Approach
   [2-3 sentences on methodology]

   ### Key Findings
   - [Finding 1]
   - [Finding 2]
   - [Finding 3]

   ### Significance
   [2-3 sentences on impact]
   ```

---

## Skill: pdf-qa — Question Answering

**Trigger**: User asks a specific question about a PDF paper.

### Question-to-Section Mapping

| Question Type | Target Sections |
|--------------|-----------------|
| Method/approach | Methods, Methodology, Experimental Setup |
| Results/findings | Results, Experiments, Evaluation |
| Background | Introduction, Related Work |
| Limitations | Discussion, Conclusion, Limitations |
| Comparison | Related Work, Discussion |
| Contribution | Abstract, Introduction, Conclusion |

### Steps

1. Identify relevant sections from question type
2. Extract with pdf-lay:
   ```bash
   pdf-lay toc paper.pdf                                    # Get section names
   pdf-lay markdown paper.pdf --section "SECTION" --no-page-numbers  # Extract
   ```
3. Answer with evidence:
   ```
   ## Answer
   [2-5 sentences]

   ### Evidence
   > "[Quote]"
   > — Section: [Name], p.[N]
   ```
4. Suggest 2-3 follow-up questions

---

## Section Name Matching

pdf-lay uses case-insensitive partial matching:
- `"intro"` → "Introduction", "1. Introduction", "I. INTRODUCTION"
- `"method"` → "Methods", "Methodology", "II. METHODS"
- `"result"` → "Results", "Results and Discussion"
- `"conclu"` → "Conclusion", "Conclusions", "Concluding Remarks"

## Token Estimation

- English: ~4 characters per token
- Japanese: ~1.5 characters per token
