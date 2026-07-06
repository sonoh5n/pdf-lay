//! `pdf-lay`: PDF Layout Analysis for Academic Papers.
//!
//! This crate re-exports the public API of [`pdf-lay-core`].
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use pdf_lay::{analyze_pdf, Config};
//! use std::path::Path;
//!
//! let config = Config::default();
//! let result = analyze_pdf(Path::new("paper.pdf"), &config).unwrap();
//! let doc = result.document;
//! println!("Pages: {}", doc.metadata.pages);
//! println!("Sections: {}", doc.toc().len());
//!
//! let selector = doc.select_sections(&["METHODS"]);
//! let markdown = selector.to_markdown(&Default::default());
//! println!("{}", markdown);
//! ```

pub use pdf_lay_core::{
    // Error types
    AnalysisResult,
    // Document types
    BlockType,
    // Config types
    CaptionStyle,
    Chunk,
    ChunkConfig,
    Config,
    Coverage,
    DocumentMetadata,
    FigureInfo,
    FigureTextFormat,
    // Table processing types
    GridBuilder,
    HeuristicTokenizer,
    ImageFormat,
    ImageInfo,
    ImageOutputFormat,
    InsertionPoint,
    LlmTextConfig,
    // Selector types
    LlmTextGenerator,
    MarkdownConfig,
    MathConfig,
    // Math processing types
    MathContext,
    MathConverter,
    MathDetector,
    MathFormatter,
    MathRegion,
    MathRepresentationPreference,
    NumberComponent,
    NumberingAnomalyKind,
    NumberingKey,
    // OCR config (P4-2)
    OcrConfig,
    OcrEngineKind,
    PaperDocument,
    PdfLayError,
    PdfLayWarning,
    Rect,
    Section,
    SectionEntry,
    SectionHeader,
    SectionSelector,
    SplitStrategy,
    TableConfig,
    TableDetector,
    TableGrid,
    TableInfo,
    TableRepresentation,
    TableTextConverter,
    TextBlock,
    TextLine,
    TextSpan,
    TocGenerator,
    // Tokenizer trait (pluggable token counting for `Chunker`)
    Tokenizer,
    // Pipeline entry points
    analyze_pdf,
    analyze_pdf_bytes,
    math_symbols,
    to_latex_map,
    to_unicode_map,
};

#[cfg(feature = "real-tokenizer")]
pub use pdf_lay_core::HfTokenizer;

// Re-export output generators for CLI and advanced use cases.
pub use pdf_lay_core::output::{Chunker, JsonGenerator, MarkdownGenerator};
