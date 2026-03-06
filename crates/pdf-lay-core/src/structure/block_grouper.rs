//! Groups TextLines within columns into paragraph-level TextBlocks.

use crate::types::{BlockType, LayoutRegion, PageLayout, Rect, TextBlock, TextLine};

/// Groups lines into blocks using line-gap, font-size, and bold-change heuristics.
pub struct BlockGrouper {
    /// Multiplier for normal line spacing to determine block boundary gap.
    /// Default: 1.8 (i.e., gap > font_size × 1.2 × 1.8 triggers a new block).
    gap_multiplier: f64,
    /// Minimum font size change (points) to trigger a block boundary.
    font_size_threshold: f64,
}

impl Default for BlockGrouper {
    fn default() -> Self {
        Self {
            gap_multiplier: 1.8,
            font_size_threshold: 1.0,
        }
    }
}

impl BlockGrouper {
    /// Create a new `BlockGrouper` with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the gap multiplier (default: 1.8).
    pub fn with_gap_multiplier(mut self, m: f64) -> Self {
        self.gap_multiplier = m;
        self
    }

    /// Group lines into blocks, respecting column layout.
    ///
    /// Returns blocks in reading order (left column before right column,
    /// top-to-bottom within each column). Each block gets a monotonically
    /// increasing `global_index` across the entire call.
    pub fn group(&self, lines: &[TextLine], layouts: &[PageLayout]) -> Vec<TextBlock> {
        let mut all_blocks = Vec::new();
        let mut global_index = 0usize;

        // Process pages in order.
        let mut pages: Vec<u32> = layouts.iter().map(|l| l.page).collect();
        pages.sort_unstable();
        pages.dedup();

        // Also include pages that have lines but no layout entry.
        let mut line_pages: Vec<u32> = lines.iter().map(|l| l.page).collect();
        line_pages.sort_unstable();
        line_pages.dedup();
        for p in line_pages {
            if !pages.contains(&p) {
                pages.push(p);
            }
        }
        pages.sort_unstable();

        for page in pages {
            let page_lines: Vec<&TextLine> = lines.iter().filter(|l| l.page == page).collect();

            let page_layout = layouts.iter().find(|l| l.page == page);
            let regions: &[LayoutRegion] = page_layout.map(|l| l.regions.as_slice()).unwrap_or(&[]);

            if regions.is_empty() {
                // No layout info: treat whole page as single column.
                let blocks = self.group_column_lines(&page_lines, &mut global_index, page, 0);
                all_blocks.extend(blocks);
                continue;
            }

            for region in regions {
                for column in &region.columns {
                    // Filter lines that belong to this column's X range and Y band.
                    let col_lines: Vec<&TextLine> = page_lines
                        .iter()
                        .copied()
                        .filter(|l| {
                            l.bbox.center_x() >= column.left
                                && l.bbox.center_x() < column.right
                                && l.bbox.top <= region.y_top
                                && l.bbox.bottom >= region.y_bottom
                        })
                        .collect();

                    let blocks =
                        self.group_column_lines(&col_lines, &mut global_index, page, column.index);
                    all_blocks.extend(blocks);
                }
            }
        }

        all_blocks
    }

