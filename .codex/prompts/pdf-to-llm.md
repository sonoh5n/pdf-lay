---
description: Convert academic paper PDF into LLM-optimized text chunks using pdf-lay
argument-hint: "<pdf-path> [--max-tokens N] [--strategy section|paragraph|token]"
---

# PDF to LLM Pipeline

Extract and chunk academic paper content for LLM consumption (RAG, summarization, Q&A).

## Usage

```
/pdf-to-llm paper.pdf                         # Section-based chunks, 4000 tokens
/pdf-to-llm paper.pdf --max-tokens 2000       # Smaller chunks
/pdf-to-llm paper.pdf --strategy paragraph    # Paragraph-level chunking
```

## Python API (preferred)

```python
import pdflay
doc = pdflay.analyze("$1", extract_images=False)

# Full document chunks
chunks = doc.to_chunks(max_tokens=4000, overlap=200, strategy="section")

# Section-filtered chunks
sel = doc.select_sections(["Methods", "Results"])
chunks = sel.to_chunks(max_tokens=4000, overlap=200)

# Single LLM text (for context window)
text = sel.to_llm_text(include_figures=True, include_tables=True)
```

## CLI (JSONL chunks)

```bash
pdf-lay chunks "$1" --max-tokens 4000 --overlap 200 --strategy section
```

## CLI fallback (plain LLM text, no JSONL splitting)

```bash
pdf-lay llm-text "$1" --section "Introduction" --section "Methods" --section "Results"
```

## For each chunk show

- Chunk ID, Section name, Token count, Page range, Continuation flag

## Defaults

- max-tokens: 4000, overlap: 200, strategy: section

## Strategies

- `section`: split at section boundaries (best for RAG)
- `paragraph`: split at paragraphs (dense papers)
- `token`: hard token count (fixed-size windows)
