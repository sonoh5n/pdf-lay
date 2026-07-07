---
name: pdf-qa
description: Answer a question about an academic-paper PDF by extracting the relevant sections with pdf-lay and grounding the answer in the paper. Use when the user gives a PDF path plus a question about the paper's method, results, background, limitations, etc. Derive the PDF path and the question from the request; no slash command, no variable substitution.
allowed-tools: Bash(pdf-lay *), Bash(cargo run *), Bash(python3 *), Read
---

# PDF Question & Answer

Extract relevant content from an academic paper PDF using pdf-lay, then answer the user's
question grounded in the paper's content.

## When to use

Invoke when the user supplies a PDF path together with a question about the paper (method,
results, background, limitations, comparisons, contributions, etc.).

## How to derive arguments from the request

- **PDF path**: the file the user names. Substitute it literally into the commands below
  in place of `<PDF_PATH>`.
- **Question**: the user's natural-language question. There is no `argument-hint` and no
  positional-argument substitution — read the question directly out of the request.

## 1. Understand the question and map it to sections

| Question Type | Likely Sections |
|--------------|-----------------|
| Method/approach | Methods, Methodology, Experimental Setup |
| Results/findings | Results, Experiments, Evaluation |
| Motivation/background | Introduction, Background, Related Work |
| Limitations/future | Discussion, Conclusion, Limitations |
| Comparison | Related Work, Discussion, Results |
| Contribution | Abstract, Introduction, Conclusion |
| Data/dataset | Methods, Experiments, Data |
| General/overview | Abstract, Introduction, Conclusion |

## 2. Get document structure

    pdf-lay toc <PDF_PATH>

Use the TOC to identify exact section names matching the question type.

## 3. Extract relevant sections

    pdf-lay markdown <PDF_PATH> --section "SECTION_NAME" --no-page-numbers

`--section` is repeatable, so multiple candidate sections can be pulled in one call:

    pdf-lay llm-text <PDF_PATH> --section "Methods" --section "Results"

If the question is broad, extract the full document:

    pdf-lay markdown <PDF_PATH> --no-page-numbers

Via the Python bindings, for programmatic multi-section extraction in one call:

    python3 -c "
import pdflay
doc = pdflay.analyze('<PDF_PATH>', extract_images=False)
sel = doc.select_sections(['Methods', 'Results'])
print(sel.to_llm_text(include_figures=True, include_tables=True))
"

## 4. Answer the question

Based on the extracted content:

1. **Answer directly** — provide a clear, specific answer to the question
2. **Cite sources** — reference specific sections and page numbers
3. **Quote when relevant** — include brief direct quotes for key claims
4. **Acknowledge gaps** — if the paper doesn't address the question, say so

Format:
```markdown
## Answer

[Direct answer to the question, 2-5 sentences]

### Evidence

> "[Relevant quote from the paper]"
> — Section: [Section Name], p.[page number]

### Additional Context

[Any additional relevant information from the paper]
```

## 5. Offer follow-up

After answering, suggest 2-3 related questions the user might want to ask about the paper,
based on what was found in the content.
