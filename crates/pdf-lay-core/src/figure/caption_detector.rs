//! Detects figure and table captions from TextBlocks using regex patterns.

use regex::Regex;

use crate::config::CaptionConfig;
use crate::error::PdfLayWarning;
use crate::types::{Rect, TextBlock};

/// Semantic type of a detected caption.
#[derive(Debug, Clone, PartialEq)]
pub enum CaptionType {
    /// A figure caption (matched by "Fig.", "Fig", "Figure", "FIG.", or 「図」).
    Figure,
    /// A table caption (matched by "Table", "Tab.", or 「表」).
    Table,
    /// A chemistry-style reaction scheme caption (matched by "Scheme N").
    /// Treated as image-matchable, like [`CaptionType::Figure`].
    Scheme,
    /// A chart caption (matched by "Chart N"). Treated as image-matchable,
    /// like [`CaptionType::Figure`].
    Chart,
}

/// Metadata about a detected caption.
#[derive(Debug, Clone)]
pub struct CaptionInfo {
    /// Index into the `blocks` slice where this caption was found.
    pub block_index: usize,
    /// Semantic type of the caption (Figure, Table, Scheme, or Chart).
    pub caption_type: CaptionType,
    /// Prefix string as matched (e.g. "Fig.", "Figure", "Table", "図").
    pub prefix: String,
    /// Caption number (e.g. 1 for "Fig. 1"). Supplementary (`S1`) and
    /// subfigure-lettered (`1a`) numbers normalize to their leading integer
    /// (`S1` → `Some(1)`, `1a` → `Some(1)`); full-width digits (`１`) are
    /// folded to ASCII before parsing. `None` when no number could be parsed
    /// (the caption is still detected and reported).
    pub number: Option<u32>,
    /// Description text after the prefix and number.
    pub description: String,
    /// Full original text of the caption block.
    pub full_text: String,
    /// Zero-based page index where the caption appears.
    pub page: u32,
    /// Bounding box of the caption block.
    pub bbox: Rect,
}

struct CaptionPattern {
    regex: Regex,
    caption_type: CaptionType,
}

/// Detects caption blocks using regex matching on block text.
///
/// The `Regex` objects are compiled once at construction and reused for all
/// calls to [`CaptionDetector::detect`], avoiding per-call compilation cost.
pub struct CaptionDetector {
    patterns: Vec<CaptionPattern>,
}

/// Half-width ASCII digit corresponding to a full-width digit (U+FF10..=U+FF19),
/// or the character unchanged if it is not a full-width digit.
fn fold_fullwidth_digit(c: char) -> char {
    match c {
        '\u{FF10}'..='\u{FF19}' => {
            let offset = c as u32 - '\u{FF10}' as u32;
            char::from_digit(offset, 10).unwrap_or(c)
        }
        other => other,
    }
}

