//! Structure analysis layer: block grouping, classification, header detection, section building.

mod block_classifier;
mod block_grouper;
mod header_detector;
mod reading_order;
mod section_builder;

pub use block_classifier::BlockClassifier;
pub use block_grouper::BlockGrouper;
pub use header_detector::HeaderDetector;
pub use reading_order::ReadingOrderSorter;
pub use section_builder::SectionBuilder;
