//! Integration smoke tests: verify the pipeline runs end-to-end.
//!
//! Tests marked `#[ignore]` only run when real PDF fixtures are present:
//!   cargo test --test integration -- --ignored
//!
//! The non-ignored tests always run and verify public API ergonomics,
//! config construction, error handling, and output generators.

use pdf_lay::{
    CaptionStyle, ChunkConfig, Config, FigureTextFormat, LlmTextConfig, LlmTextGenerator,
    MarkdownConfig, MarkdownGenerator, MathRepresentationPreference, PdfLayError, Section,
    SectionSelector, SplitStrategy, TocGenerator, analyze_pdf,
};
use std::path::Path;

// Fixture paths (relative to workspace root where `cargo test` runs).
const IEEE_TWO_COL: &str = "tests/fixtures/sample_ieee_twocol.pdf";
const SINGLE_COL: &str = "tests/fixtures/sample_single_col.pdf";

// ---- Helpers ----------------------------------------------------------------

fn text_only_config() -> Config {
    Config {
        extract_images: false,
        ..Default::default()
    }
}

fn default_markdown_config() -> MarkdownConfig {
    MarkdownConfig {
        image_base_path: "./images".to_string(),
        include_page_numbers: false,
        heading_offset: 1,
        include_metadata_header: false,
        table_as_image: false,
        figure_caption_style: CaptionStyle::Italic,
        math_config: None,
    }
}

fn default_llm_config() -> LlmTextConfig {
    LlmTextConfig {
        include_figures: true,
        include_tables: true,
        include_section_headers: true,
        figure_format: FigureTextFormat::Placeholder,
        math_representation: MathRepresentationPreference::Auto,
    }
}

/// Build a minimal PaperDocument for unit-level testing of output generators.
fn make_test_doc() -> pdf_lay::PaperDocument {
    use pdf_lay::{BlockType, DocumentMetadata, PaperDocument, Rect, SectionHeader, TextBlock};
    use pdf_lay_core::types::{Section, TextLine};
    use std::path::PathBuf;

    PaperDocument {
        paper_id: "test_paper".to_string(),
        source_file: PathBuf::from("test.pdf"),
        metadata: DocumentMetadata {
            title: Some("Integration Test Paper".to_string()),
            authors: vec!["Author A".to_string(), "Author B".to_string()],
            doi: None,
            pages: 4,
        },
        sections: vec![
            Section {
                header: Some(SectionHeader {
                    text: "I. INTRODUCTION".to_string(),
                    clean_text: "INTRODUCTION".to_string(),
                    level: 1,
                    numbering: Some("I.".to_string()),
                    page: 0,
                    bbox: Rect::new(72.0, 700.0, 540.0, 690.0),
                    block_index: 0,
                }),
                level: 1,
                blocks: vec![TextBlock {
                    global_index: 0,
                    lines: vec![TextLine {
                        spans: vec![],
                        text: "This is the introduction body text.".to_string(),
                        bbox: Rect::new(72.0, 680.0, 540.0, 670.0),
                        page: 0,
                        baseline_y: 670.0,
                        primary_font_size: 10.0,
                        primary_font_name: "Times".to_string(),
                        is_bold: false,
                    }],
                    text: "This is the introduction body text.".to_string(),
                    bbox: Rect::new(72.0, 680.0, 540.0, 670.0),
                    page: 0,
                    column_index: 0,
                    block_type: BlockType::BodyText,
                }],
                figures: vec![],
                tables: vec![],
                children: vec![],
                page_range: (0, 1),
            },
            Section {
                header: Some(SectionHeader {
                    text: "II. METHODS".to_string(),
                    clean_text: "METHODS".to_string(),
                    level: 1,
                    numbering: Some("II.".to_string()),
                    page: 2,
                    bbox: Rect::new(72.0, 700.0, 540.0, 690.0),
                    block_index: 5,
                }),
                level: 1,
                blocks: vec![TextBlock {
                    global_index: 1,
                    lines: vec![],
                    text: "We describe our methodology here.".to_string(),
                    bbox: Rect::new(72.0, 680.0, 540.0, 670.0),
                    page: 2,
                    column_index: 0,
                    block_type: BlockType::BodyText,
                }],
                figures: vec![],
                tables: vec![],
                children: vec![],
                page_range: (2, 3),
            },
        ],
        all_figures: vec![],
        all_tables: vec![],
    }
}

// ---- Always-run tests -------------------------------------------------------

/// Verify that analyzing a nonexistent file returns an appropriate error.
#[test]
fn nonexistent_pdf_returns_error() {
    let result = analyze_pdf(
        Path::new("tests/fixtures/does_not_exist.pdf"),
        &text_only_config(),
    );
    assert!(result.is_err(), "Should return Err for nonexistent file");
    match result.unwrap_err() {
        PdfLayError::FileNotFound(_) | PdfLayError::IoError(_) => {}
        e => panic!("Expected FileNotFound or IoError, got: {e:?}"),
    }
}

/// Verify Config::default() compiles and produces expected defaults.
#[test]
fn config_defaults_are_sane() {
    let cfg = Config::default();
    assert!(cfg.extract_images, "extract_images should default to true");
    assert!(cfg.detect_tables, "detect_tables should default to true");
    assert!(
        cfg.caption_max_gap_pt > 0.0,
        "caption_max_gap_pt should be positive"
    );
    assert!(
        cfg.column_detection_bin_width > 0.0,
        "column_detection_bin_width should be positive"
    );
}

/// Verify ChunkConfig::default() is sane.
#[test]
fn chunk_config_defaults_are_sane() {
    let cfg = ChunkConfig::default();
    assert!(cfg.max_tokens > 0);
    assert!(cfg.overlap_tokens < cfg.max_tokens);
    assert!(cfg.include_section_context);
}

