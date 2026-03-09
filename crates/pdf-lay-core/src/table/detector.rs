//! Table region detection.

use std::collections::HashMap;

use crate::config::TableConfig;
use crate::figure::{CaptionInfo, CaptionType};
use crate::types::{BlockType, PathObject, PathType, Rect, TextBlock};

/// Detected table region (intermediate representation).
pub(crate) struct TableRegion {
    pub bbox: Rect,
    pub page: u32,
    pub block_indices: Vec<usize>,
    pub caption: Option<CaptionInfo>,
    pub has_rules: bool,
    #[allow(dead_code)]
    pub horizontal_rules: Vec<PathObject>,
    #[allow(dead_code)]
    pub vertical_rules: Vec<PathObject>,
}

/// Detects table regions from path objects and text blocks.
pub struct TableDetector {
    config: TableConfig,
}

impl TableDetector {
    /// Create a new `TableDetector` with the given configuration.
    pub fn new(config: TableConfig) -> Self {
        Self { config }
    }

    /// Main entry: detect all table regions using both rule-based and text-alignment methods.
    pub(crate) fn detect(
        &self,
        blocks: &[TextBlock],
        paths: &[PathObject],
        table_captions: &[&CaptionInfo],
    ) -> Vec<TableRegion> {
        let mut regions = Vec::new();

        if self.config.use_rule_detection {
            regions.extend(self.detect_by_rules(blocks, paths));
        }

        if self.config.use_text_alignment {
            let text_regions = self.detect_by_text_alignment(blocks, table_captions);
            // Deduplicate: skip text-alignment regions that overlap with rule-based ones
            for tr in text_regions {
                let dominated = regions
                    .iter()
                    .any(|r| r.bbox.overlaps(&tr.bbox) && r.page == tr.page);
                if !dominated {
                    regions.push(tr);
                }
            }
        }

        // Assign captions to detected regions by spatial proximity.
        Self::assign_captions(&mut regions, table_captions);

        regions
    }

    /// Assign captions to detected table regions by spatial proximity.
    ///
    /// For each table caption, finds the closest unmatched region on the same page
    /// within a maximum vertical gap of 100 points.
    fn assign_captions(regions: &mut [TableRegion], table_captions: &[&CaptionInfo]) {
        for caption in table_captions {
            if caption.caption_type != CaptionType::Table {
                continue;
            }
            // Find the closest unmatched region on the same page.
            let mut best: Option<(usize, f64)> = None;
            for (i, region) in regions.iter().enumerate() {
                if region.page != caption.page {
                    continue;
                }
                if region.caption.is_some() {
                    continue; // already matched
                }
                // Compute vertical gap between caption and table region.
                // In PDF Y-up coordinates:
                //   caption above table: caption.bbox.bottom >= region.bbox.top
                //   caption below table: caption.bbox.top <= region.bbox.bottom
                //   overlapping: neither condition holds
                let gap = if caption.bbox.bottom >= region.bbox.top {
                    // Caption is above table (normal case)
                    caption.bbox.bottom - region.bbox.top
                } else if caption.bbox.top <= region.bbox.bottom {
                    // Caption is below table
                    region.bbox.bottom - caption.bbox.top
                } else {
                    // Caption overlaps with table vertically
                    0.0
                };

                if gap < 100.0 {
                    match best {
                        None => best = Some((i, gap)),
                        Some((_, prev_gap)) if gap < prev_gap => best = Some((i, gap)),
                        _ => {}
                    }
                }
            }
            if let Some((idx, _)) = best {
                regions[idx].caption = Some((*caption).clone());
            }
        }
    }