/// Parse a caption's number capture (group 2 of a caption regex) into an
/// integer, tolerating the variants the P4-4 pattern set allows:
///
/// - supplementary prefix: `S1`, `s12` → `1`, `12`
/// - subfigure letter suffix: `1a`, `2b` → `1`, `2`
/// - full-width digits: `１`, `２３` → `1`, `23`
///
/// Returns `None` (rather than panicking) when no leading digit run is found
/// or the run does not fit in a `u32`; the caller still reports the caption
/// itself with `number: None` (No Silent Drop — a caption is never discarded
/// merely because its number could not be parsed).
fn parse_caption_number(raw: &str) -> Option<u32> {
    let folded: String = raw.chars().map(fold_fullwidth_digit).collect();
    let without_prefix = folded
        .strip_prefix('s')
        .or_else(|| folded.strip_prefix('S'))
        .unwrap_or(&folded);
    let digits: String = without_prefix
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

// Built-in pattern set (P4-4). Each pattern is anchored at line start (`^`)
// to avoid turning ordinary sentences into captions, and allows half-width
// space/tab or full-width space (`\u{3000}`) between the prefix, the number,
// an optional delimiter, and the trailing description. The number group
// (`s?\d+[a-z]?`) accepts a supplementary `S` prefix (`S1`) and a
// subfigure-letter suffix (`1a`); see `parse_caption_number` for how that
// group is normalized into `CaptionInfo::number`.
const FIGURE_PATTERN: &str = r"(?i)^(fig(?:ure)?|fig\.)[ \t\u{3000}]*(s?\d+[a-z]?)[ \t\u{3000}]*[:.\-\u{2013}]?[ \t\u{3000}]*(.*)";
const TABLE_PATTERN: &str = r"(?i)^(tab(?:le)?|tab\.)[ \t\u{3000}]*(s?\d+[a-z]?)[ \t\u{3000}]*[:.\-\u{2013}]?[ \t\u{3000}]*(.*)";
const SCHEME_PATTERN: &str =
    r"(?i)^(scheme)[ \t\u{3000}]*(s?\d+[a-z]?)[ \t\u{3000}]*[:.\-\u{2013}]?[ \t\u{3000}]*(.*)";
const CHART_PATTERN: &str =
    r"(?i)^(chart)[ \t\u{3000}]*(s?\d+[a-z]?)[ \t\u{3000}]*[:.\-\u{2013}]?[ \t\u{3000}]*(.*)";
// Japanese patterns: no case-folding needed (no case in Kanji), digit class
// accepts both half-width and full-width digits, delimiter class accepts the
// full-width colon/comma/period equivalents used in Japanese typesetting.
const FIGURE_PATTERN_JA: &str = r"^(図)[ \t\u{3000}]*([0-9\u{FF10}-\u{FF19}]+)[ \t\u{3000}]*[:\u{FF1A}.\u{3001}\u{FF0E}\-\u{2013}]?[ \t\u{3000}]*(.*)";
const TABLE_PATTERN_JA: &str = r"^(表)[ \t\u{3000}]*([0-9\u{FF10}-\u{FF19}]+)[ \t\u{3000}]*[:\u{FF1A}.\u{3001}\u{FF0E}\-\u{2013}]?[ \t\u{3000}]*(.*)";

impl CaptionDetector {
    /// Create a new `CaptionDetector` with the default configuration: the
    /// built-in English patterns (Figure/Table/Scheme/Chart, including `FIG.`,
    /// supplementary `S1`, and subfigure-lettered `1a` numbering) plus the
    /// Japanese `図`/`表` patterns, and no user-supplied extra patterns.
    ///
    /// Equivalent to [`CaptionDetector::from_config`] with
    /// [`CaptionConfig::default()`], discarding the (always-empty, since there
    /// are no user patterns to fail) warning list. Use `from_config` directly
    /// when patterns are user-configurable and warnings must be surfaced.
    pub fn new() -> Self {
        Self::from_config(&CaptionConfig::default()).0
    }

    /// Build a `CaptionDetector` from a [`CaptionConfig`].
    ///
    /// The built-in Figure/Table patterns always apply. Scheme/Chart and
    /// Japanese patterns are included when their respective config toggles
    /// are `true` (the default). User-supplied `extra_figure_patterns` /
    /// `extra_table_patterns` are compiled and appended; a pattern that fails
    /// to compile is **not** a panic — it is skipped and reported via a
    /// returned [`PdfLayWarning::InvalidCaptionPattern`], and the remaining
    /// patterns still apply (No Silent Drop / no-panic-on-user-input).
    pub fn from_config(cfg: &CaptionConfig) -> (Self, Vec<PdfLayWarning>) {
        let mut patterns = vec![
            CaptionPattern {
                regex: Regex::new(FIGURE_PATTERN).expect("built-in figure caption regex is valid"),
                caption_type: CaptionType::Figure,
            },
            CaptionPattern {
                regex: Regex::new(TABLE_PATTERN).expect("built-in table caption regex is valid"),
                caption_type: CaptionType::Table,
            },
        ];

        if cfg.enable_scheme_chart {
            patterns.push(CaptionPattern {
                regex: Regex::new(SCHEME_PATTERN).expect("built-in scheme caption regex is valid"),
                caption_type: CaptionType::Scheme,
            });
            patterns.push(CaptionPattern {
                regex: Regex::new(CHART_PATTERN).expect("built-in chart caption regex is valid"),
                caption_type: CaptionType::Chart,
            });
        }

        if cfg.enable_japanese {
            patterns.push(CaptionPattern {
                regex: Regex::new(FIGURE_PATTERN_JA)
                    .expect("built-in Japanese figure caption regex is valid"),
                caption_type: CaptionType::Figure,
            });
            patterns.push(CaptionPattern {
                regex: Regex::new(TABLE_PATTERN_JA)
                    .expect("built-in Japanese table caption regex is valid"),
                caption_type: CaptionType::Table,
            });
        }

        let mut warnings = Vec::new();
        for raw in &cfg.extra_figure_patterns {
            match Regex::new(raw) {
                Ok(regex) => patterns.push(CaptionPattern {
                    regex,
                    caption_type: CaptionType::Figure,
                }),
                Err(e) => warnings.push(PdfLayWarning::InvalidCaptionPattern {
                    pattern: raw.clone(),
                    reason: e.to_string(),
                }),
            }
        }
        for raw in &cfg.extra_table_patterns {
            match Regex::new(raw) {
                Ok(regex) => patterns.push(CaptionPattern {
                    regex,
                    caption_type: CaptionType::Table,
                }),
                Err(e) => warnings.push(PdfLayWarning::InvalidCaptionPattern {
                    pattern: raw.clone(),
                    reason: e.to_string(),
                }),
            }
        }

        (Self { patterns }, warnings)
    }

    /// Detect captions in a slice of `TextBlock`s.
    ///
    /// Returns one [`CaptionInfo`] per matching block, preserving input order.
    pub fn detect(&self, blocks: &[TextBlock]) -> Vec<CaptionInfo> {
        blocks
            .iter()
            .enumerate()
            .filter_map(|(i, block)| {
                let text = block.text.trim();
                for pattern in &self.patterns {
                    if let Some(caps) = pattern.regex.captures(text) {
                        // Capture groups beyond the overall match are looked
                        // up defensively (`get` rather than indexing): a
                        // user-supplied extra pattern (`CaptionConfig`) is not
                        // guaranteed to define groups 1-3, and this must never
                        // panic on PDF/config-derived input.
                        let prefix = caps
                            .get(1)
                            .map(|m| m.as_str().to_string())
                            .unwrap_or_default();
                        let number = caps.get(2).and_then(|m| parse_caption_number(m.as_str()));
                        let description = caps
                            .get(3)
                            .map(|m| m.as_str().trim().to_string())
                            .unwrap_or_default();
                        return Some(CaptionInfo {
                            block_index: i,
                            caption_type: pattern.caption_type.clone(),
                            prefix,
                            number,
                            description,
                            full_text: text.to_string(),
                            page: block.page,
                            bbox: block.bbox.clone(),
                        });
                    }
                }
                None
            })
            .collect()
    }
}

impl Default for CaptionDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BlockType, TextBlock};

    fn make_block(text: &str, page: u32) -> TextBlock {
        TextBlock {
            global_index: 0,
            lines: vec![],
            text: text.to_string(),
            bbox: Rect::new(72.0, 400.0, 540.0, 390.0),
            page,
            column_index: 0,
            block_type: BlockType::Caption,
        }
    }

    #[test]
    fn figure_caption_detected() {
        let detector = CaptionDetector::new();
        let block = make_block("Fig. 1: Overview of the proposed system.", 0);
        let captions = detector.detect(&[block]);
        assert_eq!(captions.len(), 1);
        assert_eq!(captions[0].caption_type, CaptionType::Figure);
        assert_eq!(captions[0].number, Some(1));
        assert_eq!(captions[0].prefix, "Fig.");
        assert!(!captions[0].description.is_empty());
    }

    #[test]
    fn figure_without_period_detected() {
        let detector = CaptionDetector::new();
        let block = make_block("Figure 3 Schematic diagram of the process.", 0);
        let captions = detector.detect(&[block]);
        assert_eq!(captions.len(), 1);
        assert_eq!(captions[0].number, Some(3));
    }

    #[test]
    fn table_caption_detected() {
        let detector = CaptionDetector::new();
        let block = make_block("Table 2: Performance comparison.", 1);
        let captions = detector.detect(&[block]);
        assert_eq!(captions.len(), 1);
        assert_eq!(captions[0].caption_type, CaptionType::Table);
        assert_eq!(captions[0].number, Some(2));
    }

    #[test]
    fn body_text_not_detected() {
        let detector = CaptionDetector::new();
        let block = make_block("This is a normal paragraph.", 0);
        let captions = detector.detect(&[block]);
        assert!(captions.is_empty());
    }

    #[test]
    fn case_insensitive_matching() {
        let detector = CaptionDetector::new();
        let block = make_block("FIGURE 5. Results on the test set.", 0);
        let captions = detector.detect(&[block]);
        assert_eq!(captions.len(), 1);
    }

    #[test]
    fn block_index_correct() {
        let detector = CaptionDetector::new();
        let blocks = vec![
            make_block("Body text.", 0),
            make_block("Fig. 1: A caption.", 0),
        ];
        let captions = detector.detect(&blocks);
        assert_eq!(captions.len(), 1);
        assert_eq!(captions[0].block_index, 1);
    }

    // ---- P4-4: broadened caption pattern tests (design's Given/When/Then) ----

    #[test]
    fn all_caps_fig_with_period_and_subfigure_letter_detected() {
        let detector = CaptionDetector::new();
        let block = make_block("FIG. 1a Overview", 0);
        let captions = detector.detect(&[block]);
        assert_eq!(captions.len(), 1);
        assert_eq!(captions[0].caption_type, CaptionType::Figure);
        assert_eq!(captions[0].number, Some(1));
    }

    #[test]
    fn scheme_caption_detected_as_scheme_type() {
        let detector = CaptionDetector::new();
        let block = make_block("Scheme 2: Synthesis route", 0);
        let captions = detector.detect(&[block]);
        assert_eq!(captions.len(), 1);
        assert_eq!(captions[0].caption_type, CaptionType::Scheme);
        assert_eq!(captions[0].number, Some(2));
    }

    #[test]
    fn chart_caption_detected_as_chart_type() {
        let detector = CaptionDetector::new();
        let block = make_block("Chart 3: Market share breakdown", 0);
        let captions = detector.detect(&[block]);
        assert_eq!(captions.len(), 1);
        assert_eq!(captions[0].caption_type, CaptionType::Chart);
        assert_eq!(captions[0].number, Some(3));
    }

    #[test]
    fn supplementary_figure_number_detected_and_preserved_in_full_text() {
        let detector = CaptionDetector::new();
        let block = make_block("Figure S1. Supplementary data", 0);
        let captions = detector.detect(&[block]);
        assert_eq!(captions.len(), 1);
        assert_eq!(captions[0].caption_type, CaptionType::Figure);
        assert_eq!(captions[0].number, Some(1));
        assert!(captions[0].full_text.contains("S1"));
    }

    #[test]
    fn japanese_figure_caption_detected() {
        let detector = CaptionDetector::new();
        let block = make_block("図1 提案手法の概要", 0);
        let captions = detector.detect(&[block]);
        assert_eq!(captions.len(), 1);
        assert_eq!(captions[0].caption_type, CaptionType::Figure);
        assert_eq!(captions[0].number, Some(1));
    }

    #[test]
    fn japanese_table_caption_with_fullwidth_digit_and_colon_detected() {
        let detector = CaptionDetector::new();
        let block = make_block("表２：性能比較", 0);
        let captions = detector.detect(&[block]);
        assert_eq!(captions.len(), 1);
        assert_eq!(captions[0].caption_type, CaptionType::Table);
        assert_eq!(captions[0].number, Some(2));
    }

    #[test]
    fn body_sentence_not_starting_with_prefix_not_detected() {
        // Design's literal example: a sentence that merely mentions "table"
        // without the block starting with a caption prefix must never be
        // detected (line-start anchor preserved from before P4-4).
        let detector = CaptionDetector::new();
        let block = make_block(
            "This table shows the accuracy of the different methods across datasets.",
            0,
        );
        let captions = detector.detect(&[block]);
        assert!(captions.is_empty());
    }

    #[test]
    fn mid_sentence_figure_reference_not_detected() {
        // A figure reference that doesn't start the block ("As shown in Fig.
        // 3...") must not be detected as a caption — only line-start matches
        // are considered captions. Resolving such in-text references is
        // explicitly out of scope for this detector (see design's
        // non-scope note).
        let detector = CaptionDetector::new();
        let block = make_block(
            "As shown in Figure 2, the relationship holds across all runs.",
            0,
        );
        let captions = detector.detect(&[block]);
        assert!(captions.is_empty());
    }

    #[test]
    fn fig_without_space_before_number_detected() {
        let detector = CaptionDetector::new();
        let block = make_block("Fig.1 Overview diagram.", 0);
        let captions = detector.detect(&[block]);
        assert_eq!(captions.len(), 1);
        assert_eq!(captions[0].number, Some(1));
    }

    #[test]
    fn fullwidth_space_before_number_detected() {
        let detector = CaptionDetector::new();
        // "FIG" + full-width space + "1" + full-width space + description.
        let block = make_block("FIG\u{3000}1\u{3000}Overview diagram.", 0);
        let captions = detector.detect(&[block]);
        assert_eq!(captions.len(), 1);
        assert_eq!(captions[0].caption_type, CaptionType::Figure);
        assert_eq!(captions[0].number, Some(1));
    }

    #[test]
    fn invalid_extra_pattern_does_not_panic_and_warns_but_builtins_still_work() {
        let cfg = CaptionConfig {
            extra_figure_patterns: vec!["(unclosed".to_string()],
            ..CaptionConfig::default()
        };
        let (detector, warnings) = CaptionDetector::from_config(&cfg);
        assert_eq!(warnings.len(), 1);
        assert!(matches!(
            warnings[0],
            PdfLayWarning::InvalidCaptionPattern { .. }
        ));
        // Built-in patterns still work despite the bad user pattern.
        let block = make_block("Fig. 1: Still works.", 0);
        let captions = detector.detect(&[block]);
        assert_eq!(captions.len(), 1);
    }

    #[test]
    fn valid_extra_figure_pattern_is_applied() {
        let cfg = CaptionConfig {
            extra_figure_patterns: vec![r"^(?i)^(plate)\s*(\d+)\s*[:.]?\s*(.*)".to_string()],
            ..CaptionConfig::default()
        };
        let (detector, warnings) = CaptionDetector::from_config(&cfg);
        assert!(warnings.is_empty());
        let block = make_block("Plate 4: Micrograph.", 0);
        let captions = detector.detect(&[block]);
        assert_eq!(captions.len(), 1);
        assert_eq!(captions[0].caption_type, CaptionType::Figure);
        assert_eq!(captions[0].number, Some(4));
    }

    #[test]
    fn disabling_japanese_restores_pre_p4_4_behavior() {
        let cfg = CaptionConfig {
            enable_japanese: false,
            ..CaptionConfig::default()
        };
        let (detector, _warnings) = CaptionDetector::from_config(&cfg);
        let block = make_block("図1 提案手法の概要", 0);
        let captions = detector.detect(&[block]);
        assert!(captions.is_empty());
    }

    #[test]
    fn disabling_scheme_chart_restores_pre_p4_4_behavior() {
        let cfg = CaptionConfig {
            enable_scheme_chart: false,
            ..CaptionConfig::default()
        };
        let (detector, _warnings) = CaptionDetector::from_config(&cfg);
        let block = make_block("Scheme 2: Synthesis route", 0);
        let captions = detector.detect(&[block]);
        assert!(captions.is_empty());
    }

    // ---- number normalization helper ----

    #[test]
    fn parse_caption_number_variants() {
        assert_eq!(parse_caption_number("S1"), Some(1));
        assert_eq!(parse_caption_number("s12"), Some(12));
        assert_eq!(parse_caption_number("1a"), Some(1));
        assert_eq!(parse_caption_number("\u{FF11}"), Some(1)); // full-width "1"
        assert_eq!(parse_caption_number(""), None);
        assert_eq!(parse_caption_number("a"), None);
    }
}
