# Task 12: CaptionDetector + ImageMatcher

## Overview

Implement the figure module: `CaptionDetector` finds caption blocks using regex patterns,
and `ImageMatcher` pairs captions with their nearest images using a spatial scoring function.

**CaptionDetector**: Uses compiled regex `(?i)^(Fig\.?|Figure|TABLE|Tab\.)\s*(\d+)\s*[:.]?\s*(.*)`
to detect caption blocks. Returns `CaptionInfo` for each match.

**ImageMatcher**: For each figure caption, finds the nearest image on the same page by:
- Score = `vertical_distance × 10 + horizontal_distance`
- Searches both above (for journals like Nature) and below (default)
- Rejects matches where `vertical_distance > max_gap_pt` (default 50pt)

This task can be developed in parallel with Tasks 06-11 since it only depends on Task 02.

## Related Information
- **Plan**: `/Users/sonokawa/.claude/plans/fuzzy-drifting-summit.md` (Task 12)
- **Design doc**: `docs/arch/02_DESIGN.md` § 2.5 figure
- **Spec**: `docs/arch/01_SPECIFICATION.md` § 2.9 F-008
- **Overview**: `docs/develop/overview.md`
- **Dependencies**: Task 02 (types) — can run in parallel with Tasks 03-11

## Files to Create

- [ ] `crates/pdf-lay-core/src/figure/mod.rs`
- [ ] `crates/pdf-lay-core/src/figure/caption_detector.rs`
- [ ] `crates/pdf-lay-core/src/figure/image_matcher.rs`

## Files to Modify

- [ ] `crates/pdf-lay-core/src/lib.rs` — add `pub mod figure;`

## Implementation Steps

### Step 1: `figure/mod.rs`

```rust
//! Figure processing: caption detection and image-caption matching.

mod caption_detector;
mod image_matcher;

pub use caption_detector::{CaptionDetector, CaptionInfo, CaptionType};
pub use image_matcher::ImageMatcher;
```

### Step 2: `figure/caption_detector.rs`

```rust
//! Detects figure and table captions from TextBlocks using regex patterns.

use regex::Regex;
use crate::types::{Rect, TextBlock};

/// Semantic type of a detected caption.
#[derive(Debug, Clone, PartialEq)]
pub enum CaptionType {
    Figure,
    Table,
}

/// Metadata about a detected caption.
#[derive(Debug, Clone)]
pub struct CaptionInfo {
    /// Index into the `blocks` slice where this caption was found.
    pub block_index: usize,
    pub caption_type: CaptionType,
    /// Prefix string as matched (e.g. "Fig.", "Figure", "Table").
    pub prefix: String,
    /// Caption number (e.g. 1 for "Fig. 1").
    pub number: Option<u32>,
    /// Description text after the prefix and number.
    pub description: String,
    /// Full original text of the caption block.
    pub full_text: String,
    pub page: u32,
    pub bbox: Rect,
}

struct CaptionPattern {
    regex: Regex,
    caption_type: CaptionType,
}

/// Detects caption blocks using regex matching on block text.
pub struct CaptionDetector {
    patterns: Vec<CaptionPattern>,
}

impl CaptionDetector {
    pub fn new() -> Self {
        Self {
            patterns: vec![
                CaptionPattern {
                    regex: Regex::new(
                        r"(?i)^(Fig\.?|Figure)\s*(\d+)\s*[:.]?\s*(.*)"
                    ).unwrap(),
                    caption_type: CaptionType::Figure,
                },
                CaptionPattern {
                    regex: Regex::new(
                        r"(?i)^(Table|Tab\.)\s*(\d+)\s*[:.]?\s*(.*)"
                    ).unwrap(),
                    caption_type: CaptionType::Table,
                },
            ],
        }
    }

    /// Detect captions in a slice of blocks.
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
    use crate::types::{BlockType, Rect, TextBlock};

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
        assert_eq!(captions[0].block_index, 1);
    }
}
```

### Step 3: `figure/image_matcher.rs`

