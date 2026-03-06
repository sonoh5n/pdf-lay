//! Sorts TextBlocks into logical reading order for multi-column layouts.

use crate::types::{PageLayout, TextBlock};

/// Sorts blocks into reading order: page → full-width-Y → column-index → Y-within-column.
pub struct ReadingOrderSorter;

impl ReadingOrderSorter {
    /// Sort `blocks` in-place into reading order.
    ///
    /// Ordering key (lexicographic):
    /// 1. page asc
    /// 2. Y desc (top first)
    /// 3. full-width before non-full when Y ties
    /// 4. column index asc for non-full ties
    /// 5. left X asc
    /// 6. global index asc
    ///
    /// This comparator is a total order and avoids panics from non-transitive
    /// pairwise rules.
    pub fn sort(blocks: &mut [TextBlock], layouts: &[PageLayout]) {
        blocks.sort_by(|a, b| {
            let a_full = Self::is_full_width(a, layouts);
            let b_full = Self::is_full_width(b, layouts);

            a.page
                .cmp(&b.page)
                // Higher Y first.
                .then_with(|| b.bbox.top.total_cmp(&a.bbox.top))
                // If Y ties, place full-width block first.
                .then_with(|| match (a_full, b_full) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => std::cmp::Ordering::Equal,
                })
                // Only meaningful when both are non-full and Y is tied.
                .then_with(|| {
                    if !a_full && !b_full {
                        a.column_index.cmp(&b.column_index)
                    } else {
                        std::cmp::Ordering::Equal
                    }
                })
                .then_with(|| a.bbox.left.total_cmp(&b.bbox.left))
                .then_with(|| a.global_index.cmp(&b.global_index))
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

    #[test]
    fn mixed_full_and_columns_is_total_order() {
        // Previously, this pattern could trigger a non-transitive comparator:
        // full-width block compared by Y, while non-full blocks compared by column.
        let layout = two_col_layout(0);
        let mut blocks = vec![
            make_block(0, 40.0, 200.0, 0), // left column, lower
            make_block(1, 60.0, 200.0, 0), // right column, higher
            make_block(0, 50.0, 612.0, 0), // full-width, middle
        ];

        ReadingOrderSorter::sort(&mut blocks, &[layout]);

        let tops: Vec<f64> = blocks.iter().map(|b| b.bbox.top).collect();
        assert_eq!(tops, vec![60.0, 50.0, 40.0]);
    }
}
