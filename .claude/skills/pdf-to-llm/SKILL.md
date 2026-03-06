---
name: pdf-to-llm
description: Convert academic paper PDF into LLM-optimized text chunks using pdf-lay for RAG or context window input
argument-hint: "<pdf-path> [--max-tokens N] [--overlap N] [--strategy section|paragraph|token]"
allowed-tools: Bash(pdf-lay *), Bash(cargo run *), Bash(python *), Read
---

# PDF to LLM Pipeline

Extract and chunk academic paper content for LLM consumption (RAG, summarization, Q&A).

## Usage

```
/pdf-to-llm paper.pdf                              # Default: section-based chunks, 4000 tokens
/pdf-to-llm paper.pdf --max-tokens 2000            # Smaller chunks for limited context
/pdf-to-llm paper.pdf --max-tokens 8000            # Larger chunks for big context windows
/pdf-to-llm paper.pdf --strategy paragraph          # Paragraph-level chunking
/pdf-to-llm paper.pdf --section "Methods" "Results" # Chunk specific sections only
```

## Instructions

### 1. Detect pdf-lay and run analysis

Use Python API (preferred for chunking) or CLI:

```python
import pdflay

doc = pdflay.analyze("$PDF_PATH", extract_images=False)
```

Fallback to CLI if Python not available:
```bash
pdf-lay markdown "$PDF_PATH" --no-page-numbers
```

### 2. Generate chunks

**Via Python (full control)**:
```python
# Full document chunking
chunks = doc.to_chunks(max_tokens=$MAX_TOKENS, overlap=$OVERLAP, strategy="$STRATEGY")

# Section-filtered chunking
sel = doc.select_sections(["Methods", "Results"])
chunks = sel.to_chunks(max_tokens=$MAX_TOKENS, overlap=$OVERLAP)

# LLM-optimized plain text (for single context window)
sel = doc.select_sections(["Introduction", "Methods", "Results"])
text = sel.to_llm_text(include_figures=True, include_tables=True)
```

**Via CLI fallback** (manual chunking):
```bash
pdf-lay markdown "$PDF_PATH" --section "Introduction" --no-page-numbers
```
Then split the Markdown output by section boundaries.

### 3. Present results

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

### 4. Output for downstream use

Offer to:
1. **Display chunks inline** — show each chunk's content
2. **Save as JSONL** — one JSON object per line, with chunk_id, section, text, tokens, page_range
3. **Feed to LLM** — directly use the chunks in the current conversation context

## Defaults

| Parameter | Default | Description |
|-----------|---------|-------------|
| `max-tokens` | 4000 | Maximum tokens per chunk |
| `overlap` | 200 | Overlap tokens between adjacent chunks |
| `strategy` | `section` | Split at section boundaries first |

## Chunking Strategies

| Strategy | Behavior | Best For |
|----------|----------|----------|
| `section` | Split at section boundaries, subdivide large sections | RAG, structured retrieval |
| `paragraph` | Split at paragraph boundaries | Dense papers with long sections |
| `token` | Hard token count splits | Fixed-size context windows |

## Token Estimates

pdf-lay estimates: ~4 chars/token (English), ~1.5 chars/token (Japanese)
