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
    /// A section-numbering anomaly (skip, duplicate, or non-monotonic sequence)
    /// was detected. The section is still kept.
    SectionNumberingAnomaly {
        /// The kind of anomaly.
        kind: NumberingAnomalyKind,
        /// Zero-based page index where the anomalous header appears.
        page: u32,
    },
    /// A user-supplied caption pattern (`CaptionConfig::extra_figure_patterns`
    /// / `extra_table_patterns`) failed to compile as a regex and was ignored;
    /// the built-in caption patterns still apply.
    InvalidCaptionPattern {
        /// The invalid pattern string, as supplied in the configuration.
        pattern: String,
        /// The regex compiler's error message.
        reason: String,
    },
    /// An Image XObject had an `/SMask` (soft mask / alpha) entry that
    /// pdf_oxide does not apply when decoding the image. The extracted raster
    /// may be missing transparency it had in the original PDF (e.g. a
    /// checkerboard or colored background where the source was transparent).
    ImageSMaskIgnored {
        /// Zero-based page index.
        page: u32,
    },
    /// An image's bounding box could not be determined (pdf_oxide reported no
    /// bbox, or reported a degenerate one with zero width/height). The image
    /// is still extracted and saved, but is excluded from caption matching
    /// (a fabricated position would risk pairing it with the wrong caption).
    ImageBboxUnknown {
        /// Zero-based page index.
        page: u32,
    },
    /// An image on the page could not be decoded or saved. Only that image
    /// is skipped; the rest of the page's images are still extracted.
    ImageDecodeFailed {
        /// Zero-based page index.
        page: u32,
        /// Human-readable description of why the image was skipped.
        reason: String,
    },
    /// A page had little/no native text but at least one embedded image (the
    /// shape of a scanned page), and OCR recovered usable text for it (P4-2).
    PageTextRecovered {
        /// Zero-based page index.
        page: u32,
        /// Which mechanism recovered the text (e.g. `"ocr:tesseract"`).
        method: &'static str,
    },
    /// A page had little/no native text and at least one embedded image (the
    /// shape of a scanned page), and no text could be recovered for it — OCR
    /// was disabled, unavailable, or itself failed (P4-2). Emitted
    /// regardless of whether OCR is enabled, so a fully-scanned document can
    /// never analyze "successfully" with zero signal.
    PageTextMissing {
        /// Zero-based page index.
        page: u32,
        /// Human-readable description of why no text is available.
        reason: String,
    },
}

/// The kind of section-numbering anomaly detected during hierarchy validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberingAnomalyKind {
    /// A number decreased relative to its sibling (e.g. `3` then `2`).
    NonMonotonic,
    /// A number skipped one or more values (e.g. `IV` then `VI`).
    SkippedNumber,
    /// The same number appeared more than once.
    Duplicate,
}

impl std::fmt::Display for NumberingAnomalyKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::NonMonotonic => "non-monotonic",
            Self::SkippedNumber => "skipped number",
            Self::Duplicate => "duplicate",
        };
        f.write_str(s)
    }
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
            Self::SectionNumberingAnomaly { kind, page } => {
                write!(f, "section numbering anomaly ({kind}) on page {page}")
            }
            Self::InvalidCaptionPattern { pattern, reason } => {
                write!(f, "invalid caption pattern {pattern:?} ignored: {reason}")
            }
            Self::ImageSMaskIgnored { page } => {
                write!(f, "image on page {page} has an ignored SMask (soft mask)")
            }
            Self::ImageBboxUnknown { page } => {
                write!(f, "image on page {page} has an unknown bounding box")
            }
            Self::ImageDecodeFailed { page, reason } => {
                write!(f, "image on page {page} failed to decode/save: {reason}")
            }
            Self::PageTextRecovered { page, method } => {
                write!(f, "page {page} text recovered via {method}")
            }
            Self::PageTextMissing { page, reason } => {
                write!(f, "page {page} has no usable text: {reason}")
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
    /// `emitted_chars / extracted_chars`, clamped to `[0, 1]`.
    ///
    /// `0.0` when `extracted_chars == 0` (nothing was extracted at all —
    /// e.g. a fully scanned/image-only document). Earlier this short-
    /// circuited to `1.0` ("full coverage"), which let a document with zero
    /// extracted text pass through with no `LowCoverage` warning at all
    /// (see `docs/refactor/phase4_findings.md` P4-1 §2.5 / P4-2). `0.0`
    /// cannot masquerade as complete coverage, so it always triggers
    /// `PdfLayWarning::LowCoverage` against the default
    /// `Config::min_coverage_ratio`; per-page detail is additionally
    /// reported via `PdfLayWarning::PageTextMissing`/`PageTextRecovered`.
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
