//! Math expression conversion (LaTeX/Unicode/PlainText).

use crate::config::{MathConfig, MathRepresentationPreference};
use crate::math::symbol_map::{to_latex_map, to_unicode_map};
use crate::types::TextSpan;

/// Converts math spans to different text representations.
pub struct MathConverter {
    config: MathConfig,
}

impl MathConverter {
    /// Create a new `MathConverter` with the given configuration.
    pub fn new(config: MathConfig) -> Self {
        Self { config }
    }

    /// Convert a collection of math spans to LaTeX representation.
    ///
    /// Superscript spans produce `^{...}`, subscript spans produce `_{...}`,
    /// and known symbol characters are replaced with their LaTeX commands.
    pub fn to_latex(&self, _text: &str, spans: &[TextSpan]) -> String {
        let latex_map = to_latex_map();
        let mut result = String::new();

        let base_size = self.detect_base_font_size(spans);

        for span in spans {
            let is_super = self.is_superscript(span, base_size, spans);
            let is_sub = self.is_subscript(span, base_size, spans);

            let converted: String = span
                .text
                .chars()
                .map(|c| {
                    if let Some(latex) = latex_map.get(&c) {
                        format!("{} ", latex)
                    } else {
                        c.to_string()
                    }
                })
                .collect();

            let converted = converted.trim().to_string();

            if is_super {
                result.push_str(&format!("^{{{converted}}}"));
            } else if is_sub {
                result.push_str(&format!("_{{{converted}}}"));
            } else {
                if !result.is_empty() && !converted.is_empty() {
                    // Add space between tokens only when both sides are alphanumeric
                    let last = result.chars().last().unwrap_or(' ');
                    let first = converted.chars().next().unwrap_or(' ');
                    if last.is_alphanumeric() && first.is_alphanumeric() {
                        result.push(' ');
                    }
                }
                result.push_str(&converted);
            }
        }

        result
    }

    /// Convert a collection of math spans to Unicode representation.
    ///
    /// Superscript digits/letters are mapped to their Unicode superscript equivalents
    /// (e.g. `2` → `²`), and subscript digits are mapped similarly.
    pub fn to_unicode(&self, _text: &str, spans: &[TextSpan]) -> String {
        let (sup_map, sub_map) = to_unicode_map();
        let mut result = String::new();

        let base_size = self.detect_base_font_size(spans);

        for span in spans {
            let is_super = self.is_superscript(span, base_size, spans);
            let is_sub = self.is_subscript(span, base_size, spans);

            for c in span.text.chars() {
                if is_super {
                    result.push(*sup_map.get(&c).unwrap_or(&c));
                } else if is_sub {
                    result.push(*sub_map.get(&c).unwrap_or(&c));
                } else {
                    result.push(c);
                }
            }
        }

        result
    }

    /// Convert a collection of math spans to plain ASCII approximation.
    ///
    /// Known symbols are rendered as their command name without the backslash
    /// (e.g. `α` → `alpha`). Superscripts become `^(...)`, subscripts `_(...)`.
    pub fn to_plain(&self, _text: &str, spans: &[TextSpan]) -> String {
        let latex_map = to_latex_map();
        let mut result = String::new();

        let base_size = self.detect_base_font_size(spans);

        for span in spans {
            let is_super = self.is_superscript(span, base_size, spans);
            let is_sub = self.is_subscript(span, base_size, spans);

            let converted: String = span
                .text
                .chars()
                .map(|c| {
                    if let Some(latex) = latex_map.get(&c) {
                        // Strip the leading backslash for plain text
                        latex.trim_start_matches('\\').to_string()
                    } else {
                        c.to_string()
                    }
                })
                .collect();

            if is_super {
                result.push_str(&format!("^({converted})"));
            } else if is_sub {
                result.push_str(&format!("_({converted})"));
            } else {
                result.push_str(&converted);
            }
        }

        result
    }