/// Verify MarkdownConfig::default() is sane.
#[test]
fn markdown_config_defaults_are_sane() {
    let cfg = MarkdownConfig::default();
    assert!(!cfg.image_base_path.is_empty());
    assert_eq!(cfg.heading_offset, 1);
}

/// Verify LlmTextConfig::default() is sane.
#[test]
fn llm_text_config_defaults_are_sane() {
    let cfg = LlmTextConfig::default();
    assert!(cfg.include_figures);
    assert!(cfg.include_tables);
    assert!(cfg.include_section_headers);
}

/// Verify TocGenerator works on a hand-constructed PaperDocument.
#[test]
fn toc_generator_works_on_constructed_document() {
    let doc = make_test_doc();
    let toc = TocGenerator::generate(&doc);

    assert_eq!(toc.len(), 2, "TOC should have two entries");
    assert_eq!(toc[0].header, "INTRODUCTION");
    assert_eq!(toc[0].level, 1);
    assert_eq!(toc[0].page_range, (0, 1));
    assert_eq!(toc[1].header, "METHODS");
    assert_eq!(toc[1].level, 1);
    assert_eq!(toc[1].page_range, (2, 3));

    // Token estimates should be non-negative.
    for entry in &toc {
        assert!(
            !entry.display_line().is_empty(),
            "display_line should not be empty"
        );
    }
}

/// Verify SectionSelector works on a hand-constructed document.
#[test]
fn section_selector_by_name_on_constructed_document() {
    let doc = make_test_doc();

    // Select by partial name (case-insensitive).
    let sel = SectionSelector::by_names(&doc, &["intro"]);
    assert_eq!(sel.sections().len(), 1);
    assert_eq!(sel.sections()[0].header_text(), "INTRODUCTION");

    // Select by exact name.
    let sel_exact = SectionSelector::by_names(&doc, &["METHODS"]);
    assert_eq!(sel_exact.sections().len(), 1);

    // Select by level.
    let all_l1 = SectionSelector::by_level(&doc, 1);
    assert_eq!(all_l1.sections().len(), 2);

    // Select by pages.
    let page2 = SectionSelector::by_pages(&doc, 2, 3);
    assert_eq!(page2.sections().len(), 1);
    assert_eq!(page2.sections()[0].header_text(), "METHODS");

    // select_sections convenience method on PaperDocument.
    let sel2 = doc.select_sections(&["introduction"]);
    assert_eq!(sel2.sections().len(), 1);

    // No match should return empty.
    let no_match = SectionSelector::by_names(&doc, &["nonexistent_section_xyz"]);
    assert_eq!(no_match.sections().len(), 0);
}

/// Verify SectionSelector::by_predicate works.
#[test]
fn section_selector_by_predicate() {
    let doc = make_test_doc();

    // Select sections with page_range starting at 0.
    let early = doc.select_sections_where(|entry| entry.page_range.0 == 0);
    assert_eq!(early.sections().len(), 1);
    assert_eq!(early.sections()[0].header_text(), "INTRODUCTION");

    // Select by token count estimate (all should match >= 0).
    let all = doc.select_sections_where(|_| true);
    assert_eq!(all.sections().len(), 2);
}

/// Verify MarkdownGenerator produces output on a constructed document.
#[test]
fn markdown_generator_on_constructed_document() {
    let doc = make_test_doc();

    let md_gen = MarkdownGenerator::new(default_markdown_config());
    let md = md_gen.generate(&doc);

    assert!(!md.is_empty(), "Markdown output should not be empty");
    assert!(
        md.contains("INTRODUCTION"),
        "Should contain INTRODUCTION heading"
    );
    assert!(md.contains("METHODS"), "Should contain METHODS heading");
    assert!(
        md.contains("introduction body text"),
        "Should contain body text"
    );
}

/// Verify MarkdownGenerator with front matter.
#[test]
fn markdown_generator_with_front_matter() {
    let doc = make_test_doc();

    let config_with_fm = MarkdownConfig {
        include_metadata_header: true,
        ..default_markdown_config()
    };
    let md_gen = MarkdownGenerator::new(config_with_fm);
    let md = md_gen.generate(&doc);

    assert!(md.starts_with("---"), "Front matter should start with ---");
    assert!(
        md.contains("Integration Test Paper"),
        "Front matter should include title"
    );
    assert!(
        md.contains("Author A"),
        "Front matter should include first author"
    );
}

/// Verify LlmTextGenerator produces output on a constructed document.
#[test]
fn llm_text_generator_on_constructed_sections() {
    let doc = make_test_doc();
    let sections: Vec<&Section> = doc.sections.iter().collect();

    let llm_gen = LlmTextGenerator::new(default_llm_config());
    let text = llm_gen.generate(&sections);

    assert!(!text.is_empty(), "LLM text should not be empty");
    assert!(
        text.contains("INTRODUCTION"),
        "Should contain INTRODUCTION header"
    );
    assert!(text.contains("METHODS"), "Should contain METHODS header");
    assert!(text.contains("methodology"), "Should contain body text");
}

/// Verify LlmTextGenerator with headers disabled.
#[test]
fn llm_text_generator_without_headers() {
    let doc = make_test_doc();
    let sections: Vec<&Section> = doc.sections.iter().collect();

    let no_header_config = LlmTextConfig {
        include_section_headers: false,
        ..default_llm_config()
    };
    let llm_gen = LlmTextGenerator::new(no_header_config);
    let text = llm_gen.generate(&sections);

    // Header markers like "## INTRODUCTION" should not appear.
    assert!(
        !text.contains("## INTRODUCTION"),
        "Markdown header should be omitted"
    );
    // Body text should still appear.
    assert!(
        text.contains("methodology"),
        "Body text should still appear"
    );
}

