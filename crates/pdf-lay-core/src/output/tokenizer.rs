//! Pluggable token counting for chunk sizing.
//!
//! Before this module existed, [`crate::output::Chunker`] counted tokens with
//! a hardcoded static function (`ascii/4 + non_ascii/1.5`) and a hardcoded
//! `max_tokens * 4` character budget in `chunk_by_tokens`. Both assumed an
//! ASCII chars-per-token ratio; for CJK text (where a BPE tokenizer spends
//! close to one token per character, not 1.5+) this systematically
//! **underestimated** token counts and let chunks overshoot `max_tokens` by a
//! wide margin. [`Tokenizer`] makes token counting pluggable — the default
//! [`HeuristicTokenizer`] fixes the CJK underestimate, and a real BPE
//! tokenizer can be plugged in via [`Chunker::with_tokenizer`]
//! (`crate::output::Chunker`), gated behind the `real-tokenizer` cargo
//! feature (see [`HfTokenizer`] below) so the default build stays
//! dependency-light.

/// Counts the number of tokens a piece of text would occupy in an LLM's
/// context window.
///
/// Implementations must be deterministic for a given input (repeated calls
/// with the same text return the same count) so chunk-budget calculations
/// stay stable. `Send + Sync` so a `Box<dyn Tokenizer>` can be shared across
/// threads (e.g. behind `rayon`-parallel section chunking).
pub trait Tokenizer: Send + Sync {
    /// Return the estimated (or, for a real tokenizer, exact) number of
    /// tokens `text` would occupy.
    fn count(&self, text: &str) -> usize;
}

/// Default [`Tokenizer`]: a character-class heuristic requiring no model
/// download.
///
/// Classifies each character into one of three buckets and divides by a
/// fixed chars-per-token ratio per bucket:
///
/// - **ASCII** (`char::is_ascii`): [`Self::ASCII_CHARS_PER_TOKEN`] chars/token
///   — the legacy `ascii/4` estimate, kept unchanged (English/code text is
///   reasonably well approximated by GPT/BPE-style tokenizers at ~4
///   chars/token).
/// - **CJK** ([`is_cjk`]: kana, CJK unified ideographs, hangul syllables,
///   halfwidth/fullwidth forms): [`Self::CJK_CHARS_PER_TOKEN`] char/token —
///   real BPE tokenizers spend close to one token per CJK character (CJK
///   script does not sub-word-tokenize the way space-delimited ASCII text
///   does). The legacy heuristic folded CJK into a blanket
///   "non-ASCII/1.5 chars/token" bucket, which underestimated CJK token
///   counts by roughly 33% and, combined with the old `max_tokens * 4` char
///   budget in `chunk_by_tokens`, let CJK chunks overshoot the configured
///   token budget.
/// - **Other non-ASCII** (accented Latin, symbols, emoji, etc.):
///   [`Self::OTHER_NON_ASCII_CHARS_PER_TOKEN`] chars/token — between the two
///   above.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeuristicTokenizer;

impl HeuristicTokenizer {
    /// Chars per token for ASCII text.
    const ASCII_CHARS_PER_TOKEN: usize = 4;
    /// Chars per token for CJK script. `1` (not the legacy `1.5`) so CJK
    /// token counts are not underestimated.
    const CJK_CHARS_PER_TOKEN: usize = 1;
    /// Chars per token for non-ASCII, non-CJK text.
    const OTHER_NON_ASCII_CHARS_PER_TOKEN: usize = 2;
}

impl Tokenizer for HeuristicTokenizer {
    fn count(&self, text: &str) -> usize {
        let mut ascii_chars = 0usize;
        let mut cjk_chars = 0usize;
        let mut other_chars = 0usize;

        for c in text.chars() {
            if c.is_ascii() {
                ascii_chars += 1;
            } else if is_cjk(c) {
                cjk_chars += 1;
            } else {
                other_chars += 1;
            }
        }

        ascii_chars / Self::ASCII_CHARS_PER_TOKEN
            + cjk_chars / Self::CJK_CHARS_PER_TOKEN
            + other_chars / Self::OTHER_NON_ASCII_CHARS_PER_TOKEN
    }
}

