//! Layout types: column structure and page region descriptors.

use serde::{Deserialize, Serialize};

/// The complete column layout for a single page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageLayout {
    /// Horizontal bands of the page, each with a consistent column structure.
    pub regions: Vec<LayoutRegion>,
    /// Page width in points.
    pub page_width: f64,
    /// Page height in points.
    pub page_height: f64,
    /// Zero-based page number.
    pub page: u32,
}

/// A horizontal band of the page that has a consistent column structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutRegion {
    /// Top Y-coordinate of this region (larger value = higher on page).
    pub y_top: f64,
    /// Bottom Y-coordinate.
    pub y_bottom: f64,
    /// Columns detected within this region.
    pub columns: Vec<Column>,
}

impl LayoutRegion {
    /// Number of columns in this region.
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// True if a TextLine's top Y coordinate falls within this region's Y range.
    pub fn contains_y(&self, top: f64, _bottom: f64) -> bool {
        top <= self.y_top && top >= self.y_bottom
    }
}

/// A single column within a `LayoutRegion`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    /// Left X-boundary of this column.
    pub left: f64,
    /// Right X-boundary of this column.
    pub right: f64,
    /// Zero-based column index within its region (0 = leftmost).
    pub index: usize,
}
