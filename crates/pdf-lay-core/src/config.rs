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
    /// Configuration for figure/table caption detection (patterns, language
    /// toggles). See [`CaptionConfig`].
    #[serde(default)]
    pub caption: CaptionConfig,
    /// When `true`, always save extracted images as PNG, re-encoding
    /// JPEG-source images instead of passing them through losslessly as
    /// `.jpg`. Default `false` (P4-3: honor the image's real source format).
    /// Set `true` to restore the pre-P4-3 behavior of always producing PNG
    /// files.
    #[serde(default)]
    pub force_png: bool,
    /// Configuration for vector-figure detection (P4-3): clustering
    /// vector-graphic paths near an unmatched Figure/Scheme/Chart caption
    /// into a figure record when no raster image was extracted.
    #[serde(default)]
    pub figure_vector: VectorFigureConfig,
    /// Configuration for OCR recovery of scanned/image-only pages (P4-2).
    /// See [`OcrConfig`].
    #[serde(default)]
    pub ocr: OcrConfig,
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
            caption: CaptionConfig::default(),
            force_png: false,
            figure_vector: VectorFigureConfig::default(),
            ocr: OcrConfig::default(),
        }
    }
}

/// Configuration for OCR recovery of scanned/image-only pages (P4-2).
///
/// A page whose native (embedded) text falls below [`Self::min_native_chars`]
/// *and* which contains at least one embedded image is the shape of a
/// scanned page (see `docs/refactor/phase4_findings.md` P4-1 §2.5). Such a
/// page is **always** reported via a `PdfLayWarning`
/// (`PdfLayWarning::PageTextRecovered` on success,
/// `PdfLayWarning::PageTextMissing` otherwise) regardless of
/// [`Self::enabled`] — detection/reporting is not gated behind OCR being
/// turned on. [`Self::enabled`] only controls whether pdf-lay actually
/// *attempts* OCR for those pages.
///
/// OCR is opt-in and off by default, and enabling it does not by itself pull
/// in any heavy build dependency: the default [`OcrEngineKind::Tesseract`]
/// shells out to a `tesseract` binary that must already be installed and on
/// `PATH` (and requires this crate to be built with the `ocr` cargo feature —
/// see `crates/pdf-lay-core/Cargo.toml`). If the feature is not compiled in,
/// or the binary is not found, or OCR itself fails, analysis still completes
/// normally; the affected page is reported via `PageTextMissing` rather than
/// causing an error or a panic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrConfig {
    /// Whether to actually attempt OCR for pages under the native-text
    /// threshold. Default `false`. Detection/warnings fire either way (see
    /// the struct docs); this flag only gates the OCR attempt itself.
    #[serde(default)]
    pub enabled: bool,
    /// Minimum number of native (non-OCR) characters a page must contain to
    /// be considered to have "real" text rather than being likely-scanned.
    /// Default `50`, matching pdf_oxide's own internal `needs_ocr` heuristic
    /// (`ocr/mod.rs::needs_ocr`, gated behind pdf_oxide's own `ocr` feature —
    /// see `docs/refactor/phase4_findings.md` P4-1 §1 for how this crate
    /// confirmed that threshold from the pdf_oxide source).
    #[serde(default = "default_min_native_chars")]
    pub min_native_chars: usize,
    /// Language(s) passed to the OCR engine (tesseract `-l` syntax, e.g.
    /// `"eng"`, `"jpn"`, `"jpn+eng"`). Default `"jpn+eng"`.
    #[serde(default = "default_ocr_lang")]
    pub lang: String,
    /// Which OCR engine to use when [`Self::enabled`] is `true`. See
    /// [`OcrEngineKind`].
    #[serde(default)]
    pub engine: OcrEngineKind,
}

/// Default value for [`OcrConfig::min_native_chars`].
fn default_min_native_chars() -> usize {
    50
}

/// Default value for [`OcrConfig::lang`].
fn default_ocr_lang() -> String {
    "jpn+eng".to_string()
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_native_chars: default_min_native_chars(),
            lang: default_ocr_lang(),
            engine: OcrEngineKind::default(),
        }
    }
}

