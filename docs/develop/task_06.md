# Task 06: LineReconstructor

## Overview

Implement `LineReconstructor` which groups `TextSpan`s into logical `TextLine`s by Y-coordinate
proximity. This is the first step in the layout analysis pipeline and processes spans
page-by-page.

Algorithm:
1. Group spans by page.
2. Within each page, sort spans by Y-coordinate (descending = top-first).
3. Group spans whose Y difference is within `font_size * y_tolerance_factor` (default 0.5).
4. Sort each group by X-coordinate (left to right).
5. Insert spaces between spans where the horizontal gap exceeds ~30% of a character width.
6. Emit one `TextLine` per group.

## Related Information
- **Plan**: `/Users/sonokawa/.claude/plans/fuzzy-drifting-summit.md` (Task 6)
- **Design doc**: `docs/arch/02_DESIGN.md` § 2.3 layout — LineReconstructor
- **Spec**: `docs/arch/01_SPECIFICATION.md` § 2.3 F-002
- **Overview**: `docs/develop/overview.md`
- **Dependencies**: Task 02 (types) — can run in parallel with Tasks 03-05

## Files to Create

- [ ] `crates/pdf-lay-core/src/layout/mod.rs`
- [ ] `crates/pdf-lay-core/src/layout/line_reconstructor.rs`

## Files to Modify

- [ ] `crates/pdf-lay-core/src/lib.rs` — uncomment `pub mod layout;`

## Implementation Steps

### Step 1: `layout/mod.rs`

```rust
//! Layout analysis layer: line reconstruction and column detection.

mod line_reconstructor;
mod column_detector;  // added in Task 07

pub use line_reconstructor::LineReconstructor;
// pub use column_detector::ColumnDetector;  // uncommented in Task 07
```

Note: `column_detector` module is declared now but implemented in Task 07. Either leave the
`pub use` commented out, or create a stub `column_detector.rs` with an empty struct.

### Step 2: `layout/line_reconstructor.rs`

