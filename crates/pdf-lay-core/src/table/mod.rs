//! Table detection and conversion.

mod detector;
mod grid_builder;
mod text_converter;

pub use detector::TableDetector;
pub(crate) use detector::TableRegion;
pub use grid_builder::{GridBuilder, TableGrid};
pub use text_converter::TableTextConverter;
