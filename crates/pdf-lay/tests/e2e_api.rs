//! Tests that the public API compiles and basic type relationships hold.

use pdf_lay::{
    CaptionStyle, ChunkConfig, Config, LlmTextConfig, MarkdownConfig, MathRepresentationPreference,
    SplitStrategy,
};

#[test]
fn config_defaults_compile() {
    let config = Config::default();
    // Verify the Config type and its fields are accessible via the public API.
    // The default has extract_images = true and detect_tables = true.
    assert!(config.extract_images);
    assert!(config.detect_tables);
}

#[test]
fn markdown_config_compiles() {
    let config = MarkdownConfig {
        image_base_path: "./images".to_string(),
        include_page_numbers: false,
        heading_offset: 1,
        include_metadata_header: false,
        table_as_image: false,
        figure_caption_style: CaptionStyle::Italic,
        math_config: None,
        image_dir: None,
        output_dir: None,
    };
    assert_eq!(config.heading_offset, 1);
}

#[test]
fn chunk_config_defaults_compile() {
    let config = ChunkConfig {
        max_tokens: 4000,
        overlap_tokens: 200,
        split_strategy: SplitStrategy::SectionBoundary,
        include_section_context: true,
        math_config: None,
    };
    assert_eq!(config.max_tokens, 4000);
}

#[test]
fn llm_text_config_compiles() {
    let config = LlmTextConfig {
        include_figures: true,
        include_tables: true,
        include_section_headers: true,
        math_representation: MathRepresentationPreference::Auto,
        figure_format: pdf_lay::FigureTextFormat::Placeholder,
        image_base: String::new(),
    };
    assert!(config.include_figures);
}

#[test]
fn analyze_pdf_returns_error_for_nonexistent_path() {
    use std::path::Path;
    let config = Config::default();
    let result = pdf_lay::analyze_pdf(Path::new("/nonexistent/path/to/paper.pdf"), &config);
    assert!(result.is_err(), "Expected an error for a nonexistent path");
}
