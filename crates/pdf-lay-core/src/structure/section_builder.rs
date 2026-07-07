//! Builds the Section hierarchy from blocks, headers, figures, and tables.

use std::collections::{HashMap, HashSet};

use crate::config::HeaderDetectionConfig;
use crate::error::{NumberingAnomalyKind, PdfLayWarning};
use crate::structure::numbering::NumberingParser;
use crate::structure::reading_order::ReadingOrderSorter;
use crate::types::{
    FigureInfo, NumberComponent, PageLayout, Section, SectionHeader, TableInfo, TextBlock,
};

/// Assembles Section hierarchy from flat lists of blocks and headers.
pub struct SectionBuilder;

/// Numeric tag distinguishing numbering component variants (for comparing only
/// same-variant siblings, which sidesteps Roman/Alpha single-letter ambiguity).
fn variant_tag(c: &NumberComponent) -> u8 {
    match c {
        NumberComponent::Arabic(_) => 0,
        NumberComponent::Roman(_) => 1,
        NumberComponent::Alpha(_) => 2,
    }
}

/// A comparable signature of a numbering key: `(variant, ordinal)` per component.
fn key_signature(components: &[NumberComponent]) -> Vec<(u8, u32)> {
    components
        .iter()
        .map(|c| (variant_tag(c), c.ordinal()))
        .collect()
}

/// Detect section-numbering anomalies (duplicate, non-monotonic, skipped) across
/// the ordered headers, returning warnings. Sections are never dropped; this is
/// purely diagnostic (No Silent Drop).
pub fn validate_numbering(headers: &[SectionHeader]) -> Vec<PdfLayWarning> {
    let parser = NumberingParser::new();
    let mut warnings = Vec::new();
    let mut seen: HashSet<Vec<(u8, u32)>> = HashSet::new();
    // parent signature -> (variant, ordinal) of the last sibling seen.
    let mut last_child: HashMap<Vec<(u8, u32)>, (u8, u32)> = HashMap::new();

    for header in headers {
        let Some((key, _)) = parser.parse(&header.text) else {
            continue;
        };
        let sig = key_signature(&key.components);
        let Some(last) = key.components.last() else {
            continue;
        };

        if !seen.insert(sig.clone()) {
            warnings.push(PdfLayWarning::SectionNumberingAnomaly {
                kind: NumberingAnomalyKind::Duplicate,
                page: header.page,
            });
            continue;
        }

        let parent_sig = sig[..sig.len() - 1].to_vec();
        let variant = variant_tag(last);
        let ord = last.ordinal();

        if let Some((prev_variant, prev_ord)) = last_child.get(&parent_sig).copied() {
            // Only compare siblings of the same numbering variant.
            if prev_variant == variant {
                if ord <= prev_ord {
                    warnings.push(PdfLayWarning::SectionNumberingAnomaly {
                        kind: NumberingAnomalyKind::NonMonotonic,
                        page: header.page,
                    });
                } else if ord > prev_ord + 1 {
                    warnings.push(PdfLayWarning::SectionNumberingAnomaly {
                        kind: NumberingAnomalyKind::SkippedNumber,
                        page: header.page,
                    });
                }
            }
        }
        last_child.insert(parent_sig, (variant, ord));
    }

    warnings
}

impl SectionBuilder {
    /// Build the section hierarchy.
    ///
    /// 1. Sort blocks in reading order.
    /// 2. Split blocks at header boundaries -> flat sections. When too few
    ///    confident headers were detected (`headers.len() <
    ///    cfg.min_confident_headers`), use the no-confident-header fallback
    ///    segmenter instead (P1-6), so the whole document never silently
    ///    collapses into one opaque section.
    /// 3. Assign figures and tables to sections.
    /// 4. Build tree hierarchy via level-based stack.
    ///
    /// Returns the section tree plus any warnings raised while building it
    /// (currently just [`PdfLayWarning::HeaderlessSegmentation`], emitted
    /// when the fallback segmenter actually splits the document into more
    /// than one section).
    pub fn build(
        mut blocks: Vec<TextBlock>,
        headers: &[SectionHeader],
        figures: Vec<FigureInfo>,
        tables: Vec<TableInfo>,
        layouts: &[PageLayout],
        body_font_size: f64,
        cfg: &HeaderDetectionConfig,
    ) -> (Vec<Section>, Vec<PdfLayWarning>) {
        // Sort blocks into reading order.
        ReadingOrderSorter::sort(&mut blocks, layouts);

        let mut warnings = Vec::new();
        let flat = if headers.len() >= cfg.min_confident_headers {
            Self::split_by_headers(&blocks, headers)
        } else {
            let segmented = Self::segment_without_headers(&blocks, body_font_size, cfg);
            if segmented.len() > 1 {
                warnings.push(PdfLayWarning::HeaderlessSegmentation {
                    segments: segmented.len(),
                });
            }
            segmented
        };

        // Assign figures and tables.
        let flat_with_assets = Self::assign_assets(flat, figures, tables);

        // Build tree.
        (Self::build_hierarchy(flat_with_assets), warnings)
    }

