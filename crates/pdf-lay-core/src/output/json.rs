//! JSON serialization output for PaperDocument and Sections.

use crate::types::{PaperDocument, Section};

/// Generates JSON output from a [`PaperDocument`].
pub struct JsonGenerator;

impl JsonGenerator {
    /// Serialize a full document to a pretty-printed JSON string.
    pub fn generate(doc: &PaperDocument) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(doc)
    }

    /// Serialize a slice of sections to a pretty-printed JSON array.
    pub fn generate_sections(sections: &[&Section]) -> Result<String, serde_json::Error> {
        let owned: Vec<&Section> = sections.to_vec();
        serde_json::to_string_pretty(&owned)
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
}
