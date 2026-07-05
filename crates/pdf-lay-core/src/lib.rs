//! Core library for PDF layout analysis.
//!
//! This crate is not published to crates.io. Use `pdf-lay` for the public API.

#![warn(missing_docs)]

pub mod config;
pub mod error;
pub mod types;

pub mod figure;
pub mod math;
pub mod table;

// Extraction layer:
pub mod extract;

// Layout layer:
pub mod layout;

// Structure layer:
pub mod structure;

// Pipeline (wires all layers together):
pub(crate) mod pipeline;

// Selection layer (TOC, section selectors, LLM text generation):
pub mod selector;

// Output layer (Markdown, JSON, chunking):
pub mod output;

#[cfg(test)]
pub mod test_helpers;

pub use config::{
    CaptionStyle, ChunkConfig, Config, FigureTextFormat, LlmTextConfig, MarkdownConfig, MathConfig,
    MathRepresentationPreference, ResourceLimits, SplitStrategy, TableConfig,
};
pub use error::{AnalysisResult, Coverage, NumberingAnomalyKind, PdfLayError, PdfLayWarning};
pub use math::{
    MathContext, MathConverter, MathDetector, MathFormatter, MathRegion, math_symbols,
    to_latex_map, to_unicode_map,
};
pub use pipeline::{analyze_pdf, analyze_pdf_bytes};
pub use selector::{LlmTextGenerator, SectionEntry, SectionSelector, TocGenerator};
pub use table::{GridBuilder, TableDetector, TableGrid, TableTextConverter};
pub use types::{
    BlockType, Chunk, DocumentMetadata, FigureInfo, ImageFormat, ImageInfo, InsertionPoint,
    NumberComponent, NumberingKey, PaperDocument, Rect, Section, SectionHeader, TableInfo,
    TableRepresentation, TextBlock, TextLine, TextSpan,
};
