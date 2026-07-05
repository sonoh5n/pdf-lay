//! Parses a section header's leading number into a structured [`NumberingKey`].
//!
//! Supports dot-optional Arabic (`1`, `2.1`, `3.1.1.2`), Roman numerals at any
//! level (`IV.`), and appendix / alphabetic labels (`Appendix A`, `A.`). The
//! compiled regexes are shared by both the header detector (for level = depth)
//! and hierarchy validation (for anomaly detection).

use regex::Regex;

use crate::types::{NumberComponent, NumberingKey};

/// Compiles the numbering regexes once and parses leading section numbers.
pub struct NumberingParser {
    re_appendix: Regex,
    re_arabic: Regex,
    re_roman: Regex,
    re_alpha: Regex,
}

impl Default for NumberingParser {
    fn default() -> Self {
        Self::new()
    }
}

impl NumberingParser {
    /// Create a parser with compiled patterns.
    pub fn new() -> Self {
        Self {
            // "Appendix A" / "Appendix B."
            re_appendix: Regex::new(r"^Appendix\s+([A-Z])\b\.?").unwrap(),
            // Dot-optional multi-level Arabic: "1", "2.1", "3.1.1.2".
            re_arabic: Regex::new(r"^(\d+(?:\.\d+)*)\.?(?:\s+|$)").unwrap(),
            // Roman numerals with a trailing period (avoids matching words like "I am").
            re_roman: Regex::new(r"^([IVXLCDM]+)\.(?:\s+|$)").unwrap(),
            // Single alphabetic label with a trailing period: "A. ".
            re_alpha: Regex::new(r"^([A-Z])\.(?:\s+|$)").unwrap(),
        }
    }

    /// Parse the leading number of `text`, returning the key and the byte length
    /// of the matched prefix (to strip for a clean header title). Returns `None`
    /// when there is no recognizable leading number.
    pub fn parse(&self, text: &str) -> Option<(NumberingKey, usize)> {
        let t = text.trim_start();
        let lead_ws = text.len() - t.len();

        // Appendix has the most specific prefix, so try it first.
        if let Some(caps) = self.re_appendix.captures(t) {
            let letter = caps.get(1)?.as_str().chars().next()?;
            let ord = (letter as u32) - ('A' as u32) + 1;
            let key = NumberingKey {
                components: vec![NumberComponent::Alpha(ord)],
                is_appendix: true,
            };
            return Some((key, lead_ws + caps.get(0)?.end()));
        }

        if let Some(caps) = self.re_arabic.captures(t) {
            let num_str = caps.get(1)?.as_str();
            let components: Vec<NumberComponent> = num_str
                .split('.')
                .filter_map(|p| p.parse::<u32>().ok())
                .map(NumberComponent::Arabic)
                .collect();
            if !components.is_empty() {
                let key = NumberingKey {
                    components,
                    is_appendix: false,
                };
                return Some((key, lead_ws + caps.get(0)?.end()));
            }
        }

        if let Some(caps) = self.re_roman.captures(t)
            && let Some(value) = roman_to_u32(caps.get(1)?.as_str())
        {
            let key = NumberingKey {
                components: vec![NumberComponent::Roman(value)],
                is_appendix: false,
            };
            return Some((key, lead_ws + caps.get(0)?.end()));
        }

        if let Some(caps) = self.re_alpha.captures(t) {
            let letter = caps.get(1)?.as_str().chars().next()?;
            let ord = (letter as u32) - ('A' as u32) + 1;
            let key = NumberingKey {
                components: vec![NumberComponent::Alpha(ord)],
                is_appendix: false,
            };
            return Some((key, lead_ws + caps.get(0)?.end()));
        }

        None
    }
}

/// Convert a Roman numeral string to its integer value, or `None` if invalid.
pub fn roman_to_u32(s: &str) -> Option<u32> {
    let mut total: u32 = 0;
    let mut prev: u32 = 0;
    for c in s.chars().rev() {
        let value = match c {
            'I' => 1,
            'V' => 5,
            'X' => 10,
            'L' => 50,
            'C' => 100,
            'D' => 500,
            'M' => 1000,
            _ => return None,
        };
        if value < prev {
            total -= value;
        } else {
            total += value;
            prev = value;
        }
    }
    if total == 0 { None } else { Some(total) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Option<NumberingKey> {
        NumberingParser::new().parse(text).map(|(k, _)| k)
    }

    #[test]
    fn parse_dot_optional_arabic() {
        let key = parse("1 Introduction").expect("should parse");
        assert_eq!(key.components, vec![NumberComponent::Arabic(1)]);
        assert_eq!(key.depth(), 1);
    }

    #[test]
    fn parse_deep_arabic_depth4() {
        let key = parse("3.1.1.2 Sampling").expect("should parse");
        assert_eq!(key.depth(), 4);
        assert_eq!(
            key.components,
            vec![
                NumberComponent::Arabic(3),
                NumberComponent::Arabic(1),
                NumberComponent::Arabic(1),
                NumberComponent::Arabic(2),
            ]
        );
    }

    #[test]
    fn parse_roman_any_level() {
        let key = parse("IV. Experiments").expect("should parse");
        assert_eq!(key.components, vec![NumberComponent::Roman(4)]);
    }

    #[test]
    fn parse_appendix_alpha() {
        let key = parse("Appendix A").expect("should parse");
        assert_eq!(key.components, vec![NumberComponent::Alpha(1)]);
        assert!(key.is_appendix);

        let key2 = parse("B. Extra Results").expect("should parse");
        assert_eq!(key2.components, vec![NumberComponent::Alpha(2)]);
        assert!(!key2.is_appendix);
    }

    #[test]
    fn roman_to_u32_roundtrip() {
        assert_eq!(roman_to_u32("I"), Some(1));
        assert_eq!(roman_to_u32("IV"), Some(4));
        assert_eq!(roman_to_u32("IX"), Some(9));
        assert_eq!(roman_to_u32("XLII"), Some(42));
        assert_eq!(roman_to_u32("MCMXCIV"), Some(1994));
        assert_eq!(roman_to_u32("Z"), None);
    }

    #[test]
    fn no_number_returns_none() {
        assert!(parse("Introduction").is_none());
        assert!(parse("Some regular sentence.").is_none());
    }

    #[test]
    fn prefix_len_strips_to_clean_title() {
        let (_, len) = NumberingParser::new().parse("2.1 Data Collection").unwrap();
        assert_eq!("2.1 Data Collection"[len..].trim(), "Data Collection");
    }
}
