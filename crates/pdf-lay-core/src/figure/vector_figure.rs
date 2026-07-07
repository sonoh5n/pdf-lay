//! Clusters vector-graphic paths into candidate "vector figure" regions and
//! links them to figure captions that raster image matching left unmatched.
//!
//! A figure drawn with PDF path operators (line art, a hand-drawn diagram,
//! a plot rendered as vector strokes) has no Image XObject at all, so
//! [`crate::extract::ImageExtractor`] never produces an [`ImageInfo`] for it
//! and [`crate::figure::ImageMatcher`] cannot match its caption. Without this
//! module such a caption would be silently reported as
//! `PdfLayWarning::UnmatchedCaption` even though the figure is visibly
//! present. This module instead spatially clusters the page's `PathObject`s
//! (already extracted for table-rule detection — see `extract_all_paths`)
//! and, when a sufficiently large cluster sits near an unmatched caption,
//! records it as a [`FigureInfo`] with a region bounding box and no raster
//! file (`ImageInfo::path == None`).
//!
//! Rasterizing/rendering the vector graphic itself is out of scope — see
//! `docs/refactor/phase4_extraction.md` P4-3 ("非スコープ").

use std::collections::HashSet;

use crate::figure::caption_detector::CaptionInfo;
use crate::types::{
    BlockType, FigureInfo, ImageFormat, ImageInfo, InsertionPoint, PathObject, Rect, TextBlock,
};

/// Clusters `PathObject`s spatially and links sufficiently large clusters to
/// nearby captions that were not matched to a raster image.
pub struct VectorFigureClusterer {
    /// Maximum gap (points) between two path bounding boxes for them to be
    /// merged into the same cluster.
    cluster_gap_pt: f64,
    /// Minimum number of paths a cluster must contain to be considered a
    /// vector-figure candidate.
    min_paths: usize,
    /// Maximum vertical distance (points) between a caption and a cluster,
    /// mirroring `ImageMatcher::max_gap_pt`.
    caption_max_gap_pt: f64,
}

impl VectorFigureClusterer {
    /// Create a new clusterer from `Config::figure_vector`'s
    /// `cluster_gap_pt`/`min_paths` and `Config::caption_max_gap_pt`.
    pub fn new(cluster_gap_pt: f64, min_paths: usize, caption_max_gap_pt: f64) -> Self {
        Self {
            cluster_gap_pt,
            min_paths,
            caption_max_gap_pt,
        }
    }

    /// Build `FigureInfo` records (no raster image) for captions that can be
    /// matched to a nearby dense cluster of vector paths.
    ///
    /// `captions` should already be filtered to the figure-type captions
    /// (`Figure`/`Scheme`/`Chart`) that raster image matching left unmatched
    /// — this method does not re-check caption type or "already matched"
    /// status. Each cluster is used by at most one caption.
    pub fn match_captions(
        &self,
        captions: &[&CaptionInfo],
        paths: &[PathObject],
        blocks: &[TextBlock],
    ) -> Vec<FigureInfo> {
        let mut results = Vec::new();

        let mut pages: Vec<u32> = paths.iter().map(|p| p.page).collect();
        pages.sort_unstable();
        pages.dedup();

        for page in pages {
            let page_paths: Vec<&PathObject> = paths.iter().filter(|p| p.page == page).collect();
            let clusters: Vec<(Rect, usize)> = cluster_paths(&page_paths, self.cluster_gap_pt)
                .into_iter()
                .filter(|(_, count)| *count >= self.min_paths)
                .collect();
            if clusters.is_empty() {
                continue;
            }

            let mut used: HashSet<usize> = HashSet::new();
            for caption in captions.iter().filter(|c| c.page == page) {
                let nearest = clusters
                    .iter()
                    .enumerate()
                    .filter(|(idx, _)| !used.contains(idx))
                    .filter_map(|(idx, (bbox, _))| {
                        let dist = vertical_distance(&caption.bbox, bbox);
                        if dist > self.caption_max_gap_pt {
                            None
                        } else {
                            Some((idx, bbox, dist))
                        }
                    })
                    .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

                let Some((idx, bbox, _)) = nearest else {
                    continue;
                };
                used.insert(idx);

                let image = ImageInfo {
                    path: None,
                    page,
                    raw_bbox: bbox.clone(),
                    normalized_bbox: bbox.clone(),
                    width_px: 0,
                    height_px: 0,
                    format: ImageFormat::Other("vector".to_string()),
                    bbox_known: true,
                };
                let context = extract_context(caption, blocks, 500);
                let after_block_index = if caption.block_index > 0 {
                    blocks.get(caption.block_index - 1).map(|b| b.global_index)
                } else {
                    None
                };

                results.push(FigureInfo {
                    figure_id: format!("{} {}", caption.prefix, caption.number.unwrap_or(0)),
                    figure_number: caption.number,
                    caption_text: caption.full_text.clone(),
                    image,
                    context_text: context,
                    insertion_point: InsertionPoint {
                        page,
                        after_block_index,
                        y_position: bbox.bottom,
                    },
                });
            }
        }

        results
    }
}