    /// Auto-select conversion: CM fonts → LaTeX, other fonts → Unicode.
    pub fn auto_convert(&self, text: &str, spans: &[TextSpan]) -> String {
        let has_cm = spans.iter().any(|s| {
            let name = s.font_name.to_uppercase();
            // Match "CM" followed by at least one uppercase letter (e.g. CMMI10, CMR10, CMBX12)
            name.starts_with("CM")
                && name.len() >= 3
                && name[2..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_uppercase())
        });

        if has_cm {
            self.to_latex(text, spans)
        } else {
            self.to_unicode(text, spans)
        }
    }

    /// Convert spans based on the configured representation preference.
    pub fn convert(&self, text: &str, spans: &[TextSpan]) -> String {
        match self.config.representation {
            MathRepresentationPreference::LaTeX => self.to_latex(text, spans),
            MathRepresentationPreference::UnicodeMath => self.to_unicode(text, spans),
            MathRepresentationPreference::PlainText => self.to_plain(text, spans),
            MathRepresentationPreference::Auto => self.auto_convert(text, spans),
        }
    }

    /// Detect the base (largest) font size in the span set.
    ///
    /// Returns 10.0 as a fallback when the span slice is empty.
    fn detect_base_font_size(&self, spans: &[TextSpan]) -> f64 {
        if spans.is_empty() {
            return 10.0;
        }
        spans
            .iter()
            .map(|s| s.font_size)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(10.0)
    }

    /// Return `true` if `span` is positioned as a superscript relative to normal-sized spans.
    ///
    /// A span is considered superscript when:
    /// - Its font size is less than 85% of `base_size`, AND
    /// - Its bottom Y-coordinate is above the baseline of normal-sized spans by more than
    ///   `base_size * superscript_y_threshold`.
    fn is_superscript(&self, span: &TextSpan, base_size: f64, all_spans: &[TextSpan]) -> bool {
        if span.font_size >= base_size * 0.85 {
            return false;
        }
        // Find the minimum bottom Y among normal-sized spans (PDF Y-up: min bottom = lowest)
        let base_bottom = all_spans
            .iter()
            .filter(|s| s.font_size >= base_size * 0.85)
            .map(|s| s.bbox.bottom)
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(span.bbox.bottom);

        // In Y-up coordinates: superscript bottom is ABOVE baseline → higher Y value
        let offset = span.bbox.bottom - base_bottom;
        offset > base_size * self.config.superscript_y_threshold
    }

    /// Return `true` if `span` is positioned as a subscript relative to normal-sized spans.
    ///
    /// A span is considered subscript when:
    /// - Its font size is less than 85% of `base_size`, AND
    /// - Its bottom Y-coordinate is below the baseline of normal-sized spans by more than
    ///   `base_size * superscript_y_threshold`.
    fn is_subscript(&self, span: &TextSpan, base_size: f64, all_spans: &[TextSpan]) -> bool {
        if span.font_size >= base_size * 0.85 {
            return false;
        }
        let base_bottom = all_spans
            .iter()
            .filter(|s| s.font_size >= base_size * 0.85)
            .map(|s| s.bbox.bottom)
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(span.bbox.bottom);

        // In Y-up coordinates: subscript bottom is BELOW baseline → lower (more negative) Y value
        let offset = span.bbox.bottom - base_bottom;
        offset < -(base_size * self.config.superscript_y_threshold)
    }
}

/// Formats converted math expressions for document output (Markdown / LLM text).
pub struct MathFormatter;

impl MathFormatter {
    /// Format a converted math string for Markdown output.
    ///
    /// Inline math is wrapped with the configured inline delimiters (default `$...$`).
    /// Display math is wrapped with the configured display delimiters (default `$$\n...\n$$`),
    /// and an optional equation number is appended as a `\tag{N}`.
    pub fn format_for_markdown(
        converted_text: &str,
        is_display: bool,
        equation_number: Option<&str>,
        config: &MathConfig,
    ) -> String {
        if is_display {
            let tag = equation_number
                .map(|n| {
                    // Strip surrounding parentheses from the equation number if present
                    let clean = n.trim_matches(|c| c == '(' || c == ')');
                    format!(" \\tag{{{clean}}}")
                })
                .unwrap_or_default();
            format!(
                "{}{}{}{}",
                config.display_delimiter.0, converted_text, tag, config.display_delimiter.1
            )
        } else {
            format!(
                "{}{}{}",
                config.inline_delimiter.0, converted_text, config.inline_delimiter.1
            )
        }
    }

