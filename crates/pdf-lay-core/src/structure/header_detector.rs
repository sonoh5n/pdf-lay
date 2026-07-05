//! Detects section headers from TextBlocks using multi-signal scoring.

use regex::Regex;

use crate::config::{HeaderDetectionConfig, default_known_section_names};
use crate::types::{BlockType, SectionHeader, TextBlock};

/// Max extra characters beyond a known name's length for the bounded-substring
/// match, so a long paragraph merely containing "METHOD" does not match.
const KNOWN_NAME_SUBSTR_MARGIN: usize = 8;

/// Detects section headers from blocks using scoring heuristics.
pub struct HeaderDetector {
    body_font_size: f64,
    min_score: u32,
    max_chars: usize,
    max_lines: usize,
    /// When true, blocks the classifier marked as non-body (caption, running
    /// head/foot, footnote, reference, page number) are excluded from header
    /// candidates. Set false to restore the legacy classification-agnostic
    /// behavior.
    respect_classification: bool,
    /// Known section names, pre-normalized (trimmed, full-width folded, upper).
    known_section_names: Vec<String>,
    /// Score bonus for CJK-script headings.
    cjk_heading_bonus: u32,
    // Compiled regex patterns (stored to avoid recompilation).
    re_roman: Regex,
    re_arabic_dot_dot: Regex,
    re_arabic_dot: Regex,
    re_arabic: Regex,
    re_alpha: Regex,
}

impl HeaderDetector {
    /// Create a new detector with the given body font size and default config.
    pub fn new(body_font_size: f64) -> Self {
        Self {
            body_font_size,
            min_score: 4,
            max_chars: 120,
            max_lines: 3,
            respect_classification: true,
            known_section_names: normalize_names(&default_known_section_names()),
            cjk_heading_bonus: 1,
            re_roman: Regex::new(r"^([IVX]+\.)\s+").unwrap(),
            re_arabic_dot_dot: Regex::new(r"^(\d+\.\d+\.\d+)[\s.]").unwrap(),
            re_arabic_dot: Regex::new(r"^(\d+\.\d+)[\s.]").unwrap(),
            re_arabic: Regex::new(r"^(\d+\.)\s+").unwrap(),
            re_alpha: Regex::new(r"^([A-Z]\.)\s+").unwrap(),
        }
    }

    /// Create a detector from a [`HeaderDetectionConfig`].
    pub fn with_config(body_font_size: f64, cfg: &HeaderDetectionConfig) -> Self {
        Self {
            min_score: cfg.min_score,
            max_chars: cfg.max_chars,
            max_lines: cfg.max_lines,
            respect_classification: cfg.respect_classification,
            known_section_names: normalize_names(&cfg.known_section_names),
            cjk_heading_bonus: cfg.cjk_heading_bonus,
            ..Self::new(body_font_size)
        }
    }

    /// Whether a block's type makes it eligible to be a section-header candidate.
    ///
    /// Non-body types (captions, running heads/feet, footnotes, references, page
    /// numbers) are excluded so the classifier's decision is respected rather
    /// than re-derived. Excluded blocks are only removed from header *candidacy*;
    /// they remain in the document as body/other content (No Silent Drop).
    fn is_header_eligible(block_type: &BlockType) -> bool {
        !matches!(
            block_type,
            BlockType::Caption
                | BlockType::PageNumber
                | BlockType::RunningHeader
                | BlockType::RunningFooter
                | BlockType::Footnote
                | BlockType::Reference
        )
    }

    /// Detect section headers from a slice of blocks.
    ///
    /// Only blocks that score >= `min_score` (default 4) are returned. When
    /// `respect_classification` is set, blocks the classifier marked as non-body
    /// are excluded from consideration.
    pub fn detect(&self, blocks: &[TextBlock]) -> Vec<SectionHeader> {
        blocks
            .iter()
            .filter(|block| {
                !self.respect_classification || Self::is_header_eligible(&block.block_type)
            })
            .filter_map(|block| self.try_detect(block, block.global_index))
            .collect()
    }

