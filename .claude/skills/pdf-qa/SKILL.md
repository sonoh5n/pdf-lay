---
name: pdf-qa
description: Answer questions about an academic paper PDF by extracting relevant sections with pdf-lay
argument-hint: "<pdf-path> <question>"
allowed-tools: Bash(pdf-lay *), Bash(cargo run *), Bash(python *), Read
---

# PDF Question & Answer

Extract relevant content from an academic paper PDF using pdf-lay, then answer the user's question grounded in the paper's content.

## Usage

```
/pdf-qa paper.pdf "What method did the authors use?"
/pdf-qa paper.pdf "What were the main results?"
/pdf-qa paper.pdf "How does this compare to previous work?"
/pdf-qa paper.pdf "What are the limitations?"
```

## Instructions

### 1. Understand the question and map to sections

Analyze the user's question to determine which sections are most likely relevant:

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

### 2. Get document structure

```bash
pdf-lay toc "$PDF_PATH"
```

Use the TOC to identify exact section names matching the question type.

### 3. Extract relevant sections

```bash
# Extract the sections most likely to contain the answer
pdf-lay markdown "$PDF_PATH" --section "SECTION_NAME" --no-page-numbers
```

Or via Python for multiple sections at once:
```python
import pdflay
doc = pdflay.analyze("$PDF_PATH", extract_images=False)
sel = doc.select_sections(["Section1", "Section2"])
text = sel.to_llm_text(include_figures=True, include_tables=True)
```

If the question is broad, extract the full document:
```bash
pdf-lay markdown "$PDF_PATH" --no-page-numbers
```

### 4. Answer the question

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

### 5. Offer follow-up

After answering, suggest 2-3 related questions the user might want to ask about the paper, based on what was found in the content.
