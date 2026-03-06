//! Shared types used across all pipeline modules.

pub mod document;
pub mod geometry;
pub mod layout;
pub mod path;
pub mod text;

// Convenience re-exports so callers can write `use crate::types::Rect` etc.
pub use document::{
    Chunk, DocumentMetadata, FigureInfo, ImageFormat, ImageInfo, InsertionPoint, PaperDocument,
    TableInfo, TableRepresentation,
};
pub use geometry::{PageDimensions, Rect};
pub use layout::{Column, LayoutRegion, PageLayout};
pub use path::{PathObject, PathType};
pub use text::{BlockType, FontInfo, Section, SectionHeader, TextBlock, TextLine, TextSpan};
