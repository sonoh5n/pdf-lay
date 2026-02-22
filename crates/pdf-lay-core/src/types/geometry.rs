//! Geometric primitives used throughout the pipeline.
//!
//! **Coordinate system**: PDF default — origin at lower-left, Y-axis pointing up, unit = points.
//! Invariant: `Rect::top > Rect::bottom` always holds.

use serde::{Deserialize, Serialize};

/// Bounding box in PDF coordinate space (lower-left origin, Y-up).
///
/// Invariant: `top > bottom`, `right > left`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    /// X-coordinate of the left edge.
    pub left: f64,
    /// Y-coordinate of the upper edge (larger Y value).
    pub top: f64,
    /// X-coordinate of the right edge.
    pub right: f64,
    /// Y-coordinate of the lower edge (smaller Y value).
    pub bottom: f64,
}

impl Rect {
    /// Create a new `Rect`. Panics in debug if invariant is violated.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if `top < bottom` or `right < left`.
    pub fn new(left: f64, top: f64, right: f64, bottom: f64) -> Self {
        debug_assert!(
            top >= bottom,
            "Rect: top ({top}) must be >= bottom ({bottom})"
        );
        debug_assert!(
            right >= left,
            "Rect: right ({right}) must be >= left ({left})"
        );
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Horizontal span in points.
    pub fn width(&self) -> f64 {
        self.right - self.left
    }

    /// Vertical span in points.
    pub fn height(&self) -> f64 {
        self.top - self.bottom
    }

    /// Horizontal center.
    pub fn center_x(&self) -> f64 {
        (self.left + self.right) / 2.0
    }

    /// Vertical center.
    pub fn center_y(&self) -> f64 {
        (self.top + self.bottom) / 2.0
    }

    /// Vertical gap between `self` and `other`.
    ///
    /// Positive when there is a gap (self is above other or vice versa).
    /// Zero or negative when the rects overlap vertically.
    pub fn vertical_gap(&self, other: &Rect) -> f64 {
        if self.bottom > other.top {
            // self is entirely above other
            self.bottom - other.top
        } else if other.bottom > self.top {
            // other is entirely above self
            other.bottom - self.top
        } else {
            // they overlap — gap is 0 (or negative to indicate overlap amount)
            let overlap = self.top.min(other.top) - self.bottom.max(other.bottom);
            -overlap
        }
    }

    /// Smallest bounding box containing both `self` and `other`.
    pub fn union(&self, other: &Rect) -> Rect {
        Rect {
            left: self.left.min(other.left),
            top: self.top.max(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.min(other.bottom),
        }
    }

    /// Returns true if this rect overlaps with `other` in both X and Y.
    pub fn overlaps(&self, other: &Rect) -> bool {
        self.left < other.right
            && self.right > other.left
            && self.bottom < other.top
            && self.top > other.bottom
    }
}

/// Page dimensions in points.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageDimensions {
    /// Zero-based page index.
    pub page_number: u32,
    /// Page width in points (e.g. 612 for US Letter).
    pub width: f64,
    /// Page height in points (e.g. 792 for US Letter).
    pub height: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_width_height() {
        let r = Rect::new(10.0, 30.0, 50.0, 10.0);
        assert_eq!(r.width(), 40.0);
        assert_eq!(r.height(), 20.0);
    }

    #[test]
    fn rect_center() {
        let r = Rect::new(0.0, 20.0, 20.0, 0.0);
        assert_eq!(r.center_x(), 10.0);
        assert_eq!(r.center_y(), 10.0);
    }

    #[test]
    fn rect_vertical_gap_above() {
        // r1 is above r2 with a 5pt gap
        let r1 = Rect::new(0.0, 30.0, 10.0, 20.0);
        let r2 = Rect::new(0.0, 15.0, 10.0, 0.0);
        assert_eq!(r1.vertical_gap(&r2), 5.0);
    }

    #[test]
    fn rect_vertical_gap_overlap() {
        let r1 = Rect::new(0.0, 20.0, 10.0, 10.0);
        let r2 = Rect::new(0.0, 15.0, 10.0, 5.0);
        assert!(r1.vertical_gap(&r2) < 0.0);
    }

    #[test]
    fn rect_union() {
        let r1 = Rect::new(0.0, 20.0, 10.0, 10.0);
        let r2 = Rect::new(5.0, 30.0, 20.0, 5.0);
        let u = r1.union(&r2);
        assert_eq!(u, Rect::new(0.0, 30.0, 20.0, 5.0));
    }
}