    fn group_column_lines(
        &self,
        lines: &[&TextLine],
        global_index: &mut usize,
        page: u32,
        column_index: usize,
    ) -> Vec<TextBlock> {
        if lines.is_empty() {
            return Vec::new();
        }

        // Sort top-to-bottom (descending Y, since higher Y = higher on page).
        let mut sorted: Vec<&TextLine> = lines.to_vec();
        sorted.sort_by(|a, b| {
            b.bbox
                .top
                .partial_cmp(&a.bbox.top)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut blocks: Vec<TextBlock> = Vec::new();
        let mut current_lines: Vec<TextLine> = vec![sorted[0].clone()];

        for &line in &sorted[1..] {
            let prev = current_lines.last().unwrap();
            if self.detect_break(prev, line) {
                blocks.push(self.make_block(
                    std::mem::take(&mut current_lines),
                    *global_index,
                    page,
                    column_index,
                ));
                *global_index += 1;
            }
            current_lines.push(line.clone());
        }

        if !current_lines.is_empty() {
            blocks.push(self.make_block(current_lines, *global_index, page, column_index));
            *global_index += 1;
        }

        blocks
    }

    fn detect_break(&self, prev: &TextLine, current: &TextLine) -> bool {
        // 1. Large vertical gap.
        // In PDF coordinates, prev.bbox.bottom is the lower edge of the previous line,
        // and current.bbox.top is the upper edge of the current line.
        // Since Y increases upward, prev is above current, so gap = prev.bottom - current.top.
        let line_gap = prev.bbox.bottom - current.bbox.top;
        let normal_spacing = prev.primary_font_size * 1.2;
        if line_gap > normal_spacing * self.gap_multiplier {
            return true;
        }

        // 2. Significant font size change.
        if (prev.primary_font_size - current.primary_font_size).abs() > self.font_size_threshold {
            return true;
        }

        // 3. Bold <-> Regular transition.
        if prev.is_bold != current.is_bold {
            return true;
        }

        false
    }

    fn make_block(
        &self,
        lines: Vec<TextLine>,
        global_index: usize,
        page: u32,
        column_index: usize,
    ) -> TextBlock {
        let text = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        let bbox = lines
            .iter()
            .map(|l| l.bbox.clone())
            .reduce(|acc, b| acc.union(&b))
            .unwrap_or_else(|| Rect::new(0.0, 0.0, 0.0, 0.0));

        TextBlock {
            global_index,
            lines,
            text,
            bbox,
            page,
            column_index,
            block_type: BlockType::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Column, LayoutRegion, PageLayout, Rect, TextLine};

    fn make_line(top: f64, font_size: f64, bold: bool, page: u32) -> TextLine {
        TextLine {
            spans: vec![],
            text: "line text".to_string(),
            bbox: Rect::new(72.0, top, 540.0, top - font_size),
            page,
            baseline_y: top - font_size,
            primary_font_size: font_size,
            primary_font_name: "Regular".to_string(),
            is_bold: bold,
        }
    }

    fn single_col_layout(page: u32) -> PageLayout {
        PageLayout {
            regions: vec![LayoutRegion {
                y_top: 792.0,
                y_bottom: 0.0,
                columns: vec![Column {
                    left: 0.0,
                    right: 612.0,
                    index: 0,
                }],
            }],
            page_width: 612.0,
            page_height: 792.0,
            page,
        }
    }

    #[test]
    fn close_lines_form_single_block() {
        let lines = vec![
            make_line(700.0, 10.0, false, 0),
            make_line(688.0, 10.0, false, 0), // 2pt gap < 10 * 1.2 * 1.8 = 21.6
        ];
        let layout = vec![single_col_layout(0)];
        let blocks = BlockGrouper::new().group(&lines, &layout);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].lines.len(), 2);
    }

    #[test]
    fn large_gap_creates_new_block() {
        let lines = vec![
            make_line(700.0, 10.0, false, 0),
            make_line(640.0, 10.0, false, 0), // 50pt gap > 21.6 threshold
        ];
        let layout = vec![single_col_layout(0)];
        let blocks = BlockGrouper::new().group(&lines, &layout);
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn font_size_change_creates_new_block() {
        let lines = vec![
            make_line(700.0, 10.0, false, 0),
            make_line(688.0, 14.0, false, 0), // 4pt size change > 1.0 threshold
        ];
        let layout = vec![single_col_layout(0)];
        let blocks = BlockGrouper::new().group(&lines, &layout);
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn bold_regular_transition_creates_new_block() {
        let lines = vec![
            make_line(700.0, 10.0, true, 0),  // bold
            make_line(688.0, 10.0, false, 0), // regular -> break
        ];
        let layout = vec![single_col_layout(0)];
        let blocks = BlockGrouper::new().group(&lines, &layout);
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn global_indices_are_sequential() {
        let lines: Vec<TextLine> = (0..6)
            .map(|i| make_line(700.0 - i as f64 * 50.0, 10.0, false, 0))
            .collect();
        let layout = vec![single_col_layout(0)];
        let blocks = BlockGrouper::new().group(&lines, &layout);
        for (i, block) in blocks.iter().enumerate() {
            assert_eq!(block.global_index, i);
        }
    }

    #[test]
    fn empty_lines_returns_empty() {
        let layout = vec![single_col_layout(0)];
        let blocks = BlockGrouper::new().group(&[], &layout);
        assert!(blocks.is_empty());
    }
}