/// Which OCR engine [`OcrConfig`] should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OcrEngineKind {
    /// Shell out to a `tesseract` binary on `PATH` (default). Chosen over
    /// pdf_oxide's built-in `ocr` feature (ONNX Runtime + PaddleOCR-style
    /// models) because it adds no heavy build dependency or bundled model
    /// files — see `docs/refactor/phase4_findings.md` P4-1 §6 and
    /// `docs/refactor/phase4_extraction.md` P4-2 for the A/B decision.
    #[default]
    Tesseract,
    /// Reserved for pdf_oxide's built-in OCR (its own `ocr` feature: ONNX
    /// Runtime + PaddleOCR-style det/rec/dict models). **Not wired up** in
    /// this crate — selecting it always behaves as "OCR engine unavailable"
    /// (a `PdfLayWarning::PageTextMissing`, never a panic or a build-time
    /// dependency on `ort`). Kept as an explicit, honest placeholder for a
    /// future task rather than omitted silently.
    Builtin,
}

/// Configuration for vector-figure detection ([`crate::figure::VectorFigureClusterer`]).
///
/// A vector figure (line art / a diagram drawn with PDF path operators
/// rather than embedded as a raster image) has no image XObject at all, so
/// [`crate::extract::ImageExtractor`] never sees it. Without this feature its
/// caption would be reported as `PdfLayWarning::UnmatchedCaption` even though
/// the figure is visibly present in the PDF. When enabled, captions left
/// unmatched after raster image matching are matched instead to a nearby
/// spatial cluster of `PathObject`s (see `extract_all_paths`), recorded as a
/// `FigureInfo` with a region bounding box but no raster file
/// (`ImageInfo::path == None`). Rendering the vector graphic itself (as an
/// image) is out of scope — see `docs/refactor/phase4_extraction.md` P4-3.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorFigureConfig {
    /// Whether to attempt vector-figure clustering at all. Default `true`;
    /// set `false` to restore the pre-P4-3 behavior (such captions become
    /// `UnmatchedCaption`).
    #[serde(default = "default_figure_vector_enabled")]
    pub enabled: bool,
    /// Maximum gap (points) between two path bounding boxes for them to be
    /// merged into the same cluster.
    #[serde(default = "default_cluster_gap_pt")]
    pub cluster_gap_pt: f64,
    /// Minimum number of paths a cluster must contain to be considered a
    /// candidate vector figure (filters out stray rule/border lines that
    /// belong to running text or tables, not a diagram).
    #[serde(default = "default_min_paths")]
    pub min_paths: usize,
}

/// Default value for [`VectorFigureConfig::enabled`].
fn default_figure_vector_enabled() -> bool {
    true
}

/// Default value for [`VectorFigureConfig::cluster_gap_pt`].
fn default_cluster_gap_pt() -> f64 {
    15.0
}

/// Default value for [`VectorFigureConfig::min_paths`].
fn default_min_paths() -> usize {
    4
}

impl Default for VectorFigureConfig {
    fn default() -> Self {
        Self {
            enabled: default_figure_vector_enabled(),
            cluster_gap_pt: default_cluster_gap_pt(),
            min_paths: default_min_paths(),
        }
    }
}

/// Configuration for figure/table caption detection ([`CaptionDetector`]).
///
/// [`CaptionDetector`]: crate::figure::CaptionDetector
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptionConfig {
    /// Additional user-supplied regex patterns matched as `Figure` captions,
    /// on top of the built-in patterns (`Fig.`/`Figure`/`FIG.`). A pattern
    /// that fails to compile is skipped with a warning rather than causing a
    /// panic; the remaining patterns (built-in and user-supplied) still apply.
    #[serde(default)]
    pub extra_figure_patterns: Vec<String>,
    /// Additional user-supplied regex patterns matched as `Table` captions,
    /// on top of the built-in patterns (`Table`/`Tab.`). Same failure handling
    /// as [`Self::extra_figure_patterns`].
    #[serde(default)]
    pub extra_table_patterns: Vec<String>,
    /// Whether to recognize Japanese caption prefixes ("図"/"表", full-width
    /// digits included). Default `true`; set `false` to restore the
    /// ASCII/English-only pre-P4-4 behavior.
    #[serde(default = "default_enable_japanese")]
    pub enable_japanese: bool,
    /// Whether to recognize `Scheme N` / `Chart N` captions (matched as
    /// image-matchable caption types alongside `Figure`). Default `true`; set
    /// `false` to restore the pre-P4-4 behavior.
    #[serde(default = "default_enable_scheme_chart")]
    pub enable_scheme_chart: bool,
}

