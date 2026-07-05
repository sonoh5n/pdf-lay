//! Detects section headers from TextBlocks using multi-signal scoring.

use crate::config::{HeaderDetectionConfig, default_known_section_names};
use crate::structure::numbering::NumberingParser;
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
    /// Bin width (pt) for clustering candidate font sizes into levels.
    cluster_bin_width: f64,
    /// Max gap (pt) between adjacent bins merged into one level.
    cluster_merge_gap: f64,
    /// Maximum assigned heading level.
    max_level: u8,
    /// Parses leading section numbers into structured keys.
    numbering_parser: NumberingParser,
}

/// A scored header candidate before its final (font-cluster) level is assigned.
struct Candidate {
    global_index: usize,
    page: u32,
    bbox: crate::types::Rect,
    text: String,
    clean_text: String,
    numbering: Option<String>,
    /// Level from numbering depth, if the header is numbered.
    numbering_level: Option<u8>,
    font_size: f64,
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
            cluster_bin_width: 0.5,
            cluster_merge_gap: 0.5,
            max_level: 6,
            numbering_parser: NumberingParser::new(),
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
            cluster_bin_width: cfg.cluster_bin_width,
            cluster_merge_gap: cfg.cluster_merge_gap,
            max_level: cfg.max_level,
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
    /// Two passes: (1) collect scored candidates; (2) assign each candidate a
    /// level from its numbering depth (if numbered) or from a document-global
    /// clustering of candidate font sizes. Font clustering lets three or more
    /// heading tiers be distinguished, unlike the previous isolated-threshold
    /// approach. When `respect_classification` is set, classifier-marked
    /// non-body blocks are excluded.
    pub fn detect(&self, blocks: &[TextBlock]) -> Vec<SectionHeader> {
        // Pass 1: collect candidates.
        let candidates: Vec<Candidate> = blocks
            .iter()
            .filter(|block| {
                !self.respect_classification || Self::is_header_eligible(&block.block_type)
            })
            .filter_map(|block| self.try_score(block, block.global_index))
            .collect();

        // Global font clustering → per-bin level.
        let level_of_bin = self.font_levels(&candidates);

        // Pass 2: finalize levels (numbering depth wins over font level).
        candidates
            .into_iter()
            .map(|c| {
                let font_level = *level_of_bin.get(&self.font_bin(c.font_size)).unwrap_or(&1);
                let level = c
                    .numbering_level
                    .unwrap_or(font_level)
                    .clamp(1, self.max_level);
                SectionHeader {
                    text: c.text,
                    clean_text: c.clean_text,
                    level,
                    numbering: c.numbering,
                    page: c.page,
                    bbox: c.bbox,
                    block_index: c.global_index,
                }
            })
            .collect()
    }

    /// Font-size bin index for clustering.
    fn font_bin(&self, font_size: f64) -> i64 {
        (font_size / self.cluster_bin_width).round() as i64
    }

    /// Cluster candidate font sizes into levels and return a bin → level map.
    ///
    /// Bins are ranked largest-first; adjacent bins within `cluster_merge_gap`
    /// are merged into one level (absorbs measurement jitter). Deterministic:
    /// depends only on the set of candidate font sizes.
    fn font_levels(&self, candidates: &[Candidate]) -> std::collections::HashMap<i64, u8> {
        let mut bins: Vec<i64> = candidates
            .iter()
            .map(|c| self.font_bin(c.font_size))
            .collect();
        bins.sort_unstable();
        bins.dedup();
        bins.reverse(); // largest font first

        let merge_thresh = (self.cluster_merge_gap / self.cluster_bin_width)
            .round()
            .max(1.0) as i64;
        let mut level_of_bin = std::collections::HashMap::new();
        let mut rank: u8 = 0;
        let mut prev: Option<i64> = None;
        for &b in &bins {
            if let Some(p) = prev
                && p - b > merge_thresh
            {
                rank = rank.saturating_add(1);
            }
            level_of_bin.insert(b, (rank + 1).min(self.max_level));
            prev = Some(b);
        }
        level_of_bin
    }

    fn try_score(&self, block: &TextBlock, global_index: usize) -> Option<Candidate> {
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

        // Structured numbering (+3 points). Numbering depth becomes the level
        // (overrides the font cluster) in pass 2.
        let parsed = self.numbering_parser.parse(text);
        let numbering_level = parsed.as_ref().map(|(key, _)| {
            score += 3;
            key.depth()
        });

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

        // Split the leading number (display string) from the clean title.
        let (numbering, clean_text) = match &parsed {
            Some((_, prefix_len)) => (
                Some(text[..*prefix_len].trim().to_string()),
                text[*prefix_len..].trim().to_string(),
            ),
            None => (None, text.to_string()),
        };

        Some(Candidate {
            global_index,
            page: block.page,
            bbox: block.bbox.clone(),
            text: text.to_string(),
            clean_text,
            numbering,
            numbering_level,
            font_size,
        })
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

    // ---- P1-3: font clustering for level ---------------------------------

    #[test]
    fn three_font_tiers_map_to_three_levels() {
        let detector = HeaderDetector::new(10.0);
        let blocks = vec![
            make_block("Alpha Section", 16.0, true, 1),
            make_block("Beta Section", 13.0, true, 1),
            make_block("Gamma Section", 11.5, true, 1),
        ];
        let headers = detector.detect(&blocks);
        assert_eq!(headers.len(), 3);
        let level = |name: &str| {
            headers
                .iter()
                .find(|h| h.clean_text.starts_with(name))
                .map(|h| h.level)
                .unwrap()
        };
        assert_eq!(level("Alpha"), 1);
        assert_eq!(level("Beta"), 2);
        assert_eq!(level("Gamma"), 3);
    }

    #[test]
    fn near_equal_font_sizes_merge_into_one_level() {
        let detector = HeaderDetector::new(10.0);
        let blocks = vec![
            make_block("First Heading", 12.4, true, 1),
            make_block("Second Heading", 12.6, true, 1),
        ];
        let headers = detector.detect(&blocks);
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0].level, headers[1].level);
    }

    #[test]
    fn numbering_overrides_font_level() {
        let detector = HeaderDetector::new(10.0);
        // Body-size numbered subsection: font alone would be level 1, but the
        // numbering depth (2) must win.
        let block = make_block("2.1 Data Collection", 10.0, false, 1);
        let headers = detector.detect(&[block]);
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].level, 2);
    }

    #[test]
    fn level_assignment_is_deterministic() {
        let detector = HeaderDetector::new(10.0);
        let blocks = vec![
            make_block("Gamma Section", 11.5, true, 1),
            make_block("Alpha Section", 16.0, true, 1),
            make_block("Beta Section", 13.0, true, 1),
        ];
        let run1: Vec<(String, u8)> = detector
            .detect(&blocks)
            .into_iter()
            .map(|h| (h.clean_text, h.level))
            .collect();
        let run2: Vec<(String, u8)> = detector
            .detect(&blocks)
            .into_iter()
            .map(|h| (h.clean_text, h.level))
            .collect();
        assert_eq!(run1, run2);
    }
}
