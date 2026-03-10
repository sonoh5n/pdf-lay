//! Classifies TextBlocks into semantic types (Caption, Header, BodyText, etc.).

use std::collections::HashMap;

use crate::types::{BlockType, TextBlock};

/// Known section name keywords (uppercase, for case-insensitive comparison).
const KNOWN_SECTION_NAMES: &[&str] = &[
    "ABSTRACT",
    "INTRODUCTION",
    "BACKGROUND",
    "RELATED WORK",
    "METHOD",
    "METHODS",
    "METHODOLOGY",
    "APPROACH",
    "EXPERIMENT",
    "EXPERIMENTS",
    "EXPERIMENTAL",
    "RESULTS",
    "RESULT",
    "RESULTS AND DISCUSSION",
    "DISCUSSION",
    "ANALYSIS",
    "CONCLUSION",
    "CONCLUSIONS",
    "SUMMARY",
    "REFERENCES",
    "BIBLIOGRAPHY",
    "ACKNOWLEDGMENT",
    "ACKNOWLEDGMENTS",
    "APPENDIX",
    "SUPPLEMENTARY",
    "SUPPORTING INFORMATION",
];

/// Classifies TextBlocks by analyzing font size, position, and text patterns.
pub struct BlockClassifier {
    /// Statistically determined body text font size.
    pub body_font_size: f64,
}

impl BlockClassifier {
    /// Build a classifier by computing the body font size from a set of blocks.
    ///
    /// Uses a character-count-weighted histogram: the modal font size bin is the body size.
    pub fn from_blocks(blocks: &[TextBlock]) -> Self {
        let body_font_size = Self::detect_body_font_size(blocks);
        Self { body_font_size }
    }

    /// Create with an explicit body font size (for testing).
    pub fn with_body_size(body_font_size: f64) -> Self {
        Self { body_font_size }
    }

    /// Classify all blocks in place (mutates `block_type`).
    pub fn classify_all(&self, blocks: &mut [TextBlock]) {
        for block in blocks.iter_mut() {
            block.block_type = self.classify(block);
        }
    }

