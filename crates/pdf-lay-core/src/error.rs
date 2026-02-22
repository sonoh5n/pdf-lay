//! Error and warning types for the pdf-lay pipeline.

use std::path::PathBuf;

use thiserror::Error;

/// All errors that can occur during PDF analysis.
#[derive(Debug, Error)]
pub enum PdfLayError {
    /// The specified PDF file does not exist on disk.
    #[error("PDF file not found: {0}")]
    FileNotFound(PathBuf),

    /// The PDF file could not be parsed (corrupt or unsupported format).
    #[error("Failed to parse PDF: {0}")]
    PdfParseError(String),

    /// A page index was requested that exceeds the document's page count.
    #[error("Page {0} out of range (total pages: {1})")]
    PageOutOfRange(u32, u32),

    /// An image could not be extracted from a specific page.
    #[error("Image extraction failed on page {page}: {reason}")]
    ImageExtractionError {
        /// The page where extraction failed (zero-based).
        page: u32,
        /// Human-readable description of the failure.
        reason: String,
    },

    /// Could not determine a scale factor for coordinate normalization.
    #[error("Coordinate normalization failed: scale factor could not be determined")]
    CoordinateNormalizationError,

    /// An I/O error occurred (e.g. while writing an extracted image to disk).
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// An error occurred during image processing.
    #[error("Image processing error: {0}")]
    ImageError(#[from] image::ImageError),

    /// JSON serialization or deserialization failed.
    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),
}

/// Non-fatal issues that allow analysis to continue.
///
/// Accumulated in `AnalysisResult::warnings` rather than returned as `Err`.
#[derive(Debug, Clone)]
pub enum PdfLayWarning {
    /// A caption was detected but no nearby image could be matched.
    UnmatchedCaption {
        /// The caption text that could not be matched.
        caption: String,
        /// Zero-based page index where the caption appears.
        page: u32,
    },
    /// An image was found but no caption could be matched to it.
    UnmatchedImage {
        /// Path string of the unmatched image.
        image_path: String,
        /// Zero-based page index where the image appears.
        page: u32,
    },
    /// Coordinate normalization fell back to a default scale factor.
    CoordinateFallback {
        /// Zero-based page index.
        page: u32,
        /// The scale factor that was used as the fallback.
        scale_used: f64,
    },
    /// An entire page was skipped due to an extraction error.
    PageSkipped {
        /// Zero-based page index.
        page: u32,
        /// Human-readable description of why the page was skipped.
        reason: String,
    },
}

/// The result of a full PDF analysis, including any non-fatal warnings.
#[derive(Debug)]
pub struct AnalysisResult {
    /// The structured document produced by the analysis pipeline.
    pub document: crate::types::PaperDocument,
    /// Non-fatal warnings accumulated during analysis.
    pub warnings: Vec<PdfLayWarning>,
}
