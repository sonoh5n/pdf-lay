//! Python bindings for pdf-lay using PyO3.

use std::path::{Path, PathBuf};

use pyo3::prelude::*;

use pdf_lay_core::{
    AnalysisResult, analyze_pdf,
    config::{
        CaptionStyle, ChunkConfig, Config, FigureTextFormat, LlmTextConfig, MarkdownConfig,
        MathRepresentationPreference, SplitStrategy,
    },
    output::{Chunker, JsonGenerator, MarkdownGenerator},
    selector::{SectionEntry, SectionSelector, TocGenerator},
    types::{Chunk, FigureInfo, PaperDocument, Section},
};

// ---------------------------------------------------------------------------
// PyPaperDocument
// ---------------------------------------------------------------------------

/// Wraps a fully-analyzed PDF document.
///
/// Obtain an instance via :func:`pdflay.analyze`.
#[pyclass(name = "PyPaperDocument")]
#[derive(Clone)]
struct PyPaperDocument {
    inner: PaperDocument,
}

#[pymethods]
impl PyPaperDocument {
    /// Unique identifier for this paper (filename stem or DOI-derived).
    #[getter]
    fn paper_id(&self) -> &str {
        &self.inner.paper_id
    }

    /// Total number of pages in the document.
    #[getter]
    fn pages(&self) -> u32 {
        self.inner.metadata.pages
    }

    /// Document title extracted from PDF metadata (may be None).
    #[getter]
    fn title(&self) -> Option<&str> {
        self.inner.metadata.title.as_deref()
    }

    /// List of author names extracted from PDF metadata.
    #[getter]
    fn authors(&self) -> Vec<String> {
        self.inner.metadata.authors.clone()
    }

    /// DOI string if present in the PDF metadata.
    #[getter]
    fn doi(&self) -> Option<&str> {
        self.inner.metadata.doi.as_deref()
    }

    /// Top-level sections of the document.
    #[getter]
    fn sections(&self) -> Vec<PySection> {
        self.inner
            .sections
            .iter()
            .map(|s| PySection { inner: s.clone() })
            .collect()
    }

    /// All figures extracted from the document (flat list).
    #[getter]
    fn figures(&self) -> Vec<PyFigureInfo> {
        self.inner
            .all_figures
            .iter()
            .map(|f| PyFigureInfo { inner: f.clone() })
            .collect()
    }

    /// Generate Markdown output for the entire document.
    ///
    /// Args:
    ///     image_base_path: Base path prepended to image paths in Markdown links.
    ///     include_page_numbers: Whether to annotate sections with page numbers.
    ///     heading_offset: Offset added to section level for ``#`` headers (1 = level-1 → ``##``).
    #[pyo3(signature = (image_base_path = "./images", include_page_numbers = false, heading_offset = 1))]
    fn to_markdown(
        &self,
        image_base_path: &str,
        include_page_numbers: bool,
        heading_offset: u8,
    ) -> String {
        let config = MarkdownConfig {
            image_base_path: image_base_path.to_string(),
            include_page_numbers,
            heading_offset,
            include_metadata_header: false,
            table_as_image: false,
            figure_caption_style: CaptionStyle::Italic,
        };
        MarkdownGenerator::new(config).generate(&self.inner)
    }

    /// Serialize the document to a pretty-printed JSON string.
    ///
    /// Raises:
    ///     ValueError: If JSON serialization fails (should not occur in practice).
    fn to_json(&self) -> PyResult<String> {
        JsonGenerator::generate(&self.inner)
            .map_err(|e: serde_json::Error| pyo3::exceptions::PyValueError::new_err(e.to_string()))
    }

    /// Split the document into LLM-consumable chunks.
    ///
    /// Args:
    ///     max_tokens: Maximum tokens per chunk.
    ///     overlap: Number of overlap tokens between adjacent chunks.
    ///     strategy: One of ``"section"`` (default), ``"token"``, or ``"paragraph"``.
    #[pyo3(signature = (max_tokens = 4000, overlap = 200, strategy = "section"))]
    fn to_chunks(&self, max_tokens: usize, overlap: usize, strategy: &str) -> Vec<PyChunk> {
        let split_strategy = match strategy {
            "token" => SplitStrategy::TokenCount,
            "paragraph" => SplitStrategy::Paragraph,
            _ => SplitStrategy::SectionBoundary,
        };
        let config = ChunkConfig {
            max_tokens,
            overlap_tokens: overlap,
            split_strategy,
            include_section_context: true,
        };
        Chunker::new(config)
            .chunk(&self.inner)
            .into_iter()
            .map(|c| PyChunk { inner: c })
            .collect()
    }