```rust
//! Matches figure captions to images using spatial proximity scoring.

use std::collections::HashSet;
use crate::types::{FigureInfo, ImageInfo, InsertionPoint, TextBlock};
use crate::figure::caption_detector::{CaptionInfo, CaptionType};

/// Matches captions to images by spatial proximity.
pub struct ImageMatcher {
    /// Maximum vertical distance (pt) between a caption and its image.
    max_gap_pt: f64,
    /// If true, search for images both above and below the caption.
    /// (False = caption assumed to be below image, as in most IEEE journals.)
    bidirectional: bool,
}

impl Default for ImageMatcher {
    fn default() -> Self {
        Self { max_gap_pt: 50.0, bidirectional: true }
    }
}

impl ImageMatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_gap(mut self, gap_pt: f64) -> Self {
        self.max_gap_pt = gap_pt;
        self
    }

    /// Match figure captions to images and return `FigureInfo` records.
    pub fn match_all(
        &self,
        captions: &[CaptionInfo],
        images: &[ImageInfo],
        blocks: &[TextBlock],
    ) -> Vec<FigureInfo> {
        let mut used_images: HashSet<usize> = HashSet::new();
        let mut results = Vec::new();

        for caption in captions
            .iter()
            .filter(|c| c.caption_type == CaptionType::Figure)
        {
            if let Some((img_idx, image)) =
                self.find_nearest(caption, images, &used_images)
            {
                used_images.insert(img_idx);

                let context = self.extract_context(caption, blocks, 500);
                let insertion = self.determine_insertion(caption, &image, blocks);

                results.push(FigureInfo {
                    figure_id: format!(
                        "{} {}",
                        caption.prefix,
                        caption.number.unwrap_or(0)
                    ),
                    figure_number: caption.number,
                    caption_text: caption.full_text.clone(),
                    image: image.clone(),
                    context_text: context,
                    insertion_point: insertion,
                });
            }
        }

        results
    }

    // ---- private helpers ----

    fn find_nearest<'a>(
        &self,
        caption: &CaptionInfo,
        images: &'a [ImageInfo],
        used: &HashSet<usize>,
    ) -> Option<(usize, &'a ImageInfo)> {
        images
            .iter()
            .enumerate()
            .filter(|(idx, img)| img.page == caption.page && !used.contains(idx))
            .filter_map(|(idx, img)| {
                let score = self.distance_score(caption, img);
                // Reject if the vertical component alone exceeds max_gap_pt.
                let v_dist = self.vertical_distance(caption, img);
                if v_dist > self.max_gap_pt {
                    None
                } else {
                    Some((idx, img, score))
                }
            })
            .min_by(|(_, _, sa), (_, _, sb)| {
                sa.partial_cmp(sb).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(idx, img, _)| (idx, img))
    }

    fn distance_score(&self, caption: &CaptionInfo, image: &ImageInfo) -> f64 {
        let v = self.vertical_distance(caption, image);
        let h = (image.normalized_bbox.center_x() - caption.bbox.center_x()).abs();
        v * 10.0 + h
    }

    fn vertical_distance(&self, caption: &CaptionInfo, image: &ImageInfo) -> f64 {
        let bbox = &image.normalized_bbox;
        if self.bidirectional {
            // Check both: caption below image and caption above image.
            let below = (bbox.bottom - caption.bbox.top).abs(); // image above caption
            let above = (caption.bbox.bottom - bbox.top).abs(); // caption above image
            below.min(above)
        } else {
            // Only check caption below image.
            (bbox.bottom - caption.bbox.top).abs()
        }
    }

    fn extract_context(
        &self,
        caption: &CaptionInfo,
        blocks: &[TextBlock],
        max_chars: usize,
    ) -> String {
        use crate::types::BlockType;

        let mut context = String::new();
        let cap_idx = caption.block_index;

        // Collect body text from surrounding blocks.
        let start = cap_idx.saturating_sub(3);
        let end = (cap_idx + 4).min(blocks.len());

        for block in &blocks[start..end] {
            if block.global_index == cap_idx {
                continue; // skip the caption itself
            }
            if matches!(block.block_type, BlockType::BodyText | BlockType::Abstract) {
                if context.len() + block.text.len() < max_chars {
                    context.push_str(&block.text);
                    context.push(' ');
                }
            }
        }

        context.trim().chars().take(max_chars).collect()
    }

    fn determine_insertion(
        &self,
        caption: &CaptionInfo,
        image: &ImageInfo,
        blocks: &[TextBlock],
    ) -> InsertionPoint {
        // Find the last block before the caption.
        let after_block_index = if caption.block_index > 0 {
            blocks
                .get(caption.block_index - 1)
                .map(|b| b.global_index)
        } else {
            None
        };

        InsertionPoint {
            page: caption.page,
            after_block_index,
            y_position: image.normalized_bbox.bottom,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::figure::caption_detector::CaptionType;
    use crate::types::{BlockType, ImageFormat, Rect, TextBlock};
    use std::path::PathBuf;

    fn make_image(page: u32, top: f64, bottom: f64, left: f64, right: f64) -> ImageInfo {
        let bbox = Rect::new(left, top, right, bottom);
        ImageInfo {
            path: PathBuf::from("p000_img000.png"),
            page,
            raw_bbox: bbox.clone(),
            normalized_bbox: bbox,
            width_px: 300,
            height_px: 200,
            format: ImageFormat::Png,
        }
    }

    fn make_caption(block_index: usize, page: u32, top: f64) -> CaptionInfo {
        CaptionInfo {
            block_index,
            caption_type: CaptionType::Figure,
            prefix: "Fig.".to_string(),
            number: Some(1),
            description: "Test figure.".to_string(),
            full_text: "Fig. 1: Test figure.".to_string(),
            page,
            bbox: Rect::new(72.0, top, 540.0, top - 10.0),
        }
    }

    #[test]
    fn caption_matched_to_nearest_image() {
        let matcher = ImageMatcher::new();
        // Caption at y=200, image at y=300 (image above caption, 10pt gap).
        let caption = make_caption(0, 0, 200.0);
        let images = vec![
            make_image(0, 300.0, 290.0, 72.0, 540.0), // gap = 290 - 200 = 90pt (above)
        ];
        let figures = matcher.match_all(&[caption], &images, &[]);
        // Gap = 90pt > 50pt max → no match
        assert!(figures.is_empty());
    }

    #[test]
    fn caption_matched_within_max_gap() {
        let matcher = ImageMatcher::new();
        // Caption at top=200, image bottom=210 → gap = 10pt (image is just above caption)
        let caption = make_caption(0, 0, 200.0);
        let images = vec![
            make_image(0, 220.0, 210.0, 72.0, 540.0), // image bottom=210, caption top=200 → gap=10
        ];
        let figures = matcher.match_all(&[caption], &images, &[]);
        assert_eq!(figures.len(), 1);
        assert_eq!(figures[0].figure_id, "Fig. 1");
    }

    #[test]
    fn different_page_not_matched() {
        let matcher = ImageMatcher::new();
        let caption = make_caption(0, 0, 200.0);
        let images = vec![
            make_image(1, 220.0, 210.0, 72.0, 540.0), // different page
        ];
        let figures = matcher.match_all(&[caption], &images, &[]);
        assert!(figures.is_empty());
    }

    #[test]
    fn each_image_matched_at_most_once() {
        let matcher = ImageMatcher::new();
        let captions = vec![
            make_caption(0, 0, 200.0),
            make_caption(1, 0, 100.0),
        ];
        let images = vec![
            make_image(0, 220.0, 210.0, 72.0, 540.0), // one image for two captions
        ];
        let figures = matcher.match_all(&captions, &images, &[]);
        // Second caption should not match (image already used).
        assert!(figures.len() <= 1);
    }
}
```

## Acceptance Criteria

- [ ] All unit tests pass: `cargo test -p pdf-lay-core -- figure`
  - CaptionDetector: all 6 tests
  - ImageMatcher: all 4 tests
- [ ] `Regex` objects compiled once in `CaptionDetector::new()`, not per call
- [ ] Each image is matched at most once (used image set prevents double-matching)
- [ ] Captions on page N never match images on page M (N ≠ M)
- [ ] Vertical distance > `max_gap_pt` → no match produced
- [ ] `cargo clippy -p pdf-lay-core -- -D warnings` passes

## Dependencies

- Task 02 (types) must be completed first.
- This task is independent of Tasks 03-11 and can be developed in parallel.

## Commit Message

```
feat(figure): add CaptionDetector and ImageMatcher for figure-caption spatial matching
```
