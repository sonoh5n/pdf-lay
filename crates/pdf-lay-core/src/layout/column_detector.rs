//! Detects column layout (1-column, 2-column, mixed) from TextLine positions.

use crate::types::{Column, LayoutRegion, PageDimensions, PageLayout, TextLine};
use std::collections::HashMap;

/// Analyzes text line positions to detect the column layout of a page.
pub struct ColumnDetector {
    /// Histogram bin width in points (default: 10.0).
    bin_width: f64,
    /// Minimum fraction of lines needed to qualify as a column peak (default: 0.20 = 20%).
    min_peak_ratio: f64,
    /// Maximum X gap between two peaks to be considered the same cluster,
    /// expressed as a fraction of page width (default: 0.15 = 15%).
    cluster_gap_ratio: f64,
    /// Number of Y-direction zones to divide the page into (default: 4).
    zone_count: usize,
}

impl Default for ColumnDetector {
    fn default() -> Self {
        Self {
            bin_width: 10.0,
            min_peak_ratio: 0.20,
            cluster_gap_ratio: 0.15,
            zone_count: 4,
        }
    }
}

impl ColumnDetector {
    /// Create with default parameters.
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the histogram bin width in points.
    pub fn with_bin_width(mut self, bin_width: f64) -> Self {
        self.bin_width = bin_width;
        self
    }

    /// Detect the column layout for a single page.
    pub fn detect(&self, lines: &[TextLine], page_dims: &PageDimensions) -> PageLayout {
        // Filter to lines on this page.
        let page_lines: Vec<&TextLine> = lines
            .iter()
            .filter(|l| l.page == page_dims.page_number)
            .collect();

        if page_lines.is_empty() {
            // Blank page: single full-width column.
            return PageLayout {
                regions: vec![LayoutRegion {
                    y_top: page_dims.height,
                    y_bottom: 0.0,
                    columns: vec![Column {
                        left: 0.0,
                        right: page_dims.width,
                        index: 0,
                    }],
                }],
                page_width: page_dims.width,
                page_height: page_dims.height,
                page: page_dims.page_number,
            };
        }

        // Split into Y-zones and detect columns per zone.
        let zones = self.build_zones(&page_lines, page_dims);
        let zone_layouts: Vec<ZoneLayout> = zones
            .iter()
            .map(|(y_top, y_bottom, zone_lines)| {
                self.detect_zone_columns(zone_lines, page_dims, *y_top, *y_bottom)
            })
            .collect();

        // Merge adjacent zones with the same column count.
        let merged = self.merge_zones(zone_layouts);

        PageLayout {
            regions: merged,
            page_width: page_dims.width,
            page_height: page_dims.height,
            page: page_dims.page_number,
        }
    }

    // ---- private helpers ----

    /// Divide page into `zone_count` equal Y-bands and collect lines per zone.
    fn build_zones<'a>(
        &self,
        lines: &[&'a TextLine],
        page_dims: &PageDimensions,
    ) -> Vec<(f64, f64, Vec<&'a TextLine>)> {
        let zone_height = page_dims.height / self.zone_count as f64;
        let mut zones: Vec<(f64, f64, Vec<&TextLine>)> = (0..self.zone_count)
            .map(|i| {
                let y_top = page_dims.height - i as f64 * zone_height;
                let y_bottom = y_top - zone_height;
                (y_top, y_bottom, Vec::new())
            })
            .collect();

        for &line in lines {
            let line_mid_y = line.bbox.center_y();
            for zone in zones.iter_mut() {
                if line_mid_y <= zone.0 && line_mid_y >= zone.1 {
                    zone.2.push(line);
                    break;
                }
            }
        }

        zones
    }

    fn detect_zone_columns(
        &self,
        zone_lines: &[&TextLine],
        page_dims: &PageDimensions,
        y_top: f64,
        y_bottom: f64,
    ) -> ZoneLayout {
        if zone_lines.is_empty() {
            return ZoneLayout {
                y_top,
                y_bottom,
                columns: vec![Column {
                    left: 0.0,
                    right: page_dims.width,
                    index: 0,
                }],
            };
        }

        // Build X-histogram of line left edges.
        let mut histogram: HashMap<i64, usize> = HashMap::new();
        for line in zone_lines {
            let bin = (line.bbox.left / self.bin_width).floor() as i64;
            *histogram.entry(bin).or_default() += 1;
        }

        let total = zone_lines.len();
        let threshold = (total as f64 * self.min_peak_ratio).ceil() as usize;

        // Find peaks (bins that meet threshold).
        let mut peaks: Vec<f64> = histogram
            .iter()
            .filter(|&(_, count)| *count >= threshold)
            .map(|(&bin, _)| bin as f64 * self.bin_width + self.bin_width / 2.0)
            .collect();
        peaks.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // Cluster nearby peaks.
        let cluster_gap = page_dims.width * self.cluster_gap_ratio;
        let clusters = self.cluster_peaks(&peaks, cluster_gap);

        // Build columns from clusters.
        let columns = self.clusters_to_columns(&clusters, page_dims.width);

        ZoneLayout {
            y_top,
            y_bottom,
            columns,
        }
    }

