//! Structured section numbering (e.g. `2.1.3`, `IV.`, `Appendix A`).
//!
//! A section header's leading number is parsed into a [`NumberingKey`] — an
//! ordered list of [`NumberComponent`]s — so the hierarchy can be built from
//! prefix relationships and numbering anomalies (skips, duplicates,
//! non-monotonic sequences) can be detected.

use serde::{Deserialize, Serialize};

/// One component of a section number, normalized to a 1-based ordinal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NumberComponent {
    /// Arabic numeral, e.g. `3` in `3.1`.
    Arabic(u32),
    /// Roman numeral normalized to its integer value, e.g. `IV` → `4`.
    Roman(u32),
    /// Alphabetic label normalized to a 1-based ordinal, e.g. `A` → `1`.
    Alpha(u32),
}

impl NumberComponent {
    /// The component's numeric ordinal (1-based for Roman/Alpha).
    pub fn ordinal(&self) -> u32 {
        match self {
            NumberComponent::Arabic(n) | NumberComponent::Roman(n) | NumberComponent::Alpha(n) => {
                *n
            }
        }
    }
}

/// A parsed section number as an ordered list of components.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NumberingKey {
    /// Ordered numbering components (outermost first).
    pub components: Vec<NumberComponent>,
    /// Whether this number denotes an appendix (e.g. `Appendix A`).
    pub is_appendix: bool,
}

impl NumberingKey {
    /// Depth of the numbering, i.e. the number of components (a valid heading
    /// level, clamped to at least 1).
    pub fn depth(&self) -> u8 {
        self.components.len().clamp(1, u8::MAX as usize) as u8
    }

    /// The parent prefix: all components except the last.
    pub fn parent_prefix(&self) -> &[NumberComponent] {
        let len = self.components.len();
        if len <= 1 {
            &[]
        } else {
            &self.components[..len - 1]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_counts_components() {
        let key = NumberingKey {
            components: vec![
                NumberComponent::Arabic(3),
                NumberComponent::Arabic(1),
                NumberComponent::Arabic(2),
            ],
            is_appendix: false,
        };
        assert_eq!(key.depth(), 3);
        assert_eq!(key.parent_prefix().len(), 2);
    }

    #[test]
    fn single_component_has_empty_parent() {
        let key = NumberingKey {
            components: vec![NumberComponent::Arabic(1)],
            is_appendix: false,
        };
        assert_eq!(key.depth(), 1);
        assert!(key.parent_prefix().is_empty());
    }
}
