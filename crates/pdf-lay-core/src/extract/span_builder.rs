//! Groups individual character glyphs into coherent text spans.
//!
//! `pdf_oxide` already returns multi-character spans (complete strings from
//! Tj/TJ operators per ISO 32000-1:2008 §9.4.4), so `SpanBuilder::merge` is
//! primarily a safety net.  If a future version of pdf_oxide or an alternative
//! back-end returns character-level glyphs, this module will coalesce them into
//! word/phrase-level `TextSpan` units.

use crate::types::TextSpan;

/// Merges adjacent character-level `TextSpan`s into word/phrase spans.
///
/// When two consecutive spans share the same font, are on the same baseline,
/// and are horizontally close enough (within `gap_factor × font_size`), they
/// are merged into a single span.  A space is inserted between them when the
/// horizontal gap exceeds `space_threshold × font_size`.
pub struct SpanBuilder {
    /// Maximum horizontal gap between consecutive glyphs to be merged,
    /// expressed as a fraction of the font size.  Default: 0.5.
    gap_factor: f64,
}

impl Default for SpanBuilder {
    fn default() -> Self {
        Self { gap_factor: 0.5 }
    }
}

impl SpanBuilder {
    /// Create a `SpanBuilder` with the default gap factor (0.5 × font_size).
    pub fn new() -> Self {
        Self::default()
    }

    /// Merge character-level spans from one or more pages into phrase spans.
    ///
    /// Two adjacent spans are merged when:
    /// 1. They are on the same page.
    /// 2. They share the same `font_name` (exact match).
    /// 3. Their font sizes are within 0.1 pt of each other.
    /// 4. Their baselines (bbox.bottom) differ by less than `font_size × 0.3`.
    /// 5. The horizontal gap between them is less than `font_size × gap_factor`.
    ///
    /// Spans are first sorted by page, then by Y descending (top of page first),
    /// then by X ascending (left to right).
    pub fn merge(&self, mut spans: Vec<TextSpan>) -> Vec<TextSpan> {
        if spans.len() <= 1 {
            return spans;
        }

        // Sort: page asc → Y desc (top first) → X asc (left first)
        spans.sort_by(|a, b| {
            a.page
                .cmp(&b.page)
                .then(
                    b.bbox
                        .top
                        .partial_cmp(&a.bbox.top)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
                .then(
                    a.bbox
                        .left
                        .partial_cmp(&b.bbox.left)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
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

    /// Returns `true` if `b` should be merged into `a`.
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

        // Same baseline: bottom coordinates must be within font_size × 0.3
        let y_diff = (a.bbox.bottom - b.bbox.bottom).abs();
        if y_diff > a.font_size * 0.3 {
            return false;
        }

        // Horizontal gap: b must start close to (or within) a's right edge
        let gap = b.bbox.left - a.bbox.right;
        gap < a.font_size * self.gap_factor
    }

    /// Merge two spans into one.
    ///
    /// A single space is inserted between the texts when the gap exceeds
    /// 15 % of the font size (roughly half a narrow character width).
    fn merge_two(&self, a: TextSpan, b: TextSpan) -> TextSpan {
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
    use crate::types::Rect;

    /// Build a minimal TextSpan for testing.
    fn make_char_span(text: &str, left: f64, page: u32) -> TextSpan {
        let font_size = 10.0;
        let char_width = font_size * 0.6;
        TextSpan {
            text: text.to_string(),
            font_name: "Regular".to_string(),
            font_size,
            is_bold: false,
            is_italic: false,
            bbox: Rect::new(left, font_size, left + char_width, 0.0),
            page,
        }
    }

    #[test]
    fn merges_adjacent_glyphs_same_font() {
        let builder = SpanBuilder::new();
        let spans = vec![
            make_char_span("H", 0.0, 0),
            make_char_span("e", 6.0, 0),
            make_char_span("l", 12.0, 0),
            make_char_span("l", 18.0, 0),
            make_char_span("o", 24.0, 0),
        ];
        let merged = builder.merge(spans);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].text, "Hello");
    }

    #[test]
    fn does_not_merge_different_fonts() {
        let builder = SpanBuilder::new();
        let s1 = make_char_span("A", 0.0, 0);
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
    fn does_not_merge_large_gap() {
        let builder = SpanBuilder::new();
        // font_size = 10, gap_factor = 0.5 → max merge gap = 5.0
        // gap between s1 right (6.0) and s2 left (60.0) is 54.0 >> 5.0
        let s1 = make_char_span("Hello", 0.0, 0);
        let s2 = make_char_span("World", 60.0, 0);
        let merged = builder.merge(vec![s1, s2]);
        assert_eq!(merged.len(), 2, "Spans with large gap should not be merged");
    }

    #[test]
    fn inserts_space_on_moderate_gap() {
        let builder = SpanBuilder::new();
        // Create two spans that are close enough to merge but far enough to need a space.
        // font_size = 10, 15 % threshold = 1.5 pt; space_gap > 1.5 → space inserted.
        let font_size = 10.0_f64;
        let s1 = TextSpan {
            text: "Foo".to_string(),
            font_name: "R".to_string(),
            font_size,
            is_bold: false,
            is_italic: false,
            bbox: Rect::new(0.0, font_size, 20.0, 0.0),
            page: 0,
        };
        // Gap = 23.0 - 20.0 = 3.0 > 1.5 → space expected
        // 3.0 < 0.5 × 10 = 5.0 → merge allowed
        let s2 = TextSpan {
            text: "Bar".to_string(),
            font_name: "R".to_string(),
            font_size,
            is_bold: false,
            is_italic: false,
            bbox: Rect::new(23.0, font_size, 40.0, 0.0),
            page: 0,
        };
        let merged = builder.merge(vec![s1, s2]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].text, "Foo Bar");
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

    #[test]
    fn multi_char_spans_are_passthrough() {
        // When pdf_oxide returns already-merged spans (e.g. "Hello World"),
        // SpanBuilder should not split or modify them.
        let builder = SpanBuilder::new();
        let s1 = TextSpan {
            text: "Hello World".to_string(),
            font_name: "Regular".to_string(),
            font_size: 12.0,
            is_bold: false,
            is_italic: false,
            bbox: Rect::new(0.0, 12.0, 80.0, 0.0),
            page: 0,
        };
        let s2 = TextSpan {
            text: "Second span".to_string(),
            font_name: "Regular".to_string(),
            font_size: 12.0,
            is_bold: false,
            is_italic: false,
            bbox: Rect::new(200.0, 12.0, 280.0, 0.0),
            page: 0,
        };
        let merged = builder.merge(vec![s1.clone(), s2.clone()]);
        // Gap = 200 - 80 = 120 >> 0.5 × 12 = 6 → not merged
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].text, "Hello World");
        assert_eq!(merged[1].text, "Second span");
    }

    #[test]
    fn merged_bbox_contains_both_spans() {
        let builder = SpanBuilder::new();
        let s1 = make_char_span("A", 0.0, 0);
        let s2 = make_char_span("B", 6.0, 0);
        let merged = builder.merge(vec![s1, s2]);
        assert_eq!(merged.len(), 1);
        let bbox = &merged[0].bbox;
        assert!(
            bbox.left <= 0.0,
            "left should be at or before 0.0, got {:.1}",
            bbox.left
        );
        assert!(
            bbox.right >= 12.0,
            "right should include B's right edge (≥12.0), got {:.1}",
            bbox.right
        );
    }
}
