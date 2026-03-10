//! Math region detection.
//!
//! Detects mathematical expressions in text spans based on font names
//! and symbol code points.

use std::collections::HashSet;

use crate::config::MathConfig;
use crate::math::symbol_map::math_symbols;
use crate::types::{Rect, TextLine, TextSpan};

/// Context of a detected math region.
#[derive(Debug, Clone, PartialEq)]
pub enum MathContext {
    /// Inline math (part of a text line).
    Inline,
    /// Display math (standalone centered equation).
    Display,
}

/// A detected math region within a text line.
#[derive(Debug, Clone)]
pub struct MathRegion {
    /// The spans that make up this math expression.
    pub spans: Vec<TextSpan>,
    /// Combined text of the math region.
    pub text: String,
    /// Inline or Display context.
    pub context: MathContext,
    /// Equation number if detected (e.g., "(1)").
    pub equation_number: Option<String>,
    /// Bounding box of the entire math region.
    pub bbox: Rect,
}

/// Detects math regions in text content.
pub struct MathDetector {
    config: MathConfig,
    math_chars: HashSet<char>,
}

impl MathDetector {
    /// Create a new `MathDetector` with the given configuration.
    pub fn new(config: MathConfig) -> Self {
        let math_chars = math_symbols();
        Self { config, math_chars }
    }

    /// Check if a single span is mathematical based on font name or content.
    pub fn is_math_span(&self, span: &TextSpan) -> bool {
        self.is_math_font(&span.font_name) || self.has_math_symbols(&span.text)
    }

    /// Check if font name matches known math font patterns.
    pub fn is_math_font(&self, font_name: &str) -> bool {
        let name = font_name.to_uppercase();

        // CM* fonts (Computer Modern): CMMI10, CMR12, CMSY10, etc.
        // Must start with "CM" followed by an uppercase letter.
        if name.len() >= 3
            && name.starts_with("CM")
            && name[2..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase())
        {
            return true;
        }

        // Math/Symbol/MT*/STIX patterns
        if name.contains("MATH")
            || name.contains("SYMBOL")
            || name.starts_with("MT")
            || name.contains("STIX")
        {
            return true;
        }

        // User-configured additional fonts
        for extra in &self.config.additional_math_fonts {
            if font_name.contains(extra.as_str()) || name.contains(extra.to_uppercase().as_str()) {
                return true;
            }
        }

        false
    }

    /// Check if text contains math symbol characters.
    pub fn has_math_symbols(&self, text: &str) -> bool {
        text.chars().any(|c| self.math_chars.contains(&c))
    }

    /// Detect inline math regions within a single [`TextLine`].
    ///
    /// Returns one [`MathRegion`] per contiguous run of math spans.
    /// If the entire line consists of math spans (and no other region was
    /// flushed before the final run), the region is classified as
    /// [`MathContext::Display`]; otherwise [`MathContext::Inline`].
    pub fn detect_in_line(&self, line: &TextLine) -> Vec<MathRegion> {
        let mut regions: Vec<MathRegion> = Vec::new();
        let mut current_math_spans: Vec<TextSpan> = Vec::new();

        for span in &line.spans {
            if self.is_math_span(span) {
                current_math_spans.push(span.clone());
            } else if !current_math_spans.is_empty() {
                regions.push(
                    self.build_region(std::mem::take(&mut current_math_spans), MathContext::Inline),
                );
            }
        }

        // Flush remaining math spans
        if !current_math_spans.is_empty() {
            // If no other region was emitted and every span in the line is
            // part of this run, the whole line is math → Display.
            let is_all_math = regions.is_empty() && current_math_spans.len() == line.spans.len();
            let context = if is_all_math {
                MathContext::Display
            } else {
                MathContext::Inline
            };
            regions.push(self.build_region(current_math_spans, context));
        }

        regions
    }

