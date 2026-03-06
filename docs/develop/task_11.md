# Task 11: SectionBuilder + ReadingOrderSorter

## Overview

Implement `SectionBuilder` which assembles the detected headers and blocks into the final
`Section` hierarchy, and `ReadingOrderSorter` which sorts blocks into correct reading order
before the section tree is built.

**SectionBuilder** algorithm:
1. Sort blocks in reading order (via `ReadingOrderSorter`).
2. Split the block array at each `SectionHeader::block_index` boundary.
3. Assign figures and tables to sections based on `InsertionPoint::after_block_index`.
4. Build hierarchy using a stack: when a header of equal or lower level appears, pop the stack.

**ReadingOrderSorter** algorithm:
1. Sort by page number.
2. Within a page: full-width elements (bbox width >= 60% of page width) are sorted purely by Y.
3. Non-full-width elements: sort by column index, then by Y within column.

## Related Information
- **Plan**: `/Users/sonokawa/.claude/plans/fuzzy-drifting-summit.md` (Task 11)
- **Design doc**: `docs/arch/02_DESIGN.md` § 2.4 structure — SectionBuilder, ReadingOrderSorter
- **Overview**: `docs/develop/overview.md`
- **Dependencies**: Task 10 (HeaderDetector) must be completed first

## Files to Create

- [ ] `crates/pdf-lay-core/src/structure/section_builder.rs`
- [ ] `crates/pdf-lay-core/src/structure/reading_order.rs`

## Files to Modify

- [ ] `crates/pdf-lay-core/src/structure/mod.rs` — uncomment both `pub use` statements

## Implementation Steps

### Step 1: `structure/reading_order.rs`

```rust
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

            // 3. Same column → Y order (descending).
            let col_cmp = a.column_index.cmp(&b.column_index);
            if col_cmp == std::cmp::Ordering::Equal {
                return b
                    .bbox
                    .top
                    .partial_cmp(&a.bbox.top)
                    .unwrap_or(std::cmp::Ordering::Equal);
            }

            // 4. Different column → left column first.
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
                y_top: 792.0, y_bottom: 0.0,
                columns: vec![
                    Column { left: 0.0, right: 306.0, index: 0 },
                    Column { left: 306.0, right: 612.0, index: 1 },
                ],
            }],
            page_width: 612.0, page_height: 792.0, page,
        }
    }

    #[test]
    fn left_column_before_right() {
        let mut blocks = vec![
            make_block(1, 700.0, 200.0, 0),  // right column
            make_block(0, 700.0, 200.0, 0),  // left column
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
            make_block(0, 600.0, 612.0, 0),  // full-width (width = page_width)
            make_block(0, 700.0, 200.0, 0),
        ];
        ReadingOrderSorter::sort(&mut blocks, &[layout]);
        // After sort: y=700 first, then y=600 full-width, then y=500
        assert!(blocks[0].bbox.top >= blocks[1].bbox.top);
        assert!(blocks[1].bbox.top >= blocks[2].bbox.top);
    }
}
```

### Step 2: `structure/section_builder.rs`