    /// Return the table of contents as a list of :class:`PySectionEntry` objects.
    fn toc(&self) -> Vec<PySectionEntry> {
        TocGenerator::generate(&self.inner)
            .into_iter()
            .map(|e| PySectionEntry { inner: e })
            .collect()
    }

    /// Select sections by header name (case-insensitive partial match).
    ///
    /// Args:
    ///     names: List of header name substrings to match.
    ///
    /// Returns:
    ///     A :class:`PySectionSelector` for the matched sections.
    fn select_sections(&self, names: Vec<String>) -> PySectionSelector {
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let selector = SectionSelector::by_names(&self.inner, &name_refs);
        PySectionSelector {
            doc: self.inner.clone(),
            selected_indices: selector.selected_indices(),
        }
    }

    /// Select sections by their flat index in the document's section tree.
    ///
    /// Args:
    ///     indices: Flat indices (0-based) into the section tree.
    ///
    /// Returns:
    ///     A :class:`PySectionSelector` for the matched sections.
    fn select_sections_by_index(&self, indices: Vec<usize>) -> PySectionSelector {
        let selector = SectionSelector::by_indices(&self.inner, &indices);
        PySectionSelector {
            doc: self.inner.clone(),
            selected_indices: selector.selected_indices(),
        }
    }

    /// Select all sections at a given heading level.
    ///
    /// Args:
    ///     level: Heading level (1 = top-level sections, 2 = subsections, …).
    ///
    /// Returns:
    ///     A :class:`PySectionSelector` for the matched sections.
    fn select_sections_by_level(&self, level: u8) -> PySectionSelector {
        let selector = SectionSelector::by_level(&self.inner, level);
        PySectionSelector {
            doc: self.inner.clone(),
            selected_indices: selector.selected_indices(),
        }
    }

    /// Select sections whose page range overlaps [start, end].
    ///
    /// Args:
    ///     start: First page of the range (0-based page number).
    ///     end: Last page of the range (0-based page number, inclusive).
    ///
    /// Returns:
    ///     A :class:`PySectionSelector` for the matched sections.
    fn select_sections_by_pages(&self, start: u32, end: u32) -> PySectionSelector {
        let selector = SectionSelector::by_pages(&self.inner, start, end);
        PySectionSelector {
            doc: self.inner.clone(),
            selected_indices: selector.selected_indices(),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "PyPaperDocument(paper_id='{}', pages={}, sections={})",
            self.inner.paper_id,
            self.inner.metadata.pages,
            self.inner.sections.len()
        )
    }
}

// ---------------------------------------------------------------------------
// PySectionSelector
// ---------------------------------------------------------------------------

/// A selection of sections from a :class:`PyPaperDocument`.
///
/// Stores an owned copy of the document plus the flat indices of the selected
/// sections.  This avoids lifetime issues with PyO3 — no reference into the
/// parent ``PaperDocument`` is kept.
#[pyclass(name = "PySectionSelector")]
#[derive(Clone)]
struct PySectionSelector {
    doc: PaperDocument,
    selected_indices: Vec<usize>,
}

impl PySectionSelector {
    /// Reconstruct the borrowed `SectionSelector` on demand.
    fn rebuild_selector(&self) -> SectionSelector<'_> {
        SectionSelector::by_indices(&self.doc, &self.selected_indices)
    }
}

#[pymethods]
impl PySectionSelector {
    /// Number of selected sections.
    fn __len__(&self) -> usize {
        self.selected_indices.len()
    }

    /// Sum of estimated token counts across all selected sections.
    fn total_estimated_tokens(&self) -> usize {
        self.rebuild_selector().total_estimated_tokens()
    }

