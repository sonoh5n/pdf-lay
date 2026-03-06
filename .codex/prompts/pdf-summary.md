---
description: Summarize an academic paper PDF using pdf-lay to extract key sections
argument-hint: "<pdf-path> [--lang ja|en] [--depth brief|standard|detailed]"
---

# PDF Paper Summary

Extract content from academic paper PDF using pdf-lay, then generate a structured summary.

## Usage

```
/pdf-summary paper.pdf                    # Standard summary
/pdf-summary paper.pdf --lang ja          # Japanese summary
/pdf-summary paper.pdf --depth brief      # One-paragraph overview
/pdf-summary paper.pdf --depth detailed   # Section-by-section
```

## Steps

1. Get TOC: `pdf-lay toc "$1"`
2. Extract key sections:
   ```bash
   pdf-lay markdown "$1" --section "Abstract" --no-page-numbers
   pdf-lay markdown "$1" --section "Introduction" --no-page-numbers
   pdf-lay markdown "$1" --section "Methods" --no-page-numbers
   pdf-lay markdown "$1" --section "Results" --no-page-numbers
   pdf-lay markdown "$1" --section "Conclusion" --no-page-numbers
   ```
3. Generate structured summary

## Standard summary format

```markdown
**Title**: [extracted]
**Authors**: [if available]
**Pages**: N pages

### Purpose
[2-3 sentences]

### Approach
[2-3 sentences]

### Key Findings
- [Finding 1]
- [Finding 2]
- [Finding 3]

### Significance
[2-3 sentences]
```

## Language

- `--lang ja`: output in Japanese
- `--lang en`: output in English
- Default: match the paper's language
