//! Configuration types for the analysis pipeline.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Top-level configuration for `analyze_pdf`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Directory where extracted images are saved.
    pub image_output_dir: PathBuf,
    /// File format to use when writing extracted images.
    pub image_format: ImageOutputFormat,
    /// Whether to extract images at all (disable for text-only use cases).
    pub extract_images: bool,
    /// Whether to attempt table detection (Phase 2; stub in Phase 1).
    pub detect_tables: bool,
    /// Configuration for table detection and rendering.
    pub table_config: TableConfig,
    /// Configuration for math detection and conversion.
    pub math_config: MathConfig,
    /// Maximum vertical distance (points) between a caption and its image.
    pub caption_max_gap_pt: f64,
    /// Bin width (points) for the X-histogram in column detection.
    pub column_detection_bin_width: f64,
    /// Line-gap multiplier for block boundary detection.
    pub block_gap_multiplier: f64,
    /// Configuration for section header detection scoring.
    pub header_detection: HeaderDetectionConfig,
    /// Resource limits to guard against excessively large inputs.
    pub resource_limits: ResourceLimits,
    /// Maximum character count for a block to be classified as a figure/table
    /// caption. Longer blocks that merely start with "Table"/"Figure" are kept
    /// as body text rather than dropped.
    #[serde(default = "default_caption_max_chars")]
    pub caption_max_chars: usize,
    /// Maximum character count for a small-font single line to be classified as
    /// a running header. Longer lines are kept as body text even if the font is
    /// smaller than body size.
    #[serde(default = "default_running_header_max_chars")]
    pub running_header_max_chars: usize,
    /// Minimum acceptable text-coverage ratio (emitted / extracted characters).
    /// A `LowCoverage` warning is emitted when the measured ratio falls below
    /// this value.
    #[serde(default = "default_min_coverage_ratio")]
    pub min_coverage_ratio: f64,
}

/// Default value for [`Config::caption_max_chars`].
fn default_caption_max_chars() -> usize {
    240
}

/// Default value for [`Config::running_header_max_chars`].
fn default_running_header_max_chars() -> usize {
    60
}

/// Default value for [`Config::min_coverage_ratio`].
fn default_min_coverage_ratio() -> f64 {
    0.9
}

impl Default for Config {
    fn default() -> Self {
        Self {
            image_output_dir: PathBuf::from("images"),
            image_format: ImageOutputFormat::Png,
            extract_images: true,
            detect_tables: true,
            table_config: TableConfig::default(),
            math_config: MathConfig::default(),
            caption_max_gap_pt: 50.0,
            column_detection_bin_width: 10.0,
            block_gap_multiplier: 1.8,
            header_detection: HeaderDetectionConfig::default(),
            resource_limits: ResourceLimits::default(),
            caption_max_chars: default_caption_max_chars(),
            running_header_max_chars: default_running_header_max_chars(),
            min_coverage_ratio: default_min_coverage_ratio(),
        }
    }
}

/// Image output format when saving extracted images.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum ImageOutputFormat {
    /// PNG format (lossless, default).
    #[default]
    Png,
    /// JPEG format (lossy, smaller file size).
    Jpeg,
}

/// Configuration for Markdown output generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkdownConfig {
    /// Base path prepended to image paths in `![alt](path)`.
    pub image_base_path: String,
    /// Whether to include page number annotations in the output.
    pub include_page_numbers: bool,
    /// Added to the section level when generating `#` headers.
    /// Default 1 means level-1 sections become `##`.
    pub heading_offset: u8,
    /// Whether to include a YAML metadata header at the top of the document.
    pub include_metadata_header: bool,
    /// Whether to render tables as images rather than Markdown text.
    pub table_as_image: bool,
    /// How to style figure captions in the Markdown output.
    pub figure_caption_style: CaptionStyle,
    /// Optional math configuration for converting math spans at render time.
    /// When `None`, math spans are output as plain `block.text` without conversion.
    pub math_config: Option<MathConfig>,
    /// On-disk directory where extracted images live (from `--image-dir`).
    ///
    /// When both this and [`Self::output_dir`] are set, image links are written
    /// as a path relative to the output file's directory instead of prefixing
    /// [`Self::image_base_path`]. `None` keeps the legacy prefix behavior.
    #[serde(default)]
    pub image_dir: Option<PathBuf>,
    /// Directory of the Markdown output file (from `-o`), or `None` for stdout.
    ///
    /// Used together with [`Self::image_dir`] to compute relative image links.
    #[serde(default)]
    pub output_dir: Option<PathBuf>,
}

