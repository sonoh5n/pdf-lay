//! Metadata extraction from document blocks.

use regex::Regex;

use crate::types::{BlockType, DocumentMetadata, TextBlock};

/// Extracts document metadata (title, authors, DOI) from classified text blocks.
pub struct MetadataExtractor;

impl MetadataExtractor {
    /// Extract metadata from pre-classified blocks and page count.
    pub fn extract(blocks: &[TextBlock], page_count: u32) -> DocumentMetadata {
        let title = Self::extract_title(blocks);
        let authors = Self::extract_authors(blocks, &title);
        let doi = Self::extract_doi(blocks);

        DocumentMetadata {
            title,
            authors,
            doi,
            pages: page_count,
        }
    }

    /// Find the document title from classified blocks.
    ///
    /// Strategy:
    /// 1. First try blocks explicitly classified as `BlockType::Title`.
    /// 2. Fallback: on page 0, find the block with the largest `primary_font_size`
    ///    that sits in the top 40% of the page and has ≤ 3 lines.
    fn extract_title(blocks: &[TextBlock]) -> Option<String> {
        // Strategy 1: explicitly classified title block.
        if let Some(title_block) = blocks
            .iter()
            .find(|b| b.block_type == BlockType::Title && !b.text.trim().is_empty())
        {
            return Some(title_block.text.trim().to_string());
        }

        // Strategy 2: largest-font block in the top 40% of page 0 with ≤ 3 lines.
        // We need the page height to compute the 40% threshold.  We approximate it
        // as the maximum `bbox.top` value seen on page 0, which should be near the
        // top of the page.
        let page0_blocks: Vec<&TextBlock> = blocks.iter().filter(|b| b.page == 0).collect();
        if page0_blocks.is_empty() {
            return None;
        }

        // Approximate page top (highest Y coordinate on page 0).
        let page_top = page0_blocks
            .iter()
            .map(|b| b.bbox.top)
            .fold(f64::NEG_INFINITY, f64::max);
        let page_bottom = page0_blocks
            .iter()
            .map(|b| b.bbox.bottom)
            .fold(f64::INFINITY, f64::min);
        let page_height = page_top - page_bottom;
        // Top 40% threshold: blocks whose top edge is above this Y value.
        let top_40_threshold = page_bottom + page_height * 0.60;

        page0_blocks
            .iter()
            .filter(|b| {
                // Must be in the top 40% of the page.
                b.bbox.top >= top_40_threshold
                // Must have ≤ 3 lines.
                && b.lines.len() <= 3
                // Exclude obvious non-title types.
                && !matches!(
                    b.block_type,
                    BlockType::Caption
                        | BlockType::PageNumber
                        | BlockType::RunningHeader
                        | BlockType::RunningFooter
                        | BlockType::Footnote
                        | BlockType::Reference
                )
                && !b.text.trim().is_empty()
            })
            .max_by(|a, b| {
                a.primary_font_size()
                    .partial_cmp(&b.primary_font_size())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|b| b.text.trim().to_string())
    }

    /// Find author names from text blocks below the title on page 0.
    ///
    /// Strategy:
    /// 1. Locate the title block's bottom Y position on page 0.
    /// 2. Look for blocks on page 0 that are:
    ///    - Below the title (within 50 pts).
    ///    - Not a section header.
    ///    - Smaller font than the title but larger than typical body text.
    /// 3. Split author text by commas, "and", and newlines.
    fn extract_authors(blocks: &[TextBlock], title: &Option<String>) -> Vec<String> {
        // Determine title bottom Y on page 0.
        let title_text = match title.as_deref() {
            Some(t) => t,
            None => return Vec::new(),
        };

        let title_block = blocks
            .iter()
            .find(|b| b.page == 0 && b.text.trim() == title_text.trim());

        let title_bottom = match title_block {
            Some(b) => b.bbox.bottom,
            None => return Vec::new(),
        };
        let title_font_size = title_block.map(|b| b.primary_font_size()).unwrap_or(0.0);

        // Collect candidate blocks: on page 0, below title within 50 pts.
        let mut candidates: Vec<&TextBlock> = blocks
            .iter()
            .filter(|b| {
                b.page == 0
                    && b.bbox.top < title_bottom        // below title (Y-up: smaller top = lower)
                    && (title_bottom - b.bbox.top) <= 50.0  // within 50 pts
                    && !matches!(
                        b.block_type,
                        BlockType::SectionHeader
                            | BlockType::SubsectionHeader
                            | BlockType::Caption
                            | BlockType::PageNumber
                            | BlockType::RunningHeader
                            | BlockType::RunningFooter
                            | BlockType::Footnote
                    )
                    // Must be strictly smaller font than title (avoid picking the title itself).
                    && b.primary_font_size() < title_font_size * 0.99
                    && !b.text.trim().is_empty()
            })
            .collect();

        // Sort by descending Y (closest to title first).
        candidates.sort_by(|a, b| {
            b.bbox
                .top
                .partial_cmp(&a.bbox.top)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if candidates.is_empty() {
            return Vec::new();
        }

        // Use the first (closest) candidate block as the author line.
        let author_text = candidates[0].text.trim();

        Self::split_authors(author_text)
    }

    /// Split a raw author string into individual author names.
    fn split_authors(raw: &str) -> Vec<String> {
        // Replace " and " / " AND " with comma.
        let normalised = regex_replace_all(r"(?i)\band\b", raw, ",");
        // Split by commas or newlines, trim each part.
        normalised
            .split([',', '\n'])
            .map(str::trim)
            .filter(|s| !s.is_empty() && s.len() > 1)
            .map(String::from)
            .collect()
    }

    /// Search all blocks for a DOI pattern.
    ///
    /// Recognizes:
    /// - Bare DOI: `10.XXXX/...`
    /// - Prefixed: `DOI: 10.XXXX/...`, `doi:10.XXXX/...`, `Doi: 10.XXXX/...` (case-insensitive)
    /// - URL-format: `https://doi.org/10.XXXX/...` or `http://doi.org/10.XXXX/...`
    ///   The regex matches from `10.` onward so the URL prefix is automatically excluded.
    /// - DOI spanning line breaks (whitespace is normalized before matching)
    ///
    /// When multiple DOI-bearing blocks exist the first one found is returned.
    fn extract_doi(blocks: &[TextBlock]) -> Option<String> {
        // Matches the canonical DOI prefix (10.NNNN/) followed by the suffix.
        // Because the pattern starts at "10." it will naturally skip any
        // preceding "https://doi.org/" URL prefix.
        let doi_re = Regex::new(r"10\.\d{4,9}/[-._;()/:A-Za-z0-9]+").expect("valid DOI regex");

        for block in blocks {
            // Collapse whitespace (including newlines) so that a DOI that was
            // split across a line break in the source PDF is reunited into one
            // token before we apply the regex.
            let normalized = block.text.split_whitespace().collect::<Vec<_>>().join(" ");

            if let Some(m) = doi_re.find(&normalized) {
                // Trim trailing punctuation that may have been absorbed by the
                // character class (e.g. a trailing period at end of sentence).
                let doi = m.as_str().trim_end_matches('.');
                return Some(doi.to_string());
            }
        }
        None
    }
}

/// Helper: replace all regex matches in `text` with `replacement`.
fn regex_replace_all(pattern: &str, text: &str, replacement: &str) -> String {
    let re = Regex::new(pattern).expect("valid regex");
    re.replace_all(text, replacement).into_owned()
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BlockType, Rect, TextBlock, TextLine, TextSpan};

    /// Construct a minimal `TextBlock` for testing.
    fn make_block(
        text: &str,
        page: u32,
        font_size: f64,
        top: f64,
        bottom: f64,
        block_type: BlockType,
    ) -> TextBlock {
        let span = TextSpan {
            text: text.to_string(),
            font_name: "TestFont".to_string(),
            font_size,
            is_bold: false,
            is_italic: false,
            bbox: Rect::new(50.0, top, 550.0, bottom),
            page,
        };
        let line = TextLine {
            spans: vec![span],
            text: text.to_string(),
            bbox: Rect::new(50.0, top, 550.0, bottom),
            page,
            baseline_y: bottom,
            primary_font_size: font_size,
            primary_font_name: "TestFont".to_string(),
            is_bold: false,
        };
        TextBlock {
            global_index: 0,
            lines: vec![line],
            text: text.to_string(),
            bbox: Rect::new(50.0, top, 550.0, bottom),
            page,
            column_index: 0,
            block_type,
        }
    }

    // ── Title tests ──────────────────────────────────────────────────────────

    #[test]
    fn test_extract_title_from_title_block() {
        let blocks = vec![
            make_block(
                "Deep Learning for NLP",
                0,
                18.0,
                720.0,
                700.0,
                BlockType::Title,
            ),
            make_block(
                "Abstract goes here.",
                0,
                10.0,
                650.0,
                640.0,
                BlockType::Abstract,
            ),
        ];
        let title = MetadataExtractor::extract_title(&blocks);
        assert_eq!(title, Some("Deep Learning for NLP".to_string()));
    }

    #[test]
    fn test_extract_title_by_font_size_fallback() {
        // No explicitly classified Title block — should pick the largest-font block
        // in the top 40% of the page.
        let blocks = vec![
            // Largest font, near top of page → should be picked as title.
            make_block(
                "Large Title Text",
                0,
                20.0,
                730.0,
                710.0,
                BlockType::BodyText,
            ),
            // Smaller font, also near top.
            make_block("Author Names", 0, 12.0, 690.0, 678.0, BlockType::BodyText),
            // Body text lower on the page.
            make_block(
                "Body paragraph.",
                0,
                10.0,
                400.0,
                390.0,
                BlockType::BodyText,
            ),
        ];
        let title = MetadataExtractor::extract_title(&blocks);
        assert_eq!(title, Some("Large Title Text".to_string()));
    }

    // ── Author tests ─────────────────────────────────────────────────────────

    #[test]
    fn test_extract_authors_split_by_comma() {
        let title = Some("A Study of Things".to_string());
        let blocks = vec![
            make_block("A Study of Things", 0, 18.0, 720.0, 700.0, BlockType::Title),
            // Author block directly below the title.
            make_block(
                "Alice Smith, Bob Jones, Carol Lee",
                0,
                12.0,
                698.0, // top < title_bottom (700) → below title
                686.0,
                BlockType::BodyText,
            ),
        ];
        let authors = MetadataExtractor::extract_authors(&blocks, &title);
        assert_eq!(authors, vec!["Alice Smith", "Bob Jones", "Carol Lee"]);
    }

    #[test]
    fn test_extract_authors_split_by_and() {
        let title = Some("Paper Title".to_string());
        let blocks = vec![
            make_block("Paper Title", 0, 18.0, 720.0, 700.0, BlockType::Title),
            make_block(
                "John Doe and Jane Roe",
                0,
                12.0,
                698.0,
                686.0,
                BlockType::BodyText,
            ),
        ];
        let authors = MetadataExtractor::extract_authors(&blocks, &title);
        assert_eq!(authors, vec!["John Doe", "Jane Roe"]);
    }

    // ── DOI tests ────────────────────────────────────────────────────────────

    #[test]
    fn test_extract_doi_pattern() {
        let blocks = vec![make_block(
            "DOI: 10.1109/TPAMI.2020.123456",
            0,
            10.0,
            500.0,
            490.0,
            BlockType::BodyText,
        )];
        let doi = MetadataExtractor::extract_doi(&blocks);
        assert_eq!(doi, Some("10.1109/TPAMI.2020.123456".to_string()));
    }

    #[test]
    fn test_no_doi_returns_none() {
        let blocks = vec![make_block(
            "This paper has no DOI reference.",
            0,
            10.0,
            500.0,
            490.0,
            BlockType::BodyText,
        )];
        let doi = MetadataExtractor::extract_doi(&blocks);
        assert_eq!(doi, None);
    }

    #[test]
    fn test_doi_from_url_format() {
        // A block containing a full doi.org URL.
        // The extractor should return only the DOI string without the URL prefix.
        let blocks = vec![make_block(
            "https://doi.org/10.1109/ACCESS.2020.1234567",
            0,
            10.0,
            500.0,
            490.0,
            BlockType::BodyText,
        )];
        let doi = MetadataExtractor::extract_doi(&blocks);
        assert_eq!(doi, Some("10.1109/ACCESS.2020.1234567".to_string()));
    }

    #[test]
    fn test_doi_case_insensitive_prefix() {
        // "Doi:" (mixed case) prefix — the numeric DOI must still be extracted.
        let blocks = vec![make_block(
            "Doi: 10.1234/test.2020",
            0,
            10.0,
            500.0,
            490.0,
            BlockType::BodyText,
        )];
        let doi = MetadataExtractor::extract_doi(&blocks);
        assert_eq!(doi, Some("10.1234/test.2020".to_string()));
    }

    #[test]
    fn test_doi_whitespace_across_line_break() {
        // Simulate a DOI that was split by a line break in the extracted text.
        // After whitespace normalisation the regex should still find it.
        let blocks = vec![make_block(
            "DOI:\n10.1145/1234567.7654321",
            0,
            10.0,
            500.0,
            490.0,
            BlockType::BodyText,
        )];
        let doi = MetadataExtractor::extract_doi(&blocks);
        assert_eq!(doi, Some("10.1145/1234567.7654321".to_string()));
    }

    #[test]
    fn test_doi_first_occurrence_returned() {
        // When multiple blocks contain DOIs, the first block's DOI wins.
        let blocks = vec![
            make_block(
                "doi:10.1000/first.doi",
                0,
                10.0,
                600.0,
                590.0,
                BlockType::BodyText,
            ),
            make_block(
                "doi:10.2000/second.doi",
                0,
                10.0,
                500.0,
                490.0,
                BlockType::BodyText,
            ),
        ];
        let doi = MetadataExtractor::extract_doi(&blocks);
        assert_eq!(doi, Some("10.1000/first.doi".to_string()));
    }

    #[test]
    fn test_extract_full_metadata() {
        let blocks = vec![
            make_block(
                "Transformer Networks for Vision",
                0,
                18.0,
                740.0,
                720.0,
                BlockType::Title,
            ),
            make_block(
                "Alice Smith, Bob Jones",
                0,
                12.0,
                718.0,
                706.0,
                BlockType::BodyText,
            ),
            make_block(
                "doi:10.1145/12345.67890",
                0,
                10.0,
                680.0,
                670.0,
                BlockType::BodyText,
            ),
        ];
        let meta = MetadataExtractor::extract(&blocks, 12);
        assert_eq!(
            meta.title,
            Some("Transformer Networks for Vision".to_string())
        );
        assert_eq!(meta.authors, vec!["Alice Smith", "Bob Jones"]);
        assert_eq!(meta.doi, Some("10.1145/12345.67890".to_string()));
        assert_eq!(meta.pages, 12);
    }
}