/// Verify Chunker works on a constructed document.
#[test]
fn chunker_on_constructed_document() {
    use pdf_lay_core::output::Chunker;

    let doc = make_test_doc();
    let config = ChunkConfig {
        max_tokens: 2000,
        overlap_tokens: 100,
        split_strategy: SplitStrategy::SectionBoundary,
        include_section_context: true,
    };

    let chunker = Chunker::new(config);
    let chunks = chunker.chunk(&doc);

    assert!(!chunks.is_empty(), "Should produce at least one chunk");

    // Chunk IDs should be sequential starting at 0.
    for (i, chunk) in chunks.iter().enumerate() {
        assert_eq!(chunk.chunk_id, i, "Chunk IDs should be sequential");
    }

    // Last chunk must have has_continuation = false.
    assert!(
        !chunks.last().unwrap().has_continuation,
        "Last chunk should not have continuation"
    );

    // Each chunk must have non-empty text.
    for chunk in &chunks {
        assert!(!chunk.text.is_empty(), "Chunk text should not be empty");
    }
}

/// Verify Chunker with TokenCount strategy.
#[test]
fn chunker_token_count_strategy() {
    use pdf_lay_core::output::Chunker;

    let doc = make_test_doc();
    let config = ChunkConfig {
        max_tokens: 500,
        overlap_tokens: 10,
        split_strategy: SplitStrategy::TokenCount,
        include_section_context: false,
    };

    let chunker = Chunker::new(config);
    let chunks = chunker.chunk(&doc);

    assert!(!chunks.is_empty());
    if let Some(last) = chunks.last() {
        assert!(!last.has_continuation);
    }
}

/// Verify Chunker with Paragraph strategy.
#[test]
fn chunker_paragraph_strategy() {
    use pdf_lay_core::output::Chunker;

    let doc = make_test_doc();
    let config = ChunkConfig {
        max_tokens: 500,
        overlap_tokens: 10,
        split_strategy: SplitStrategy::Paragraph,
        include_section_context: false,
    };

    let chunker = Chunker::new(config);
    let chunks = chunker.chunk(&doc);

    assert!(!chunks.is_empty());
    if let Some(last) = chunks.last() {
        assert!(!last.has_continuation);
    }
}

/// Verify JsonGenerator produces valid JSON from a constructed document.
#[test]
fn json_generator_on_constructed_document() {
    use pdf_lay_core::output::JsonGenerator;
    use pdf_lay_core::types::Section;

    let doc = make_test_doc();

    // Full document JSON.
    let json = JsonGenerator::generate(&doc).expect("JSON serialization should succeed");
    assert!(!json.is_empty(), "JSON should not be empty");
    assert!(json.contains('\n'), "JSON should be pretty-printed");

    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("Output should be valid JSON");
    assert!(parsed.is_object(), "JSON root should be an object");
    assert_eq!(parsed["paper_id"], "test_paper");
    assert!(
        parsed["sections"].is_array(),
        "sections should be a JSON array"
    );
    assert_eq!(parsed["sections"].as_array().unwrap().len(), 2);

    // Section-only JSON.
    let sections: Vec<&Section> = doc.sections.iter().collect();
    let json_secs =
        JsonGenerator::generate_sections(&sections).expect("Section JSON should succeed");
    let parsed_secs: serde_json::Value =
        serde_json::from_str(&json_secs).expect("Section JSON should be valid");
    assert!(parsed_secs.is_array(), "Section JSON root should be array");
    assert_eq!(parsed_secs.as_array().unwrap().len(), 2);
}

/// Verify token estimation is stable and non-zero for non-empty text.
#[test]
fn token_estimation_is_positive_for_nonempty_text() {
    use pdf_lay_core::output::Chunker;

    let text = "The quick brown fox jumps over the lazy dog.";
    let tokens = Chunker::estimate_tokens(text);
    assert!(
        tokens > 0,
        "Token estimate should be positive for non-empty text"
    );
    // 44 chars / 4 ≈ 11 tokens — allow a reasonable range.
    assert!(
        tokens >= 8,
        "Token estimate should be at least 8 for ~44-char text"
    );
    assert!(
        tokens <= 20,
        "Token estimate should be at most 20 for ~44-char text"
    );

    // Empty string returns 0.
    assert_eq!(Chunker::estimate_tokens(""), 0);
}

/// Verify SectionEntry::display_line format.
#[test]
fn section_entry_display_line_format() {
    let doc = make_test_doc();
    let toc = TocGenerator::generate(&doc);

    assert!(!toc.is_empty());
    let line = toc[0].display_line();
    assert!(
        line.contains("INTRODUCTION"),
        "display_line should contain header text"
    );
    assert!(
        line.contains("[1]"),
        "display_line should contain level marker"
    );
    assert!(line.contains("p."), "display_line should contain page info");
    assert!(
        line.contains("tokens"),
        "display_line should contain token count"
    );
}

/// Verify SectionSelector output methods work on a constructed document.
#[test]
fn section_selector_output_methods() {
    use pdf_lay_core::output::Chunker;

    let doc = make_test_doc();
    let sel = SectionSelector::by_level(&doc, 1);

    // LLM text via selector.
    let llm_text = sel.to_llm_text(&default_llm_config());
    assert!(!llm_text.is_empty());

    // Markdown via selector.
    let md = sel.to_markdown(&default_markdown_config());
    assert!(!md.is_empty());

    // JSON via selector.
    let json = sel.to_json().expect("Selector JSON should succeed");
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.is_array());

    // Chunks via selector.
    let chunks = sel.to_chunks(&ChunkConfig::default());
    assert!(!chunks.is_empty());
    if let Some(last) = chunks.last() {
        assert!(!last.has_continuation);
    }

    // Token estimate.
    let total = sel.total_estimated_tokens();
    assert!(
        total > 0,
        "Selected sections should have positive token estimate"
    );

    // Selected indices.
    let indices = sel.selected_indices();
    assert_eq!(indices.len(), 2, "Both level-1 sections should appear");

    // Chunker on selected sections directly.
    let sections = sel.sections();
    let chunker = Chunker::new(ChunkConfig::default());
    let section_chunks = chunker.chunk_sections(sections);
    assert!(!section_chunks.is_empty());
}