    /// Detect repeated text patterns across pages as running headers/footers.
    ///
    /// Scans all blocks across pages and classifies them as `RunningHeader` or
    /// `RunningFooter` when the same normalised text (trimmed, lowercased) appears
    /// on **3 or more pages** in the same vertical zone:
    ///
    /// - **Top 10%** of page height → `RunningHeader`
    /// - **Bottom 10%** of page height → `RunningFooter`
    ///
    /// Blocks already classified as `PageNumber` are never upgraded to a running
    /// header/footer.
    ///
    /// This method is intended to be called **after** `classify_all` has been run
    /// on the same slice.
    pub fn detect_repeated_headers_footers(blocks: &mut [TextBlock]) {
        if blocks.is_empty() {
            return;
        }

        // --- Step 1: Infer page height for each page from the maximum `top` value
        //             seen among all blocks on that page.
        let mut page_heights: HashMap<u32, f64> = HashMap::new();
        for block in blocks.iter() {
            let entry = page_heights.entry(block.page).or_insert(0.0_f64);
            if block.bbox.top > *entry {
                *entry = block.bbox.top;
            }
        }

        // Threshold: 10% of page height.
        const ZONE_FRACTION: f64 = 0.10;

        // --- Step 2: For every block decide whether it falls into the header zone
        //             (top 10%) or footer zone (bottom 10%) of its page.
        //             Collect (normalized_text, zone, page) triples.
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        enum Zone {
            Header,
            Footer,
        }

        // Map (normalized_text, zone) -> set of page indices that contain it.
        let mut occurrences: HashMap<(String, Zone), std::collections::HashSet<u32>> =
            HashMap::new();

        for block in blocks.iter() {
            // Blocks already identified as page numbers are exempt.
            if block.block_type == BlockType::PageNumber {
                continue;
            }

            let page_height = match page_heights.get(&block.page) {
                Some(&h) if h > 0.0 => h,
                _ => continue,
            };

            let threshold = page_height * ZONE_FRACTION;
            let normalized = block.text.trim().to_lowercase();
            if normalized.is_empty() {
                continue;
            }

            let zone = if block.bbox.bottom >= page_height - threshold {
                // Block sits in the top 10% (high Y values).
                Some(Zone::Header)
            } else if block.bbox.top <= threshold {
                // Block sits in the bottom 10% (low Y values).
                Some(Zone::Footer)
            } else {
                None
            };

            if let Some(z) = zone {
                occurrences
                    .entry((normalized, z))
                    .or_default()
                    .insert(block.page);
            }
        }

        // --- Step 3: Collect keys that appear on 3+ distinct pages.
        let repeated: std::collections::HashSet<(String, Zone)> = occurrences
            .into_iter()
            .filter(|(_, pages)| pages.len() >= 3)
            .map(|(key, _)| key)
            .collect();

        if repeated.is_empty() {
            return;
        }

        // --- Step 4: Re-visit every block and reclassify matches.
        for block in blocks.iter_mut() {
            if block.block_type == BlockType::PageNumber {
                continue;
            }

            let page_height = match page_heights.get(&block.page) {
                Some(&h) if h > 0.0 => h,
                _ => continue,
            };

            let threshold = page_height * ZONE_FRACTION;
            let normalized = block.text.trim().to_lowercase();
            if normalized.is_empty() {
                continue;
            }

            if block.bbox.bottom >= page_height - threshold
                && repeated.contains(&(normalized.clone(), Zone::Header))
            {
                block.block_type = BlockType::RunningHeader;
            } else if block.bbox.top <= threshold
                && repeated.contains(&(normalized.clone(), Zone::Footer))
            {
                block.block_type = BlockType::RunningFooter;
            }
        }
    }

    /// Classify a single block.
    pub fn classify(&self, block: &TextBlock) -> BlockType {
        let text = block.text.trim();
        let font_size = block.primary_font_size();
        let is_bold = block.is_bold();
        let size_ratio = if self.body_font_size > 0.0 {
            font_size / self.body_font_size
        } else {
            1.0
        };

        // Check in priority order:
        if Self::is_caption(text) {
            BlockType::Caption
        } else if Self::is_page_number(text) {
            BlockType::PageNumber
        } else if self.is_running_header(block) {
            BlockType::RunningHeader
        } else if self.is_footnote(block, size_ratio) {
            BlockType::Footnote
        } else if size_ratio > 1.5 && block.lines.len() <= 3 {
            // Large font, short block: Title takes priority over generic SectionHeader.
            BlockType::Title
        } else if self.is_section_header_candidate(text, is_bold, size_ratio, block.lines.len()) {
            // Coarse classification; HeaderDetector (Task 10) adds numbering + level.
            if size_ratio >= 1.15 || Self::is_known_section_name(text) {
                BlockType::SectionHeader
            } else {
                BlockType::SubsectionHeader
            }
        } else {
            BlockType::BodyText
        }
    }

    // ---- detection helpers ----

    fn is_caption(text: &str) -> bool {
        let lower = text.to_lowercase();
        lower.starts_with("fig.")
            || lower.starts_with("figure")
            || lower.starts_with("table")
            || lower.starts_with("tab.")
    }

    fn is_page_number(text: &str) -> bool {
        let trimmed = text.trim();
        // All digits (possibly with surrounding spaces)
        trimmed.chars().all(|c| c.is_ascii_digit() || c == ' ')
            && trimmed.chars().any(|c| c.is_ascii_digit())
            && trimmed.len() <= 4
    }

    fn is_running_header(&self, block: &TextBlock) -> bool {
        // Heuristic: small font, near top of page (Y > page_height - 3 × body_size)
        // Without access to page height, use: font_size < body * 0.85
        // and block is only 1 line
        let size_ratio = block.primary_font_size() / self.body_font_size.max(1.0);
        block.lines.len() == 1 && size_ratio < 0.85
    }

