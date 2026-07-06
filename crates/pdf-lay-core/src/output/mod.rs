//! Output generation: Markdown, JSON, chunking.

mod chunker;
pub mod content_ir;
mod json;
mod markdown;
pub mod render_core;
pub mod tokenizer;

pub use chunker::Chunker;
pub use content_ir::{ContentDocument, ContentFigure, ContentHeader, ContentSection, ContentTable};
pub use json::JsonGenerator;
pub use markdown::MarkdownGenerator;
#[cfg(feature = "real-tokenizer")]
pub use tokenizer::HfTokenizer;
pub use tokenizer::{HeuristicTokenizer, Tokenizer};