/// Verify PaperDocument convenience methods.
#[test]
fn paper_document_toc_method() {
    let doc = make_test_doc();
    let toc = doc.toc();
    assert_eq!(toc.len(), 2);
    assert_eq!(toc[0].header, "INTRODUCTION");
    assert_eq!(toc[1].header, "METHODS");
}

/// Verify FigureInfo::caption_description strips prefix correctly.
#[test]
fn figure_info_caption_description() {
    use pdf_lay::{FigureInfo, ImageFormat, ImageInfo, InsertionPoint, Rect};
    use std::path::PathBuf;

    let fig = FigureInfo {
        figure_id: "Fig. 1".to_string(),
        figure_number: Some(1),
        caption_text: "Fig. 1: A schematic of the system architecture.".to_string(),
        image: ImageInfo {
            path: PathBuf::from("images/fig1.png"),
            page: 0,
            // Rect::new(left, top, right, bottom) with top >= bottom (Y-up coordinates).
            raw_bbox: Rect::new(0.0, 100.0, 200.0, 0.0),
            normalized_bbox: Rect::new(0.0, 100.0, 200.0, 0.0),
            width_px: 400,
            height_px: 300,
            format: ImageFormat::Png,
        },
        context_text: "See figure 1 for details.".to_string(),
        insertion_point: InsertionPoint {
            page: 0,
            after_block_index: Some(3),
            y_position: 400.0,
        },
    };

    let desc = fig.caption_description();
    assert!(
        desc.contains("schematic"),
        "Description should strip the Fig. 1: prefix"
    );
    assert!(
        !desc.starts_with("Fig."),
        "Description should not start with Fig."
    );
}

/// Verify analyze_pdf_bytes returns an error for empty/invalid bytes.
#[test]
fn analyze_pdf_bytes_invalid_returns_error() {
    use pdf_lay::analyze_pdf_bytes;

    let bad_bytes = b"not a real pdf file contents";
    let result = analyze_pdf_bytes(bad_bytes, &text_only_config());
    assert!(result.is_err(), "Invalid bytes should return Err");
}

// ---- Ignored tests (require PDF fixtures) -----------------------------------

#[test]
#[ignore = "requires tests/fixtures/sample_ieee_twocol.pdf"]
fn ieee_paper_has_sections() {
    let result = analyze_pdf(Path::new(IEEE_TWO_COL), &text_only_config())
        .expect("Analysis should succeed on a valid PDF");

    let doc = &result.document;
    assert!(doc.metadata.pages > 0, "Should have at least one page");
    assert!(
        !doc.sections.is_empty(),
        "IEEE papers should produce at least one section"
    );

    println!("Pages: {}", doc.metadata.pages);
    println!("Sections: {}", doc.sections.len());
    println!("Warnings: {}", result.warnings.len());
}

#[test]
#[ignore = "requires tests/fixtures/sample_ieee_twocol.pdf"]
fn ieee_paper_toc_is_non_empty() {
    let result = analyze_pdf(Path::new(IEEE_TWO_COL), &text_only_config()).unwrap();
    let toc = TocGenerator::generate(&result.document);

    assert!(!toc.is_empty(), "TOC should not be empty");

    for entry in &toc {
        println!(
            "[L{}] {} (p.{}-{}, ~{} tokens)",
            entry.level,
            entry.header,
            entry.page_range.0 + 1,
            entry.page_range.1 + 1,
            entry.estimated_tokens
        );
    }
}

#[test]
#[ignore = "requires tests/fixtures/sample_ieee_twocol.pdf"]
fn ieee_paper_markdown_output_non_empty() {
    let result = analyze_pdf(Path::new(IEEE_TWO_COL), &text_only_config()).unwrap();
    let md_gen = MarkdownGenerator::new(default_markdown_config());
    let md = md_gen.generate(&result.document);

    assert!(!md.is_empty(), "Markdown output should not be empty");
    assert!(
        md.contains("##"),
        "Markdown should contain at least one heading"
    );
    println!("Markdown length: {} chars", md.len());
}

#[test]
#[ignore = "requires tests/fixtures/sample_ieee_twocol.pdf"]
fn ieee_paper_llm_text_non_empty() {
    let result = analyze_pdf(Path::new(IEEE_TWO_COL), &text_only_config()).unwrap();
    let all_sections: Vec<&Section> = result.document.sections.iter().collect();
    let llm_gen = LlmTextGenerator::new(default_llm_config());
    let text = llm_gen.generate(&all_sections);

    assert!(!text.is_empty(), "LLM text output should not be empty");
    println!("LLM text length: {} chars", text.len());
}

#[test]
#[ignore = "requires tests/fixtures/sample_ieee_twocol.pdf"]
fn ieee_paper_json_output_is_valid() {
    use pdf_lay_core::output::JsonGenerator;

    let result = analyze_pdf(Path::new(IEEE_TWO_COL), &text_only_config()).unwrap();
    let json = JsonGenerator::generate(&result.document).expect("Serialization should not fail");

    assert!(!json.is_empty());
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("Output should be valid JSON");
    assert!(parsed.is_object(), "JSON root should be an object");
    assert!(
        parsed["sections"].is_array(),
        "sections should be a JSON array"
    );
}