    /// No-confident-header fallback (P1-6): segments reading-order-sorted
    /// blocks at font-shift / bold-shift boundaries instead of collapsing the
    /// whole document into one section. Each resulting [`FlatSection`] is
    /// headerless (`header: None`); a pseudo-heading block that triggered a
    /// boundary is kept as the first body block of its new section rather
    /// than being promoted to a real [`SectionHeader`] (no level/numbering is
    /// invented for it — see the module-level task doc). Every input block
    /// ends up in exactly one output section, so no text is dropped.
    ///
    /// If no boundary is ever found (e.g. a uniform-font document), the
    /// result is a single section — matching the legacy behavior for the case
    /// the fallback cannot improve on.
    fn segment_without_headers(
        blocks: &[TextBlock],
        body_font_size: f64,
        cfg: &HeaderDetectionConfig,
    ) -> Vec<FlatSection> {
        let mut sections: Vec<FlatSection> = Vec::new();
        let mut current: Vec<TextBlock> = Vec::new();

        for block in blocks {
            let boundary = current
                .last()
                .map(|prev| Self::is_fallback_boundary(prev, block, body_font_size, cfg))
                .unwrap_or(false);
            if boundary {
                sections.push(FlatSection {
                    header: None,
                    blocks: std::mem::take(&mut current),
                    figures: Vec::new(),
                    tables: Vec::new(),
                });
            }
            current.push(block.clone());
        }

        // Always emit a final section, even if empty, mirroring
        // `split_by_headers`'s trailing "final section" so a zero-block input
        // still produces one (empty) section rather than none.
        if !current.is_empty() || sections.is_empty() {
            sections.push(FlatSection {
                header: None,
                blocks: current,
                figures: Vec::new(),
                tables: Vec::new(),
            });
        }

        sections
    }

    /// Whether `block` (following `prev` in reading order) looks like a
    /// pseudo-heading boundary for the no-confident-header fallback: either a
    /// jump from body-sized text to a heading-sized font
    /// (`fallback_font_shift_ratio` above body size), or a shift from
    /// non-bold to bold. A bare page turn is not, by itself, treated as a
    /// boundary (too weak a signal on its own — it would otherwise fragment
    /// every multi-page document).
    fn is_fallback_boundary(
        prev: &TextBlock,
        block: &TextBlock,
        body_font_size: f64,
        cfg: &HeaderDetectionConfig,
    ) -> bool {
        let threshold = body_font_size * cfg.fallback_font_shift_ratio;
        let font_shift = body_font_size > 0.0
            && block.primary_font_size() >= threshold
            && prev.primary_font_size() < threshold;
        let bold_shift = block.is_bold() && !prev.is_bold();
        font_shift || bold_shift
    }

    fn split_by_headers(blocks: &[TextBlock], headers: &[SectionHeader]) -> Vec<FlatSection> {
        // Index headers by their anchor block's global_index for fast lookup.
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
    use crate::types::{BlockType, Rect, TextBlock, TextLine};

    const DEFAULT_BODY_FONT_SIZE: f64 = 10.0;

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

    /// A block with an explicit font size/bold flag (via a single line), for
    /// exercising the P1-6 font-shift/bold-shift fallback segmenter.
    fn make_styled_block(global_index: usize, page: u32, font_size: f64, bold: bool) -> TextBlock {
        let mut block = make_block(global_index, page);
        block.lines = vec![TextLine {
            spans: vec![],
            text: block.text.clone(),
            bbox: block.bbox.clone(),
            page,
            baseline_y: block.bbox.bottom,
            primary_font_size: font_size,
            primary_font_name: "Regular".to_string(),
            is_bold: bold,
        }];
        block
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

    /// Convenience wrapper for tests that don't care about the P1-6 fallback
    /// knobs: default config, and a body font size chosen so that
    /// `make_block`'s zero-font/non-bold synthetic blocks never trigger the
    /// fallback boundary.
    fn build_default(blocks: Vec<TextBlock>, headers: &[SectionHeader]) -> Vec<Section> {
        SectionBuilder::build(
            blocks,
            headers,
            vec![],
            vec![],
            &[],
            DEFAULT_BODY_FONT_SIZE,
            &HeaderDetectionConfig::default(),
        )
        .0
    }

    #[test]
    fn two_level1_sections() {
        let blocks: Vec<TextBlock> = (0..4).map(|i| make_block(i, 0)).collect();
        let headers = vec![
            make_header(0, 1, "INTRODUCTION", 0),
            make_header(2, 1, "METHODS", 0),
        ];
        let sections = build_default(blocks, &headers);
        assert_eq!(sections.len(), 2);
        assert_eq!(
            sections[0].header.as_ref().unwrap().clean_text,
            "INTRODUCTION"
        );
        assert_eq!(sections[1].header.as_ref().unwrap().clean_text, "METHODS");
    }

    #[test]
    fn numbering_prefix_builds_tree() {
        // "2 Methods" (depth 1) then "2.1 Data" (depth 2) → 2.1 nests under 2.
        let blocks = vec![make_block(0, 0), make_block(1, 0)];
        let headers = vec![
            make_header(0, 1, "2 Methods", 0),
            make_header(1, 2, "2.1 Data Collection", 0),
        ];
        let sections = build_default(blocks, &headers);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].children.len(), 1);
        assert_eq!(
            sections[0].children[0].header.as_ref().unwrap().clean_text,
            "2.1 Data Collection"
        );
    }