    /// Text alignment-based detection: find columnar text below table captions.
    fn detect_by_text_alignment(
        &self,
        blocks: &[TextBlock],
        table_captions: &[&CaptionInfo],
    ) -> Vec<TableRegion> {
        let mut regions = Vec::new();

        for caption in table_captions {
            if caption.caption_type != CaptionType::Table {
                continue;
            }

            // Collect candidate blocks below the caption
            let candidates = collect_candidates_below_caption(blocks, caption);
            if candidates.is_empty() {
                continue;
            }

            // Cluster X-coordinates
            let x_coords: Vec<f64> = candidates.iter().map(|b| b.bbox.left).collect();
            let n_clusters = count_x_clusters(&x_coords, self.config.column_alignment_tolerance);

            if n_clusters >= self.config.min_columns {
                let block_indices: Vec<usize> = candidates.iter().map(|b| b.global_index).collect();
                let mut bbox = candidates[0].bbox.clone();
                for b in &candidates[1..] {
                    bbox = bbox.union(&b.bbox);
                }
                regions.push(TableRegion {
                    bbox,
                    page: caption.page,
                    block_indices,
                    caption: None,
                    has_rules: false,
                    horizontal_rules: vec![],
                    vertical_rules: vec![],
                });
            }
        }

        regions
    }