    fn is_footnote(&self, block: &TextBlock, size_ratio: f64) -> bool {
        // Smaller font than body text
        size_ratio < 0.85 && block.lines.len() <= 4
    }

    fn is_section_header_candidate(
        &self,
        text: &str,
        is_bold: bool,
        size_ratio: f64,
        line_count: usize,
    ) -> bool {
        if text.len() > 120 || line_count > 3 {
            return false;
        }
        // Bold, or larger font, or known name
        (is_bold || size_ratio > 1.05) && !text.is_empty()
    }

    fn is_known_section_name(text: &str) -> bool {
        let upper = text.to_uppercase();
        KNOWN_SECTION_NAMES
            .iter()
            .any(|&name| upper == name || upper.contains(name))
    }

    // ---- body font size detection ----

    /// Compute the most common font size, weighted by character count.
    ///
    /// Returns 10.0 as a safe default if no blocks exist.
    pub fn detect_body_font_size(blocks: &[TextBlock]) -> f64 {
        if blocks.is_empty() {
            return 10.0;
        }

        // Use 0.5-pt bins to avoid over-splitting.
        let bin_width = 0.5_f64;
        let mut histogram: HashMap<i64, usize> = HashMap::new();

        for block in blocks {
            let font_size = block.primary_font_size();
            if font_size <= 0.0 {
                continue;
            }
            let bin = (font_size / bin_width).round() as i64;
            let char_count = block.text.len();
            *histogram.entry(bin).or_default() += char_count;
        }

        let best_bin = histogram
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(bin, _)| bin)
            .unwrap_or(20); // 20 × 0.5 = 10.0 pt default