    #[test]
    fn skipped_number_warns_but_keeps_section() {
        let headers = vec![
            make_header(0, 1, "IV. Experiments", 0),
            make_header(1, 1, "VI. Results", 1),
        ];
        let warnings = validate_numbering(&headers);
        assert!(warnings.iter().any(|w| matches!(
            w,
            PdfLayWarning::SectionNumberingAnomaly {
                kind: NumberingAnomalyKind::SkippedNumber,
                ..
            }
        )));
        // The sections themselves are not dropped.
        let blocks = vec![make_block(0, 0), make_block(1, 1)];
        let sections = build_default(blocks, &headers);
        assert_eq!(sections.len(), 2);
    }

    #[test]
    fn duplicate_number_warns() {
        let headers = vec![
            make_header(0, 2, "2.1 First", 0),
            make_header(1, 2, "2.1 Second", 1),
        ];
        let warnings = validate_numbering(&headers);
        assert!(warnings.iter().any(|w| matches!(
            w,
            PdfLayWarning::SectionNumberingAnomaly {
                kind: NumberingAnomalyKind::Duplicate,
                ..
            }
        )));
    }

    #[test]
    fn unnumbered_falls_back_without_warnings() {
        let headers = vec![
            make_header(0, 1, "Introduction", 0),
            make_header(1, 1, "Methods", 0),
        ];
        assert!(validate_numbering(&headers).is_empty());
    }

    #[test]
    fn monotonic_numbering_has_no_warnings() {
        let headers = vec![
            make_header(0, 1, "1 Introduction", 0),
            make_header(1, 1, "2 Methods", 0),
            make_header(2, 2, "2.1 Data", 0),
            make_header(3, 1, "3 Results", 1),
        ];
        assert!(validate_numbering(&headers).is_empty());
    }

    #[test]
    fn header_anchored_by_global_index_not_position() {
        // Blocks have non-contiguous global_index values (as if some were
        // filtered upstream). A header anchored at global_index 20 must split at
        // that block regardless of its slice position.
        let blocks = vec![make_block(10, 0), make_block(20, 0), make_block(30, 0)];
        let headers = vec![make_header(20, 1, "METHODS", 0)];
        let sections = build_default(blocks, &headers);
        assert_eq!(sections.len(), 2);
        assert!(
            sections[0].header.is_none(),
            "preamble should be headerless"
        );
        assert_eq!(sections[0].blocks.len(), 1); // global_index 10
        assert_eq!(sections[1].header.as_ref().unwrap().clean_text, "METHODS");
        assert_eq!(sections[1].blocks.len(), 1); // global_index 30 (20 is the anchor)
    }

