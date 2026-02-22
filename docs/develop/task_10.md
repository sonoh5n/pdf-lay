# Task 10: HeaderDetector

## Overview

Implement `HeaderDetector` which scores each `TextBlock` as a section header candidate
using multiple signals and promotes blocks with score >= 4 to `SectionHeader` structs.

Scoring signals:
- +3: Matches a numbering pattern (roman numeral, arabic, arabic.arabic, arabic.arabic.arabic, alpha)
- +2: All uppercase text
- +2: Bold font
- +1: Font size ratio > 1.1 relative to body font
- +2: Known section name (case-insensitive)
- +1: Single line

Exclusion conditions (checked before scoring):
- Text length > 120 characters
- More than 3 lines
- Text is only digits (page number)
- Font size > body × 1.8 AND not a known section name (likely a title, not a header)

The `level` is set based on the matching numbering pattern:
- Roman numeral (I., II.) → level 1
- Arabic (3.) → level 1
- Alpha (A.) → level 2
- Arabic.Arabic (3.1) → level 2
- Arabic.Arabic.Arabic (3.1.1) → level 3
- No numbering but other signals → level 1 or 2 based on font size

## Related Information
- **Plan**: `/Users/sonokawa/.claude/plans/fuzzy-drifting-summit.md` (Task 10)
- **Design doc**: `docs/arch/02_DESIGN.md` § 2.4 structure — HeaderDetector
- **Spec**: `docs/arch/01_SPECIFICATION.md` § 2.6 F-005
- **Overview**: `docs/develop/overview.md`
- **Dependencies**: Task 09 (BlockClassifier) must be completed first

## Files to Create

- [ ] `crates/pdf-lay-core/src/structure/header_detector.rs`

## Files to Modify

- [ ] `crates/pdf-lay-core/src/structure/mod.rs` — uncomment `pub use header_detector::HeaderDetector`
- [ ] `crates/pdf-lay-core/Cargo.toml` — add `regex` dependency if not already present

## Implementation Steps

### Step 1: `structure/header_detector.rs`

