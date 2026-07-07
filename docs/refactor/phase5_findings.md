# Phase 5 Findings — Real-PDF Accuracy Investigation

Investigation of the three defects surfaced by testing the refactored pipeline
against two real dummy fixtures:

- `tests/fixtures/en_twocol_figs.pdf` — English two-column, 16 pages, figures/tables/equations.
- `tests/fixtures/ja_twocol.pdf` — Japanese two-column (A4), 16 pages, CID/ToUnicode CJK.

All three are **improvable**. This document records the root cause of each with
on-disk evidence, and the recommended fix and its risk. Method: raw
`pdf_oxide` span dumps + instrumented dumps of reconstructed lines and detected
column layouts (instrumentation removed after measuring).

---

## Problem 1 — Two-column reading order is broken (highest impact)

### Symptom
Output interleaves the left and right columns line-by-line, and section headers
are contaminated with adjacent-column body text.

```
## Abstract variables such as treatment condition and acquisition
batch.The task is to estimate a discrete phenotype class
Wepresent a realisticEnglishsamplepaperdesigned while preservingenough ...
```
TOC entries such as `Results tion.`, `研究目的 した。`, `2 Introduction respect
to trac...` are the same defect seen through header detection.

### Root cause (layered — this is NOT only "the column detector")

**1a. `LineReconstructor` merges spans across the column gutter.**
`reconstruct_page` groups spans into a logical line purely by Y-proximity
(`y_diff <= font_size * 0.5`), with **no horizontal-gap awareness**. In a
two-column layout the left- and right-column lines share a baseline, so their
spans are grouped into a *single* line whose text is `left…right…` concatenated
and whose bbox spans the full page width.

Evidence — reconstructed lines, `ja_twocol.pdf` p.3 (`x=[left..right]`):
```
L x=[48..563] y=623 "情報システムの観点からは、処理の自動化と人間による評価項目、"   ← left+right merged
L x=[54..572] y=555 "tion science and bioinformatic…"  (en p.0, n_spans=4, full width)
L x=[54..573] y=344 "2 Introduction respect to trac…"  ← header "2 Introduction" merged with right column
```

**1b. `ColumnDetector` runs *after* line reconstruction and is fed the merged,
full-width lines.** It builds an X-histogram of *line left-edges*; merged lines
all start at the left-column x (~48), collapsing the histogram to a single peak
→ **1 column detected**. Even for pages where lines do *not* fully merge, the
per-Y-zone independent detection with a hard 20 % peak threshold is unstable.

Evidence — detected column counts per page:
```
en_twocol_figs.pdf: page0 [2,1]  page3 [1,2,1]  page8 [1] …   (flapping)
ja_twocol.pdf:      pages 0–7,9–15 all [1]; only page8 [2,1]  (collapsed to 1 col)
```

**1c. Architecture ordering.** The pipeline is `spans → lines → columns →
blocks`. Robust column handling needs column boundaries established *from spans*
(or the raw gutter) **before** lines are reconstructed, so lines are only ever
built within a single column. The current order makes 1b structurally unable to
recover from 1a.

### Recommended fix
Re-order / augment layout so the column structure is derived from the raw span
X-distribution (vertical-whitespace "gutter" / projection-profile detection) at
the page or region level **before** line reconstruction; assign each span to a
column; reconstruct lines *within* each column; emit columns left-to-right,
top-to-bottom. This fixes 1a and 1b together and removes the fragile per-zone
20 %-peak heuristic.

Minimum-viable alternative (smaller diff, less robust): make
`LineReconstructor` gutter-aware — split a Y-group wherever the horizontal gap
between consecutive spans exceeds a detected/estimated gutter width. Measured
gutter here is ~18 pt (≈2× the 9 pt CJK font), so a fixed multiple-of-font
threshold is brittle; a detected gutter is preferred. Recommend the
detect-columns-first approach as the primary fix, keeping the gutter split as a
fallback for single-region pages.

### Risk
Medium. Touches the layout core (`line_reconstructor`, `column_detector`,
pipeline ordering). Must preserve the **No Silent Drop** invariant (every span
lands in exactly one column/line) and keep single-column and mixed-region pages
working. Needs the committed fixtures as regression guards plus targeted unit
tests for gutter detection.

---

## Problem 2 — English inter-word spaces are lost (`Wepresent`, `resultingPDF`)