#[test]
#[ignore = "requires tests/fixtures/sample_ieee_twocol.pdf"]
fn ieee_paper_chunking_produces_chunks() {
    use pdf_lay_core::output::Chunker;

    let result = analyze_pdf(Path::new(IEEE_TWO_COL), &text_only_config()).unwrap();
    let config = ChunkConfig {
        max_tokens: 2000,
        overlap_tokens: 100,
        split_strategy: SplitStrategy::SectionBoundary,
        include_section_context: true,
    };
    let chunker = Chunker::new(config);
    let chunks = chunker.chunk(&result.document);

    assert!(!chunks.is_empty(), "Should produce at least one chunk");
    for (i, chunk) in chunks.iter().enumerate() {
        assert_eq!(chunk.chunk_id, i, "Chunk IDs should be sequential");
    }
    assert!(!chunks.last().unwrap().has_continuation);
    println!("Chunks: {}", chunks.len());
}

#[test]
#[ignore = "requires tests/fixtures/sample_ieee_twocol.pdf"]
fn ieee_paper_section_selector_by_name() {
    let result = analyze_pdf(Path::new(IEEE_TWO_COL), &text_only_config()).unwrap();
    let doc = &result.document;

    let selector = SectionSelector::by_names(doc, &["introduction"]);
    let text = selector.to_llm_text(&LlmTextConfig {
        include_figures: false,
        include_tables: false,
        include_section_headers: true,
        figure_format: FigureTextFormat::Omit,
        math_representation: MathRepresentationPreference::Auto,
    });

    if !selector.sections().is_empty() {
        assert!(
            !text.is_empty(),
            "Selected introduction text should not be empty"
        );
        println!("Introduction text length: {} chars", text.len());
    } else {
        println!("NOTE: No INTRODUCTION section found (paper may use different naming)");
    }
}

#[test]
#[ignore = "requires tests/fixtures/sample_ieee_twocol.pdf"]
fn ieee_paper_no_panic_on_warnings() {
    let result = analyze_pdf(Path::new(IEEE_TWO_COL), &text_only_config()).unwrap();
    for w in &result.warnings {
        println!("[warning] {w:?}");
    }
    assert!(!result.document.sections.is_empty() || result.document.metadata.pages > 0);
}

#[test]
#[ignore = "requires tests/fixtures/sample_single_col.pdf"]
fn single_col_paper_has_sections() {
    let result =
        analyze_pdf(Path::new(SINGLE_COL), &text_only_config()).expect("Analysis should succeed");

    assert!(result.document.metadata.pages > 0);
    assert!(!result.document.sections.is_empty());
    println!("Single-col pages: {}", result.document.metadata.pages);
    println!("Single-col sections: {}", result.document.sections.len());
}

#[test]
#[ignore = "requires tests/fixtures/sample_ieee_twocol.pdf"]
fn ieee_paper_all_strategies_produce_chunks() {
    use pdf_lay_core::output::Chunker;

    let result = analyze_pdf(Path::new(IEEE_TWO_COL), &text_only_config()).unwrap();

    for strategy in [
        SplitStrategy::SectionBoundary,
        SplitStrategy::TokenCount,
        SplitStrategy::Paragraph,
    ] {
        let config = ChunkConfig {
            max_tokens: 1000,
            overlap_tokens: 50,
            split_strategy: strategy,
            include_section_context: true,
        };
        let chunker = Chunker::new(config);
        let chunks = chunker.chunk(&result.document);
        assert!(
            !chunks.is_empty(),
            "Every strategy should produce at least one chunk"
        );
        assert!(
            !chunks.last().unwrap().has_continuation,
            "Last chunk should never have continuation"
        );
    }
}

// ---- P2-10: Table integration tests ----------------------------------------

/// Build a PaperDocument with a table in a section.
fn make_doc_with_table() -> pdf_lay::PaperDocument {
    use pdf_lay::{
        BlockType, DocumentMetadata, InsertionPoint, PaperDocument, Rect, SectionHeader, TableInfo,
        TableRepresentation, TextBlock,
    };
    use pdf_lay_core::types::{Section, TextLine};
    use std::path::PathBuf;

    let markdown_text = "| Name | Value |\n| --- | --- |\n| α | 0.5 |\n".to_string();
    let table = TableInfo {
        table_id: "Table 1".to_string(),
        table_number: Some(1),
        caption: Some("Table 1. Results".to_string()),
        representation: TableRepresentation::Markdown {
            header: vec!["Name".into(), "Value".into()],
            rows: vec![vec!["α".into(), "0.5".into()]],
            caption: Some("Table 1. Results".into()),
            markdown_text: markdown_text.clone(),
        },
        insertion_point: InsertionPoint {
            page: 0,
            after_block_index: Some(0),
            y_position: 500.0,
        },
        page: 0,
    };

    PaperDocument {
        paper_id: "table_test".to_string(),
        source_file: PathBuf::from("test.pdf"),
        metadata: DocumentMetadata {
            title: Some("Table Test Paper".to_string()),
            authors: vec!["Author A".to_string()],
            doi: None,
            pages: 2,
        },
        sections: vec![Section {
            header: Some(SectionHeader {
                text: "I. RESULTS".to_string(),
                clean_text: "RESULTS".to_string(),
                level: 1,
                numbering: Some("I.".to_string()),
                page: 0,
                bbox: Rect::new(72.0, 700.0, 540.0, 690.0),
                block_index: 0,
            }),
            level: 1,
            blocks: vec![TextBlock {
                global_index: 0,
                lines: vec![TextLine {
                    spans: vec![],
                    text: "We present our results in Table 1.".to_string(),
                    bbox: Rect::new(72.0, 680.0, 540.0, 670.0),
                    page: 0,
                    baseline_y: 670.0,
                    primary_font_size: 10.0,
                    primary_font_name: "Times".to_string(),
                    is_bold: false,
                }],
                text: "We present our results in Table 1.".to_string(),
                bbox: Rect::new(72.0, 680.0, 540.0, 670.0),
                page: 0,
                column_index: 0,
                block_type: BlockType::BodyText,
            }],
            figures: vec![],
            tables: vec![table.clone()],
            children: vec![],
            page_range: (0, 1),
        }],
        all_figures: vec![],
        all_tables: vec![table],
    }
}