/// Whether `c` falls in a CJK Unicode block: Hiragana/Katakana, CJK Unified
/// Ideographs (and Extension A), Hangul Syllables, or Halfwidth/Fullwidth
/// Forms. Used by [`HeuristicTokenizer`] to avoid folding CJK script into the
/// same bucket as other non-ASCII text.
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF   // Hiragana, Katakana
        | 0x3400..=0x4DBF // CJK Unified Ideographs Extension A
        | 0x4E00..=0x9FFF // CJK Unified Ideographs
        | 0xAC00..=0xD7AF // Hangul Syllables
        | 0xFF00..=0xFFEF // Halfwidth and Fullwidth Forms
    )
}

/// Wraps a real BPE tokenizer loaded via the [`tokenizers`] crate.
///
/// Behind the `real-tokenizer` cargo feature (opt-in): the default build does
/// not pull in `tokenizers` or its transitive dependencies. Plug an instance
/// in via `Chunker::with_tokenizer` (`crate::output::Chunker`). The CLI's
/// `--tokenizer <model-or-path>` flag that loads one of these is wired up in
/// a later task; this type is the library-side seam it will use.
#[cfg(feature = "real-tokenizer")]
pub struct HfTokenizer(tokenizers::Tokenizer);

#[cfg(feature = "real-tokenizer")]
impl HfTokenizer {
    /// Load a tokenizer from a local `tokenizer.json` file.
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self, String> {
        tokenizers::Tokenizer::from_file(path.as_ref())
            .map(Self)
            .map_err(|e| e.to_string())
    }

    /// Load a tokenizer from a Hugging Face Hub model id (e.g.
    /// `"Qwen/Qwen2.5-7B"`). Requires network access at load time.
    pub fn from_pretrained(model_id: &str) -> Result<Self, String> {
        tokenizers::Tokenizer::from_pretrained(model_id, None)
            .map(Self)
            .map_err(|e| e.to_string())
    }
}

#[cfg(feature = "real-tokenizer")]
impl Tokenizer for HfTokenizer {
    fn count(&self, text: &str) -> usize {
        // No Silent Drop is a text-fidelity concern, not applicable to a
        // count; an encode failure falls back to 0 rather than panicking on
        // user-derived PDF text.
        self.0
            .encode(text, false)
            .map(|encoding| encoding.get_ids().len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heuristic_ascii_matches_legacy() {
        // 100 ASCII chars / 4 = 25 tokens (unchanged from the legacy static
        // `estimate_tokens`).
        let text = "a".repeat(100);
        assert_eq!(HeuristicTokenizer.count(&text), 25);
    }

    #[test]
    fn heuristic_cjk_not_underestimated() {
        // 100 CJK chars: 1 char = 1 token, not the legacy 1.5 chars/token
        // (which would have given 66).
        let text = "あ".repeat(100);
        assert_eq!(HeuristicTokenizer.count(&text), 100);
    }

    #[test]
    fn heuristic_mixed() {
        // 40 ASCII (10 tokens) + 20 CJK (20 tokens) + 10 other non-ASCII
        // accented Latin (5 tokens) = 35 tokens.
        let text = format!("{}{}{}", "a".repeat(40), "漢".repeat(20), "é".repeat(10));
        assert_eq!(HeuristicTokenizer.count(&text), 35);
    }

    #[test]
    fn heuristic_is_deterministic() {
        let text = "Mixed 漢字 and ASCII text with émphasis.";
        assert_eq!(
            HeuristicTokenizer.count(text),
            HeuristicTokenizer.count(text)
        );
    }

    #[test]
    fn heuristic_empty_text_is_zero() {
        assert_eq!(HeuristicTokenizer.count(""), 0);
    }
}