impl Default for MarkdownConfig {
    fn default() -> Self {
        Self {
            image_base_path: "./images".to_string(),
            include_page_numbers: false,
            heading_offset: 1,
            include_metadata_header: false,
            table_as_image: false,
            figure_caption_style: CaptionStyle::Italic,
            math_config: None,
            image_dir: None,
            output_dir: None,
        }
    }
}

/// How figure captions are rendered in Markdown output.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum CaptionStyle {
    /// Italic caption: `*Fig. 1: ...*`
    #[default]
    Italic,
    /// Bold label: `**Fig. 1:** ...`
    Bold,
    /// Plain text: `Fig. 1: ...`
    PlainText,
}

/// Configuration for table detection and rendering (Phase 2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableConfig {
    /// Minimum number of columns for a region to be classified as a table.
    pub min_columns: usize,
    /// Horizontal tolerance (points) for column alignment detection.
    pub column_alignment_tolerance: f64,
    /// Whether to use PDF rule (line) detection for table boundaries.
    pub use_rule_detection: bool,
    /// Whether to use text X-alignment for table detection when no rules are found.
    pub use_text_alignment: bool,
}

impl Default for TableConfig {
    fn default() -> Self {
        Self {
            min_columns: 2,
            column_alignment_tolerance: 5.0,
            use_rule_detection: true,
            use_text_alignment: true,
        }
    }
}

/// Configuration for math detection and conversion (Phase 2/3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MathConfig {
    /// Preferred output format for mathematical expressions.
    pub representation: MathRepresentationPreference,
    /// Delimiter pair for inline math (default: `$` … `$`).
    pub inline_delimiter: (String, String),
    /// Delimiter pair for display math (default: `$$\n` … `\n$$`).
    pub display_delimiter: (String, String),
    /// Y-offset threshold for superscript/subscript detection (as ratio of font_size).
    pub superscript_y_threshold: f64,
    /// Additional font name patterns to treat as math fonts.
    pub additional_math_fonts: Vec<String>,
}

impl Default for MathConfig {
    fn default() -> Self {
        Self {
            representation: MathRepresentationPreference::Auto,
            inline_delimiter: ("$".to_string(), "$".to_string()),
            display_delimiter: ("$$\n".to_string(), "\n$$".to_string()),
            superscript_y_threshold: 0.3,
            additional_math_fonts: Vec::new(),
        }
    }
}

/// Preferred output format for mathematical expressions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum MathRepresentationPreference {
    /// LaTeX math notation (e.g. `$E = mc^{2}$`).
    LaTeX,
    /// Unicode math characters (e.g. `E = mc²`).
    UnicodeMath,
    /// Plain ASCII approximation (e.g. `E = mc^2`).
    PlainText,
    /// Automatically choose based on detected font types.
    #[default]
    Auto,
}

/// Configuration for section header detection scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderDetectionConfig {
    /// Minimum score for a block to be classified as a header.
    pub min_score: u32,
    /// Maximum character count for a header candidate.
    pub max_chars: usize,
    /// Maximum line count for a header candidate.
    pub max_lines: usize,
    /// When true, blocks the classifier marked as non-body (caption, running
    /// head/foot, footnote, reference, page number) are excluded from header
    /// candidates. Set false to restore the legacy classification-agnostic
    /// behavior.
    #[serde(default = "default_respect_classification")]
    pub respect_classification: bool,
}

/// Default value for [`HeaderDetectionConfig::respect_classification`].
fn default_respect_classification() -> bool {
    true
}