/// Verify that a section's table appears in Markdown output.
#[test]
fn test_table_in_markdown_output() {
    let doc = make_doc_with_table();
    let md_gen = pdf_lay::MarkdownGenerator::new(default_markdown_config());
    let md = md_gen.generate(&doc);

    assert!(!md.is_empty(), "Markdown output should not be empty");
    // Caption should be bolded above the table.
    assert!(
        md.contains("Table 1. Results"),
        "Markdown output should contain table caption"
    );
    // Markdown table rows should be present.
    assert!(
        md.contains("| Name | Value |"),
        "Markdown output should contain table header row"
    );
    assert!(
        md.contains("| α | 0.5 |"),
        "Markdown output should contain table data row"
    );
    // Section heading should still be present.
    assert!(
        md.contains("RESULTS"),
        "Section heading should still appear"
    );
}

/// Verify that a section's table appears in LLM text output.
#[test]
fn test_table_in_llm_text_output() {
    let doc = make_doc_with_table();
    let sections: Vec<&Section> = doc.sections.iter().collect();
    let llm_gen = LlmTextGenerator::new(default_llm_config());
    let text = llm_gen.generate(&sections);

    assert!(!text.is_empty(), "LLM text output should not be empty");
    // Caption should appear.
    assert!(
        text.contains("Table 1. Results"),
        "LLM text should contain table caption"
    );
    // Table markdown content should appear.
    assert!(
        text.contains("| Name | Value |"),
        "LLM text should contain table header row"
    );
    assert!(
        text.contains("| α | 0.5 |"),
        "LLM text should contain table data row"
    );
}

/// Verify that tables are omitted when include_tables is false.
#[test]
fn test_table_excluded_when_include_tables_false() {
    use pdf_lay::{FigureTextFormat, MathRepresentationPreference};

    let doc = make_doc_with_table();
    let sections: Vec<&Section> = doc.sections.iter().collect();

    let no_table_config = LlmTextConfig {
        include_tables: false,
        include_figures: false,
        include_section_headers: true,
        figure_format: FigureTextFormat::Placeholder,
        math_representation: MathRepresentationPreference::Auto,
    };
    let llm_gen = LlmTextGenerator::new(no_table_config);
    let text = llm_gen.generate(&sections);

    // Table markdown should NOT appear.
    assert!(
        !text.contains("| Name | Value |"),
        "Table should be excluded when include_tables is false"
    );
    // Body text should still appear.
    assert!(
        text.contains("results in Table 1"),
        "Body text should still appear"
    );
}

// ---- P2-19: Math integration tests ------------------------------------------

/// Build a TextBlock containing a mixed line (plain text + CMMI10 math span).
fn make_mixed_math_block_for_integration() -> pdf_lay::TextBlock {
    use pdf_lay::{BlockType, Rect, TextBlock, TextLine, TextSpan};

    let plain_span = TextSpan {
        text: "where ".to_string(),
        font_name: "TimesNewRoman".to_string(),
        font_size: 10.0,
        is_bold: false,
        is_italic: false,
        bbox: Rect::new(50.0, 700.0, 95.0, 690.0),
        page: 0,
    };
    let math_span = TextSpan {
        text: "α".to_string(),
        font_name: "CMMI10".to_string(),
        font_size: 10.0,
        is_bold: false,
        is_italic: true,
        bbox: Rect::new(100.0, 700.0, 110.0, 690.0),
        page: 0,
    };
    let line = TextLine {
        text: "where α".to_string(),
        spans: vec![plain_span, math_span],
        bbox: Rect::new(50.0, 700.0, 110.0, 690.0),
        page: 0,
        baseline_y: 690.0,
        primary_font_size: 10.0,
        primary_font_name: "TimesNewRoman".to_string(),
        is_bold: false,
    };
    TextBlock {
        global_index: 0,
        lines: vec![line],
        text: "where α".to_string(),
        bbox: Rect::new(50.0, 700.0, 110.0, 690.0),
        page: 0,
        column_index: 0,
        block_type: BlockType::BodyText,
    }
}

/// Build a PaperDocument whose first section contains a math-font block.
fn make_doc_with_math() -> pdf_lay::PaperDocument {
    use pdf_lay::{DocumentMetadata, PaperDocument, Rect, SectionHeader};
    use pdf_lay_core::types::Section;
    use std::path::PathBuf;

    let math_block = make_mixed_math_block_for_integration();

    PaperDocument {
        paper_id: "math_test".to_string(),
        source_file: PathBuf::from("test.pdf"),
        metadata: DocumentMetadata {
            title: Some("Math Test Paper".to_string()),
            authors: vec!["Author B".to_string()],
            doi: None,
            pages: 1,
        },
        sections: vec![Section {
            header: Some(SectionHeader {
                text: "I. ANALYSIS".to_string(),
                clean_text: "ANALYSIS".to_string(),
                level: 1,
                numbering: Some("I.".to_string()),
                page: 0,
                bbox: Rect::new(72.0, 720.0, 540.0, 710.0),
                block_index: 0,
            }),
            level: 1,
            blocks: vec![math_block],
            figures: vec![],
            tables: vec![],
            children: vec![],
            page_range: (0, 0),
        }],
        all_figures: vec![],
        all_tables: vec![],
    }
}