    /// Generate Markdown for the selected sections.
    ///
    /// Args:
    ///     image_base_path: Base path prepended to image paths in Markdown links.
    #[pyo3(signature = (image_base_path = "./images"))]
    fn to_markdown(&self, image_base_path: &str) -> String {
        let config = MarkdownConfig {
            image_base_path: image_base_path.to_string(),
            include_page_numbers: false,
            heading_offset: 1,
            include_metadata_header: false,
            table_as_image: false,
            figure_caption_style: CaptionStyle::Italic,
        };
        self.rebuild_selector().to_markdown(&config)
    }

    /// Serialize selected sections to a pretty-printed JSON array string.
    ///
    /// Raises:
    ///     ValueError: If JSON serialization fails.
    fn to_json(&self) -> PyResult<String> {
        self.rebuild_selector()
            .to_json()
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
    }

    /// Generate LLM-optimized plain text for the selected sections.
    ///
    /// Args:
    ///     include_figures: Whether to include figure placeholders/captions.
    ///     include_tables: Whether to include inline table text.
    ///     figure_format: One of ``"placeholder"`` (default), ``"markdown"``,
    ///         ``"caption"``, or ``"omit"``.
    #[pyo3(signature = (include_figures = true, include_tables = true, figure_format = "placeholder"))]
    fn to_llm_text(
        &self,
        include_figures: bool,
        include_tables: bool,
        figure_format: &str,
    ) -> String {
        let config = LlmTextConfig {
            include_figures,
            include_tables,
            include_section_headers: true,
            figure_format: match figure_format {
                "markdown" => FigureTextFormat::MarkdownLink,
                "caption" => FigureTextFormat::CaptionOnly,
                "omit" => FigureTextFormat::Omit,
                _ => FigureTextFormat::Placeholder,
            },
            math_representation: MathRepresentationPreference::Auto,
        };
        self.rebuild_selector().to_llm_text(&config)
    }

    /// Split selected sections into LLM-consumable chunks.
    ///
    /// Args:
    ///     max_tokens: Maximum tokens per chunk.
    ///     overlap: Number of overlap tokens between adjacent chunks.
    #[pyo3(signature = (max_tokens = 4000, overlap = 200))]
    fn to_chunks(&self, max_tokens: usize, overlap: usize) -> Vec<PyChunk> {
        let config = ChunkConfig {
            max_tokens,
            overlap_tokens: overlap,
            split_strategy: SplitStrategy::SectionBoundary,
            include_section_context: true,
        };
        self.rebuild_selector()
            .to_chunks(&config)
            .into_iter()
            .map(|c| PyChunk { inner: c })
            .collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "PySectionSelector(selected={}, total_tokens={})",
            self.selected_indices.len(),
            self.rebuild_selector().total_estimated_tokens(),
        )
    }
}

// ---------------------------------------------------------------------------
// PySectionEntry
// ---------------------------------------------------------------------------

/// Lightweight section metadata entry (table-of-contents row).
#[pyclass(name = "PySectionEntry")]
#[derive(Clone)]
struct PySectionEntry {
    inner: SectionEntry,
}

#[pymethods]
impl PySectionEntry {
    /// Position within the parent's children list (or top-level list).
    #[getter]
    fn index(&self) -> usize {
        self.inner.index
    }

    /// Clean header text (numbering stripped).
    #[getter]
    fn header(&self) -> &str {
        &self.inner.header
    }

    /// Raw header text as it appeared in the PDF.
    #[getter]
    fn header_raw(&self) -> &str {
        &self.inner.header_raw
    }

    /// Unique path string (e.g. numbering prefix or clean header text).
    #[getter]
    fn path(&self) -> &str {
        &self.inner.path
    }

    /// Heading level (1 = top-level section).
    #[getter]
    fn level(&self) -> u8 {
        self.inner.level
    }

    /// First page of the section (0-based).
    #[getter]
    fn page_start(&self) -> u32 {
        self.inner.page_range.0
    }

    /// Last page of the section (0-based, inclusive).
    #[getter]
    fn page_end(&self) -> u32 {
        self.inner.page_range.1
    }