    /// Detect display equations: lines that are entirely math and centered on
    /// the page.
    ///
    /// A line qualifies when:
    /// - Every span in the line is classified as math.
    /// - The horizontal center of the line's bounding box falls within ±15 %
    ///   of the page width from the page centre.
    pub fn detect_display_equations(&self, lines: &[TextLine], page_width: f64) -> Vec<MathRegion> {
        let mut regions = Vec::new();

        for line in lines {
            if line.spans.is_empty() {
                continue;
            }

            // All spans must be math.
            if !line.spans.iter().all(|s| self.is_math_span(s)) {
                continue;
            }

            // Line centre must be close to the page centre.
            let center_x = line.bbox.center_x();
            let page_center = page_width / 2.0;
            let is_centered = (center_x - page_center).abs() < page_width * 0.15;

            if !is_centered {
                continue;
            }

            let spans: Vec<TextSpan> = line.spans.clone();
            let mut region = self.build_region(spans, MathContext::Display);

            // Look for an equation number in the last span or the full line text.
            let last_span_text = line.spans.last().map(|s| s.text.as_str()).unwrap_or("");
            if let Some(eq_num) = Self::extract_equation_number(last_span_text) {
                region.equation_number = Some(eq_num);
            } else {
                // Try the concatenated full-line text as a fallback.
                let full_text: String = line.spans.iter().map(|s| s.text.as_str()).collect();
                region.equation_number = Self::extract_equation_number(&full_text);
            }

            regions.push(region);
        }

        regions
    }