/// Default value for [`CaptionConfig::enable_japanese`].
fn default_enable_japanese() -> bool {
    true
}

/// Default value for [`CaptionConfig::enable_scheme_chart`].
fn default_enable_scheme_chart() -> bool {
    true
}

impl Default for CaptionConfig {
    fn default() -> Self {
        Self {
            extra_figure_patterns: Vec::new(),
            extra_table_patterns: Vec::new(),
            enable_japanese: default_enable_japanese(),
            enable_scheme_chart: default_enable_scheme_chart(),
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
    /// Minimum number of distinct aligned row levels required to accept a
    /// caption-less text-alignment candidate as a table (only consulted when
    /// [`Self::allow_captionless_alignment`] is enabled — the legacy
    /// caption-anchored path is unaffected by this value, so existing
    /// detection results do not change unless the new path is opted into).
    #[serde(default = "default_borderless_min_rows")]
    pub borderless_min_rows: usize,
    /// When `true`, text-alignment detection also accepts column-aligned
    /// regions that have **no** adjacent "Table N" caption, provided they
    /// meet `min_columns` and `borderless_min_rows`. Default `false`
    /// preserves the legacy behavior of requiring a caption (no regression).
    #[serde(default)]
    pub allow_captionless_alignment: bool,
    /// Maximum vertical gap (points) between consecutive aligned rows that
    /// are still considered part of the same caption-less table candidate.
    /// Only consulted when [`Self::allow_captionless_alignment`] is enabled.
    #[serde(default = "default_captionless_row_gap")]
    pub captionless_row_gap: f64,
}

/// Default value for [`TableConfig::borderless_min_rows`].
fn default_borderless_min_rows() -> usize {
    3
}

/// Default value for [`TableConfig::captionless_row_gap`].
fn default_captionless_row_gap() -> f64 {
    60.0
}

impl Default for TableConfig {
    fn default() -> Self {
        Self {
            min_columns: 2,
            column_alignment_tolerance: 5.0,
            use_rule_detection: true,
            use_text_alignment: true,
            borderless_min_rows: default_borderless_min_rows(),
            allow_captionless_alignment: false,
            captionless_row_gap: default_captionless_row_gap(),
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
    /// When true, text repeated in the top/bottom zone across 3+ pages is
    /// reclassified as a running header/footer before header detection, so it
    /// cannot become a spurious section header. Set false for legacy behavior.
    #[serde(default = "default_detect_repeated_running")]
    pub detect_repeated_running: bool,
    /// Known section-name keywords used as a header signal. Matched
    /// case-insensitively (and full-width folded) with a bounded substring rule.
    /// Includes English and Japanese defaults; extend for other formats.
    #[serde(default = "default_known_section_names")]
    pub known_section_names: Vec<String>,
    /// Score bonus added when a block looks like a CJK-script heading. Provides
    /// a signal for languages where the all-caps heuristic does not apply.
    #[serde(default = "default_cjk_heading_bonus")]
    pub cjk_heading_bonus: u32,
    /// Bin width (points) for clustering heading candidate font sizes into
    /// levels. Matches the body-font histogram bin for consistency.
    #[serde(default = "default_cluster_bin_width")]
    pub cluster_bin_width: f64,
    /// Maximum gap (points) between adjacent font-size bins that are still
    /// merged into one heading level (absorbs measurement jitter).
    #[serde(default = "default_cluster_merge_gap")]
    pub cluster_merge_gap: f64,
    /// Maximum heading level assigned by font clustering / numbering depth.
    #[serde(default = "default_max_level")]
    pub max_level: u8,
    /// Minimum number of confident (scored) headers required for
    /// `SectionBuilder` to use ordinary header-based splitting. When
    /// `headers.len()` falls below this count, the document is instead
    /// segmented by the no-confident-header fallback (font-shift / bold-shift
    /// boundaries), so it never collapses into a single opaque section
    /// (P1-6). Set to `0` to always use header-based splitting, restoring the
    /// pre-P1-6 behavior of a single section when zero headers are detected.
    #[serde(default = "default_min_confident_headers")]
    pub min_confident_headers: usize,
    /// Font-size ratio (relative to body text) a block must reach, coming
    /// from a block below that ratio, to be treated as a pseudo-heading
    /// font-shift boundary by the no-confident-header fallback segmenter
    /// (P1-6). Mirrors the spirit of the (now-removed) isolated `1.15`
    /// level-assignment threshold, but only used for fallback segmentation.
    #[serde(default = "default_fallback_font_shift_ratio")]
    pub fallback_font_shift_ratio: f64,
}

/// Default value for [`HeaderDetectionConfig::respect_classification`].
fn default_respect_classification() -> bool {
    true
}

/// Default value for [`HeaderDetectionConfig::detect_repeated_running`].
fn default_detect_repeated_running() -> bool {
    true
}

/// Default value for [`HeaderDetectionConfig::cjk_heading_bonus`].
fn default_cjk_heading_bonus() -> u32 {
    1
}

/// Default value for [`HeaderDetectionConfig::cluster_bin_width`].
fn default_cluster_bin_width() -> f64 {
    0.5
}

/// Default value for [`HeaderDetectionConfig::cluster_merge_gap`].
fn default_cluster_merge_gap() -> f64 {
    0.5
}

/// Default value for [`HeaderDetectionConfig::max_level`].
fn default_max_level() -> u8 {
    6
}

/// Default value for [`HeaderDetectionConfig::min_confident_headers`].
fn default_min_confident_headers() -> usize {
    1
}

/// Default value for [`HeaderDetectionConfig::fallback_font_shift_ratio`].
fn default_fallback_font_shift_ratio() -> f64 {
    1.15
}

/// Default known section-name keywords (English + Japanese).
pub(crate) fn default_known_section_names() -> Vec<String> {
    [
        // English
        "ABSTRACT",
        "INTRODUCTION",
        "BACKGROUND",
        "RELATED WORK",
        "METHOD",
        "METHODS",
        "METHODOLOGY",
        "APPROACH",
        "EXPERIMENT",
        "EXPERIMENTS",
        "EXPERIMENTAL",
        "RESULTS",
        "RESULT",
        "RESULTS AND DISCUSSION",
        "DISCUSSION",
        "ANALYSIS",
        "CONCLUSION",
        "CONCLUSIONS",
        "SUMMARY",
        "REFERENCES",
        "BIBLIOGRAPHY",
        "ACKNOWLEDGMENT",
        "ACKNOWLEDGMENTS",
        "APPENDIX",
        "SUPPLEMENTARY",
        "SUPPORTING INFORMATION",
        // Japanese
        "概要",
        "序論",
        "はじめに",
        "関連研究",
        "手法",
        "提案手法",
        "実験",
        "評価",
        "結果",
        "考察",
        "議論",
        "結論",
        "まとめ",
        "参考文献",
        "謝辞",
        "付録",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

impl Default for HeaderDetectionConfig {
    fn default() -> Self {
        Self {
            min_score: 4,
            max_chars: 120,
            max_lines: 3,
            respect_classification: default_respect_classification(),
            detect_repeated_running: default_detect_repeated_running(),
            known_section_names: default_known_section_names(),
            cjk_heading_bonus: default_cjk_heading_bonus(),
            cluster_bin_width: default_cluster_bin_width(),
            cluster_merge_gap: default_cluster_merge_gap(),
            max_level: default_max_level(),
            min_confident_headers: default_min_confident_headers(),
            fallback_font_shift_ratio: default_fallback_font_shift_ratio(),
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
    /// Base path prepended to figure image filenames in LLM text output
    /// (e.g. `"./images"`). Empty (the default) emits only the filename
    /// component of `fig.image.path` — the raw on-disk path (which may be
    /// absolute) is never embedded. Mirrors `MarkdownConfig::image_base_path`.
    #[serde(default)]
    pub image_base: String,
}

impl Default for LlmTextConfig {
    fn default() -> Self {
        Self {
            include_figures: true,
            include_tables: true,
            include_section_headers: true,
            math_representation: MathRepresentationPreference::Auto,
            figure_format: FigureTextFormat::Placeholder,
            image_base: String::new(),
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
    /// Whether to prepend a breadcrumb of ancestor section headings plus the
    /// section's own heading line to the start of each chunk's text, e.g.
    /// `[Context: METHODS > Data Collection]\n# Data Collection`. Ancestor
    /// headings are joined with `" > "`; headerless sections contribute no
    /// path segment. Applies to the `SectionBoundary` split strategy
    /// (including its oversized-section sub-splits, where every sub-chunk
    /// carries the same prefix); `TokenCount` and `Paragraph` do not yet
    /// carry per-chunk section attribution to prefix (see P2-4).
    pub include_section_context: bool,
    /// Optional math configuration used to render chunk body text.
    ///
    /// `None` (the default) keeps the legacy behavior of chunk text carrying
    /// unconverted math glyphs; `Some` routes chunk rendering through the same
    /// math detector/converter used by Markdown and LLM text output, via
    /// `output::render_core`.
    #[serde(default)]
    pub math_config: Option<MathConfig>,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            max_tokens: 4000,
            overlap_tokens: 200,
            split_strategy: SplitStrategy::SectionBoundary,
            include_section_context: true,
            math_config: None,
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

    /// P2-8: the borderless/caption-less relaxation knobs must default to
    /// the pre-P2-8 behavior (caption required, no regression).
    #[test]
    fn table_config_borderless_default() {
        let tc = TableConfig::default();
        assert_eq!(tc.borderless_min_rows, 3);
        assert!(!tc.allow_captionless_alignment);
        assert_eq!(tc.captionless_row_gap, 60.0);
    }

    #[test]
    fn header_detection_defaults() {
        let hd = HeaderDetectionConfig::default();
        assert_eq!(hd.min_score, 4);
        assert_eq!(hd.max_chars, 120);
        assert_eq!(hd.max_lines, 3);
    }

    /// P1-6: the no-confident-header fallback defaults to engaging when zero
    /// headers are detected (min_confident_headers=1), with the documented
    /// font-shift ratio.
    #[test]
    fn header_detection_fallback_defaults() {
        let hd = HeaderDetectionConfig::default();
        assert_eq!(hd.min_confident_headers, 1);
        assert_eq!(hd.fallback_font_shift_ratio, 1.15);
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

    /// P4-4: caption pattern broadening (Japanese, Scheme/Chart) defaults to
    /// enabled, with no user-supplied extra patterns.
    #[test]
    fn caption_config_defaults() {
        let cc = CaptionConfig::default();
        assert!(cc.enable_japanese);
        assert!(cc.enable_scheme_chart);
        assert!(cc.extra_figure_patterns.is_empty());
        assert!(cc.extra_table_patterns.is_empty());
        assert!(Config::default().caption.enable_japanese);
    }

    /// P4-3: real-format image saving is on by default (no `force_png`
    /// back-compat opt-out needed), and vector-figure clustering defaults to
    /// enabled with the documented thresholds.
    #[test]
    fn image_and_vector_figure_defaults() {
        let cfg = Config::default();
        assert!(!cfg.force_png);
        assert!(cfg.figure_vector.enabled);
        assert_eq!(cfg.figure_vector.cluster_gap_pt, 15.0);
        assert_eq!(cfg.figure_vector.min_paths, 4);
    }

    /// P4-2: OCR is off by default (no behavior change, no heavy dependency
    /// pulled in), with the documented tesseract-matching threshold/lang.
    #[test]
    fn ocr_config_defaults() {
        let cfg = Config::default();
        assert!(!cfg.ocr.enabled);
        assert_eq!(cfg.ocr.min_native_chars, 50);
        assert_eq!(cfg.ocr.lang, "jpn+eng");
        assert_eq!(cfg.ocr.engine, OcrEngineKind::Tesseract);
    }
}
