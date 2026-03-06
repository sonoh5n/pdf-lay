//! Figure processing: caption detection and image-caption matching.

mod caption_detector;
mod image_matcher;

pub use caption_detector::{CaptionDetector, CaptionInfo, CaptionType};
pub use image_matcher::ImageMatcher;