    fn try_detect(&self, block: &TextBlock, global_index: usize) -> Option<SectionHeader> {
        let text = block.text.trim();

        // Quick exclusions. Use character count (not UTF-8 byte length) so CJK
        // headings are not wrongly excluded.
        if text.chars().count() > self.max_chars || block.lines.len() > self.max_lines {
            return None;
        }
        if text
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_whitespace())
        {
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

        // CJK-script heading signal (alternative to all-caps for CJK languages).
        if self.is_cjk_heading_like(text) {
            score += self.cjk_heading_bonus;
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
            level = if size_ratio > 1.15 || self.is_all_caps(text) {
                1
            } else {
                2
            };
        }

        let clean_text = self.clean_header_text(text, &numbering);

        Some(SectionHeader {
            text: text.to_string(),
            clean_text,
            level,
            numbering,
            page: block.page,
            bbox: block.bbox.clone(),
            block_index: global_index,
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
        let norm = normalize(text);
        let norm_len = norm.chars().count();
        self.known_section_names.iter().any(|name| {
            if norm == *name {
                return true;
            }
            // Bounded substring: only match when the block is not much longer
            // than the known name, so a long paragraph containing "METHOD"
            // does not fire (S6).
            norm_len <= name.chars().count() + KNOWN_NAME_SUBSTR_MARGIN
                && norm.contains(name.as_str())
        })
    }

    /// Whether a block looks like a CJK-script heading: mostly CJK characters,
    /// short, and not ending in sentence-final punctuation.
    fn is_cjk_heading_like(&self, text: &str) -> bool {
        let t = text.trim();
        cjk_ratio(t) > 0.5 && t.chars().count() <= self.max_chars && !t.ends_with(['。', '．', '.'])
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

/// Normalize a string for known-name comparison: trim, fold full-width ASCII to
/// half-width, then uppercase (a no-op for CJK scripts).
fn normalize(s: &str) -> String {
    s.trim()
        .chars()
        .map(|c| {
            if ('\u{FF01}'..='\u{FF5E}').contains(&c) {
                char::from_u32(c as u32 - 0xFEE0).unwrap_or(c)
            } else {
                c
            }
        })
        .collect::<String>()
        .to_uppercase()
}

/// Normalize a list of known section names.
fn normalize_names(names: &[String]) -> Vec<String> {
    names.iter().map(|n| normalize(n)).collect()
}

/// Whether a character belongs to a CJK script (Han, Kana, Hangul).
fn is_cjk_char(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'   // CJK Unified Ideographs
        | '\u{3400}'..='\u{4DBF}' // CJK Unified Ideographs Extension A
        | '\u{F900}'..='\u{FAFF}' // CJK Compatibility Ideographs
        | '\u{3040}'..='\u{309F}' // Hiragana
        | '\u{30A0}'..='\u{30FF}' // Katakana
        | '\u{AC00}'..='\u{D7A3}' // Hangul Syllables
    )
}

/// Fraction of non-whitespace characters that are CJK-script characters.
fn cjk_ratio(text: &str) -> f64 {
    let letters: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    if letters.is_empty() {
        return 0.0;
    }
    let cjk = letters.iter().filter(|c| is_cjk_char(**c)).count();
    cjk as f64 / letters.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BlockType, Rect, TextBlock, TextLine};

    fn make_block(text: &str, font_size: f64, bold: bool, lines: usize) -> TextBlock {
        make_typed_block(text, font_size, bold, lines, BlockType::BodyText)
    }

    fn make_typed_block(
        text: &str,
        font_size: f64,
        bold: bool,
        lines: usize,
        block_type: BlockType,
    ) -> TextBlock {
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
            block_type,
        }
    }

    fn with_global_index(mut block: TextBlock, global_index: usize) -> TextBlock {
        block.global_index = global_index;
        block
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
        // Short text, not bold, same size -> score likely < 4
        let block = make_block("Some regular text", 10.0, false, 1);
        let headers = detector.detect(&[block]);
        assert!(headers.is_empty());
    }

    #[test]
    fn block_index_preserved() {
        let detector = HeaderDetector::new(10.0);
        let blocks = vec![
            with_global_index(make_block("Body text", 10.0, false, 1), 0),
            with_global_index(make_block("II. INTRODUCTION", 11.0, true, 1), 1),
        ];
        let headers = detector.detect(&blocks);
        assert_eq!(headers[0].block_index, 1);
    }

    #[test]
    fn block_index_is_global_index_not_slice_position() {
        // A header at slice position 1 but with global_index 42 must anchor by
        // global_index, so future filtering/reordering cannot mis-anchor it.
        let detector = HeaderDetector::new(10.0);
        let blocks = vec![
            with_global_index(make_block("Body text", 10.0, false, 1), 7),
            with_global_index(make_block("II. INTRODUCTION", 11.0, true, 1), 42),
        ];
        let headers = detector.detect(&blocks);
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].block_index, 42);
    }

    // ---- P1-1: classifier-informed candidate filtering -------------------

    #[test]
    fn caption_block_excluded_from_headers() {
        let detector = HeaderDetector::new(10.0);
        // Bold, all-caps, single line, known-name substring — would score high,
        // but its type is Caption so it must not become a header.
        let block = make_typed_block("TABLE 1 RESULTS", 11.0, true, 1, BlockType::Caption);
        assert!(detector.detect(&[block]).is_empty());
    }

    #[test]
    fn running_header_block_excluded() {
        let detector = HeaderDetector::new(10.0);
        let block = make_typed_block(
            "IEEE TRANSACTIONS ON EXAMPLES",
            10.0,
            true,
            1,
            BlockType::RunningHeader,
        );
        assert!(detector.detect(&[block]).is_empty());
    }

    #[test]
    fn footnote_block_excluded() {
        let detector = HeaderDetector::new(10.0);
        let block = make_typed_block(
            "1. Corresponding author",
            10.0,
            true,
            1,
            BlockType::Footnote,
        );
        assert!(detector.detect(&[block]).is_empty());
    }

    #[test]
    fn reference_block_excluded() {
        let detector = HeaderDetector::new(10.0);
        let block = make_typed_block("1. A. Author, Title.", 10.0, true, 1, BlockType::Reference);
        assert!(detector.detect(&[block]).is_empty());
    }

    #[test]
    fn bodytext_header_still_detected() {
        let detector = HeaderDetector::new(10.0);
        let block = make_typed_block("3. Methods", 10.0, true, 1, BlockType::BodyText);
        assert_eq!(detector.detect(&[block]).len(), 1);
    }

    #[test]
    fn respect_classification_false_restores_legacy() {
        // With classification disabled, a Caption block can still score as a header.
        let cfg = HeaderDetectionConfig {
            respect_classification: false,
            ..HeaderDetectionConfig::default()
        };
        let detector = HeaderDetector::with_config(10.0, &cfg);
        let block = make_typed_block("TABLE 1 RESULTS", 11.0, true, 1, BlockType::Caption);
        assert_eq!(detector.detect(&[block]).len(), 1);
    }

    // ---- P1-5: Unicode / CJK handling -----------------------------------

    #[test]
    fn char_count_filter_allows_long_cjk_heading() {
        let detector = HeaderDetector::new(10.0);
        // 40 CJK chars = 120 bytes (> max_chars as bytes) but 40 chars (<= 120).
        let text: String = "あ".repeat(40);
        let block = make_block(&text, 12.0, true, 1);
        // Should not be filtered out by the length guard (would have been under
        // the old byte-length check). It is a candidate; assert it isn't dropped
        // purely on length by checking a bold CJK line scores as a header.
        let headers = detector.detect(&[block]);
        assert_eq!(
            headers.len(),
            1,
            "long CJK heading must pass the length filter"
        );
    }

    #[test]
    fn cjk_heading_detected() {
        let detector = HeaderDetector::new(10.0);
        // "関連研究" (Related Work) — known Japanese name + CJK signal + bold.
        let block = make_block("関連研究", 10.0, true, 1);
        let headers = detector.detect(&[block]);
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].clean_text, "関連研究");
    }

    #[test]
    fn known_name_no_overfire_in_long_body() {
        let detector = HeaderDetector::new(10.0);
        // A long sentence that merely contains "method" must not be a known name.
        let long =
            "In this work we introduce a novel method for evaluating the approach across datasets";
        assert!(!detector.is_known_name(long));
    }

    #[test]
    fn configurable_known_names_extend() {
        let mut cfg = HeaderDetectionConfig::default();
        cfg.known_section_names.push("提案手法".to_string());
        let detector = HeaderDetector::with_config(10.0, &cfg);
        assert!(detector.is_known_name("提案手法"));
    }

    #[test]
    fn ascii_length_regression_unchanged() {
        let detector = HeaderDetector::new(10.0);
        let long_text = "A".repeat(121);
        let block = make_block(&long_text, 12.0, true, 1);
        assert!(detector.detect(&[block]).is_empty());
    }
}
