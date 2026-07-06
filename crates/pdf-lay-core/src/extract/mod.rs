//! PDF extraction layer — the only module that imports from `pdf_oxide`.
//!
//! Other modules must not import `pdf_oxide` directly.  All inter-module
//! communication happens through the types defined in `crate::types`.

mod coordinate;
mod image_extractor;
#[cfg(feature = "ocr")]
mod ocr;
mod pdf_reader;
mod span_builder;

pub use coordinate::CoordinateNormalizer;
pub use image_extractor::ImageExtractor;
#[cfg(feature = "ocr")]
pub(crate) use ocr::{engine_available, ocr_page};
pub use pdf_reader::PdfReader;
pub use span_builder::SpanBuilder;