        best_bin as f64 * bin_width
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BlockType, Rect, TextBlock, TextLine};

    fn make_block(text: &str, font_size: f64, lines: usize, bold: bool) -> TextBlock {
        let line = TextLine {
            spans: vec![],
            text: text.to_string(),
            bbox: Rect::new(72.0, 700.0, 540.0, 700.0 - font_size),
            page: 0,
            baseline_y: 700.0 - font_size,
            primary_font_size: font_size,
            primary_font_name: "Regular".to_string(),
            is_bold: bold,
        };
        TextBlock {
            global_index: 0,
            lines: vec![line; lines],
            text: text.to_string(),
            bbox: Rect::new(72.0, 700.0, 540.0, 700.0 - font_size * lines as f64),
            page: 0,
            column_index: 0,
            block_type: BlockType::default(),
        }
    }

    /// Build a block with explicit page and bbox for testing repeated-header detection.
    ///
    /// `page_height` is used to set the tallest block on the page so the zone
    /// threshold is derived correctly.  Pass `top` and `bottom` in PDF coordinates
    /// (Y-up, so top > bottom).
    fn make_block_at(
        text: &str,
        page: u32,
        top: f64,
        bottom: f64,
        block_type: BlockType,
    ) -> TextBlock {
        let font_size = top - bottom;
        let line = TextLine {
            spans: vec![],
            text: text.to_string(),
            bbox: Rect::new(72.0, top, 540.0, bottom),
            page,
            baseline_y: bottom,
            primary_font_size: font_size,
            primary_font_name: "Regular".to_string(),
            is_bold: false,
        };
        TextBlock {
            global_index: 0,
            lines: vec![line],
            text: text.to_string(),
            bbox: Rect::new(72.0, top, 540.0, bottom),
            page,
            column_index: 0,
            block_type,
        }
    }

    /// Sentinel block that anchors the page height (bbox.top == page_height).
    fn make_anchor_block(page: u32, page_height: f64) -> TextBlock {
        // A tall block whose top edge equals the page height.
        make_block_at(
            "anchor body text paragraph",
            page,
            page_height,
            100.0,
            BlockType::BodyText,
        )
    }

    #[test]
    fn caption_detected() {
        let classifier = BlockClassifier::with_body_size(10.0);
        let block = make_block("Fig. 1: Overview of the system.", 10.0, 1, false);
        assert_eq!(classifier.classify(&block), BlockType::Caption);
    }

    #[test]
    fn page_number_detected() {
        let classifier = BlockClassifier::with_body_size(10.0);
        let block = make_block("42", 9.0, 1, false);
        assert_eq!(classifier.classify(&block), BlockType::PageNumber);
    }

    #[test]
    fn body_text_default() {
        let classifier = BlockClassifier::with_body_size(10.0);
        let block = make_block(
            "This is a regular paragraph with normal text content.",
            10.0,
            3,
            false,
        );
        assert_eq!(classifier.classify(&block), BlockType::BodyText);
    }

    #[test]
    fn section_header_detected_bold() {
        let classifier = BlockClassifier::with_body_size(10.0);
        let block = make_block("Introduction", 10.0, 1, true);
        let t = classifier.classify(&block);
        assert!(matches!(
            t,
            BlockType::SectionHeader | BlockType::SubsectionHeader
        ));
    }

    #[test]
    fn title_detected_large_font_short_block() {
        let classifier = BlockClassifier::with_body_size(10.0);
        let block = make_block("My Paper Title", 18.0, 2, false); // size_ratio = 1.8 > 1.5
        assert_eq!(classifier.classify(&block), BlockType::Title);
    }

    #[test]
    fn body_font_size_detection() {
        // 10 blocks at 10pt (long text) and 2 blocks at 14pt (short headers)
        let mut blocks: Vec<TextBlock> = (0..10)
            .map(|_| {
                make_block(
                    "This is a long paragraph of body text in normal font size.",
                    10.0,
                    3,
                    false,
                )
            })
            .collect();
        blocks.push(make_block("INTRODUCTION", 14.0, 1, true));
        blocks.push(make_block("CONCLUSION", 14.0, 1, true));

        let size = BlockClassifier::detect_body_font_size(&blocks);
        // Should be 10.0 (weighted by char count, body text dominates)
        assert!((size - 10.0).abs() < 1.0, "Expected ~10pt, got {size}");
    }

    #[test]
    fn known_section_name_classified() {
        let classifier = BlockClassifier::with_body_size(10.0);
        let block = make_block("ABSTRACT", 10.0, 1, false);
        // Even without bold or size change, known name should trigger header classification.
        // The key is that it's NOT Caption or PageNumber.
        let t = classifier.classify(&block);
        assert!(!matches!(t, BlockType::Caption | BlockType::PageNumber));
    }

    /// The same header text appearing in the top 10% of a 792pt page on 3+ pages
    /// must be reclassified as RunningHeader.
    #[test]
    fn test_repeated_header_detected() {
        // Page height = 792 pt (US Letter).  Top 10% threshold = 79.2 pt from the top,
        // i.e. blocks with bottom >= 792 - 79.2 = 712.8.
        let page_height = 792.0_f64;
        let header_text = "Journal of Rust Research";

        // Three pages each with:
        //   • an anchor block that pins page_height
        //   • a header block in the top 10%
        //   • a body block in the middle (should NOT be reclassified)
        let mut blocks: Vec<TextBlock> = Vec::new();
        for p in 0u32..3 {
            // Anchor: top == page_height, bottom well below the header zone.
            blocks.push(make_anchor_block(p, page_height));
            // Header candidate: lives in the top 10% (bottom >= 712.8).
            blocks.push(make_block_at(
                header_text,
                p,
                page_height,         // top = 792
                page_height - 12.0,  // bottom = 780  (well inside top 10%)
                BlockType::BodyText, // initially unclassified
            ));
            // Body block: sits in the middle of the page.
            blocks.push(make_block_at(
                "Some body paragraph.",
                p,
                400.0,
                380.0,
                BlockType::BodyText,
            ));
        }

        BlockClassifier::detect_repeated_headers_footers(&mut blocks);

        // Every block with `header_text` should now be RunningHeader.
        let header_blocks: Vec<_> = blocks.iter().filter(|b| b.text == header_text).collect();
        assert_eq!(header_blocks.len(), 3, "Expected 3 header blocks");
        for b in &header_blocks {
            assert_eq!(
                b.block_type,
                BlockType::RunningHeader,
                "Block on page {} should be RunningHeader",
                b.page
            );
        }

        // Body blocks must remain BodyText.
        let body_blocks: Vec<_> = blocks
            .iter()
            .filter(|b| b.text == "Some body paragraph.")
            .collect();
        for b in &body_blocks {
            assert_eq!(b.block_type, BlockType::BodyText);
        }
    }

    /// Page-number blocks ("42", "43", "44") must never be upgraded to RunningHeader
    /// even when they appear in the top zone on 3+ pages.
    #[test]
    fn test_page_number_not_header() {
        let page_height = 792.0_f64;

        let mut blocks: Vec<TextBlock> = Vec::new();
        for (p, num) in [(0u32, "42"), (1, "43"), (2, "44")] {
            blocks.push(make_anchor_block(p, page_height));
            blocks.push(make_block_at(
                num,
                p,
                page_height,
                page_height - 12.0,
                BlockType::PageNumber, // already classified
            ));
        }

        BlockClassifier::detect_repeated_headers_footers(&mut blocks);

        // All "4x" blocks must stay PageNumber.
        for b in blocks
            .iter()
            .filter(|b| b.block_type != BlockType::BodyText)
        {
            assert_eq!(
                b.block_type,
                BlockType::PageNumber,
                "Block '{}' on page {} should remain PageNumber",
                b.text,
                b.page
            );
        }
    }

    /// A running footer appearing in the bottom 10% of 3+ pages is classified
    /// as RunningFooter.
    #[test]
    fn test_repeated_footer_detected() {
        let page_height = 792.0_f64;
        // Bottom 10% threshold = 79.2 pt.  Blocks with top <= 79.2 are in footer zone.
        let footer_text = "© 2024 The Authors";

        let mut blocks: Vec<TextBlock> = Vec::new();
        for p in 0u32..3 {
            blocks.push(make_anchor_block(p, page_height));
            // Footer block: lives in the bottom 10% (top <= 79.2).
            blocks.push(make_block_at(
                footer_text,
                p,
                60.0, // top = 60, well inside bottom 10%
                48.0, // bottom = 48
                BlockType::BodyText,
            ));
        }

        BlockClassifier::detect_repeated_headers_footers(&mut blocks);

        let footer_blocks: Vec<_> = blocks.iter().filter(|b| b.text == footer_text).collect();
        assert_eq!(footer_blocks.len(), 3);
        for b in &footer_blocks {
            assert_eq!(
                b.block_type,
                BlockType::RunningFooter,
                "Block on page {} should be RunningFooter",
                b.page
            );
        }
    }

    /// Text appearing on only 2 pages must NOT be reclassified.
    #[test]
    fn test_two_pages_not_enough() {
        let page_height = 792.0_f64;
        let header_text = "Rare Header";

        let mut blocks: Vec<TextBlock> = Vec::new();
        for p in 0u32..2 {
            blocks.push(make_anchor_block(p, page_height));
            blocks.push(make_block_at(
                header_text,
                p,
                page_height,
                page_height - 12.0,
                BlockType::BodyText,
            ));
        }

        BlockClassifier::detect_repeated_headers_footers(&mut blocks);

        for b in blocks.iter().filter(|b| b.text == header_text) {
            assert_eq!(
                b.block_type,
                BlockType::BodyText,
                "Block on page {} should remain BodyText (only 2 pages)",
                b.page
            );
        }
    }
}