    #[test]
    fn nested_subsection() {
        let blocks: Vec<TextBlock> = (0..4).map(|i| make_block(i, 0)).collect();
        let headers = vec![
            make_header(0, 1, "METHODS", 0),
            make_header(1, 2, "Data Collection", 0),
            make_header(3, 1, "RESULTS", 0),
        ];
        let sections = build_default(blocks, &headers);
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

    // ---- P1-6: no-confident-header fallback -------------------------------

    #[test]
    fn no_headers_uniform_font_single_section() {
        // Zero headers, and every block shares the same (body-sized,
        // non-bold) font — there is no boundary signal for the fallback to
        // find, so it degrades to the legacy single-section result, and does
        // *not* emit HeaderlessSegmentation (the acceptance criterion is:
        // don't warn when the fallback couldn't actually split anything).
        let blocks: Vec<TextBlock> = (0..3)
            .map(|i| make_styled_block(i, 0, DEFAULT_BODY_FONT_SIZE, false))
            .collect();
        let (sections, warnings) = SectionBuilder::build(
            blocks,
            &[],
            vec![],
            vec![],
            &[],
            DEFAULT_BODY_FONT_SIZE,
            &HeaderDetectionConfig::default(),
        );
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].blocks.len(), 3);
        assert!(
            warnings.is_empty(),
            "a fallback that could not segment anything must not warn"
        );
    }

    #[test]
    fn no_headers_font_shift_segments_document() {
        // Zero headers, but 3 blocks jump from body size (10pt) to a
        // heading-like size (>= 1.15x body = 11.5pt): each such jump is a
        // fallback segmentation boundary, so the document must split into
        // multiple sections instead of collapsing into one, and
        // HeaderlessSegmentation must be reported (No Silent Drop: the
        // fallback is not a silent behavior switch).
        let blocks = vec![
            make_styled_block(0, 0, 10.0, false), // body
            make_styled_block(1, 0, 14.0, true),  // pseudo-heading (boundary)
            make_styled_block(2, 0, 10.0, false), // body
            make_styled_block(3, 0, 10.0, false), // body
            make_styled_block(4, 1, 14.0, true),  // pseudo-heading (boundary)
            make_styled_block(5, 1, 10.0, false), // body
        ];
        let cfg = HeaderDetectionConfig::default();
        let (sections, warnings) =
            SectionBuilder::build(blocks, &[], vec![], vec![], &[], 10.0, &cfg);
        assert!(
            sections.len() > 1,
            "font-shift boundaries must split a headerless document into multiple sections"
        );
        assert!(sections.iter().all(|s| s.header.is_none()));
        assert!(warnings.iter().any(
            |w| matches!(w, PdfLayWarning::HeaderlessSegmentation { segments } if *segments == sections.len())
        ));
    }

    #[test]
    fn fallback_preserves_all_blocks() {
        // No text may be dropped by the fallback segmenter (No Silent Drop):
        // every input block must appear in exactly one output section.
        let blocks = vec![
            make_styled_block(0, 0, 10.0, false),
            make_styled_block(1, 0, 14.0, true),
            make_styled_block(2, 0, 10.0, false),
            make_styled_block(3, 1, 14.0, true),
            make_styled_block(4, 1, 10.0, false),
        ];
        let input_len = blocks.len();
        let cfg = HeaderDetectionConfig::default();
        let (sections, _) = SectionBuilder::build(blocks, &[], vec![], vec![], &[], 10.0, &cfg);
        let total_blocks: usize = sections.iter().map(|s| s.blocks.len()).sum();
        assert_eq!(total_blocks, input_len);
    }

    #[test]
    fn fallback_disabled_reproduces_legacy_single_section() {
        // `min_confident_headers = 0` restores the pre-P1-6 behavior: always
        // use header-based splitting, so zero detected headers still
        // collapses into a single implicit section (opt-out for callers that
        // relied on the old shape).
        let blocks = vec![
            make_styled_block(0, 0, 10.0, false),
            make_styled_block(1, 0, 14.0, true),
            make_styled_block(2, 1, 10.0, false),
        ];
        let cfg = HeaderDetectionConfig {
            min_confident_headers: 0,
            ..HeaderDetectionConfig::default()
        };
        let (sections, warnings) =
            SectionBuilder::build(blocks, &[], vec![], vec![], &[], 10.0, &cfg);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].blocks.len(), 3);
        assert!(warnings.is_empty());
    }

    #[test]
    fn documents_with_headers_are_unaffected_by_fallback() {
        // With >= min_confident_headers detected, the fallback path is never
        // taken, even if the blocks would otherwise look like they contain
        // font-shift boundaries; no HeaderlessSegmentation warning fires.
        let blocks: Vec<TextBlock> = (0..4)
            .map(|i| make_styled_block(i, 0, 14.0, true))
            .collect();
        let headers = vec![
            make_header(0, 1, "INTRODUCTION", 0),
            make_header(2, 1, "METHODS", 0),
        ];
        let cfg = HeaderDetectionConfig::default();
        let (sections, warnings) =
            SectionBuilder::build(blocks, &headers, vec![], vec![], &[], 10.0, &cfg);
        assert_eq!(sections.len(), 2);
        assert!(
            !warnings
                .iter()
                .any(|w| matches!(w, PdfLayWarning::HeaderlessSegmentation { .. }))
        );
    }
}