    /// Extract an equation number from text such as `"...text (1)"`,
    /// `"...text (2.3)"`, or `"...text (A.1)"`.
    ///
    /// Returns `Some("(N)")` when a valid number token is found at the end of
    /// the string, `None` otherwise.
    pub fn extract_equation_number(text: &str) -> Option<String> {
        let trimmed = text.trim();
        if trimmed.ends_with(')')
            && let Some(open) = trimmed.rfind('(')
        {
            let inside = &trimmed[open + 1..trimmed.len() - 1];
            // Validate: non-empty, ≤10 chars, only alphanumerics and dots.
            if !inside.is_empty()
                && inside.len() <= 10
                && inside.chars().all(|c| c.is_alphanumeric() || c == '.')
            {
                return Some(format!("({})", inside));
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn build_region(&self, spans: Vec<TextSpan>, context: MathContext) -> MathRegion {
        let text: String = spans
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("");
        let bbox = if spans.len() == 1 {
            spans[0].bbox.clone()
        } else {
            let mut b = spans[0].bbox.clone();
            for s in &spans[1..] {
                b = b.union(&s.bbox);
            }
            b
        };
        MathRegion {
            spans,
            text,
            context,
            equation_number: None,
            bbox,
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::make_math_span;

    fn default_detector() -> MathDetector {
        MathDetector::new(MathConfig::default())
    }

    // ---- P2-13: font / symbol detection -----------------------------------

    #[test]
    fn test_cm_font_is_math() {
        let d = default_detector();
        assert!(d.is_math_font("CMMI10"));
        assert!(d.is_math_font("CMR12"));
        assert!(d.is_math_font("CMSY10"));
    }

    #[test]
    fn test_regular_font_not_math() {
        let d = default_detector();
        assert!(!d.is_math_font("TimesNewRoman"));
        assert!(!d.is_math_font("Arial"));
        assert!(!d.is_math_font("Helvetica"));
    }

    #[test]
    fn test_math_symbol_detection() {
        let d = default_detector();
        assert!(d.has_math_symbols("α + β"));
        assert!(!d.has_math_symbols("hello world"));
    }

    #[test]
    fn test_additional_math_fonts() {
        let config = MathConfig {
            additional_math_fonts: vec!["MyCustomMath".to_string()],
            ..MathConfig::default()
        };
        let d = MathDetector::new(config);
        assert!(d.is_math_font("MyCustomMath-Regular"));
    }

    #[test]
    fn test_stix_font_is_math() {
        let d = default_detector();
        assert!(d.is_math_font("STIXMath-Regular"));
        assert!(d.is_math_font("STIXGeneral"));
    }

    #[test]
    fn test_math_span_detected_by_font() {
        let d = default_detector();
        let span = make_math_span("x", "CMMI10", 0.0, 100.0, 10.0);
        assert!(d.is_math_span(&span));
    }

    #[test]
    fn test_math_span_detected_by_symbols() {
        let d = default_detector();
        // Regular font but contains Greek letter
        let span = TextSpan {
            text: "α".to_string(),
            font_name: "TimesNewRoman".to_string(),
            font_size: 10.0,
            is_bold: false,
            is_italic: false,
            bbox: Rect::new(0.0, 100.0, 10.0, 90.0),
            page: 0,
        };
        assert!(d.is_math_span(&span));
    }

    // ---- P2-14: inline detection ------------------------------------------

    #[test]
    fn test_detect_inline_math_in_line() {
        let d = default_detector();
        let math_span = make_math_span("α", "CMMI10", 100.0, 100.0, 10.0);
        let text_span = TextSpan {
            text: "where ".to_string(),
            font_name: "TimesNewRoman".to_string(),
            font_size: 10.0,
            is_bold: false,
            is_italic: false,
            bbox: Rect::new(50.0, 100.0, 95.0, 90.0),
            page: 0,
        };
        let line = TextLine {
            spans: vec![text_span, math_span],
            text: "where α".to_string(),
            bbox: Rect::new(50.0, 100.0, 150.0, 90.0),
            baseline_y: 90.0,
            page: 0,
            primary_font_size: 10.0,
            primary_font_name: "TimesNewRoman".to_string(),
            is_bold: false,
        };
        let regions = d.detect_in_line(&line);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].context, MathContext::Inline);
    }

    #[test]
    fn test_all_math_line_is_display() {
        let d = default_detector();
        let s1 = make_math_span("α", "CMMI10", 50.0, 100.0, 10.0);
        let s2 = make_math_span("+", "CMSY10", 60.0, 100.0, 10.0);
        let s3 = make_math_span("β", "CMMI10", 70.0, 100.0, 10.0);
        let line = TextLine {
            spans: vec![s1, s2, s3],
            text: "α+β".to_string(),
            bbox: Rect::new(50.0, 100.0, 80.0, 90.0),
            baseline_y: 90.0,
            page: 0,
            primary_font_size: 10.0,
            primary_font_name: "CMMI10".to_string(),
            is_bold: false,
        };
        let regions = d.detect_in_line(&line);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].context, MathContext::Display);
    }

    #[test]
    fn test_multiple_inline_math_runs() {
        // Text | math | text | math  → two inline math regions
        let d = default_detector();
        let text1 = TextSpan {
            text: "Let ".to_string(),
            font_name: "Times".to_string(),
            font_size: 10.0,
            is_bold: false,
            is_italic: false,
            bbox: Rect::new(0.0, 100.0, 20.0, 90.0),
            page: 0,
        };
        let math1 = make_math_span("α", "CMMI10", 20.0, 100.0, 10.0);
        let text2 = TextSpan {
            text: " and ".to_string(),
            font_name: "Times".to_string(),
            font_size: 10.0,
            is_bold: false,
            is_italic: false,
            bbox: Rect::new(30.0, 100.0, 55.0, 90.0),
            page: 0,
        };
        let math2 = make_math_span("β", "CMMI10", 55.0, 100.0, 10.0);
        let line = TextLine {
            spans: vec![text1, math1, text2, math2],
            text: "Let α and β".to_string(),
            bbox: Rect::new(0.0, 100.0, 70.0, 90.0),
            baseline_y: 90.0,
            page: 0,
            primary_font_size: 10.0,
            primary_font_name: "Times".to_string(),
            is_bold: false,
        };
        let regions = d.detect_in_line(&line);
        assert_eq!(regions.len(), 2);
        assert!(regions.iter().all(|r| r.context == MathContext::Inline));
    }

    // ---- P2-15: display equation detection --------------------------------

    #[test]
    fn test_detect_display_equation_centered() {
        let d = default_detector();
        let s = make_math_span("E=mc²", "CMMI10", 240.0, 400.0, 12.0);
        let line = TextLine {
            spans: vec![s],
            text: "E=mc²".to_string(),
            bbox: Rect::new(240.0, 400.0, 340.0, 388.0),
            baseline_y: 388.0,
            page: 0,
            primary_font_size: 12.0,
            primary_font_name: "CMMI10".to_string(),
            is_bold: false,
        };
        // A4 width = 595 pt → centre = 297.5 pt; line centre = 290 pt → within 15%
        let regions = d.detect_display_equations(&[line], 595.0);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].context, MathContext::Display);
    }

    #[test]
    fn test_equation_number_extraction() {
        assert_eq!(
            MathDetector::extract_equation_number("...text (1)"),
            Some("(1)".to_string())
        );
        assert_eq!(
            MathDetector::extract_equation_number("...text (2.3)"),
            Some("(2.3)".to_string())
        );
        assert_eq!(
            MathDetector::extract_equation_number("...text (A.1)"),
            Some("(A.1)".to_string())
        );
        assert_eq!(MathDetector::extract_equation_number("no parens"), None);
        assert_eq!(MathDetector::extract_equation_number("empty ()"), None);
    }

    #[test]
    fn test_non_centered_math_line_not_display() {
        let d = default_detector();
        let s = make_math_span("x", "CMMI10", 10.0, 400.0, 12.0);
        let line = TextLine {
            spans: vec![s],
            text: "x".to_string(),
            bbox: Rect::new(10.0, 400.0, 30.0, 388.0),
            baseline_y: 388.0,
            page: 0,
            primary_font_size: 12.0,
            primary_font_name: "CMMI10".to_string(),
            is_bold: false,
        };
        // Centre = 20 pt; page centre = 297.5 pt → difference far exceeds 15%
        let regions = d.detect_display_equations(&[line], 595.0);
        assert!(regions.is_empty());
    }

    #[test]
    fn test_display_equations_skips_mixed_lines() {
        let d = default_detector();
        let math_span = make_math_span("α", "CMMI10", 270.0, 400.0, 12.0);
        let text_span = TextSpan {
            text: "where ".to_string(),
            font_name: "TimesNewRoman".to_string(),
            font_size: 10.0,
            is_bold: false,
            is_italic: false,
            bbox: Rect::new(200.0, 400.0, 265.0, 388.0),
            page: 0,
        };
        let line = TextLine {
            spans: vec![text_span, math_span],
            text: "where α".to_string(),
            bbox: Rect::new(200.0, 400.0, 290.0, 388.0),
            baseline_y: 388.0,
            page: 0,
            primary_font_size: 10.0,
            primary_font_name: "TimesNewRoman".to_string(),
            is_bold: false,
        };
        // Mixed line: not all spans are math → excluded even if centred
        let regions = d.detect_display_equations(&[line], 595.0);
        assert!(regions.is_empty());
    }

    #[test]
    fn test_display_equation_with_equation_number() {
        let d = default_detector();
        let eq_span = make_math_span("E = mc²", "CMMI10", 220.0, 400.0, 12.0);
        let num_span = make_math_span("(1)", "CMMI10", 530.0, 400.0, 12.0);
        let line = TextLine {
            spans: vec![eq_span, num_span],
            text: "E = mc²(1)".to_string(),
            bbox: Rect::new(220.0, 400.0, 545.0, 388.0),
            baseline_y: 388.0,
            page: 0,
            primary_font_size: 12.0,
            primary_font_name: "CMMI10".to_string(),
            is_bold: false,
        };
        let regions = d.detect_display_equations(&[line], 595.0);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].equation_number, Some("(1)".to_string()));
    }

    #[test]
    fn test_empty_line_skipped() {
        let d = default_detector();
        let line = TextLine {
            spans: vec![],
            text: String::new(),
            bbox: Rect::new(0.0, 100.0, 100.0, 90.0),
            baseline_y: 90.0,
            page: 0,
            primary_font_size: 10.0,
            primary_font_name: "Times".to_string(),
            is_bold: false,
        };
        let regions_inline = d.detect_in_line(&line);
        let regions_display = d.detect_display_equations(&[line], 595.0);
        assert!(regions_inline.is_empty());
        assert!(regions_display.is_empty());
    }
}