/// Verify that inline math (CMMI10 font + plain text prefix) produces $...$ in Markdown.
#[test]
fn test_math_in_markdown_output() {
    use pdf_lay::MathConfig;

    let doc = make_doc_with_math();

    let md_config = pdf_lay::MarkdownConfig {
        math_config: Some(MathConfig {
            representation: pdf_lay::MathRepresentationPreference::LaTeX,
            ..MathConfig::default()
        }),
        ..default_markdown_config()
    };
    let md_gen = pdf_lay::MarkdownGenerator::new(md_config);
    let md = md_gen.generate(&doc);

    assert!(!md.is_empty(), "Markdown output should not be empty");
    // The mixed block has a plain-text prefix ("where ") followed by a CMMI10 span → Inline math.
    assert!(
        md.contains("$\\alpha$") || md.contains("$α$"),
        "Markdown should contain inline math delimiters for the CMMI10 span; got:\n{md}"
    );
    // Section heading should still be present.
    assert!(
        md.contains("ANALYSIS"),
        "Section heading should still appear"
    );
}

/// Verify that without math_config, CMMI10 text is passed through unchanged.
#[test]
fn test_math_passthrough_without_config() {
    let doc = make_doc_with_math();

    // No math_config → plain passthrough.
    let md_gen = pdf_lay::MarkdownGenerator::new(default_markdown_config());
    let md = md_gen.generate(&doc);

    // There should be no $ delimiters.
    assert!(
        !md.contains("$\\alpha$"),
        "Without math_config there should be no LaTeX delimiters"
    );
    // The raw text of the block should appear.
    assert!(
        md.contains("where α") || md.contains("α"),
        "Raw block text should appear when math_config is None"
    );
}

// ---- P2-25: Full pipeline integration tests ---------------------------------

/// Build a comprehensive PaperDocument with metadata, tables, and math spans.
fn make_comprehensive_doc() -> pdf_lay::PaperDocument {
    use pdf_lay::{
        BlockType, DocumentMetadata, InsertionPoint, PaperDocument, Rect, SectionHeader, TableInfo,
        TableRepresentation, TextBlock, TextLine, TextSpan,
    };
    use pdf_lay_core::types::Section;
    use std::path::PathBuf;

    let table = TableInfo {
        table_id: "Table 1".to_string(),
        table_number: Some(1),
        caption: Some("Table 1. Experimental Results".to_string()),
        representation: TableRepresentation::Markdown {
            header: vec!["Method".into(), "Accuracy".into()],
            rows: vec![
                vec!["Baseline".into(), "0.72".into()],
                vec!["Proposed".into(), "0.91".into()],
            ],
            caption: Some("Table 1. Experimental Results".into()),
            markdown_text:
                "| Method | Accuracy |\n| --- | --- |\n| Baseline | 0.72 |\n| Proposed | 0.91 |\n"
                    .to_string(),
        },
        insertion_point: InsertionPoint {
            page: 1,
            after_block_index: Some(1),
            y_position: 400.0,
        },
        page: 1,
    };

    let math_span = TextSpan {
        text: "α".to_string(),
        font_name: "CMMI10".to_string(),
        font_size: 10.0,
        is_bold: false,
        is_italic: true,
        bbox: Rect::new(100.0, 650.0, 110.0, 640.0),
        page: 0,
    };
    let plain_span = TextSpan {
        text: "The parameter ".to_string(),
        font_name: "TimesNewRoman".to_string(),
        font_size: 10.0,
        is_bold: false,
        is_italic: false,
        bbox: Rect::new(50.0, 650.0, 95.0, 640.0),
        page: 0,
    };
    let math_line = TextLine {
        text: "The parameter α".to_string(),
        spans: vec![plain_span, math_span],
        bbox: Rect::new(50.0, 650.0, 110.0, 640.0),
        page: 0,
        baseline_y: 640.0,
        primary_font_size: 10.0,
        primary_font_name: "TimesNewRoman".to_string(),
        is_bold: false,
    };

    PaperDocument {
        paper_id: "comprehensive_test".to_string(),
        source_file: PathBuf::from("comprehensive.pdf"),
        metadata: DocumentMetadata {
            title: Some("A Comprehensive Test Paper".to_string()),
            authors: vec!["Alice Smith".to_string(), "Bob Jones".to_string()],
            doi: Some("10.1234/test.2024".to_string()),
            pages: 4,
        },
        sections: vec![
            Section {
                header: Some(SectionHeader {
                    text: "I. INTRODUCTION".to_string(),
                    clean_text: "INTRODUCTION".to_string(),
                    level: 1,
                    numbering: Some("I.".to_string()),
                    page: 0,
                    bbox: Rect::new(72.0, 700.0, 540.0, 690.0),
                    block_index: 0,
                }),
                level: 1,
                blocks: vec![TextBlock {
                    global_index: 0,
                    lines: vec![math_line],
                    text: "The parameter α".to_string(),
                    bbox: Rect::new(50.0, 650.0, 110.0, 640.0),
                    page: 0,
                    column_index: 0,
                    block_type: BlockType::BodyText,
                }],
                figures: vec![],
                tables: vec![],
                children: vec![],
                page_range: (0, 0),
            },
            Section {
                header: Some(SectionHeader {
                    text: "II. EXPERIMENTS".to_string(),
                    clean_text: "EXPERIMENTS".to_string(),
                    level: 1,
                    numbering: Some("II.".to_string()),
                    page: 1,
                    bbox: Rect::new(72.0, 700.0, 540.0, 690.0),
                    block_index: 1,
                }),
                level: 1,
                blocks: vec![TextBlock {
                    global_index: 1,
                    lines: vec![],
                    text: "We conducted extensive experiments on standard benchmarks.".to_string(),
                    bbox: Rect::new(72.0, 680.0, 540.0, 670.0),
                    page: 1,
                    column_index: 0,
                    block_type: BlockType::BodyText,
                }],
                figures: vec![],
                tables: vec![table.clone()],
                children: vec![],
                page_range: (1, 2),
            },
        ],
        all_figures: vec![],
        all_tables: vec![table],
    }
}

