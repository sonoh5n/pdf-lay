//! Section selection layer: TOC generation and selective section output.

mod llm_text;
#[allow(clippy::module_inception)]
mod selector;
mod toc;

pub use llm_text::LlmTextGenerator;
pub use selector::{MatchMode, SectionSelector};
pub use toc::{SectionEntry, TocGenerator};
