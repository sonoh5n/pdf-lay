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

    /// A resource limit was exceeded (file size, page count, etc.).
    #[error("Resource limit exceeded: {limit} (actual: {actual})")]
    ResourceLimitExceeded {
        /// Description of the limit that was exceeded.
        limit: String,
        /// The actual value that exceeded the limit.
        actual: String,
    },
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
    /// The page MediaBox could not be read, so page dimensions fell back to a
    /// derived or default value.
    PageDimensionsFallback {
        /// Zero-based page index.
        page: u32,
        /// Which fallback was used: `"span-bbox"` (derived from span extents) or
        /// `"letter-default"` (612 × 792).
        method: &'static str,
    },
    /// The fraction of extracted text that reached the output fell below the
    /// configured threshold, suggesting content was lost during analysis.
    LowCoverage {
        /// Ratio of emitted characters to extracted characters, in `[0, 1]`.
        ratio: f64,
    },
    /// Blocks were reclassified as repeated running headers/footers before
    /// header detection.
    RepeatedRunningReclassified {
        /// Number of blocks reclassified.
        count: usize,
    },
}

impl std::fmt::Display for PdfLayWarning {
    /// Display format that omits PDF-derived text content to prevent information leakage.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnmatchedCaption { page, .. } => {
                write!(f, "unmatched caption on page {page}")
            }
            Self::UnmatchedImage { page, .. } => {
                write!(f, "unmatched image on page {page}")
            }
            Self::CoordinateFallback { page, scale_used } => {
                write!(
                    f,
                    "coordinate normalization fallback on page {page} (scale={scale_used:.3})"
                )
            }
            Self::PageSkipped { page, reason } => {
                write!(f, "page {page} skipped: {reason}")
            }
            Self::PageDimensionsFallback { page, method } => {
                write!(f, "page {page} dimensions fallback ({method})")
            }
            Self::LowCoverage { ratio } => {
                write!(
                    f,
                    "low text coverage: {:.1}% of extracted text reached the output",
                    ratio * 100.0
                )
            }
            Self::RepeatedRunningReclassified { count } => {
                write!(
                    f,
                    "reclassified {count} repeated running header/footer block(s)"
                )
            }
        }
    }
}

/// Text-coverage metrics for one analysis run.
///
/// A ratio well below 1.0 indicates that a meaningful fraction of the extracted
/// text did not reach the output (e.g. due to misclassification or layout
/// issues). Used as a regression guard for the "No Silent Drop" principle.
#[derive(Debug, Clone, Copy)]
pub struct Coverage {
    /// Total characters across all extracted text spans.
    pub extracted_chars: usize,
    /// Characters that reached the output (section body text plus headers).
    pub emitted_chars: usize,
    /// Number of blocks classified into render-skipped types (caption, page
    /// number, running header/footer).
    pub dropped_blocks: usize,
    /// `emitted_chars / extracted_chars`, clamped to `[0, 1]` (1.0 when nothing
    /// was extracted).
    pub ratio: f64,
}

/// The result of a full PDF analysis, including any non-fatal warnings.
#[derive(Debug)]
pub struct AnalysisResult {
    /// The structured document produced by the analysis pipeline.
    pub document: crate::types::PaperDocument,
    /// Non-fatal warnings accumulated during analysis.
    pub warnings: Vec<PdfLayWarning>,
    /// Text-coverage metrics for this run.
    pub coverage: Coverage,
}
