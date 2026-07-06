//! Figure processing: caption detection and image-caption matching.

mod caption_detector;
mod image_matcher;
mod vector_figure;

pub use caption_detector::{CaptionDetector, CaptionInfo, CaptionType};
pub use image_matcher::ImageMatcher;
pub use vector_figure::VectorFigureClusterer;