    fn cluster_peaks(&self, peaks: &[f64], max_gap: f64) -> Vec<f64> {
        if peaks.is_empty() {
            return Vec::new();
        }

        let mut clusters: Vec<f64> = Vec::new();
        let mut current_cluster_sum = peaks[0];
        let mut current_cluster_count = 1usize;
        let mut prev = peaks[0];

        for &peak in &peaks[1..] {
            if peak - prev <= max_gap {
                current_cluster_sum += peak;
                current_cluster_count += 1;
            } else {
                clusters.push(current_cluster_sum / current_cluster_count as f64);
                current_cluster_sum = peak;
                current_cluster_count = 1;
            }
            prev = peak;
        }
        clusters.push(current_cluster_sum / current_cluster_count as f64);
        clusters
    }

    fn clusters_to_columns(&self, clusters: &[f64], page_width: f64) -> Vec<Column> {
        match clusters.len() {
            0 | 1 => vec![Column {
                left: 0.0,
                right: page_width,
                index: 0,
            }],
            2 => {
                // Two column layout: gap between columns estimated as midpoint.
                let gap = (clusters[0] + clusters[1]) / 2.0;
                vec![
                    Column {
                        left: 0.0,
                        right: gap,
                        index: 0,
                    },
                    Column {
                        left: gap,
                        right: page_width,
                        index: 1,
                    },
                ]
            }
            _ => {
                // 3+ clusters → treat as single column (rare, may be noise).
                vec![Column {
                    left: 0.0,
                    right: page_width,
                    index: 0,
                }]
            }
        }
    }

    fn merge_zones(&self, zones: Vec<ZoneLayout>) -> Vec<LayoutRegion> {
        if zones.is_empty() {
            return Vec::new();
        }

        let mut regions: Vec<LayoutRegion> = Vec::new();
        let mut zone_iter = zones.into_iter();
        let first = zone_iter.next().unwrap();
        let mut cur_region = LayoutRegion {
            y_top: first.y_top,
            y_bottom: first.y_bottom,
            columns: first.columns,
        };

        for zone in zone_iter {
            if zone.columns.len() == cur_region.columns.len() {
                // Same column count → extend current region downward.
                cur_region.y_bottom = zone.y_bottom;
            } else {
                regions.push(cur_region);
                cur_region = LayoutRegion {
                    y_top: zone.y_top,
                    y_bottom: zone.y_bottom,
                    columns: zone.columns,
                };
            }
        }
        regions.push(cur_region);
        regions
    }
}

/// Intermediate: column layout of a single Y-zone.
struct ZoneLayout {
    y_top: f64,
    y_bottom: f64,
    columns: Vec<Column>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Rect;

    fn make_line(left: f64, top: f64, page: u32) -> TextLine {
        TextLine {
            spans: vec![],
            text: "text".to_string(),
            bbox: Rect::new(left, top, left + 200.0, top - 10.0),
            page,
            baseline_y: top - 10.0,
            primary_font_size: 10.0,
            primary_font_name: "Regular".to_string(),
            is_bold: false,
        }
    }

    fn make_dims(page: u32) -> PageDimensions {
        PageDimensions {
            page_number: page,
            width: 612.0,
            height: 792.0,
        }
    }

    #[test]
    fn single_column_detected() {
        let lines: Vec<TextLine> = (0..10)
            .map(|i| make_line(72.0, 700.0 - i as f64 * 20.0, 0))
            .collect();
        let layout = ColumnDetector::new().detect(&lines, &make_dims(0));
        // Should detect 1 region with 1 column
        assert!(layout.regions.iter().all(|r| r.column_count() == 1));
    }

    #[test]
    fn two_column_detected() {
        // Left column lines at x≈72, right column lines at x≈320
        let mut lines: Vec<TextLine> = (0..10)
            .map(|i| make_line(72.0, 700.0 - i as f64 * 20.0, 0))
            .collect();
        lines.extend((0..10).map(|i| make_line(320.0, 700.0 - i as f64 * 20.0, 0)));
        let layout = ColumnDetector::new().detect(&lines, &make_dims(0));
        // At least one region should have 2 columns
        assert!(layout.regions.iter().any(|r| r.column_count() == 2));
    }

    #[test]
    fn empty_page_returns_single_column() {
        let layout = ColumnDetector::new().detect(&[], &make_dims(0));
        assert_eq!(layout.regions.len(), 1);
        assert_eq!(layout.regions[0].column_count(), 1);
    }

    #[test]
    fn mixed_layout_has_multiple_regions() {
        // Top zone: single column (wide lines at x=72)
        let mut lines: Vec<TextLine> = (0..5)
            .map(|i| make_line(72.0, 750.0 - i as f64 * 20.0, 0))
            .collect();
        // Bottom zone: two columns
        lines.extend((0..5).map(|i| make_line(72.0, 400.0 - i as f64 * 20.0, 0)));
        lines.extend((0..5).map(|i| make_line(320.0, 400.0 - i as f64 * 20.0, 0)));

        let layout = ColumnDetector::new().detect(&lines, &make_dims(0));
        // Should have at least 2 regions with different column counts
        let col_counts: Vec<usize> = layout.regions.iter().map(|r| r.column_count()).collect();
        // At minimum: detect the two-column section
        assert!(col_counts.contains(&2));
    }
}