```rust
//! Detects section headers from TextBlocks using multi-signal scoring.

use regex::Regex;
use crate::types::{BlockType, SectionHeader, TextBlock};

const KNOWN_SECTION_NAMES: &[&str] = &[
    "ABSTRACT", "INTRODUCTION", "BACKGROUND", "RELATED WORK",
    "METHOD", "METHODS", "METHODOLOGY", "APPROACH",
    "EXPERIMENT", "EXPERIMENTS", "EXPERIMENTAL",
    "RESULTS", "RESULT", "RESULTS AND DISCUSSION",
    "DISCUSSION", "ANALYSIS", "CONCLUSION", "CONCLUSIONS",
    "SUMMARY", "REFERENCES", "BIBLIOGRAPHY",
    "ACKNOWLEDGMENT", "ACKNOWLEDGMENTS", "APPENDIX",
    "SUPPLEMENTARY", "SUPPORTING INFORMATION",
];

/// Detects section headers from blocks using scoring heuristics.
pub struct HeaderDetector {
    body_font_size: f64,
    min_score: u32,
    max_chars: usize,
    max_lines: usize,
    // Compiled regex patterns (stored to avoid recompilation).
    re_roman:        Regex,
    re_arabic_dot_dot: Regex,
    re_arabic_dot:   Regex,
    re_arabic:       Regex,
    re_alpha:        Regex,
}

impl HeaderDetector {
    /// Create a new detector with the given body font size.
    pub fn new(body_font_size: f64) -> Self {
        Self {
            body_font_size,
            min_score: 4,
            max_chars: 120,
            max_lines: 3,
            re_roman:          Regex::new(r"^([IVX]+\.)\s+").unwrap(),
            re_arabic_dot_dot: Regex::new(r"^(\d+\.\d+\.\d+)[\s.]").unwrap(),
            re_arabic_dot:     Regex::new(r"^(\d+\.\d+)[\s.]").unwrap(),
            re_arabic:         Regex::new(r"^(\d+\.)\s+").unwrap(),
            re_alpha:          Regex::new(r"^([A-Z]\.)\s+").unwrap(),
        }
    }

    /// Detect section headers from a slice of blocks.
    ///
    /// Only blocks already classified as `SectionHeader` or `SubsectionHeader`
    /// by `BlockClassifier`, or blocks that score highly enough, are promoted.
    pub fn detect(&self, blocks: &[TextBlock]) -> Vec<SectionHeader> {
        blocks
            .iter()
            .enumerate()
            .filter_map(|(i, block)| self.try_detect(block, i))
            .collect()
    }

    fn try_detect(&self, block: &TextBlock, block_index: usize) -> Option<SectionHeader> {
        let text = block.text.trim();

        // Quick exclusions.
        if text.len() > self.max_chars || block.lines.len() > self.max_lines {
            return None;
        }
        if text.chars().all(|c| c.is_ascii_digit() || c.is_whitespace()) {
            return None; // Page number.
        }

        let font_size = block.primary_font_size();
        let is_bold = block.is_bold();
        let size_ratio = if self.body_font_size > 0.0 {
            font_size / self.body_font_size
        } else {
            1.0
        };

        // Exclude very large fonts that aren't known names (likely document title).
        if size_ratio > 1.8 && !self.is_known_name(text) {
            return None;
        }

        let mut score: u32 = 0;
        let mut level: u8 = 1;
        let mut numbering: Option<String> = None;

        // Numbering pattern (+3 points).
        if let Some((num, pat_level)) = self.match_numbering(text) {
            score += 3;
            level = pat_level;
            numbering = Some(num);
        }

        // All uppercase (+2).
        if self.is_all_caps(text) {
            score += 2;
        }

        // Bold (+2).
        if is_bold {
            score += 2;
        }

        // Larger font (+1).
        if size_ratio > 1.1 {
            score += 1;
        }

        // Known section name (+2).
        if self.is_known_name(text) {
            score += 2;
        }

        // Single line (+1).
        if block.lines.len() == 1 {
            score += 1;
        }

        if score < self.min_score {
            return None;
        }

        // Refine level if no numbering was detected.
        if numbering.is_none() {
            level = if size_ratio > 1.15 || self.is_all_caps(text) { 1 } else { 2 };
        }

        let clean_text = self.clean_header_text(text, &numbering);

        Some(SectionHeader {
            text: text.to_string(),
            clean_text,
            level,
            numbering,
            page: block.page,
            bbox: block.bbox.clone(),
            block_index,
        })
    }

    // ---- pattern matching ----

    /// Match a numbering pattern and return `(number_string, level)`.
    fn match_numbering(&self, text: &str) -> Option<(String, u8)> {
        let t = text.trim();

        if let Some(caps) = self.re_roman.captures(t) {
            return Some((caps[1].to_string(), 1));
        }
        if let Some(caps) = self.re_arabic_dot_dot.captures(t) {
            return Some((caps[1].to_string(), 3));
        }
        if let Some(caps) = self.re_arabic_dot.captures(t) {
            return Some((caps[1].to_string(), 2));
        }
        if let Some(caps) = self.re_arabic.captures(t) {
            return Some((caps[1].to_string(), 1));
        }
        if let Some(caps) = self.re_alpha.captures(t) {
            return Some((caps[1].to_string(), 2));
        }
        None
    }

    fn is_all_caps(&self, text: &str) -> bool {
        let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
        !letters.is_empty() && letters.iter().all(|c| c.is_uppercase())
    }

    fn is_known_name(&self, text: &str) -> bool {
        let upper = text.to_uppercase();
        let clean = upper.trim();
        KNOWN_SECTION_NAMES
            .iter()
            .any(|&name| clean == name || clean.contains(name))
    }

    /// Remove leading numbering from header text.
    fn clean_header_text(&self, text: &str, numbering: &Option<String>) -> String {
        if let Some(num) = numbering {
            text.trim_start_matches(num.as_str()).trim().to_string()
        } else {
            text.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BlockType, Rect, TextLine, TextBlock};

    fn make_block(text: &str, font_size: f64, bold: bool, lines: usize) -> TextBlock {
        let line = TextLine {
            spans: vec![],
            text: text.to_string(),
            bbox: Rect::new(72.0, 700.0, 540.0, 700.0 - font_size),
            page: 0,
            baseline_y: 690.0,
            primary_font_size: font_size,
            primary_font_name: "Regular".to_string(),
            is_bold: bold,
        };
        TextBlock {
            global_index: 0,
            lines: vec![line; lines.max(1)],
            text: text.to_string(),
            bbox: Rect::new(72.0, 700.0, 540.0, 690.0),
            page: 0,
            column_index: 0,
            block_type: BlockType::BodyText,
        }
    }

    #[test]
    fn roman_numeral_header_detected() {
        let detector = HeaderDetector::new(10.0);
        let block = make_block("II. KNOWLEDGE GRAPHS", 11.0, true, 1);
        let headers = detector.detect(&[block]);
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].level, 1);
        assert_eq!(headers[0].numbering, Some("II.".to_string()));
        assert_eq!(headers[0].clean_text, "KNOWLEDGE GRAPHS");
    }

    #[test]
    fn arabic_dot_header_level1() {
        let detector = HeaderDetector::new(10.0);
        let block = make_block("3. Methods", 10.0, true, 1);
        let headers = detector.detect(&[block]);
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].level, 1);
    }

    #[test]
    fn arabic_dot_dot_header_level2() {
        let detector = HeaderDetector::new(10.0);
        let block = make_block("3.1 Data Collection", 10.0, true, 1);
        let headers = detector.detect(&[block]);
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].level, 2);
    }

    #[test]
    fn arabic_dot_dot_dot_header_level3() {
        let detector = HeaderDetector::new(10.0);
        let block = make_block("3.1.1 Sampling", 10.0, true, 1);
        let headers = detector.detect(&[block]);
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].level, 3);
    }

    #[test]
    fn known_name_all_caps_detected() {
        let detector = HeaderDetector::new(10.0);
        // "ABSTRACT" matches known name (+2) + all caps (+2) + single line (+1) = 5 >= 4
        let block = make_block("ABSTRACT", 10.0, false, 1);
        let headers = detector.detect(&[block]);
        assert_eq!(headers.len(), 1);
    }

    #[test]
    fn long_text_excluded() {
        let detector = HeaderDetector::new(10.0);
        let long_text = "A".repeat(121);
        let block = make_block(&long_text, 12.0, true, 1);
        let headers = detector.detect(&[block]);
        assert!(headers.is_empty());
    }

    #[test]
    fn page_number_excluded() {
        let detector = HeaderDetector::new(10.0);
        let block = make_block("42", 9.0, false, 1);
        let headers = detector.detect(&[block]);
        assert!(headers.is_empty());
    }

    #[test]
    fn low_score_body_text_excluded() {
        let detector = HeaderDetector::new(10.0);
        // Short text, not bold, same size → score likely < 4
        let block = make_block("Some regular text", 10.0, false, 1);
        let headers = detector.detect(&[block]);
        assert!(headers.is_empty());
    }

    #[test]
    fn block_index_preserved() {
        let detector = HeaderDetector::new(10.0);
        let blocks = vec![
            make_block("Body text", 10.0, false, 1),
            make_block("II. INTRODUCTION", 11.0, true, 1),
        ];
        let headers = detector.detect(&blocks);
        assert_eq!(headers[0].block_index, 1);
    }
}
```

## Acceptance Criteria

- [ ] All unit tests pass: `cargo test -p pdf-lay-core -- structure::header_detector`
  - `roman_numeral_header_detected`
  - `arabic_dot_header_level1`
  - `arabic_dot_dot_header_level2`
  - `arabic_dot_dot_dot_header_level3`
  - `known_name_all_caps_detected`
  - `long_text_excluded`
  - `page_number_excluded`
  - `low_score_body_text_excluded`
  - `block_index_preserved`
- [ ] `Regex` objects are stored in the struct (not recompiled per call)
- [ ] `SectionHeader::clean_text` has the numbering prefix stripped
- [ ] `SectionHeader::block_index` matches the block's position in the input slice
- [ ] `cargo clippy -p pdf-lay-core -- -D warnings` passes

## Dependencies

- Task 09 (BlockClassifier) must be completed first.

## Commit Message

```
feat(structure): add HeaderDetector with multi-signal scoring for section header extraction
```
