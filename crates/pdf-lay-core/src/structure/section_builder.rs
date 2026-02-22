//! Builds the Section hierarchy from blocks, headers, figures, and tables.

use crate::structure::reading_order::ReadingOrderSorter;
use crate::types::{FigureInfo, PageLayout, Section, SectionHeader, TableInfo, TextBlock};

/// Assembles Section hierarchy from flat lists of blocks and headers.
pub struct SectionBuilder;

impl SectionBuilder {
    /// Build the section hierarchy.
    ///
    /// 1. Sort blocks in reading order.
    /// 2. Split blocks at header boundaries -> flat sections.
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

    fn split_by_headers(blocks: &[TextBlock], headers: &[SectionHeader]) -> Vec<FlatSection> {
        // Collect header block_indices for fast lookup.
        let header_at: std::collections::HashMap<usize, &SectionHeader> =
            headers.iter().map(|h| (h.block_index, h)).collect();

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
                    // No block index -> assign to first section.
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
            let page_end = flat_sec.blocks.last().map(|b| b.page).unwrap_or(page_start);

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

        roots
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
    use crate::types::{BlockType, Rect, TextBlock};

    fn make_block(global_index: usize, page: u32) -> TextBlock {
        TextBlock {
            global_index,
            lines: vec![],
            text: format!("block {global_index}"),
            bbox: Rect::new(
                72.0,
                700.0 - global_index as f64 * 20.0,
                540.0,
                680.0 - global_index as f64 * 20.0,
            ),
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
        assert_eq!(
            sections[0].header.as_ref().unwrap().clean_text,
            "INTRODUCTION"
        );
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
        let methods = sections
            .iter()
            .find(|s| {
                s.header
                    .as_ref()
                    .map(|h| h.clean_text == "METHODS")
                    .unwrap_or(false)
            })
            .expect("METHODS section not found");
        assert_eq!(methods.children.len(), 1);
        assert_eq!(
            methods.children[0].header.as_ref().unwrap().clean_text,
            "Data Collection"
        );
    }
}
