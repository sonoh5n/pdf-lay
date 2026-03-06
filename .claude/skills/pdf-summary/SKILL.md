---
name: pdf-summary
description: Summarize an academic paper PDF by extracting and analyzing its key sections using pdf-lay
argument-hint: "<pdf-path> [--lang ja|en] [--depth brief|standard|detailed]"
allowed-tools: Bash(pdf-lay *), Bash(cargo run *), Bash(python *), Read
---

# PDF Paper Summary

Extract structured content from an academic paper PDF using pdf-lay, then generate a comprehensive summary.

## Usage

```
/pdf-summary paper.pdf                    # Standard summary in auto-detected language
/pdf-summary paper.pdf --lang ja          # Summary in Japanese
/pdf-summary paper.pdf --depth brief      # One-paragraph overview
/pdf-summary paper.pdf --depth detailed   # Section-by-section detailed summary
```

## Instructions

### 1. Extract document structure

```bash
# Get TOC to understand paper structure
pdf-lay toc "$PDF_PATH"
```

### 2. Extract key sections

Use pdf-lay to extract content section by section:

```bash
# Extract the most important sections for summarization
pdf-lay markdown "$PDF_PATH" --section "Abstract" --no-page-numbers
pdf-lay markdown "$PDF_PATH" --section "Introduction" --no-page-numbers
pdf-lay markdown "$PDF_PATH" --section "Methods" --no-page-numbers
pdf-lay markdown "$PDF_PATH" --section "Results" --no-page-numbers
pdf-lay markdown "$PDF_PATH" --section "Discussion" --no-page-numbers
pdf-lay markdown "$PDF_PATH" --section "Conclusion" --no-page-numbers
```

Or via Python for efficiency:
```python
import pdflay
doc = pdflay.analyze("$PDF_PATH", extract_images=False)
sel = doc.select_sections(["Abstract", "Introduction", "Methods", "Results", "Discussion", "Conclusion"])
text = sel.to_llm_text()
```

### 3. Generate summary

Based on the extracted content, produce a structured summary:

#### Brief (`--depth brief`)
Single paragraph (3-5 sentences): what the paper does, key method, main finding.

#### Standard (default)
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

#### Detailed (`--depth detailed`)
Include all of the Standard format plus:
- Section-by-section summaries for every top-level section
- Methodology details
- Quantitative results with numbers
- Limitations mentioned by authors
- Future work directions

### 4. Language handling

- If `--lang ja`: output the summary in Japanese
- If `--lang en`: output in English
- If not specified: match the primary language of the paper content
