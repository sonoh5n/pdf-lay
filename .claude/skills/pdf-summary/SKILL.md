---
name: pdf-summary
description: Summarize an academic-paper PDF by extracting its key sections with pdf-lay and composing a structured summary. Use when the user gives a PDF path and wants a summary; honor any requested language (ja/en) and depth (brief/standard/detailed) expressed in natural language. No slash command, no variable substitution.
allowed-tools: Bash(pdf-lay *), Bash(cargo run *), Bash(python3 *), Read
---

# PDF Paper Summary

Extract structured content from an academic paper PDF using pdf-lay, then generate a
comprehensive summary.

## When to use

Invoke when the user supplies a PDF path and asks for a summary, overview, or TL;DR of the
paper. There is no `--lang`/`--depth` flag: read the desired language and depth out of the
user's natural-language request (e.g. "summarize this in Japanese" → Japanese output;
"give me a one-paragraph summary" → brief depth).

## How to derive arguments from the request

- **PDF path**: the file the user names. Substitute it literally into the commands below
  in place of `<PDF_PATH>`.
- **Language**: if the user asks for Japanese (or writes in Japanese), respond in Japanese;
  if English, respond in English; otherwise match the primary language of the paper content.
- **Depth**: "brief"/"quick" → one paragraph; unspecified → standard structured summary;
  "detailed"/"in depth" → section-by-section detail.

## 1. Extract document structure

    pdf-lay toc <PDF_PATH>

## 2. Extract key sections

    pdf-lay markdown <PDF_PATH> --section "Abstract" --no-page-numbers
    pdf-lay markdown <PDF_PATH> --section "Introduction" --no-page-numbers
    pdf-lay markdown <PDF_PATH> --section "Methods" --no-page-numbers
    pdf-lay markdown <PDF_PATH> --section "Results" --no-page-numbers
    pdf-lay markdown <PDF_PATH> --section "Discussion" --no-page-numbers
    pdf-lay markdown <PDF_PATH> --section "Conclusion" --no-page-numbers

Or in a single call with the repeatable `--section` flag:

    pdf-lay llm-text <PDF_PATH> --section "Abstract" --section "Introduction" \
      --section "Methods" --section "Results" --section "Discussion" --section "Conclusion"

Or via the Python bindings for the same result:

    python3 -c "
import pdflay
doc = pdflay.analyze('<PDF_PATH>', extract_images=False)
sel = doc.select_sections(['Abstract', 'Introduction', 'Methods', 'Results', 'Discussion', 'Conclusion'])
print(sel.to_llm_text())
"

## 3. Generate summary

Based on the extracted content, produce a structured summary:

### Brief depth
Single paragraph (3-5 sentences): what the paper does, key method, main finding.

### Standard depth (default)
```markdown
## Paper Summary

**Title**: [extracted or inferred from content]
**Authors**: [if available from metadata]
**Pages**: N pages

### Purpose
[What problem does this paper address? 2-3 sentences]

### Approach
[What method/framework was used? 2-3 sentences]

### Key Findings
- [Finding 1]
- [Finding 2]
- [Finding 3]

### Significance
[Why does this matter? Implications and contributions. 2-3 sentences]

### Figures & Tables
[Summary of N figures and M tables, highlighting key visual results]
```

### Detailed depth
Include all of the Standard format plus:
- Section-by-section summaries for every top-level section
- Methodology details
- Quantitative results with numbers
- Limitations mentioned by authors
- Future work directions

## 4. Language handling

- If the user asked for Japanese: output the summary in Japanese.
- If the user asked for English: output in English.
- If not specified: match the primary language of the paper content.