    /// Rule-based detection: find grid patterns from horizontal/vertical lines.
    ///
    /// Algorithm:
    /// 1. Group paths by page.
    /// 2. For each page, separate horizontal and vertical lines.
    /// 3. Cluster horizontal lines by Y-coordinate (within ~3.0 pt tolerance).
    ///    Each unique Y-level is one rule line.
    /// 4. Sort Y-level clusters from top to bottom and split them into separate
    ///    "bands" wherever consecutive Y-levels are more than `GRID_GAP_THRESHOLD`
    ///    points apart (they belong to different candidate tables).
    /// 5. For each horizontal band, collect vertical lines whose X-range overlaps
    ///    the band's X-extent and whose Y-range overlaps the band's Y-extent, then
    ///    cluster those by X-coordinate.
    /// 6. Require at least 3 distinct Y-levels and 2 distinct X-levels.
    /// 7. Build `TableRegion` from the grid bounding box.
    /// 8. Assign any `TextBlock` whose bbox overlaps the region to `block_indices`.
    fn detect_by_rules(&self, blocks: &[TextBlock], paths: &[PathObject]) -> Vec<TableRegion> {
        const CLUSTER_TOLERANCE: f64 = 3.0;
        // Maximum gap between consecutive Y-levels still considered the same table.
        const GRID_GAP_THRESHOLD: f64 = 60.0;

        // Group paths by page.
        let mut by_page: HashMap<u32, Vec<&PathObject>> = HashMap::new();
        for p in paths {
            by_page.entry(p.page).or_default().push(p);
        }

        let mut regions = Vec::new();

        for (page, page_paths) in &by_page {
            // Separate horizontal and vertical lines.
            let horizontals: Vec<&PathObject> = page_paths
                .iter()
                .copied()
                .filter(|p| matches!(p.path_type, PathType::Horizontal))
                .collect();

            let verticals: Vec<&PathObject> = page_paths
                .iter()
                .copied()
                .filter(|p| matches!(p.path_type, PathType::Vertical))
                .collect();

            // Cluster horizontals by Y-center coordinate into distinct Y-levels.
            let mut h_clusters =
                cluster_by_coordinate(&horizontals, CLUSTER_TOLERANCE, |p| p.bbox.center_y());

            if h_clusters.is_empty() {
                continue;
            }

            // Sort Y-level clusters from top (highest Y) to bottom (lowest Y).
            h_clusters.sort_by(|a, b| {
                let ya: f64 = a.iter().map(|p| p.bbox.center_y()).sum::<f64>() / a.len() as f64;
                let yb: f64 = b.iter().map(|p| p.bbox.center_y()).sum::<f64>() / b.len() as f64;
                yb.partial_cmp(&ya).unwrap_or(std::cmp::Ordering::Equal)
            });

            // Split sorted Y-level clusters into separate bands wherever there is a
            // large Y-gap between consecutive levels.
            let bands = split_into_bands(h_clusters, GRID_GAP_THRESHOLD);

            for band in bands {
                if band.len() < 3 {
                    // Not enough distinct Y-levels for a table.
                    continue;
                }

                // Flatten all horizontal paths in this band.
                let all_h_in_band: Vec<&PathObject> = band
                    .iter()
                    .flat_map(|cluster| cluster.iter().copied())
                    .collect();

                // Bounding box of the horizontal band.
                let band_bbox = bounding_box_of_paths(all_h_in_band.iter().copied());

                // Collect verticals that spatially overlap this band:
                // X-center must fall within the band's X-range (with tolerance) and
                // Y-range must overlap the band's Y-range.
                let relevant_verticals: Vec<&PathObject> = verticals
                    .iter()
                    .copied()
                    .filter(|v| {
                        let vx = v.bbox.center_x();
                        let x_ok = vx >= band_bbox.left - CLUSTER_TOLERANCE
                            && vx <= band_bbox.right + CLUSTER_TOLERANCE;
                        let y_ok = v.bbox.top >= band_bbox.bottom && v.bbox.bottom <= band_bbox.top;
                        x_ok && y_ok
                    })
                    .collect();

                // Cluster relevant verticals by X-center coordinate.
                let v_clusters =
                    cluster_by_coordinate(&relevant_verticals, CLUSTER_TOLERANCE, |p| {
                        p.bbox.center_x()
                    });

                if v_clusters.len() < 2 {
                    continue;
                }

                // Collect owned copies of participating paths.
                let h_paths: Vec<PathObject> = all_h_in_band.iter().map(|p| (*p).clone()).collect();
                let v_paths: Vec<PathObject> = v_clusters
                    .iter()
                    .flat_map(|cluster| cluster.iter().map(|p| (*p).clone()))
                    .collect();

                // Compute final bounding box from all participating paths.
                let bbox = bounding_box_of_paths(h_paths.iter().chain(v_paths.iter()));

                // Find text blocks that overlap with this region on the same page.
                let block_indices: Vec<usize> = blocks
                    .iter()
                    .filter(|b| b.page == *page && b.bbox.overlaps(&bbox))
                    .map(|b| b.global_index)
                    .collect();

                regions.push(TableRegion {
                    bbox,
                    page: *page,
                    block_indices,
                    caption: None,
                    has_rules: true,
                    horizontal_rules: h_paths,
                    vertical_rules: v_paths,
                });
            }
        }

        // Sort regions for deterministic output: page first, then top-to-bottom.
        regions.sort_by(|a, b| {
            a.page.cmp(&b.page).then_with(|| {
                b.bbox
                    .top
                    .partial_cmp(&a.bbox.top)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        });

        regions
    }
}

/// Group a slice of path references into clusters where each cluster's representative
/// coordinate is within `tolerance` of every other member.
///
/// Uses a greedy single-pass approach: for each path, find an existing cluster whose
/// representative (mean) coordinate is within `tolerance`; if none found, start a new
/// cluster.
fn cluster_by_coordinate<'a, F>(
    paths: &[&'a PathObject],
    tolerance: f64,
    coord: F,
) -> Vec<Vec<&'a PathObject>>
where
    F: Fn(&PathObject) -> f64,
{
    // Each entry: (representative coordinate, members).
    let mut clusters: Vec<(f64, Vec<&'a PathObject>)> = Vec::new();

    for &path in paths {
        let c = coord(path);
        let mut placed = false;
        for (rep, members) in clusters.iter_mut() {
            if (c - *rep).abs() <= tolerance {
                members.push(path);
                // Update representative to mean of current cluster coordinates.
                *rep = members.iter().map(|p| coord(p)).sum::<f64>() / members.len() as f64;
                placed = true;
                break;
            }
        }
        if !placed {
            clusters.push((c, vec![path]));
        }
    }

    clusters.into_iter().map(|(_, members)| members).collect()
}

/// Split a list of Y-level clusters (already sorted top-to-bottom, i.e. descending Y)
/// into separate "bands" wherever the gap between consecutive Y-levels exceeds
/// `gap_threshold` points.
///
/// Each cluster is a `Vec<&PathObject>` representing a single horizontal rule level.
/// Returns a `Vec` of bands, each band being a `Vec<Vec<&PathObject>>`.
fn split_into_bands<'a>(
    sorted_clusters: Vec<Vec<&'a PathObject>>,
    gap_threshold: f64,
) -> Vec<Vec<Vec<&'a PathObject>>> {
    if sorted_clusters.is_empty() {
        return Vec::new();
    }

    let mut bands: Vec<Vec<Vec<&'a PathObject>>> = Vec::new();
    let mut current_band: Vec<Vec<&'a PathObject>> = Vec::new();

    for cluster in sorted_clusters {
        if current_band.is_empty() {
            current_band.push(cluster);
            continue;
        }

        // Mean Y of the last cluster in the current band and the incoming cluster.
        let last = current_band.last().unwrap();
        let last_y: f64 = last.iter().map(|p| p.bbox.center_y()).sum::<f64>() / last.len() as f64;
        let this_y: f64 =
            cluster.iter().map(|p| p.bbox.center_y()).sum::<f64>() / cluster.len() as f64;

        // Sorted top-to-bottom: last_y >= this_y; gap = last_y - this_y.
        let gap = last_y - this_y;
        if gap > gap_threshold {
            // Start a new band.
            bands.push(current_band);
            current_band = vec![cluster];
        } else {
            current_band.push(cluster);
        }
    }

    if !current_band.is_empty() {
        bands.push(current_band);
    }

    bands
}

/// Collect candidate blocks below a table caption for text-alignment detection.
fn collect_candidates_below_caption<'a>(
    blocks: &'a [TextBlock],
    caption: &CaptionInfo,
) -> Vec<&'a TextBlock> {
    let mut candidates = Vec::new();
    for block in blocks {
        if block.page != caption.page {
            continue;
        }
        // Block must be below caption (Y-up: block.top < caption.bottom)
        if block.bbox.top >= caption.bbox.bottom - 5.0 {
            continue;
        }
        // Within 200 pts vertical distance
        if caption.bbox.bottom - block.bbox.top > 200.0 {
            continue;
        }
        // Stop on section headers or long paragraphs
        if matches!(
            block.block_type,
            BlockType::SectionHeader | BlockType::SubsectionHeader
        ) {
            break;
        }
        if block.text.len() > 150 {
            break;
        }
        // Only short blocks (table cells)
        if block.text.len() < 80 || block.lines.len() <= 2 {
            candidates.push(block);
        }
    }
    candidates
}

