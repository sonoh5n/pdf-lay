//! Sorts TextBlocks into logical reading order for multi-column layouts.

use crate::types::{PageLayout, TextBlock};

/// Sorts blocks into reading order: page → full-width-Y → column-index → Y-within-column.
pub struct ReadingOrderSorter;

impl ReadingOrderSorter {
    /// Sort `blocks` in-place into reading order.
    ///
    /// Full-width elements (width >= 60% of page width) are interleaved with
    /// column elements at their Y position.
    pub fn sort(blocks: &mut [TextBlock], layouts: &[PageLayout]) {
        blocks.sort_by(|a, b| {
            // 1. Page order.
            let page_cmp = a.page.cmp(&b.page);
            if page_cmp != std::cmp::Ordering::Equal {
                return page_cmp;
            }

            let a_full = Self::is_full_width(a, layouts);
            let b_full = Self::is_full_width(b, layouts);

            // 2. Full-width elements: sorted purely by Y (descending = top first).
            if a_full || b_full {
                return b
                    .bbox
                    .top
                    .partial_cmp(&a.bbox.top)
                    .unwrap_or(std::cmp::Ordering::Equal);
            }

            // 3. Same column -> Y order (descending).
            let col_cmp = a.column_index.cmp(&b.column_index);
            if col_cmp == std::cmp::Ordering::Equal {
                return b
                    .bbox
                    .top
                    .partial_cmp(&a.bbox.top)
                    .unwrap_or(std::cmp::Ordering::Equal);
            }

            // 4. Different column -> left column first.
            col_cmp
        });
    }

    fn is_full_width(block: &TextBlock, layouts: &[PageLayout]) -> bool {
        let page_layout = layouts.iter().find(|l| l.page == block.page);
        let page_width = page_layout.map(|l| l.page_width).unwrap_or(612.0);
        block.bbox.width() >= page_width * 0.60
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BlockType, Column, LayoutRegion, PageLayout, Rect, TextBlock};

    fn make_block(col: usize, top: f64, width: f64, page: u32) -> TextBlock {
        TextBlock {
            global_index: 0,
            lines: vec![],
            text: String::new(),
            bbox: Rect::new(72.0, top, 72.0 + width, top - 10.0),
            page,
            column_index: col,
            block_type: BlockType::BodyText,
        }
    }

    fn two_col_layout(page: u32) -> PageLayout {
        PageLayout {
            regions: vec![LayoutRegion {
                y_top: 792.0,
                y_bottom: 0.0,
                columns: vec![
                    Column {
                        left: 0.0,
                        right: 306.0,
                        index: 0,
                    },
                    Column {
                        left: 306.0,
                        right: 612.0,
                        index: 1,
                    },
                ],
            }],
            page_width: 612.0,
            page_height: 792.0,
            page,
        }
    }

    #[test]
    fn left_column_before_right() {
        let mut blocks = vec![
            make_block(1, 700.0, 200.0, 0), // right column
            make_block(0, 700.0, 200.0, 0), // left column
        ];
        ReadingOrderSorter::sort(&mut blocks, &[two_col_layout(0)]);
        assert_eq!(blocks[0].column_index, 0);
        assert_eq!(blocks[1].column_index, 1);
    }

    #[test]
    fn top_to_bottom_within_column() {
        let mut blocks = vec![
            make_block(0, 500.0, 200.0, 0),
            make_block(0, 700.0, 200.0, 0),
        ];
        ReadingOrderSorter::sort(&mut blocks, &[two_col_layout(0)]);
        assert!(blocks[0].bbox.top > blocks[1].bbox.top);
    }

    #[test]
    fn full_width_interleaved_by_y() {
        // Full-width block at y=600 should appear between col-0 blocks at y=700 and y=500.
        let layout = two_col_layout(0);
        let mut blocks = vec![
            make_block(0, 500.0, 200.0, 0),
            make_block(0, 600.0, 612.0, 0), // full-width (width = page_width)
            make_block(0, 700.0, 200.0, 0),
        ];
        ReadingOrderSorter::sort(&mut blocks, &[layout]);
        // After sort: y=700 first, then y=600 full-width, then y=500
        assert!(blocks[0].bbox.top >= blocks[1].bbox.top);
        assert!(blocks[1].bbox.top >= blocks[2].bbox.top);
    }
}