```rust
//! Reconstructs logical text lines from individual TextSpan objects.

use std::collections::BTreeMap;
use crate::types::{Rect, TextLine, TextSpan};

/// Groups `TextSpan`s into logical `TextLine`s based on Y-coordinate proximity.
pub struct LineReconstructor {
    /// Spans whose Y-tops differ by less than `font_size * y_tolerance_factor`
    /// are placed on the same line.
    y_tolerance_factor: f64,
}

impl Default for LineReconstructor {
    fn default() -> Self {
        Self { y_tolerance_factor: 0.5 }
    }
}

impl LineReconstructor {
    /// Create with default tolerance (0.5 × font_size).
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the Y-tolerance factor.
    pub fn with_tolerance(mut self, factor: f64) -> Self {
        self.y_tolerance_factor = factor;
        self
    }

    /// Reconstruct lines from a flat slice of spans (may span multiple pages).
    pub fn reconstruct(&self, spans: &[TextSpan]) -> Vec<TextLine> {
        if spans.is_empty() {
            return Vec::new();
        }

        // Group by page (BTreeMap preserves page order).
        let mut by_page: BTreeMap<u32, Vec<&TextSpan>> = BTreeMap::new();
        for span in spans {
            by_page.entry(span.page).or_default().push(span);
        }

        let mut all_lines = Vec::new();
        for (_, page_spans) in by_page {
            all_lines.extend(self.reconstruct_page(&page_spans));
        }
        all_lines
    }

    fn reconstruct_page<'a>(&self, spans: &[&'a TextSpan]) -> Vec<TextLine> {
        if spans.is_empty() {
            return Vec::new();
        }

        // Sort descending by top Y (PDF Y-up, so higher Y = higher on page).
        let mut sorted: Vec<&TextSpan> = spans.to_vec();
        sorted.sort_by(|a, b| {
            b.bbox.top
                .partial_cmp(&a.bbox.top)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.bbox.left.partial_cmp(&b.bbox.left).unwrap_or(std::cmp::Ordering::Equal))
        });

        // Group into lines by Y proximity.
        let mut groups: Vec<Vec<&TextSpan>> = Vec::new();
        let mut current_group: Vec<&TextSpan> = vec![sorted[0]];

        for &span in &sorted[1..] {
            let ref_span = current_group.last().unwrap();
            let tolerance = ref_span.font_size * self.y_tolerance_factor;
            let y_diff = (ref_span.bbox.top - span.bbox.top).abs();

            if y_diff <= tolerance {
                current_group.push(span);
            } else {
                groups.push(std::mem::take(&mut current_group));
                current_group = vec![span];
            }
        }
        if !current_group.is_empty() {
            groups.push(current_group);
        }

        // Convert each group to a TextLine.
        groups
            .into_iter()
            .filter_map(|group| self.group_to_line(group))
            .collect()
    }

    fn group_to_line(&self, mut group: Vec<&TextSpan>) -> Option<TextLine> {
        if group.is_empty() {
            return None;
        }

        // Sort left-to-right by X.
        group.sort_by(|a, b| {
            a.bbox.left.partial_cmp(&b.bbox.left).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Build text with inter-span spaces.
        let mut text = String::new();
        let mut prev: Option<&TextSpan> = None;
        for span in &group {
            if let Some(p) = prev {
                if Self::needs_space(p, span) {
                    text.push(' ');
                }
            }
            text.push_str(&span.text);
            prev = Some(span);
        }

        // Compute bbox as union of all spans.
        let bbox = group
            .iter()
            .map(|s| s.bbox.clone())
            .reduce(|acc, b| acc.union(&b))
            .unwrap();

        // Determine primary font (by longest span text length).
        let primary = group
            .iter()
            .max_by_key(|s| s.text.len())
            .unwrap();

        // Count bold spans by character weight.
        let bold_chars: usize = group.iter()
            .filter(|s| s.is_bold)
            .map(|s| s.text.len())
            .sum();
        let total_chars: usize = group.iter().map(|s| s.text.len()).sum();
        let is_bold = total_chars > 0 && bold_chars * 2 >= total_chars;

        let page = group[0].page;
        let baseline_y = bbox.bottom;

        Some(TextLine {
            spans: group.into_iter().cloned().collect(),
            text,
            bbox,
            page,
            baseline_y,
            primary_font_size: primary.font_size,
            primary_font_name: primary.font_name.clone(),
            is_bold,
        })
    }

    /// Determine if a space should be inserted between two adjacent spans.
    fn needs_space(prev: &TextSpan, next: &TextSpan) -> bool {
        let gap = next.bbox.left - prev.bbox.right;
        let char_width = prev.font_size * 0.5; // rough approximation
        gap > char_width * 0.3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_span(text: &str, left: f64, top: f64, font_size: f64, page: u32) -> TextSpan {
        TextSpan {
            text: text.to_string(),
            font_name: "Regular".to_string(),
            font_size,
            is_bold: false,
            is_italic: false,
            bbox: Rect::new(
                left,
                top,
                left + text.len() as f64 * font_size * 0.5,
                top - font_size,
            ),
            page,
        }
    }

    #[test]
    fn same_y_groups_into_one_line() {
        let spans = vec![
            make_span("Hello", 0.0, 100.0, 10.0, 0),
            make_span("World", 60.0, 100.0, 10.0, 0),
        ];
        let lines = LineReconstructor::new().reconstruct(&spans);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].text.contains("Hello"));
        assert!(lines[0].text.contains("World"));
    }

    #[test]
    fn different_y_creates_two_lines() {
        let spans = vec![
            make_span("Line1", 0.0, 100.0, 10.0, 0),
            make_span("Line2", 0.0, 80.0, 10.0, 0), // 20pt gap > 10 * 0.5 = 5pt tolerance
        ];
        let lines = LineReconstructor::new().reconstruct(&spans);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn spans_sorted_left_to_right_within_line() {
        let spans = vec![
            make_span("B", 50.0, 100.0, 10.0, 0),
            make_span("A", 0.0, 100.0, 10.0, 0),
        ];
        let lines = LineReconstructor::new().reconstruct(&spans);
        assert_eq!(lines.len(), 1);
        // A should come before B
        assert!(lines[0].text.starts_with("A"));
    }

    #[test]
    fn space_inserted_on_large_gap() {
        let spans = vec![
            make_span("Hello", 0.0, 100.0, 10.0, 0),
            // Gap of 10pt between spans, char_width ≈ 5pt, threshold 1.5pt → needs space
            make_span("World", 40.0, 100.0, 10.0, 0),
        ];
        let lines = LineReconstructor::new().reconstruct(&spans);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "Hello World");
    }

    #[test]
    fn empty_input_returns_empty() {
        let lines = LineReconstructor::new().reconstruct(&[]);
        assert!(lines.is_empty());
    }

    #[test]
    fn multiple_pages_stay_separate() {
        let spans = vec![
            make_span("Page0", 0.0, 100.0, 10.0, 0),
            make_span("Page1", 0.0, 100.0, 10.0, 1),
        ];
        let lines = LineReconstructor::new().reconstruct(&spans);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].page, 0);
        assert_eq!(lines[1].page, 1);
    }

    #[test]
    fn mixed_font_sizes_bold_detection() {
        let mut spans = vec![
            make_span("Bold", 0.0, 100.0, 12.0, 0),
            make_span("Regular", 50.0, 100.0, 12.0, 0),
        ];
        spans[0].is_bold = true;
        let lines = LineReconstructor::new().reconstruct(&spans);
        // "Bold" is 4 chars out of 11 total — not majority bold
        assert!(!lines[0].is_bold);
    }
}
```

## Acceptance Criteria

- [ ] All unit tests pass: `cargo test -p pdf-lay-core -- layout::line_reconstructor`
  - `same_y_groups_into_one_line`
  - `different_y_creates_two_lines`
  - `spans_sorted_left_to_right_within_line`
  - `space_inserted_on_large_gap`
  - `empty_input_returns_empty`
  - `multiple_pages_stay_separate`
  - `mixed_font_sizes_bold_detection`
- [ ] `TextLine::bbox` is the union of all constituent span bboxes
- [ ] `TextLine::primary_font_size` is the font size of the longest-text span
- [ ] Lines from different pages are never merged
- [ ] `cargo clippy -p pdf-lay-core -- -D warnings` passes

## Dependencies

- Task 02 (types) must be completed first.
- This task can run in parallel with Tasks 03, 04, 05, and 12.

## Commit Message

```
feat(layout): add LineReconstructor grouping TextSpans into TextLines by Y-proximity
```