```rust
//! Builds the Section hierarchy from blocks, headers, figures, and tables.

use crate::types::{FigureInfo, PageLayout, Section, SectionHeader, TableInfo, TextBlock};
use crate::structure::reading_order::ReadingOrderSorter;

/// Assembles Section hierarchy from flat lists of blocks and headers.
pub struct SectionBuilder;

impl SectionBuilder {
    /// Build the section hierarchy.
    ///
    /// 1. Sort blocks in reading order.
    /// 2. Split blocks at header boundaries → flat sections.
    /// 3. Assign figures and tables to sections.
    /// 4. Build tree hierarchy via level-based stack.
    pub fn build(
        mut blocks: Vec<TextBlock>,
        headers: &[SectionHeader],
        figures: Vec<FigureInfo>,
        tables: Vec<TableInfo>,
        layouts: &[PageLayout],
    ) -> Vec<Section> {
        // Sort blocks into reading order.
        ReadingOrderSorter::sort(&mut blocks, layouts);

        // Build flat sections split at header boundaries.
        let flat = Self::split_by_headers(&blocks, headers);

        // Assign figures and tables.
        let flat_with_assets = Self::assign_assets(flat, figures, tables);

        // Build tree.
        Self::build_hierarchy(flat_with_assets)
    }

    fn split_by_headers(
        blocks: &[TextBlock],
        headers: &[SectionHeader],
    ) -> Vec<FlatSection> {
        // Collect header block_indices for fast lookup.
        let header_at: std::collections::HashMap<usize, &SectionHeader> = headers
            .iter()
            .map(|h| (h.block_index, h))
            .collect();

        let mut sections: Vec<FlatSection> = Vec::new();
        let mut current_header: Option<&SectionHeader> = None;
        let mut current_blocks: Vec<TextBlock> = Vec::new();

        for block in blocks {
            if let Some(header) = header_at.get(&block.global_index) {
                // Flush current section.
                if !current_blocks.is_empty() || current_header.is_some() {
                    sections.push(FlatSection {
                        header: current_header.cloned(),
                        blocks: std::mem::take(&mut current_blocks),
                        figures: Vec::new(),
                        tables: Vec::new(),
                    });
                }
                current_header = Some(header);
            } else {
                current_blocks.push(block.clone());
            }
        }

        // Final section.
        sections.push(FlatSection {
            header: current_header.cloned(),
            blocks: current_blocks,
            figures: Vec::new(),
            tables: Vec::new(),
        });

        sections
    }

    fn assign_assets(
        mut sections: Vec<FlatSection>,
        figures: Vec<FigureInfo>,
        tables: Vec<TableInfo>,
    ) -> Vec<FlatSection> {
        for figure in figures {
            let target_block_idx = figure.insertion_point.after_block_index;
            // Find the section that contains the target block.
            let section = sections.iter_mut().find(|s| {
                if let Some(idx) = target_block_idx {
                    s.blocks.iter().any(|b| b.global_index == idx)
                } else {
                    // No block index → assign to first section.
                    true
                }
            });
            if let Some(s) = section {
                s.figures.push(figure);
            }
        }

        for table in tables {
            let target_block_idx = table.insertion_point.after_block_index;
            let section = sections.iter_mut().find(|s| {
                if let Some(idx) = target_block_idx {
                    s.blocks.iter().any(|b| b.global_index == idx)
                } else {
                    true
                }
            });
            if let Some(s) = section {
                s.tables.push(table);
            }
        }

        sections
    }

    fn build_hierarchy(flat: Vec<FlatSection>) -> Vec<Section> {
        let mut roots: Vec<Section> = Vec::new();
        // Stack holds (level, section) pairs for building the tree.
        let mut stack: Vec<(u8, Section)> = Vec::new();

        for flat_sec in flat {
            let level = flat_sec.header.as_ref().map(|h| h.level).unwrap_or(1);
            let page_start = flat_sec
                .blocks
                .first()
                .map(|b| b.page)
                .or_else(|| flat_sec.header.as_ref().map(|h| h.page))
                .unwrap_or(0);
            let page_end = flat_sec
                .blocks
                .last()
                .map(|b| b.page)
                .unwrap_or(page_start);

            let section = Section {
                header: flat_sec.header,
                level,
                blocks: flat_sec.blocks,
                figures: flat_sec.figures,
                tables: flat_sec.tables,
                children: Vec::new(),
                page_range: (page_start, page_end),
            };

            // Pop stack entries at the same or deeper level.
            while stack.last().map(|(l, _)| *l >= level).unwrap_or(false) {
                let (_, finished) = stack.pop().unwrap();
                if let Some((_, parent)) = stack.last_mut() {
                    parent.children.push(finished);
                } else {
                    roots.push(finished);
                }
            }

            stack.push((level, section));
        }

        // Drain remaining stack into roots.
        while let Some((_, finished)) = stack.pop() {
            if let Some((_, parent)) = stack.last_mut() {
                parent.children.push(finished);
            } else {
                roots.push(finished);
            }
        }

        // Children were pushed in reverse — reverse them.
        Self::fix_children_order(&mut roots);
        roots
    }

    fn fix_children_order(sections: &mut Vec<Section>) {
        sections.reverse();
        for s in sections.iter_mut() {
            Self::fix_children_order(&mut s.children);
        }
    }
}

/// Intermediate flat section (before hierarchy construction).
struct FlatSection {
    header: Option<SectionHeader>,
    blocks: Vec<TextBlock>,
    figures: Vec<FigureInfo>,
    tables: Vec<TableInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BlockType, Rect, TextBlock, TextLine};

    fn make_block(global_index: usize, page: u32) -> TextBlock {
        TextBlock {
            global_index,
            lines: vec![],
            text: format!("block {global_index}"),
            bbox: Rect::new(72.0, 700.0 - global_index as f64 * 20.0, 540.0,
                            680.0 - global_index as f64 * 20.0),
            page,
            column_index: 0,
            block_type: BlockType::BodyText,
        }
    }

    fn make_header(block_index: usize, level: u8, text: &str, page: u32) -> SectionHeader {
        SectionHeader {
            text: text.to_string(),
            clean_text: text.to_string(),
            level,
            numbering: None,
            page,
            bbox: Rect::new(72.0, 700.0, 540.0, 690.0),
            block_index,
        }
    }

    #[test]
    fn flat_document_no_headers() {
        let blocks: Vec<TextBlock> = (0..3).map(|i| make_block(i, 0)).collect();
        let sections = SectionBuilder::build(blocks, &[], vec![], vec![], &[]);
        // Without headers, all blocks go into one implicit section.
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].blocks.len(), 3);
    }

    #[test]
    fn two_level1_sections() {
        let blocks: Vec<TextBlock> = (0..4).map(|i| make_block(i, 0)).collect();
        let headers = vec![
            make_header(0, 1, "INTRODUCTION", 0),
            make_header(2, 1, "METHODS", 0),
        ];
        let sections = SectionBuilder::build(blocks, &headers, vec![], vec![], &[]);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].header.as_ref().unwrap().clean_text, "INTRODUCTION");
        assert_eq!(sections[1].header.as_ref().unwrap().clean_text, "METHODS");
    }

    #[test]
    fn nested_subsection() {
        let blocks: Vec<TextBlock> = (0..4).map(|i| make_block(i, 0)).collect();
        let headers = vec![
            make_header(0, 1, "METHODS", 0),
            make_header(1, 2, "Data Collection", 0),
            make_header(3, 1, "RESULTS", 0),
        ];
        let sections = SectionBuilder::build(blocks, &headers, vec![], vec![], &[]);
        // Should produce 2 level-1 sections; METHODS should contain Data Collection.
        assert_eq!(sections.len(), 2);
        let methods = sections.iter().find(|s| {
            s.header.as_ref().map(|h| h.clean_text == "METHODS").unwrap_or(false)
        }).expect("METHODS section not found");
        assert_eq!(methods.children.len(), 1);
        assert_eq!(
            methods.children[0].header.as_ref().unwrap().clean_text,
            "Data Collection"
        );
    }
}
```

## Acceptance Criteria

- [ ] All unit tests pass: `cargo test -p pdf-lay-core -- structure`
  - ReadingOrderSorter: `left_column_before_right`, `top_to_bottom_within_column`, `full_width_interleaved_by_y`
  - SectionBuilder: `flat_document_no_headers`, `two_level1_sections`, `nested_subsection`
- [ ] Level-1 headers produce root sections; level-2 headers become children of the preceding level-1
- [ ] `Section::page_range` correctly reflects the first and last page of its blocks
- [ ] Figures and tables are correctly assigned to the section containing the referenced block
- [ ] `cargo clippy -p pdf-lay-core -- -D warnings` passes

## Dependencies

- Task 10 (HeaderDetector) must be completed first.

## Commit Message

```
feat(structure): add SectionBuilder and ReadingOrderSorter assembling Section hierarchy
```
