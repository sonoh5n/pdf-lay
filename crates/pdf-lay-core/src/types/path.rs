//! Path objects extracted from PDF (used for table rule detection in Phase 2).

use serde::{Deserialize, Serialize};

use super::geometry::Rect;

/// A line segment or path from the PDF page content stream.
///
/// Used in Phase 2 (table detection) to detect horizontal/vertical rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathObject {
    /// Bounding box of this path in PDF coordinates.
    pub bbox: Rect,
    /// Zero-based page index.
    pub page: u32,
    /// Classification of this path for table detection.
    pub path_type: PathType,
    /// Line width in points.
    pub line_width: f64,
}

/// Classification of a PDF path for table detection purposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PathType {
    /// A horizontal line (height < 2pt).
    Horizontal,
    /// A vertical line (width < 2pt).
    Vertical,
    /// A rectangle (potential table cell border).
    Rectangle,
    /// Any other path shape.
    Other,
}