    /// Estimated token count for this section's body text.
    #[getter]
    fn estimated_tokens(&self) -> usize {
        self.inner.estimated_tokens
    }

    /// True if this section contains at least one figure.
    #[getter]
    fn has_figures(&self) -> bool {
        self.inner.has_figures
    }

    /// Number of figures in this section.
    #[getter]
    fn figure_count(&self) -> usize {
        self.inner.figure_count
    }

    /// True if this section contains at least one table.
    #[getter]
    fn has_tables(&self) -> bool {
        self.inner.has_tables
    }

    /// Number of tables in this section.
    #[getter]
    fn table_count(&self) -> usize {
        self.inner.table_count
    }

    /// Child section entries (subsections).
    #[getter]
    fn children(&self) -> Vec<PySectionEntry> {
        self.inner
            .children
            .iter()
            .map(|c: &SectionEntry| PySectionEntry { inner: c.clone() })
            .collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "PySectionEntry([L{}] '{}' p.{}-{} ~{} tokens fig:{} tab:{})",
            self.inner.level,
            self.inner.header,
            self.inner.page_range.0,
            self.inner.page_range.1,
            self.inner.estimated_tokens,
            self.inner.figure_count,
            self.inner.table_count,
        )
    }
}

// ---------------------------------------------------------------------------
// PySection
// ---------------------------------------------------------------------------

/// A document section with its header, body text, figures, and children.
#[pyclass(name = "PySection")]
#[derive(Clone)]
struct PySection {
    inner: Section,
}

#[pymethods]
impl PySection {
    /// Clean header text (numbering stripped), or None for the document preamble.
    #[getter]
    fn header(&self) -> Option<String> {
        self.inner.header.as_ref().map(|h| h.clean_text.clone())
    }

    /// Raw header text as it appeared in the PDF, or None for the document preamble.
    #[getter]
    fn header_raw(&self) -> Option<String> {
        self.inner.header.as_ref().map(|h| h.text.clone())
    }

    /// Heading level (1 = top-level section, 2 = subsection, …).
    #[getter]
    fn level(&self) -> u8 {
        self.inner.level
    }

    /// Concatenated body text of this section (excluding captions, page numbers, etc.).
    #[getter]
    fn text(&self) -> String {
        self.inner.full_text()
    }

    /// First page of the section (0-based).
    #[getter]
    fn page_start(&self) -> u32 {
        self.inner.page_range.0
    }

    /// Last page of the section (0-based, inclusive).
    #[getter]
    fn page_end(&self) -> u32 {
        self.inner.page_range.1
    }

    /// Figures belonging directly to this section.
    #[getter]
    fn figures(&self) -> Vec<PyFigureInfo> {
        self.inner
            .figures
            .iter()
            .map(|f| PyFigureInfo { inner: f.clone() })
            .collect()
    }

    /// Child sections (subsections) of this section.
    #[getter]
    fn children(&self) -> Vec<PySection> {
        self.inner
            .children
            .iter()
            .map(|s| PySection { inner: s.clone() })
            .collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "PySection(header='{}', level={}, blocks={})",
            self.inner.header_text(),
            self.inner.level,
            self.inner.blocks.len()
        )
    }
}

// ---------------------------------------------------------------------------
// PyFigureInfo
// ---------------------------------------------------------------------------

/// A figure extracted from the PDF (image + caption + metadata).
#[pyclass(name = "PyFigureInfo")]
#[derive(Clone)]
struct PyFigureInfo {
    inner: FigureInfo,
}

#[pymethods]
impl PyFigureInfo {
    /// Identifier string such as ``"Fig. 1"`` or ``"Figure 3"``.
    #[getter]
    fn figure_id(&self) -> &str {
        &self.inner.figure_id
    }

    /// Numeric figure number if present, otherwise None.
    #[getter]
    fn figure_number(&self) -> Option<u32> {
        self.inner.figure_number
    }

    /// Full caption text.
    #[getter]
    fn caption_text(&self) -> &str {
        &self.inner.caption_text
    }

    /// Path to the extracted image on disk.
    #[getter]
    fn image_path(&self) -> String {
        self.inner.image.path.display().to_string()
    }

