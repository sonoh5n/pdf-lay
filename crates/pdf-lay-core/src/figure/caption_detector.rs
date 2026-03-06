//! Detects figure and table captions from TextBlocks using regex patterns.

use regex::Regex;

use crate::types::{Rect, TextBlock};

/// Semantic type of a detected caption.
#[derive(Debug, Clone, PartialEq)]
pub enum CaptionType {
    /// A figure caption (matched by "Fig.", "Fig", "Figure").
    Figure,
    /// A table caption (matched by "Table", "Tab.").
    Table,
}

/// Metadata about a detected caption.
#[derive(Debug, Clone)]
pub struct CaptionInfo {
    /// Index into the `blocks` slice where this caption was found.
    pub block_index: usize,
    /// Semantic type of the caption (Figure or Table).
    pub caption_type: CaptionType,
    /// Prefix string as matched (e.g. "Fig.", "Figure", "Table").
    pub prefix: String,
    /// Caption number (e.g. 1 for "Fig. 1").
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

impl CaptionDetector {
    /// Create a new `CaptionDetector` with compiled regex patterns.
    pub fn new() -> Self {
        Self {
            patterns: vec![
                CaptionPattern {
                    regex: Regex::new(r"(?i)^(Fig\.?|Figure)\s*(\d+)\s*[:.]?\s*(.*)")
                        .expect("figure caption regex is valid"),
                    caption_type: CaptionType::Figure,
                },
                CaptionPattern {
                    regex: Regex::new(r"(?i)^(Table|Tab\.)\s*(\d+)\s*[:.]?\s*(.*)")
                        .expect("table caption regex is valid"),
                    caption_type: CaptionType::Table,
                },
            ],
        }
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
                        let description = caps
                            .get(3)
                            .map(|m| m.as_str().trim().to_string())
                            .unwrap_or_default();
                        return Some(CaptionInfo {
                            block_index: i,
                            caption_type: pattern.caption_type.clone(),
                            prefix: caps[1].to_string(),
                            number: caps[2].parse().ok(),
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
}
