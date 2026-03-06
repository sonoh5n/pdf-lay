# pdf-lay Phase 0 + Phase 1 Overview

## Input Source
- **Plan File**: `/Users/sonokawa/.claude/plans/fuzzy-drifting-summit.md`
- **Spec**: `docs/arch/01_SPECIFICATION.md`
- **Design**: `docs/arch/02_DESIGN.md`
- **Agent Guide**: `AGENTS.md`
- **Created**: 2026-02-21

## Overall Purpose

Build a Rust library (`pdf-lay`) that extracts structured content (text, figures, tables, math)
from academic PDF papers and generates LLM-optimized output (Markdown, JSON, chunks).

The library is delivered as:
- `pdf-lay-core` — internal core crate
- `pdf-lay` — public re-export crate
- `pdf-lay-cli` — CLI binary
- `pdflay-python` — PyO3 Python bindings

## Dependency Graph

```
T1(workspace) → T2(types) ─┬→ T3(PdfReader) → T4(span_builder) → T5(images+coord)
                             ├→ T6(lines) → T7(columns)
                             │    └→ T8(blocks) → T9(classifier) → T10(headers) → T11(sections)
                             ├→ T12(figure: caption+matcher)
                             └→ T13(pipeline) ← [T5, T11, T12]
                                  ├→ T14(toc+selector) → T15(llm_text)
                                  ├→ T16(markdown)
                                  ├→ T17(json+chunker)
                                  └→ T18(public API) → T19(PyO3) / T20(CLI) → T21(integration)
```

## Task List

| Index | Title | Dependencies | Parallel Group |
|-------|-------|--------------|----------------|
| 01 | Workspace Skeleton + CI | - | - |
| 02 | Common Type Definitions | T01 | - |
| 03 | PdfReader (pdf_oxide wrapper) | T02 | Extract group |
| 04 | Character-to-Span Grouping | T03 | Extract group |
| 05 | ImageExtractor + CoordinateNormalizer | T02 | Extract group |
| 06 | LineReconstructor | T02 | Layout group |
| 07 | ColumnDetector | T06 | Layout group |
| 08 | BlockGrouper | T07 | Structure group |
| 09 | BlockClassifier | T08 | Structure group |
| 10 | HeaderDetector | T09 | Structure group |
| 11 | SectionBuilder + ReadingOrderSorter | T10 | Structure group |
| 12 | CaptionDetector + ImageMatcher | T02 | Figure group (parallel with T06-T11) |
| 13 | Pipeline Integration | T05, T11, T12 | - |
| 14 | TocGenerator + SectionSelector | T13 | Output group |
| 15 | LlmTextGenerator | T14 | Output group |
| 16 | MarkdownGenerator | T13 | Output group |
| 17 | JsonGenerator + Chunker | T13 | Output group |
| 18 | pdf-lay Public Crate + Pipeline Final | T14, T15, T16, T17 | - |
| 19 | PyO3 Bindings | T18 | Bindings group |
| 20 | CLI (toc + markdown) | T18 | Bindings group |
| 21 | Integration Tests + Phase 1 Cleanup | T19, T20 | - |

## Parallelization Points

- **T03-T05** (extract group) and **T06-T07** (layout group) can be developed in parallel after T02
- **T12** (figure) can start independently after T02
- **T14-T17** (output group) can be developed in parallel after T13
- **T19** and **T20** can be developed in parallel after T18

## Progress Log

- 2026-02-21: Overview document created, all 21 task files created (task_01.md through task_21.md)
