//! Matches figure captions to images using spatial proximity scoring.

use std::collections::HashSet;

use crate::figure::caption_detector::{CaptionInfo, CaptionType};
use crate::types::{BlockType, FigureInfo, ImageInfo, InsertionPoint, TextBlock};

/// Matches captions to images by spatial proximity.
///
/// For each figure caption the matcher finds the nearest unmatched image on the
/// same page using the score:
///
/// ```text
/// score = vertical_distance × 10 + horizontal_distance
/// ```
///
/// Images whose vertical distance exceeds `max_gap_pt` are rejected outright.
/// Each image is assigned to at most one caption.
pub struct ImageMatcher {
    /// Maximum vertical distance (pt) between a caption and its image.
    max_gap_pt: f64,
    /// If true, search for images both above and below the caption.
    /// Set to `false` to assume captions always appear below their image (IEEE style).
    bidirectional: bool,
}

impl Default for ImageMatcher {
    fn default() -> Self {
        Self {
            max_gap_pt: 50.0,
            bidirectional: true,
        }
    }
}

impl ImageMatcher {
    /// Create a new `ImageMatcher` with default settings (max gap 50pt, bidirectional).
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the maximum allowed vertical gap between a caption and its image.
    pub fn with_max_gap(mut self, gap_pt: f64) -> Self {
        self.max_gap_pt = gap_pt;
        self
    }

    /// Match figure captions to images and return [`FigureInfo`] records.
    ///
    /// Only captions of type [`CaptionType::Figure`] are matched. Table captions
    /// are silently ignored (they are handled by the table module).
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
            if let Some((img_idx, image)) = self.find_nearest(caption, images, &used_images) {
                used_images.insert(img_idx);

                let context = self.extract_context(caption, blocks, 500);
                let insertion = self.determine_insertion(caption, image, blocks);

                results.push(FigureInfo {
                    figure_id: format!("{} {}", caption.prefix, caption.number.unwrap_or(0)),
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
                let v_dist = self.vertical_distance(caption, img);
                if v_dist > self.max_gap_pt {
                    None
                } else {
                    let score = self.distance_score(caption, img);
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
            // Distance when caption is below image: image.bottom vs caption.top
            let below = (bbox.bottom - caption.bbox.top).abs();
            // Distance when caption is above image: caption.bottom vs image.top
            let above = (caption.bbox.bottom - bbox.top).abs();
            below.min(above)
        } else {
            // Caption assumed to be below image only.
            (bbox.bottom - caption.bbox.top).abs()
        }
    }

    /// Extract surrounding body text (~`max_chars` characters) for context.
    ///
    /// Looks at blocks in a window around the caption's position in the slice.
    /// Skips the caption block itself and non-body blocks (headers, page numbers,
    /// etc.).
    fn extract_context(
        &self,
        caption: &CaptionInfo,
        blocks: &[TextBlock],
        max_chars: usize,
    ) -> String {
        let mut context = String::new();
        let cap_idx = caption.block_index;

        let start = cap_idx.saturating_sub(3);
        let end = (cap_idx + 4).min(blocks.len());

        for (pos, block) in blocks[start..end].iter().enumerate() {
            // Skip the caption block itself (by slice position, not global_index).
            if start + pos == cap_idx {
                continue;
            }
            if matches!(block.block_type, BlockType::BodyText | BlockType::Abstract)
                && context.len() + block.text.len() < max_chars
            {
                context.push_str(&block.text);
                context.push(' ');
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
        let after_block_index = if caption.block_index > 0 {
            blocks.get(caption.block_index - 1).map(|b| b.global_index)
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

    fn make_body_block(text: &str, page: u32, global_index: usize) -> TextBlock {
        TextBlock {
            global_index,
            lines: vec![],
            text: text.to_string(),
            bbox: Rect::new(72.0, 700.0, 540.0, 680.0),
            page,
            column_index: 0,
            block_type: BlockType::BodyText,
        }
    }

    #[test]
    fn caption_not_matched_when_gap_too_large() {
        let matcher = ImageMatcher::new();
        // Caption at top=200; image bottom=290 → gap = |290 - 200| = 90pt > 50pt max.
        let caption = make_caption(0, 0, 200.0);
        let images = vec![
            make_image(0, 300.0, 290.0, 72.0, 540.0), // gap = 90pt
        ];
        let figures = matcher.match_all(&[caption], &images, &[]);
        assert!(figures.is_empty());
    }

    #[test]
    fn caption_matched_within_max_gap() {
        let matcher = ImageMatcher::new();
        // Caption at top=200; image bottom=210 → gap = |210 - 200| = 10pt ≤ 50pt.
        let caption = make_caption(0, 0, 200.0);
        let images = vec![
            make_image(0, 220.0, 210.0, 72.0, 540.0), // gap = 10pt
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
        // Two captions close to the same image.
        let captions = vec![make_caption(0, 0, 200.0), make_caption(1, 0, 100.0)];
        let images = vec![
            make_image(0, 220.0, 210.0, 72.0, 540.0), // one image for two captions
        ];
        let figures = matcher.match_all(&captions, &images, &[]);
        // At most one figure should be produced (the image can only be used once).
        assert!(figures.len() <= 1);
    }

    #[test]
    fn context_extracted_from_surrounding_blocks() {
        let matcher = ImageMatcher::new();
        let blocks = vec![
            make_body_block("Preceding body text.", 0, 0),
            {
                // The caption block itself — should be skipped.
                let mut b = make_body_block("Fig. 1: Caption.", 0, 1);
                b.block_type = BlockType::Caption;
                b
            },
            make_body_block("Following body text.", 0, 2),
        ];
        let caption = make_caption(1, 0, 200.0);
        let images = vec![make_image(0, 220.0, 210.0, 72.0, 540.0)];
        let figures = matcher.match_all(&[caption], &images, &blocks);
        assert_eq!(figures.len(), 1);
        // Context should contain surrounding body text but not the caption text.
        assert!(figures[0].context_text.contains("Preceding body text."));
        assert!(figures[0].context_text.contains("Following body text."));
        assert!(!figures[0].context_text.contains("Fig. 1"));
    }
}
