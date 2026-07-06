//! JSON serialization output for PaperDocument and Sections.

use crate::config::MathConfig;
use crate::output::content_ir::{self, ContentDocument};
use crate::types::{PaperDocument, Section};

/// Generates JSON output from a [`PaperDocument`].
pub struct JsonGenerator;

impl JsonGenerator {
    /// Serialize a full document to a pretty-printed JSON string.
    ///
    /// This is a raw dump of the entire [`PaperDocument`] tree — every
    /// `TextBlock`/`TextSpan`/`TextLine` bounding box and font metadata is
    /// included, and body text is the unconverted raw `block.text` (no math
    /// conversion, no table Markdown). For a lightweight, LLM-facing
    /// projection instead, see [`Self::generate_content_only`].
    pub fn generate(doc: &PaperDocument) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(doc)
    }

    /// Serialize a slice of sections to a pretty-printed JSON array.
    pub fn generate_sections(sections: &[&Section]) -> Result<String, serde_json::Error> {
        let owned: Vec<&Section> = sections.to_vec();
        serde_json::to_string_pretty(&owned)
    }

    /// Serialize a **content-only** projection of `doc`: section headers,
    /// breadcrumb paths, math-converted body text, and lightweight figure/
    /// table summaries — with no `bbox`, font metadata, or per-span/per-line
    /// arrays anywhere in the output.
    ///
    /// `math_config` is forwarded to `render_core` for math detection/
    /// conversion of section body text, exactly like `Chunker`/
    /// `MarkdownGenerator`/`LlmTextGenerator`. `None` disables math
    /// conversion (body text keeps raw, unconverted math glyphs) — matching
    /// the rest of the codebase's "no math config supplied = no conversion"
    /// convention.
    ///
    /// This is purely additive: [`Self::generate`] is unchanged and remains
    /// available for callers that need the full geometry-carrying dump.
    pub fn generate_content_only(
        doc: &PaperDocument,
        math_config: Option<&MathConfig>,
    ) -> Result<String, serde_json::Error> {
        let opts = content_ir::content_render_options(math_config);
        let content: ContentDocument = content_ir::project_content(doc, &opts);
        serde_json::to_string_pretty(&content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        BlockType, DocumentMetadata, PaperDocument, Rect, Section, SectionHeader, TextBlock,
    };
    use std::path::PathBuf;

    fn make_doc() -> PaperDocument {
        PaperDocument {
            paper_id: "test_paper".to_string(),
            source_file: PathBuf::from("test.pdf"),
            metadata: DocumentMetadata {
                pages: 2,
                ..Default::default()
            },
            sections: vec![Section {
                header: Some(SectionHeader {
                    text: "INTRODUCTION".to_string(),
                    clean_text: "INTRODUCTION".to_string(),
                    level: 1,
                    numbering: None,
                    page: 0,
                    bbox: Rect::new(72.0, 700.0, 540.0, 690.0),
                    block_index: 0,
                }),
                level: 1,
                blocks: vec![TextBlock {
                    global_index: 0,
                    lines: vec![],
                    text: "Body text here.".to_string(),
                    bbox: Rect::new(72.0, 680.0, 540.0, 670.0),
                    page: 0,
                    column_index: 0,
                    block_type: BlockType::BodyText,
                }],
                figures: vec![],
                tables: vec![],
                children: vec![],
                page_range: (0, 1),
            }],
            all_figures: vec![],
            all_tables: vec![],
        }
    }

    #[test]
    fn document_serializes_to_json() {
        let doc = make_doc();
        let json = JsonGenerator::generate(&doc).expect("JSON serialization should succeed");
        assert!(json.contains("test_paper"), "Should contain paper_id");
        assert!(
            json.contains("INTRODUCTION"),
            "Should contain section header"
        );
        assert!(
            json.contains("Body text here."),
            "Should contain block text"
        );
    }

    #[test]
    fn json_is_valid_and_pretty() {
        let doc = make_doc();
        let json = JsonGenerator::generate(&doc).unwrap();
        // Pretty-printed JSON contains newlines.
        assert!(json.contains('\n'), "Should be pretty-printed");
        // Parse it back to verify validity.
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("Output should be valid JSON");
        assert!(parsed.is_object());
    }

    // ---- content-only projection (P2-7) ----

    #[test]
    fn full_generate_still_includes_bbox() {
        // Regression: `generate` (the raw dump) must be unaffected by the
        // addition of `generate_content_only` — it keeps every geometry
        // field.
        let doc = make_doc();
        let json = JsonGenerator::generate(&doc).unwrap();
        assert!(json.contains("bbox"), "raw dump must still include bbox");
    }

    #[test]
    fn content_only_omits_geometry() {
        let doc = make_doc();
        let json = JsonGenerator::generate_content_only(&doc, None)
            .expect("content-only serialization should succeed");
        assert!(
            !json.contains("bbox"),
            "content-only output must not contain bbox: {json}"
        );
        assert!(
            !json.contains("font_name"),
            "content-only output must not contain font_name: {json}"
        );
    }

    #[test]
    fn content_only_is_valid_json() {
        let doc = make_doc();
        let json = JsonGenerator::generate_content_only(&doc, None).unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("content-only output should be valid JSON");
        assert!(parsed.is_object());
        assert!(parsed["sections"].is_array());
        assert!(
            parsed["sections"][0]["breadcrumb"].is_array(),
            "sections should carry a breadcrumb array"
        );
    }

    fn make_math_doc() -> PaperDocument {
        use crate::types::{TextLine, TextSpan};

        let span = TextSpan {
            text: "α".to_string(),
            font_name: "CMMI10".to_string(),
            font_size: 10.0,
            is_bold: false,
            is_italic: true,
            bbox: Rect::new(100.0, 700.0, 150.0, 690.0),
            page: 0,
        };
        let line = TextLine {
            text: "α".to_string(),
            spans: vec![span],
            bbox: Rect::new(100.0, 700.0, 150.0, 690.0),
            page: 0,
            baseline_y: 690.0,
            primary_font_size: 10.0,
            primary_font_name: "CMMI10".to_string(),
            is_bold: false,
        };
        let block = TextBlock {
            global_index: 0,
            lines: vec![line],
            text: "α".to_string(),
            bbox: Rect::new(100.0, 700.0, 150.0, 690.0),
            page: 0,
            column_index: 0,
            block_type: BlockType::BodyText,
        };
        PaperDocument {
            paper_id: "math_paper".to_string(),
            source_file: PathBuf::from("math.pdf"),
            metadata: DocumentMetadata {
                pages: 1,
                ..Default::default()
            },
            sections: vec![Section {
                header: Some(SectionHeader {
                    text: "SEC".to_string(),
                    clean_text: "SEC".to_string(),
                    level: 1,
                    numbering: None,
                    page: 0,
                    bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                    block_index: 0,
                }),
                level: 1,
                blocks: vec![block],
                figures: vec![],
                tables: vec![],
                children: vec![],
                page_range: (0, 0),
            }],
            all_figures: vec![],
            all_tables: vec![],
        }
    }

    #[test]
    fn content_only_includes_converted_math() {
        use crate::config::{MathConfig, MathRepresentationPreference};

        let doc = make_math_doc();
        let math_config = MathConfig {
            representation: MathRepresentationPreference::LaTeX,
            ..MathConfig::default()
        };
        let json = JsonGenerator::generate_content_only(&doc, Some(&math_config))
            .expect("content-only serialization should succeed");

        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let text = parsed["sections"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("\\alpha"),
            "expected converted math '\\alpha' in content-only text, got: {text}"
        );
        assert!(!json.contains("bbox"), "must still omit bbox: {json}");
    }
}