/// Verify that metadata fields appear correctly in JSON output.
#[test]
fn test_metadata_in_json_output() {
    use pdf_lay_core::output::JsonGenerator;

    let doc = make_comprehensive_doc();
    let json = JsonGenerator::generate(&doc).expect("JSON serialization should succeed");

    assert!(!json.is_empty(), "JSON output should not be empty");

    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("Output should be valid JSON");

    // paper_id
    assert_eq!(
        parsed["paper_id"], "comprehensive_test",
        "JSON should contain correct paper_id"
    );

    // metadata.title
    assert_eq!(
        parsed["metadata"]["title"], "A Comprehensive Test Paper",
        "JSON should contain the correct title"
    );

    // metadata.authors
    let authors = parsed["metadata"]["authors"]
        .as_array()
        .expect("authors should be a JSON array");
    assert!(
        authors.iter().any(|a| a == "Alice Smith"),
        "JSON should contain author Alice Smith"
    );
    assert!(
        authors.iter().any(|a| a == "Bob Jones"),
        "JSON should contain author Bob Jones"
    );

    // metadata.doi
    assert_eq!(
        parsed["metadata"]["doi"], "10.1234/test.2024",
        "JSON should contain the DOI"
    );

    // metadata.pages
    assert_eq!(
        parsed["metadata"]["pages"], 4,
        "JSON should contain correct page count"
    );

    // sections array
    let sections = parsed["sections"]
        .as_array()
        .expect("sections should be a JSON array");
    assert_eq!(sections.len(), 2, "JSON should contain two sections");
}

/// Verify full pipeline: document with tables, math, and metadata produces correct Markdown.
#[test]
fn test_full_doc_with_tables_math_metadata() {
    use pdf_lay::MathConfig;

    let doc = make_comprehensive_doc();

    // Markdown with math and front matter enabled.
    let md_config = pdf_lay::MarkdownConfig {
        include_metadata_header: true,
        math_config: Some(MathConfig {
            representation: pdf_lay::MathRepresentationPreference::LaTeX,
            ..MathConfig::default()
        }),
        ..default_markdown_config()
    };
    let md_gen = pdf_lay::MarkdownGenerator::new(md_config);
    let md = md_gen.generate(&doc);

    assert!(!md.is_empty(), "Markdown output should not be empty");

    // Front matter with metadata.
    assert!(
        md.starts_with("---"),
        "Output should begin with YAML front matter"
    );
    assert!(
        md.contains("A Comprehensive Test Paper"),
        "Front matter should contain the title"
    );
    assert!(
        md.contains("Alice Smith"),
        "Front matter should contain author Alice Smith"
    );

    // Section headings.
    assert!(
        md.contains("INTRODUCTION"),
        "Markdown should contain INTRODUCTION section"
    );
    assert!(
        md.contains("EXPERIMENTS"),
        "Markdown should contain EXPERIMENTS section"
    );

    // Math (inline, CMMI10 span with plain-text prefix).
    assert!(
        md.contains("$\\alpha$") || md.contains("$α$"),
        "Markdown should contain LaTeX math delimiters for α; got:\n{md}"
    );

    // Table content.
    assert!(
        md.contains("Table 1. Experimental Results"),
        "Markdown should contain table caption"
    );
    assert!(
        md.contains("| Method | Accuracy |"),
        "Markdown should contain table header row"
    );
    assert!(
        md.contains("| Proposed | 0.91 |"),
        "Markdown should contain table data row"
    );
}

/// End-to-end: analyze_pdf + toc + select + markdown + json + chunks, all chained.
#[test]
#[ignore = "requires tests/fixtures/sample_ieee_twocol.pdf"]
fn ieee_paper_full_pipeline_chain() {
    use pdf_lay::AnalysisResult;
    use pdf_lay_core::output::{Chunker, JsonGenerator};

    let result: AnalysisResult =
        analyze_pdf(Path::new(IEEE_TWO_COL), &text_only_config()).expect("Pipeline should succeed");

    let doc = &result.document;

    // TOC.
    let toc = TocGenerator::generate(doc);
    assert!(!toc.is_empty());

    // Select first section (if any).
    if !doc.sections.is_empty() {
        let sel = doc.select_sections_by_index(&[0]);
        let md = sel.to_markdown(&default_markdown_config());
        let llm = sel.to_llm_text(&default_llm_config());
        let json_sel = sel.to_json().expect("Section JSON should succeed");
        let chunks_sel = sel.to_chunks(&ChunkConfig::default());
        // Suppress unused-variable warnings while still exercising the methods.
        let _ = (md, llm, json_sel, chunks_sel);
    }

    // Full doc JSON.
    let json = JsonGenerator::generate(doc).expect("Full doc JSON should succeed");
    assert!(!json.is_empty());

    // Full doc chunks.
    let chunks = Chunker::new(ChunkConfig::default()).chunk(doc);
    for (i, c) in chunks.iter().enumerate() {
        assert_eq!(c.chunk_id, i);
    }
    if let Some(last) = chunks.last() {
        assert!(!last.has_continuation);
    }
}