    /// Format a converted math string for LLM text output.
    ///
    /// Currently identical to [`format_for_markdown`][Self::format_for_markdown].
    pub fn format_for_llm(
        converted_text: &str,
        is_display: bool,
        equation_number: Option<&str>,
        config: &MathConfig,
    ) -> String {
        Self::format_for_markdown(converted_text, is_display, equation_number, config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::make_math_span;
    use crate::types::Rect;

    fn default_converter() -> MathConverter {
        MathConverter::new(MathConfig::default())
    }

    #[test]
    fn test_greek_to_latex() {
        let c = default_converter();
        let span = make_math_span("α", "CMMI10", 0.0, 100.0, 10.0);
        let result = c.to_latex("α", &[span]);
        assert!(
            result.contains("\\alpha"),
            "expected \\alpha, got: {result}"
        );
    }

    #[test]
    fn test_superscript_to_latex() {
        let c = default_converter();
        // Base span: font_size=12, bottom = top - font_size = 100 - 12 = 88
        let base_span = TextSpan {
            text: "x".to_string(),
            font_name: "CMMI10".to_string(),
            font_size: 12.0,
            is_bold: false,
            is_italic: true,
            bbox: Rect {
                left: 0.0,
                top: 100.0,
                right: 10.0,
                bottom: 88.0,
            },
            page: 0,
        };
        // Superscript: smaller font (8pt < 12*0.85=10.2), bottom higher than base_bottom (88)
        // Need offset > 12 * 0.3 = 3.6, so bottom > 88 + 3.6 = 91.6
        // Set bottom = 94.0 → offset = 94 - 88 = 6.0 > 3.6 → superscript
        let sup = TextSpan {
            text: "2".to_string(),
            font_name: "CMMI10".to_string(),
            font_size: 8.0,
            is_bold: false,
            is_italic: true,
            bbox: Rect {
                left: 10.0,
                top: 102.0,
                right: 15.0,
                bottom: 94.0,
            },
            page: 0,
        };
        let result = c.to_latex("x2", &[base_span, sup]);
        assert!(result.contains("^{2}"), "expected ^{{2}}, got: {result}");
    }

    #[test]
    fn test_subscript_to_latex() {
        let c = default_converter();
        // Base span: bottom = 88
        let base = TextSpan {
            text: "x".to_string(),
            font_name: "CMMI10".to_string(),
            font_size: 12.0,
            is_bold: false,
            is_italic: true,
            bbox: Rect {
                left: 0.0,
                top: 100.0,
                right: 10.0,
                bottom: 88.0,
            },
            page: 0,
        };
        // Subscript: smaller font, bottom BELOW base_bottom (88)
        // Need offset < -(12 * 0.3) = -3.6, so bottom < 88 - 3.6 = 84.4
        // Set bottom = 82.0 → offset = 82 - 88 = -6.0 < -3.6 → subscript
        let sub = TextSpan {
            text: "i".to_string(),
            font_name: "CMMI10".to_string(),
            font_size: 8.0,
            is_bold: false,
            is_italic: true,
            bbox: Rect {
                left: 10.0,
                top: 90.0,
                right: 15.0,
                bottom: 82.0,
            },
            page: 0,
        };
        let result = c.to_latex("xi", &[base, sub]);
        assert!(result.contains("_{i}"), "expected _{{i}}, got: {result}");
    }

    #[test]
    fn test_unicode_superscript() {
        let c = default_converter();
        let base = TextSpan {
            text: "x".to_string(),
            font_name: "Symbol".to_string(),
            font_size: 12.0,
            is_bold: false,
            is_italic: false,
            bbox: Rect {
                left: 0.0,
                top: 100.0,
                right: 10.0,
                bottom: 88.0,
            },
            page: 0,
        };
        // Superscript: bottom = 94.0 → offset = 94 - 88 = 6.0 > 3.6
        let sup = TextSpan {
            text: "2".to_string(),
            font_name: "Symbol".to_string(),
            font_size: 8.0,
            is_bold: false,
            is_italic: false,
            bbox: Rect {
                left: 10.0,
                top: 102.0,
                right: 15.0,
                bottom: 94.0,
            },
            page: 0,
        };
        let result = c.to_unicode("x2", &[base, sup]);
        assert!(result.contains('²'), "expected ², got: {result}");
    }

    #[test]
    fn test_plain_text_conversion() {
        let c = default_converter();
        let span = make_math_span("α", "CMMI10", 0.0, 100.0, 10.0);
        let result = c.to_plain("α", &[span]);
        assert!(result.contains("alpha"), "expected 'alpha', got: {result}");
    }

    #[test]
    fn test_auto_cm_uses_latex() {
        let c = default_converter();
        let span = make_math_span("α", "CMMI10", 0.0, 100.0, 10.0);
        let result = c.auto_convert("α", &[span]);
        assert!(
            result.contains("\\alpha"),
            "CM font should use LaTeX, got: {result}"
        );
    }

    #[test]
    fn test_auto_non_cm_uses_unicode() {
        let c = default_converter();
        let span = TextSpan {
            text: "2".to_string(),
            font_name: "Symbol".to_string(),
            font_size: 12.0,
            is_bold: false,
            is_italic: false,
            bbox: Rect {
                left: 0.0,
                top: 100.0,
                right: 10.0,
                bottom: 88.0,
            },
            page: 0,
        };
        let result = c.auto_convert("2", &[span]);
        // Non-CM should use Unicode; '2' with no super/sub context stays as '2'
        assert_eq!(result, "2", "Non-CM font should use Unicode, got: {result}");
    }

    #[test]
    fn test_markdown_inline_formatting() {
        let config = MathConfig::default();
        let result = MathFormatter::format_for_markdown("\\alpha", false, None, &config);
        assert_eq!(result, "$\\alpha$");
    }

    #[test]
    fn test_markdown_display_formatting() {
        let config = MathConfig::default();
        let result = MathFormatter::format_for_markdown("E = mc^{2}", true, Some("(1)"), &config);
        assert!(
            result.contains("$$"),
            "expected $$ delimiters, got: {result}"
        );
        assert!(
            result.contains("\\tag{1}"),
            "expected \\tag{{1}}, got: {result}"
        );
    }

    #[test]
    fn test_convert_respects_latex_preference() {
        let mut config = MathConfig::default();
        config.representation = MathRepresentationPreference::LaTeX;
        let c = MathConverter::new(config);
        let span = make_math_span("α", "Symbol", 0.0, 100.0, 10.0);
        let result = c.convert("α", &[span]);
        assert!(
            result.contains("\\alpha"),
            "LaTeX pref: expected \\alpha, got: {result}"
        );
    }

    #[test]
    fn test_convert_respects_unicode_preference() {
        let mut config = MathConfig::default();
        config.representation = MathRepresentationPreference::UnicodeMath;
        let c = MathConverter::new(config);
        // Plain '2' with no sub/super stays '2' in unicode mode
        let span = make_math_span("2", "Symbol", 0.0, 100.0, 10.0);
        let result = c.convert("2", &[span]);
        assert_eq!(result, "2");
    }

    #[test]
    fn test_detect_base_font_size_empty() {
        let c = default_converter();
        assert_eq!(c.detect_base_font_size(&[]), 10.0);
    }

    #[test]
    fn test_superscript_not_triggered_for_same_size() {
        let c = default_converter();
        let base = make_math_span("x", "CMMI10", 0.0, 100.0, 12.0);
        // Same size span — should NOT be superscript even if positioned slightly higher
        let same_size = TextSpan {
            text: "2".to_string(),
            font_name: "CMMI10".to_string(),
            font_size: 12.0,
            is_bold: false,
            is_italic: true,
            bbox: Rect {
                left: 10.0,
                top: 102.0,
                right: 18.0,
                bottom: 90.0,
            },
            page: 0,
        };
        let result = c.to_latex("x2", &[base, same_size]);
        assert!(
            !result.contains("^{"),
            "same-size span should not be superscript, got: {result}"
        );
    }
}
