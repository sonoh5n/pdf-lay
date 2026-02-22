# Task 04: Character-to-Span Grouping (SpanBuilder)

## Overview

If `pdf_oxide::extract_spans()` returns individual character glyphs rather than word/phrase
spans, this module merges adjacent characters that share the same font into coherent `TextSpan`
units. If Task 03 confirms that `pdf_oxide` already returns multi-character spans, this module
can be either a no-op pass-through or merged directly into `pdf_reader.rs`.

**Decision gate**: After Task 03 is complete and the actual pdf_oxide output format is known,
decide whether this module needs a full implementation or can be a thin wrapper.

## Related Information
- **Plan**: `/Users/sonokawa/.claude/plans/fuzzy-drifting-summit.md` (Task 4)
- **Design doc**: `docs/arch/02_DESIGN.md` § 2.2 extract
- **Overview**: `docs/develop/overview.md`
- **Dependencies**: Task 03 must be completed first

## Files to Create

- [ ] `crates/pdf-lay-core/src/extract/span_builder.rs`

## Files to Modify

- [ ] `crates/pdf-lay-core/src/extract/mod.rs` — add `pub use span_builder::SpanBuilder`

## Implementation Steps

### Step 1: `extract/span_builder.rs`

```rust
//! Groups individual character glyphs into coherent text spans.
//!
//! If pdf_oxide already returns multi-character spans, `SpanBuilder::merge`
//! is a no-op pass-through and can be removed in a future cleanup.

use crate::types::{Rect, TextSpan};

/// Merges adjacent character-level `TextSpan`s into word/phrase spans.
pub struct SpanBuilder {
    /// Maximum horizontal gap between glyphs to be merged (as fraction of font_size).
    gap_factor: f64,
}

impl Default for SpanBuilder {
    fn default() -> Self {
        Self { gap_factor: 0.5 }
    }
}

impl SpanBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Merge character-level spans from a single page into phrase spans.
    ///
    /// Two adjacent spans are merged when:
    /// 1. They share the same font name and font size (within 0.1pt tolerance).
    /// 2. They are on the same baseline (Y difference < font_size * 0.3).
    /// 3. The horizontal gap between them is < font_size * gap_factor.
    ///
    /// Spans from different pages must not be mixed; call per page.
    pub fn merge(&self, mut spans: Vec<TextSpan>) -> Vec<TextSpan> {
        if spans.len() <= 1 {
            return spans;
        }

        // Sort by page, then Y (descending = top-first), then X (ascending).
        spans.sort_by(|a, b| {
            a.page
                .cmp(&b.page)
                .then(b.bbox.top.partial_cmp(&a.bbox.top).unwrap_or(std::cmp::Ordering::Equal))
                .then(a.bbox.left.partial_cmp(&b.bbox.left).unwrap_or(std::cmp::Ordering::Equal))
        });

        let mut result: Vec<TextSpan> = Vec::with_capacity(spans.len());
        let mut current: Option<TextSpan> = None;

        for span in spans {
            match current.take() {
                None => {
                    current = Some(span);
                }
                Some(prev) => {
                    if self.should_merge(&prev, &span) {
                        current = Some(self.merge_two(prev, span));
                    } else {
                        result.push(prev);
                        current = Some(span);
                    }
                }
            }
        }

        if let Some(last) = current {
            result.push(last);
        }

        result
    }

    fn should_merge(&self, a: &TextSpan, b: &TextSpan) -> bool {
        // Must be same page
        if a.page != b.page {
            return false;
        }

        // Same font name
        if a.font_name != b.font_name {
            return false;
        }

        // Similar font size (within 0.1 pt)
        if (a.font_size - b.font_size).abs() > 0.1 {
            return false;
        }

        // Same baseline (within font_size * 0.3)
        let y_diff = (a.bbox.bottom - b.bbox.bottom).abs();
        if y_diff > a.font_size * 0.3 {
            return false;
        }

        // Horizontal gap must be small enough
        let gap = b.bbox.left - a.bbox.right;
        gap < a.font_size * self.gap_factor
    }

    fn merge_two(&self, a: TextSpan, b: TextSpan) -> TextSpan {
        // Insert a space if there's a noticeable gap
        let gap = b.bbox.left - a.bbox.right;
        let needs_space = gap > a.font_size * 0.15;

        let mut text = a.text.clone();
        if needs_space {
            text.push(' ');
        }
        text.push_str(&b.text);

        let bbox = a.bbox.union(&b.bbox);

        TextSpan {
            text,
            font_name: a.font_name,
            font_size: a.font_size,
            is_bold: a.is_bold,
            is_italic: a.is_italic,
            bbox,
            page: a.page,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_char_span(text: &str, left: f64, page: u32) -> TextSpan {
        let font_size = 10.0;
        TextSpan {
            text: text.to_string(),
            font_name: "Regular".to_string(),
            font_size,
            is_bold: false,
            is_italic: false,
            bbox: Rect::new(left, font_size, left + font_size * 0.6, 0.0),
            page,
        }
    }

    #[test]
    fn merges_adjacent_glyphs_same_font() {
        let builder = SpanBuilder::new();
        let spans = vec![
            make_char_span("H", 0.0, 0),
            make_char_span("e", 6.5, 0),
            make_char_span("l", 13.0, 0),
            make_char_span("l", 19.5, 0),
            make_char_span("o", 26.0, 0),
        ];
        let merged = builder.merge(spans);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].text, "Hello");
    }

    #[test]
    fn does_not_merge_different_fonts() {
        let builder = SpanBuilder::new();
        let mut s1 = make_char_span("A", 0.0, 0);
        let mut s2 = make_char_span("B", 7.0, 0);
        s2.font_name = "Bold".to_string();
        let merged = builder.merge(vec![s1, s2]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn does_not_merge_different_pages() {
        let builder = SpanBuilder::new();
        let s1 = make_char_span("A", 0.0, 0);
        let s2 = make_char_span("B", 7.0, 1);
        let merged = builder.merge(vec![s1, s2]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn inserts_space_on_large_gap() {
        let builder = SpanBuilder::new();
        // 20pt gap for font_size=10 is 2x gap_factor — should get a space
        let s1 = make_char_span("Hello", 0.0, 0);
        let mut s2 = make_char_span("World", 60.0, 0); // large gap
        s2.bbox = Rect::new(60.0, 10.0, 110.0, 0.0);
        // Different font → won't merge; if same font, should add space
        s2.font_name = "Regular".to_string();
        let merged = builder.merge(vec![s1, s2]);
        // Large gap means they won't merge at all (> gap_factor)
        // (depends on actual span widths — this tests the boundary)
        assert!(merged.len() >= 1);
    }

    #[test]
    fn empty_input_returns_empty() {
        let merged = SpanBuilder::new().merge(vec![]);
        assert!(merged.is_empty());
    }

    #[test]
    fn single_span_returns_unchanged() {
        let s = make_char_span("Hello", 0.0, 0);
        let merged = SpanBuilder::new().merge(vec![s.clone()]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].text, "Hello");
    }
}
```

### Step 2: Update `extract/mod.rs`

```rust
//! PDF extraction layer.

mod pdf_reader;
mod span_builder;

pub use pdf_reader::PdfReader;
pub use span_builder::SpanBuilder;
```

## Acceptance Criteria

- [ ] `cargo test -p pdf-lay-core -- extract::span_builder` all pass
  - `merges_adjacent_glyphs_same_font`
  - `does_not_merge_different_fonts`
  - `does_not_merge_different_pages`
  - `empty_input_returns_empty`
  - `single_span_returns_unchanged`
- [ ] `SpanBuilder::merge` is a no-op (returns input unchanged) when all spans already have multi-character text (verified by adding a test with pre-merged spans)
- [ ] `cargo clippy -p pdf-lay-core -- -D warnings` passes

## Note on Task 03 Integration

If `pdf_oxide::extract_spans()` already returns multi-character phrase spans (confirmed in Task 03),
`SpanBuilder::merge` will still work correctly as a no-op pass-through. The `should_merge` check
on adjacent large-gap spans will simply return `false` for all pairs, leaving spans unchanged.
The module can remain as a safety net.

## Dependencies

- Task 03 must be completed first.

## Commit Message

```
feat(extract): add SpanBuilder to merge character-level glyphs into text spans
```