/// Vertical distance between a caption and a cluster bbox, checking both
/// "caption below cluster" and "caption above cluster" (same bidirectional
/// approach as `ImageMatcher::vertical_distance`).
fn vertical_distance(caption_bbox: &Rect, cluster_bbox: &Rect) -> f64 {
    let below = (cluster_bbox.bottom - caption_bbox.top).abs();
    let above = (caption_bbox.bottom - cluster_bbox.top).abs();
    below.min(above)
}

/// Extract surrounding body text (~`max_chars` characters) for context.
///
/// Mirrors `ImageMatcher::extract_context`'s block-window approach (kept as
/// an independent copy rather than a shared helper to avoid touching the
/// already-tested `ImageMatcher` for this unrelated feature).
fn extract_context(caption: &CaptionInfo, blocks: &[TextBlock], max_chars: usize) -> String {
    let mut context = String::new();
    let cap_idx = caption.block_index;

    let start = cap_idx.saturating_sub(3);
    let end = (cap_idx + 4).min(blocks.len());

    for (pos, block) in blocks[start..end].iter().enumerate() {
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

/// Expand a rect by `gap` in every direction (used to turn a "within `gap`
/// points" proximity test into a plain rect-overlap check).
fn inflate(r: &Rect, gap: f64) -> Rect {
    Rect::new(r.left - gap, r.top + gap, r.right + gap, r.bottom - gap)
}

/// Cluster path bounding boxes using union-find: two paths are joined when
/// their bboxes are within `gap_pt` of each other (checked by inflating one
/// and testing for overlap). Returns each resulting cluster's union bbox and
/// path count.
fn cluster_paths(paths: &[&PathObject], gap_pt: f64) -> Vec<(Rect, usize)> {
    let n = paths.len();
    if n == 0 {
        return Vec::new();
    }

    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut [usize], x: usize) -> usize {
        if parent[x] != x {
            parent[x] = find(parent, parent[x]);
        }
        parent[x]
    }

    for i in 0..n {
        for j in (i + 1)..n {
            if inflate(&paths[i].bbox, gap_pt).overlaps(&paths[j].bbox) {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }

    let mut clusters: std::collections::HashMap<usize, (Rect, usize)> =
        std::collections::HashMap::new();
    for (i, path) in paths.iter().enumerate() {
        let root = find(&mut parent, i);
        clusters
            .entry(root)
            .and_modify(|(bbox, count)| {
                *bbox = bbox.union(&path.bbox);
                *count += 1;
            })
            .or_insert_with(|| (path.bbox.clone(), 1));
    }

    clusters.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::figure::caption_detector::CaptionType;
    use crate::types::PathType;

    fn path_at(page: u32, left: f64, top: f64, right: f64, bottom: f64) -> PathObject {
        PathObject {
            bbox: Rect::new(left, top, right, bottom),
            page,
            path_type: PathType::Other,
            line_width: 1.0,
        }
    }

    fn caption_at(page: u32, top: f64) -> CaptionInfo {
        CaptionInfo {
            block_index: 0,
            caption_type: CaptionType::Figure,
            prefix: "Fig.".to_string(),
            number: Some(2),
            description: "A diagram.".to_string(),
            full_text: "Fig. 2: A diagram.".to_string(),
            page,
            bbox: Rect::new(72.0, top, 400.0, top - 10.0),
        }
    }

    #[test]
    fn nearby_paths_cluster_into_one_region() {
        let paths = [
            path_at(0, 100.0, 300.0, 150.0, 250.0),
            path_at(0, 152.0, 300.0, 200.0, 250.0), // 2pt gap from previous
            path_at(0, 100.0, 248.0, 150.0, 200.0), // 2pt gap vertically
        ];
        let refs: Vec<&PathObject> = paths.iter().collect();
        let clusters = cluster_paths(&refs, 5.0);
        assert_eq!(
            clusters.len(),
            1,
            "all three paths should merge into one cluster"
        );
        assert_eq!(clusters[0].1, 3);
    }

    #[test]
    fn distant_paths_do_not_cluster() {
        let paths = [
            path_at(0, 0.0, 100.0, 10.0, 90.0),
            path_at(0, 500.0, 100.0, 510.0, 90.0), // far away
        ];
        let refs: Vec<&PathObject> = paths.iter().collect();
        let clusters = cluster_paths(&refs, 5.0);
        assert_eq!(
            clusters.len(),
            2,
            "distant paths must stay in separate clusters"
        );
    }

    #[test]
    fn dense_cluster_near_unmatched_caption_becomes_a_vector_figure() {
        // 5 paths tightly packed (a little "diagram"), then a caption just
        // below it — should become a FigureInfo with no raster image.
        let mut paths = Vec::new();
        for i in 0..5 {
            let x = 100.0 + i as f64 * 17.0;
            paths.push(path_at(0, x, 300.0, x + 15.0, 250.0));
        }
        let caption = caption_at(0, 240.0); // just below the cluster (bottom=250)
        let clusterer = VectorFigureClusterer::new(5.0, 4, 50.0);

        let figures = clusterer.match_captions(&[&caption], &paths, &[]);
        assert_eq!(figures.len(), 1);
        assert_eq!(figures[0].figure_id, "Fig. 2");
        assert!(
            figures[0].image.path.is_none(),
            "a vector figure must not carry a raster image path"
        );
        assert!(figures[0].image.bbox_known);
    }

    #[test]
    fn sparse_cluster_below_min_paths_is_not_a_figure() {
        // Only 2 paths — below the default min_paths threshold of 4.
        let paths = vec![
            path_at(0, 100.0, 300.0, 115.0, 250.0),
            path_at(0, 117.0, 300.0, 132.0, 250.0),
        ];
        let caption = caption_at(0, 240.0);
        let clusterer = VectorFigureClusterer::new(5.0, 4, 50.0);

        let figures = clusterer.match_captions(&[&caption], &paths, &[]);
        assert!(figures.is_empty());
    }

    #[test]
    fn caption_too_far_from_cluster_is_not_matched() {
        let mut paths = Vec::new();
        for i in 0..5 {
            let x = 100.0 + i as f64 * 17.0;
            paths.push(path_at(0, x, 300.0, x + 15.0, 250.0));
        }
        // Caption 200pt below the cluster — beyond the 50pt max gap.
        let caption = caption_at(0, 40.0);
        let clusterer = VectorFigureClusterer::new(5.0, 4, 50.0);

        let figures = clusterer.match_captions(&[&caption], &paths, &[]);
        assert!(figures.is_empty());
    }

    #[test]
    fn each_cluster_matched_at_most_once() {
        let mut paths = Vec::new();
        for i in 0..5 {
            let x = 100.0 + i as f64 * 17.0;
            paths.push(path_at(0, x, 300.0, x + 15.0, 250.0));
        }
        let mut cap_a = caption_at(0, 240.0);
        cap_a.number = Some(2);
        let mut cap_b = caption_at(0, 235.0);
        cap_b.number = Some(3);
        cap_b.full_text = "Fig. 3: Another diagram.".to_string();
        let clusterer = VectorFigureClusterer::new(5.0, 4, 50.0);

        let figures = clusterer.match_captions(&[&cap_a, &cap_b], &paths, &[]);
        assert!(figures.len() <= 1, "one cluster can only back one figure");
    }
}
