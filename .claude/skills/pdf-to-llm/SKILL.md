---
name: pdf-to-llm
description: Convert an academic-paper PDF into LLM-ready text chunks (for RAG or context-window input) using pdf-lay. Use when the user gives a PDF path and wants chunked or LLM-optimized text; read max-tokens / overlap / strategy from the request as natural language. The pdf-lay CLI's `chunks` and `llm-text` subcommands are the primary path; the pdflay Python bindings are an alternative for programmatic use. No slash command, no variable substitution.
allowed-tools: Bash(pdf-lay *), Bash(cargo run *), Bash(python3 *), Read
---

# PDF to LLM Pipeline

Extract and chunk academic paper content for LLM consumption (RAG, summarization, Q&A).

## How to derive arguments from the request

Read these from the user's natural-language message — nothing is substituted for you and
there are no `$MAX_TOKENS`/`$OVERLAP`/`$STRATEGY` variables:
- **max-tokens**: if the user says a number (e.g. "2000 token chunks"), pass that literal
  number as `--max-tokens 2000` (CLI) or `max_tokens=2000` (Python). Default: 4000.
- **overlap**: literal number the user gives, else default 200.
- **strategy**: "section" (default, split at section boundaries), "paragraph" (split at
  paragraph boundaries), or "token" (hard token-count splits) — derived from words like
  "by paragraph" or "fixed size" in the request.
- **section filter**: if the user names specific sections (e.g. "just Methods and
  Results"), pass one `--section <NAME>` per section.

## 1. Generate chunks (CLI, JSONL)

    pdf-lay chunks <PDF_PATH> --max-tokens 4000 --overlap 200 --strategy section

Smaller chunks, written to a file:

    pdf-lay chunks <PDF_PATH> --max-tokens 2000 --overlap 100 -o chunks.jsonl

Only specific sections, without the `[Context: ...]` breadcrumb prefix:

    pdf-lay chunks <PDF_PATH> --section "Methods" --section "Results" --no-section-context

Each JSONL line has: `chunk_id`, `paper_id`, `section`, `page_range`, `estimated_tokens`,
`has_continuation`, `text`, `figures`, `tables`.

## 2. LLM-optimized plain text (single context-window injection, no JSONL splitting)

    pdf-lay llm-text <PDF_PATH> --section "Introduction" --section "Methods" --section "Results"

## 3. Python bindings (alternative, for programmatic use in the current process)

```python
import pdflay
doc = pdflay.analyze("<PDF_PATH>", extract_images=False)

# Full document chunking
chunks = doc.to_chunks(max_tokens=4000, overlap=200, strategy="section")

# Section-filtered chunking
sel = doc.select_sections(["Methods", "Results"])
chunks = sel.to_chunks(max_tokens=4000, overlap=200)   # NOTE: selector.to_chunks has no `strategy` arg

# LLM-optimized plain text (for single context window)
text = sel.to_llm_text(include_figures=True, include_tables=True)
```

## 4. Present results

For each chunk, show:
- **Chunk ID**: sequential number
- **Section**: which section it belongs to
- **Tokens**: estimated token count
- **Pages**: page range
- **Continuation**: whether it continues in next chunk

Then show total stats:
- Total chunks generated
- Average tokens per chunk
- Total estimated tokens
- Section coverage

## 5. Output for downstream use

Offer to:
1. **Display chunks inline** — show each chunk's content
2. **Save as JSONL** — via `pdf-lay chunks ... -o <file>.jsonl`
3. **Feed to LLM** — directly use the chunks in the current conversation context

## Chunking strategies

| Strategy | Behavior | Best For |
|----------|----------|----------|
| `section` | Split at section boundaries, subdivide large sections | RAG, structured retrieval |
| `paragraph` | Split at paragraph boundaries | Dense papers with long sections |
| `token` | Hard token count splits | Fixed-size context windows |

## Defaults

| Parameter | Default | Description |
|-----------|---------|--------------|
| `max-tokens` | 4000 | Maximum tokens per chunk |
| `overlap` | 200 | Overlap tokens between adjacent chunks |
| `strategy` | `section` | Split at section boundaries first |

## Token estimates

pdf-lay's built-in heuristic tokenizer estimates ~4 chars/token (English), ~1.5 chars/token
(Japanese). Pass `--tokenizer <hf-model-id-or-tokenizer.json>` to `pdf-lay chunks` for a real
BPE tokenizer instead (requires a binary built with the `real-tokenizer` cargo feature).