/// Count distinct X-coordinate clusters using a simple sort-and-gap approach.
fn count_x_clusters(x_coords: &[f64], tolerance: f64) -> usize {
    if x_coords.is_empty() {
        return 0;
    }
    let mut sorted: Vec<f64> = x_coords.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut count = 1;
    for i in 1..sorted.len() {
        if sorted[i] - sorted[i - 1] > tolerance {
            count += 1;
        }
    }
    count
}

/// Compute the union bounding box of an iterator of `PathObject` references.
///
/// # Panics
///
/// Panics in debug builds if the iterator is empty.
fn bounding_box_of_paths<'a>(mut paths: impl Iterator<Item = &'a PathObject>) -> Rect {
    let first = paths
        .next()
        .expect("bounding_box_of_paths called with empty iterator");
    let mut bbox = first.bbox.clone();
    for p in paths {
        bbox = bbox.union(&p.bbox);
    }
    bbox
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{make_block_from_line, make_line, make_path_object};

    fn make_horizontal(page: u32, left: f64, y: f64, right: f64) -> PathObject {
        // Horizontal line: thin height around the given Y center.
        make_path_object(page, left, y + 0.5, right, y - 0.5, PathType::Horizontal)
    }

    fn make_vertical(page: u32, x: f64, top: f64, bottom: f64) -> PathObject {
        // Vertical line: thin width around the given X center.
        make_path_object(page, x - 0.5, top, x + 0.5, bottom, PathType::Vertical)
    }

    /// Default detector with text-alignment disabled to isolate rule-based logic.
    fn default_detector() -> TableDetector {
        TableDetector::new(TableConfig {
            use_text_alignment: false,
            ..TableConfig::default()
        })
    }

    /// A simple 3-horizontal x 2-vertical grid should yield exactly 1 table region.
    #[test]
    fn test_detect_simple_grid() {
        let detector = default_detector();

        // 3 horizontal lines at y = 100, 80, 60.
        let h1 = make_horizontal(0, 50.0, 100.0, 300.0);
        let h2 = make_horizontal(0, 50.0, 80.0, 300.0);
        let h3 = make_horizontal(0, 50.0, 60.0, 300.0);

        // 2 vertical lines at x = 50 and x = 300.
        let v1 = make_vertical(0, 50.0, 100.0, 60.0);
        let v2 = make_vertical(0, 300.0, 100.0, 60.0);

        let paths = vec![h1, h2, h3, v1, v2];
        let regions = detector.detect(&[], &paths, &[]);

        assert_eq!(regions.len(), 1, "expected exactly 1 table region");
        assert!(regions[0].has_rules);
        assert_eq!(regions[0].page, 0);
    }

    /// Fewer than 3 horizontal or fewer than 2 vertical lines should produce no table.
    #[test]
    fn test_no_table_without_enough_lines() {
        let detector = default_detector();

        // Only 2 horizontal lines — not enough.
        let paths = vec![
            make_horizontal(0, 50.0, 100.0, 300.0),
            make_horizontal(0, 50.0, 80.0, 300.0),
            make_vertical(0, 50.0, 100.0, 80.0),
            make_vertical(0, 300.0, 100.0, 80.0),
        ];
        let regions = detector.detect(&[], &paths, &[]);
        assert!(
            regions.is_empty(),
            "should detect no table with only 2 horizontal lines"
        );

        // 3 horizontal lines but only 1 vertical line — not enough verticals.
        let paths2 = vec![
            make_horizontal(0, 50.0, 100.0, 300.0),
            make_horizontal(0, 50.0, 80.0, 300.0),
            make_horizontal(0, 50.0, 60.0, 300.0),
            make_vertical(0, 50.0, 100.0, 60.0),
        ];
        let regions2 = detector.detect(&[], &paths2, &[]);
        assert!(
            regions2.is_empty(),
            "should detect no table with only 1 vertical line"
        );
    }

    /// Two spatially separated grid patterns on the same page yield 2 separate regions.
    #[test]
    fn test_multiple_tables_on_page() {
        let detector = default_detector();

        // Table A: y = 700, 680, 660 and x = 50, 200.
        // Table B: y = 400, 380, 360 and x = 50, 200 (same X range, different Y range).
        // Gap between A and B: 660 - 400 = 260 pt > 60 pt threshold → separate bands.
        let a_h1 = make_horizontal(0, 50.0, 700.0, 200.0);
        let a_h2 = make_horizontal(0, 50.0, 680.0, 200.0);
        let a_h3 = make_horizontal(0, 50.0, 660.0, 200.0);
        let a_v1 = make_vertical(0, 50.0, 700.0, 660.0);
        let a_v2 = make_vertical(0, 200.0, 700.0, 660.0);

        let b_h1 = make_horizontal(0, 50.0, 400.0, 200.0);
        let b_h2 = make_horizontal(0, 50.0, 380.0, 200.0);
        let b_h3 = make_horizontal(0, 50.0, 360.0, 200.0);
        let b_v1 = make_vertical(0, 50.0, 400.0, 360.0);
        let b_v2 = make_vertical(0, 200.0, 400.0, 360.0);

        let paths = vec![a_h1, a_h2, a_h3, a_v1, a_v2, b_h1, b_h2, b_h3, b_v1, b_v2];
        let regions = detector.detect(&[], &paths, &[]);

        assert_eq!(regions.len(), 2, "expected 2 separate table regions");
        // Regions are sorted top-to-bottom, so region 0 should have higher Y.
        assert!(
            regions[0].bbox.top > regions[1].bbox.top,
            "region 0 should be higher on the page than region 1"
        );
    }

    /// Text blocks whose bbox overlaps the detected region are included in block_indices.
    #[test]
    fn test_blocks_assigned_to_region() {
        let detector = default_detector();

        // Simple 3x2 grid: y = 100, 80, 60; x = 50, 300.
        let h1 = make_horizontal(0, 50.0, 100.0, 300.0);
        let h2 = make_horizontal(0, 50.0, 80.0, 300.0);
        let h3 = make_horizontal(0, 50.0, 60.0, 300.0);
        let v1 = make_vertical(0, 50.0, 100.0, 60.0);
        let v2 = make_vertical(0, 300.0, 100.0, 60.0);
        let paths = vec![h1, h2, h3, v1, v2];

        // Block 0: inside the table bbox (x: ~50-300, y: ~60-100).
        let line_inside = make_line("Cell text", 70.0, 95.0, 10.0, 0);
        let block_inside = make_block_from_line(line_inside, 0);

        // Block 1: outside the table bbox (far above the table).
        let line_outside = make_line("Outside text", 10.0, 500.0, 10.0, 0);
        let block_outside = make_block_from_line(line_outside, 1);

        let blocks = vec![block_inside, block_outside];
        let regions = detector.detect(&blocks, &paths, &[]);

        assert_eq!(regions.len(), 1);
        assert!(
            regions[0].block_indices.contains(&0),
            "block 0 (inside) should be in block_indices"
        );
        assert!(
            !regions[0].block_indices.contains(&1),
            "block 1 (outside) should NOT be in block_indices"
        );
    }

    // --- Text alignment detection tests ---

    fn make_short_block(text: &str, left: f64, top: f64, page: u32, idx: usize) -> TextBlock {
        let line = make_line(text, left, top, 10.0, page);
        make_block_from_line(line, idx)
    }

    #[test]
    fn test_text_alignment_detects_columnar_data() {
        use crate::test_helpers::make_caption_info;
        let detector = TableDetector::new(TableConfig {
            use_rule_detection: false,
            use_text_alignment: true,
            min_columns: 2,
            column_alignment_tolerance: 5.0,
        });

        let caption = make_caption_info(0, CaptionType::Table, 1, "Results", 0);
        // Caption bbox is at top=100, bottom=90. Blocks below should have top < 85
        let b1 = make_short_block("Val1", 50.0, 80.0, 0, 1);
        let b2 = make_short_block("Val2", 150.0, 80.0, 0, 2);
        let b3 = make_short_block("Val3", 50.0, 70.0, 0, 3);
        let b4 = make_short_block("Val4", 150.0, 70.0, 0, 4);

        let blocks = vec![
            make_short_block("Table 1. Results", 50.0, 100.0, 0, 0),
            b1,
            b2,
            b3,
            b4,
        ];
        let captions = vec![&caption];
        let regions = detector.detect(&blocks, &[], &captions);

        assert_eq!(regions.len(), 1, "expected 1 text-alignment table region");
        assert!(!regions[0].has_rules);
    }

    #[test]
    fn test_no_table_without_caption() {
        let detector = TableDetector::new(TableConfig {
            use_rule_detection: false,
            use_text_alignment: true,
            ..TableConfig::default()
        });
        let blocks = vec![make_short_block("data", 50.0, 80.0, 0, 0)];
        let regions = detector.detect(&blocks, &[], &[]);
        assert!(regions.is_empty());
    }

    #[test]
    fn test_x_cluster_count() {
        assert_eq!(count_x_clusters(&[50.0, 52.0, 150.0, 153.0], 5.0), 2);
        assert_eq!(count_x_clusters(&[50.0, 52.0, 150.0, 153.0], 200.0), 1);
        assert_eq!(count_x_clusters(&[], 5.0), 0);
    }

    /// Lines within tolerance are treated as a single cluster level.
    #[test]
    fn test_lines_within_tolerance_form_single_cluster() {
        let detector = default_detector();

        // h1 (y=100) and h1b (y=101.5): within 3.0 pt — collapse to 1 level.
        // Together with h2 (y=80) that is only 2 distinct Y-levels → no table.
        let h1 = make_horizontal(0, 50.0, 100.0, 300.0);
        let h1b = make_horizontal(0, 50.0, 101.5, 300.0);
        let h2 = make_horizontal(0, 50.0, 80.0, 300.0);
        let v1 = make_vertical(0, 50.0, 101.5, 80.0);
        let v2 = make_vertical(0, 300.0, 101.5, 80.0);

        let paths = vec![h1, h1b, h2, v1, v2];
        let regions = detector.detect(&[], &paths, &[]);

        assert!(
            regions.is_empty(),
            "clustered horizontals collapse to 2 levels → no table"
        );
    }

    // --- Caption assignment tests ---

    /// Helper to build a TableRegion with an explicit bbox and page.
    fn make_table_region(page: u32, left: f64, top: f64, right: f64, bottom: f64) -> TableRegion {
        TableRegion {
            bbox: Rect {
                left,
                top,
                right,
                bottom,
            },
            page,
            block_indices: vec![],
            caption: None,
            has_rules: true,
            horizontal_rules: vec![],
            vertical_rules: vec![],
        }
    }

    /// Helper to build a CaptionInfo with an explicit bbox.
    fn make_caption_with_bbox(
        page: u32,
        left: f64,
        top: f64,
        right: f64,
        bottom: f64,
    ) -> CaptionInfo {
        CaptionInfo {
            block_index: 0,
            caption_type: CaptionType::Table,
            prefix: "Table 1".to_string(),
            number: Some(1),
            description: "Test caption".to_string(),
            full_text: "Table 1. Test caption".to_string(),
            page,
            bbox: Rect {
                left,
                top,
                right,
                bottom,
            },
        }
    }

    /// A caption within 100pt gap should be assigned to the nearest region.
    #[test]
    fn test_caption_assigned_to_nearest_region() {
        // Table region: top=500, bottom=400 on page 0.
        // Caption: top=520, bottom=510 — caption is above table.
        // Gap = caption.bbox.bottom - region.bbox.top = 510 - 500 = 10 pt (< 100).
        let mut regions = vec![make_table_region(0, 50.0, 500.0, 300.0, 400.0)];
        let caption = make_caption_with_bbox(0, 50.0, 520.0, 300.0, 510.0);
        let captions = vec![&caption];

        TableDetector::assign_captions(&mut regions, &captions);

        assert!(
            regions[0].caption.is_some(),
            "caption should be assigned to the region within 100pt gap"
        );
        assert_eq!(
            regions[0].caption.as_ref().unwrap().number,
            Some(1),
            "assigned caption should have number 1"
        );
    }

    /// A caption more than 100pt away should NOT be assigned.
    #[test]
    fn test_caption_not_assigned_when_too_far() {
        // Table region: top=500, bottom=400 on page 0.
        // Caption: top=620, bottom=610 — caption is above table.
        // Gap = caption.bbox.bottom - region.bbox.top = 610 - 500 = 110 pt (>= 100).
        let mut regions = vec![make_table_region(0, 50.0, 500.0, 300.0, 400.0)];
        let caption = make_caption_with_bbox(0, 50.0, 620.0, 300.0, 610.0);
        let captions = vec![&caption];

        TableDetector::assign_captions(&mut regions, &captions);

        assert!(
            regions[0].caption.is_none(),
            "caption more than 100pt away should not be assigned"
        );
    }

    /// Caption on a different page should not match any region.
    #[test]
    fn test_caption_not_assigned_to_different_page() {
        // Region is on page 0; caption is on page 1.
        let mut regions = vec![make_table_region(0, 50.0, 500.0, 300.0, 400.0)];
        let caption = make_caption_with_bbox(1, 50.0, 520.0, 300.0, 510.0);
        let captions = vec![&caption];

        TableDetector::assign_captions(&mut regions, &captions);

        assert!(
            regions[0].caption.is_none(),
            "caption on a different page must not be assigned"
        );
    }

    /// Among multiple regions, the caption should be assigned to the closest one.
    #[test]
    fn test_caption_assigned_to_nearest_of_two_regions() {
        // Region A: top=500, bottom=400 — gap from caption = 510 - 500 = 10 pt.
        // Region B: top=470, bottom=350 — gap from caption = 510 - 470 = 40 pt.
        // Caption should go to Region A (closer).
        let mut regions = vec![
            make_table_region(0, 50.0, 500.0, 300.0, 400.0),
            make_table_region(0, 50.0, 470.0, 300.0, 350.0),
        ];
        let caption = make_caption_with_bbox(0, 50.0, 520.0, 300.0, 510.0);
        let captions = vec![&caption];

        TableDetector::assign_captions(&mut regions, &captions);

        assert!(
            regions[0].caption.is_some(),
            "caption should be assigned to the nearer region (A)"
        );
        assert!(
            regions[1].caption.is_none(),
            "farther region (B) should not receive the caption"
        );
    }
}