    /// Page where this figure appears (0-based).
    #[getter]
    fn page(&self) -> u32 {
        self.inner.image.page
    }

    fn __repr__(&self) -> String {
        format!(
            "PyFigureInfo(id='{}', page={})",
            self.inner.figure_id, self.inner.image.page
        )
    }
}

// ---------------------------------------------------------------------------
// PyChunk
// ---------------------------------------------------------------------------

/// A text chunk suitable for an LLM context window.
#[pyclass(name = "PyChunk")]
#[derive(Clone)]
struct PyChunk {
    inner: Chunk,
}

#[pymethods]
impl PyChunk {
    /// Sequential chunk index within this document.
    #[getter]
    fn chunk_id(&self) -> usize {
        self.inner.chunk_id
    }

    /// Paper identifier matching :attr:`PyPaperDocument.paper_id`.
    #[getter]
    fn paper_id(&self) -> &str {
        &self.inner.paper_id
    }

    /// Header text of the containing section.
    #[getter]
    fn section(&self) -> &str {
        &self.inner.section
    }

    /// The text content of this chunk.
    #[getter]
    fn text(&self) -> &str {
        &self.inner.text
    }

    /// Estimated token count for this chunk.
    #[getter]
    fn estimated_tokens(&self) -> usize {
        self.inner.estimated_tokens
    }

    /// First page covered by this chunk (0-based).
    #[getter]
    fn page_start(&self) -> u32 {
        self.inner.page_range.0
    }

    /// Last page covered by this chunk (0-based, inclusive).
    #[getter]
    fn page_end(&self) -> u32 {
        self.inner.page_range.1
    }

    /// True if this chunk continues in the next chunk.
    #[getter]
    fn has_continuation(&self) -> bool {
        self.inner.has_continuation
    }

    /// Figures whose insertion points fall within this chunk.
    #[getter]
    fn figures(&self) -> Vec<PyFigureInfo> {
        self.inner
            .figures
            .iter()
            .map(|f| PyFigureInfo { inner: f.clone() })
            .collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "PyChunk(id={}, section='{}', tokens={}, continuation={})",
            self.inner.chunk_id,
            self.inner.section,
            self.inner.estimated_tokens,
            self.inner.has_continuation,
        )
    }
}

// ---------------------------------------------------------------------------
// Top-level functions
// ---------------------------------------------------------------------------

/// Analyze a PDF file and return a :class:`PyPaperDocument`.
///
/// Args:
///     path: Path to the PDF file.
///     image_dir: Output directory for extracted images (default: ``"./images"``).
///     extract_images: Whether to extract embedded images (default: ``True``).
///     detect_tables: Whether to detect tables (default: ``True``).
///
/// Returns:
///     :class:`PyPaperDocument` with sections, figures, and metadata.
///
/// Raises:
///     RuntimeError: If the PDF cannot be read or parsed.
#[pyfunction]
#[pyo3(signature = (path, image_dir = "./images", extract_images = true, detect_tables = true))]
fn analyze(
    path: &str,
    image_dir: &str,
    extract_images: bool,
    detect_tables: bool,
) -> PyResult<PyPaperDocument> {
    let config = Config {
        image_output_dir: PathBuf::from(image_dir),
        extract_images,
        detect_tables,
        ..Config::default()
    };

    let AnalysisResult { document, warnings } = analyze_pdf(Path::new(path), &config)
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

    // Log non-fatal warnings to stderr.
    for w in &warnings {
        eprintln!("[pdflay warning] {:?}", w);
    }

    Ok(PyPaperDocument { inner: document })
}

// ---------------------------------------------------------------------------
// Module registration
// ---------------------------------------------------------------------------

/// Python extension module ``pdflay`` — PDF layout analysis for academic papers.
#[pymodule]
fn pdflay(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(analyze, m)?)?;
    m.add_class::<PyPaperDocument>()?;
    m.add_class::<PySection>()?;
    m.add_class::<PySectionEntry>()?;
    m.add_class::<PySectionSelector>()?;
    m.add_class::<PyFigureInfo>()?;
    m.add_class::<PyChunk>()?;
    Ok(())
}
