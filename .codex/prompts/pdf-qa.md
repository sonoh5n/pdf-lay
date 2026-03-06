---
description: Answer questions about an academic paper PDF using pdf-lay for extraction
argument-hint: "<pdf-path> <question>"
---

# PDF Question & Answer

Extract relevant content from academic paper PDF using pdf-lay, then answer the question.

## Usage

```
/pdf-qa paper.pdf "What method did the authors use?"
/pdf-qa paper.pdf "What were the main results?"
/pdf-qa paper.pdf "What are the limitations?"
```

## Steps

1. Map question type to sections:
   - Method/approach → Methods, Methodology
   - Results → Results, Experiments, Evaluation
   - Background → Introduction, Related Work
   - Limitations → Discussion, Conclusion
   - General → Abstract, Introduction, Conclusion

2. Get structure: `pdf-lay toc "$1"`

3. Extract relevant sections:
   ```bash
   pdf-lay markdown "$1" --section "SECTION" --no-page-numbers
   ```

4. Answer with citations:
   ```markdown
   ## Answer
   [Direct answer, 2-5 sentences]

   ### Evidence
   > "[Quote from paper]"
   > — Section: [Name], p.[N]
   ```

5. Suggest 2-3 follow-up questions