### Root cause (single, precise)
`pdf_oxide` represents inter-word spacing by emitting the space as a **leading
space inside the following span's text**, not as a separate whitespace span or a
positional gap. `convert_span` (`extract/pdf_reader.rs`) calls
`raw.text.trim()`, which **strips exactly those word-boundary spaces**.

Evidence — raw `pdf_oxide` spans, `en_twocol_figs.pdf` p.0:
```
[030] x=53.5  w=14.7 text="We"        (right edge = 68.2)
[031] x=67.6  w=34.2 text=" present"  ← leading space; left edge 67.6 OVERLAPS prev
```
After `trim()` → `"We"` + `"present"`. The geometry cannot recover the space:
`LineReconstructor::needs_space` computes `gap = next.left - prev.right =
67.6 - 68.2 = -0.6` (< threshold) → no space inserted → `Wepresent`.

The `pdf_oxide` `TextSpan` also carries `char_spacing` / `word_spacing` fields
(currently ignored), but here the authoritative signal is simply the leading
space already in `raw.text`.

### Recommended fix
Stop discarding word-boundary whitespace in `convert_span`: preserve a
significant leading/trailing space (normalize internal runs, drop pure-whitespace
spans only), and update `LineReconstructor::group_to_line` to concatenate using
the embedded spaces without double-spacing (a span that already begins with a
space suppresses the heuristic `needs_space` insertion; the first span of a line
is left-trimmed). CJK is unaffected (no inter-word spaces are emitted — see the
JA span dump, where each physical line is one gap-free span).

### Risk
Low. Localized to `convert_span` + `group_to_line`. Guard against double spaces
and leading/trailing space on line text with unit tests.

---

## Problem 3 — Chunks exceed `max_tokens` (not CJK-specific)

### Correction to the earlier report
The default `chunks --max-tokens` is **4000**, not 1000. At the default there is
**no** over-budget chunk (JA max 3783, EN max 3910). The earlier "16/20 over"
figure used the wrong 1000 threshold. The real defect only manifests once the
budget is small enough to force splitting.

### Root cause (single, precise)
The section splitter packs body paragraphs up to `max_tokens`, then **prepends
the `[Context: …]` breadcrumb prefix and the carried-over overlap** and counts
both in `estimated_tokens` — but the packing loop's budget check counts *only*
the body paragraphs. So the emitted chunk = (body up to budget) + prefix +
overlap, breaching the documented "no chunk exceeds `max_tokens`" contract.

Evidence — `ja_twocol.pdf --max-tokens 1000`, count of chunks over 1000 / max:
```
--overlap 0  --no-section-context :  0 / 60   max 1000   ← core splitter is correct
--overlap 0  (context on)         : 28 / 60   max 1175   ← prefix adds ~175
--overlap 200 --no-section-context: 28 / 60   max 1201   ← overlap adds ~200
--overlap 200 (context on)        : 35 / 60   max 1377   ← both stack
```

### Recommended fix
Reserve budget for the non-body additions when packing: effective packing budget
= `max_tokens − prefix_tokens − overlap_tokens` (floored at a sane minimum). The
core sentence/char-window splitter already keeps pieces within budget, so only
the accumulate-and-flush loop in `split_section_text` needs the reserved-budget
accounting; the overlap re-seed must also respect it.

### Risk
Low. Confined to `output/chunker.rs`. Latent at the default 4000 budget; real for
smaller budgets (common for RAG). Add a regression test asserting every chunk's
`estimated_tokens <= max_tokens` across the fixtures at several budgets (the one
unavoidable exception: a single indivisible token run, already documented).

---

## Summary

| # | Problem | Root cause | Fix size | Risk |
|---|---------|-----------|----------|------|
| 1 | 2-column reading order | Lines reconstructed across the gutter; columns detected after, per-zone, from merged lines | Large (layout core) | Med |
| 2 | English word spaces lost | `convert_span` `.trim()` strips pdf_oxide's leading-space word boundaries | Small | Low |
| 3 | Chunks over `max_tokens` | prefix + overlap added on top of a full-budget chunk, not reserved for | Small | Low |

Recommended sequence: **#2 → #3 → #1** (land the two low-risk, high-clarity fixes
first; tackle the layout re-order last as its own reviewed change). All coding
delegated to a lower-power subagent per task; each verified and committed
independently, one task = one commit.