impl Default for HeaderDetectionConfig {
    fn default() -> Self {
        Self {
            min_score: 4,
            max_chars: 120,
            max_lines: 3,
            respect_classification: default_respect_classification(),
        }
    }
}

/// Resource limits to guard against excessively large PDF inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Maximum input file size in bytes (default: 200 MB).
    pub max_file_size: u64,
    /// Maximum number of pages to process (default: 2000).
    pub max_pages: u32,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_file_size: 200 * 1024 * 1024, // 200 MB
            max_pages: 2000,
        }
    }
}

/// Configuration for LLM text generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmTextConfig {
    /// Whether to include figure placeholders in LLM text output.
    pub include_figures: bool,
    /// Whether to include inline table text in LLM text output.
    pub include_tables: bool,
    /// Whether to include section header lines in LLM text output.
    pub include_section_headers: bool,
    /// Math representation format for LLM text output.
    pub math_representation: MathRepresentationPreference,
    /// How figures are represented in the LLM text output.
    pub figure_format: FigureTextFormat,
}

impl Default for LlmTextConfig {
    fn default() -> Self {
        Self {
            include_figures: true,
            include_tables: true,
            include_section_headers: true,
            math_representation: MathRepresentationPreference::Auto,
            figure_format: FigureTextFormat::Placeholder,
        }
    }
}

/// How figures are represented in LLM text output.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum FigureTextFormat {
    /// `[IMAGE: Fig. 1 path/to/img.png]`
    #[default]
    Placeholder,
    /// `![Fig. 1](path/to/img.png)`
    MarkdownLink,
    /// Caption text only, no path.
    CaptionOnly,
    /// Omit figures entirely.
    Omit,
}

/// Configuration for chunk splitting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkConfig {
    /// Maximum tokens per chunk.
    pub max_tokens: usize,
    /// Number of tokens of overlap between adjacent chunks.
    pub overlap_tokens: usize,
    /// Strategy for determining chunk boundaries.
    pub split_strategy: SplitStrategy,
    /// Whether to prepend the section path as context at the start of each chunk.
    pub include_section_context: bool,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            max_tokens: 4000,
            overlap_tokens: 200,
            split_strategy: SplitStrategy::SectionBoundary,
            include_section_context: true,
        }
    }
}

/// Strategy for splitting sections into chunks.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum SplitStrategy {
    /// Split at section boundaries first (recommended).
    #[default]
    SectionBoundary,
    /// Split purely by token count.
    TokenCount,
    /// Split at paragraph boundaries.
    Paragraph,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default_values() {
        let cfg = Config::default();
        assert_eq!(cfg.caption_max_gap_pt, 50.0);
        assert!((cfg.block_gap_multiplier - 1.8).abs() < f64::EPSILON);
        assert_eq!(cfg.column_detection_bin_width, 10.0);
        assert!(cfg.extract_images);
        assert!(cfg.detect_tables);
    }

    #[test]
    fn table_config_defaults() {
        let tc = TableConfig::default();
        assert_eq!(tc.min_columns, 2);
        assert_eq!(tc.column_alignment_tolerance, 5.0);
        assert!(tc.use_rule_detection);
        assert!(tc.use_text_alignment);
    }

    #[test]
    fn header_detection_defaults() {
        let hd = HeaderDetectionConfig::default();
        assert_eq!(hd.min_score, 4);
        assert_eq!(hd.max_chars, 120);
        assert_eq!(hd.max_lines, 3);
    }

    #[test]
    fn math_config_defaults() {
        let mc = MathConfig::default();
        assert_eq!(mc.superscript_y_threshold, 0.3);
        assert_eq!(mc.inline_delimiter, ("$".to_string(), "$".to_string()));
        assert_eq!(
            mc.display_delimiter,
            ("$$\n".to_string(), "\n$$".to_string())
        );
    }

    #[test]
    fn chunk_config_defaults() {
        let cc = ChunkConfig::default();
        assert_eq!(cc.max_tokens, 4000);
        assert_eq!(cc.overlap_tokens, 200);
        assert!(cc.include_section_context);
    }
}
