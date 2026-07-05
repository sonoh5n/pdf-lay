//! Structure analysis layer: block grouping, classification, header detection, section building.

mod block_classifier;
mod block_grouper;
mod header_detector;
mod metadata;
mod numbering;
mod reading_order;
mod section_builder;

pub use block_classifier::BlockClassifier;
pub use block_grouper::BlockGrouper;
pub use header_detector::HeaderDetector;
pub use metadata::MetadataExtractor;
pub use numbering::{NumberingParser, roman_to_u32};
pub use reading_order::ReadingOrderSorter;
pub use section_builder::{SectionBuilder, validate_numbering};
