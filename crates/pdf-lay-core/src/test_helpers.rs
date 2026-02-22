//! Test helpers for building synthetic TextSpan, TextLine, TextBlock etc.
//!
//! Used throughout unit tests across all modules.
//! Only compiled under `#[cfg(test)]`.

use crate::types::{BlockType, Rect, TextBlock, TextLine, TextSpan};

/// Build a minimal TextSpan for use in tests.
pub fn make_span(text: &str, left: f64, top: f64, font_size: f64) -> TextSpan {
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
        page: 0,
    }
}

/// Build a bold TextSpan.
pub fn make_bold_span(text: &str, left: f64, top: f64, font_size: f64) -> TextSpan {
    let mut s = make_span(text, left, top, font_size);
    s.is_bold = true;
    s.font_name = "Bold".to_string();
    s
}

/// Build a minimal TextLine.
pub fn make_line(text: &str, left: f64, top: f64, font_size: f64, page: u32) -> TextLine {
    let span = {
        let mut s = make_span(text, left, top, font_size);
        s.page = page;
        s
    };
    let bbox = span.bbox.clone();
    TextLine {
        spans: vec![span],
        text: text.to_string(),
        bbox,
        page,
        baseline_y: top - font_size,
        primary_font_size: font_size,
        primary_font_name: "Regular".to_string(),
        is_bold: false,
    }
}

/// Build a bold TextLine.
pub fn make_bold_line(text: &str, left: f64, top: f64, font_size: f64, page: u32) -> TextLine {
    let mut l = make_line(text, left, top, font_size, page);
    l.is_bold = true;
    l.spans.iter_mut().for_each(|s| s.is_bold = true);
    l
}

/// Build a TextBlock from a single line.
pub fn make_block_from_line(line: TextLine, global_index: usize) -> TextBlock {
    let bbox = line.bbox.clone();
    let page = line.page;
    let text = line.text.clone();
    TextBlock {
        global_index,
        lines: vec![line],
        text,
        bbox,
        page,
        column_index: 0,
        block_type: BlockType::BodyText,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_span_produces_valid_bbox() {
        let s = make_span("Hello", 10.0, 100.0, 12.0);
        assert_eq!(s.text, "Hello");
        assert_eq!(s.font_size, 12.0);
        assert_eq!(s.bbox.left, 10.0);
        assert!(!s.is_bold);
        assert!(!s.is_italic);
        assert!(s.bbox.top > s.bbox.bottom, "top must exceed bottom");
    }

    #[test]
    fn make_bold_span_sets_bold_flag() {
        let s = make_bold_span("Bold", 0.0, 50.0, 10.0);
        assert!(s.is_bold);
        assert_eq!(s.font_name, "Bold");
    }

    #[test]
    fn make_line_links_span_and_bbox() {
        let l = make_line("Test", 5.0, 200.0, 11.0, 1);
        assert_eq!(l.page, 1);
        assert_eq!(l.text, "Test");
        assert_eq!(l.spans.len(), 1);
        assert_eq!(l.spans[0].page, 1);
        assert!(!l.is_bold);
        assert!(l.bbox.top > l.bbox.bottom);
    }

    #[test]
    fn make_bold_line_sets_flags() {
        let l = make_bold_line("Bold line", 0.0, 100.0, 12.0, 0);
        assert!(l.is_bold);
        assert!(l.spans.iter().all(|s| s.is_bold));
    }

    #[test]
    fn make_block_from_line_preserves_metadata() {
        let line = make_line("Block text", 72.0, 700.0, 10.0, 2);
        let block = make_block_from_line(line, 5);
        assert_eq!(block.global_index, 5);
        assert_eq!(block.page, 2);
        assert_eq!(block.text, "Block text");
        assert_eq!(block.lines.len(), 1);
        assert_eq!(block.block_type, BlockType::BodyText);
    }
}
