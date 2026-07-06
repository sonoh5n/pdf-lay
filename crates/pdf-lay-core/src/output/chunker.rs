//! Splits a PaperDocument into Chunks for LLM consumption.

use crate::config::{ChunkConfig, FigureTextFormat, SplitStrategy};
use crate::output::render_core::{self, EscapeMode, RenderOptions};
use crate::output::tokenizer::{HeuristicTokenizer, Tokenizer};
use crate::types::{Chunk, FigureInfo, PaperDocument, Section, TableInfo};

/// One section's worth of pre-rendered chunk material, produced by
/// [`Chunker::flatten_render_units`].
///
/// `TokenCount` and `Paragraph` used to lose all section attribution because
/// they flattened every section into one giant string before splitting
/// (`section.full_text()`-style concatenation, pre-P2-1) or via an
/// `empty_section` placeholder (pre-P2-4). A `RenderUnit` keeps each
/// section's rich text bundled with the section name/breadcrumb prefix and
/// the figures/tables that belong to it, so every downstream chunk — even
/// one assembled by `TokenCount`/`Paragraph` — can carry that context
/// forward instead of dropping it (see `chunk_by_tokens`/`chunk_by_paragraph`
/// below).
struct RenderUnit {
    /// This section's own clean heading text (empty for headerless sections).
    section_name: String,
    /// The `[Context: ...]` breadcrumb + heading prefix for this section
    /// (empty when `include_section_context` is disabled or there is no
    /// heading path to show), computed the same way as
    /// `chunk_section_recursive`'s prefix for the `SectionBoundary` strategy.
    prefix: String,
    /// This section's own rendered body text (via `render_opts()`), not
    /// including child sections.
    text: String,
    /// Figures belonging to this section.
    figures: Vec<FigureInfo>,
    /// Tables belonging to this section.
    tables: Vec<TableInfo>,
    /// This section's own page range.
    page_range: (u32, u32),
}

/// Splits a [`PaperDocument`] into [`Chunk`] records for LLM consumption.
pub struct Chunker {
    /// The configuration controlling chunk sizes and split strategy.
    pub config: ChunkConfig,
    /// Counts tokens for chunk sizing, `estimated_tokens`, and overlap.
    ///
    /// Not part of `ChunkConfig`: a `Box<dyn Tokenizer>` is not `serde`
    /// representable, so it lives on `Chunker` itself and is supplied at
    /// construction time (`new` defaults to [`HeuristicTokenizer`];
    /// [`Chunker::with_tokenizer`] plugs in another implementation, e.g. a
    /// real BPE tokenizer behind the `real-tokenizer` feature).
    tokenizer: Box<dyn Tokenizer>,
}

impl Chunker {
    /// Create a new chunker with the given configuration and the default
    /// [`HeuristicTokenizer`] for token counting.
    pub fn new(config: ChunkConfig) -> Self {
        Self {
            config,
            tokenizer: Box::new(HeuristicTokenizer),
        }
    }

    /// Create a new chunker with a custom [`Tokenizer`], e.g. a real BPE
    /// tokenizer loaded via `output::tokenizer::HfTokenizer` (behind the
    /// `real-tokenizer` feature).
    pub fn with_tokenizer(config: ChunkConfig, tokenizer: Box<dyn Tokenizer>) -> Self {
        Self { config, tokenizer }
    }

    /// Count tokens in `text` using this chunker's configured tokenizer.
    ///
    /// The single entry point for all token counting inside `Chunker` (chunk
    /// sizing, `estimated_tokens`, the token-budget split, and overlap) so
    /// every measurement agrees with whichever [`Tokenizer`] is plugged in.
    fn count_tokens(&self, text: &str) -> usize {
        self.tokenizer.count(text)
    }

    /// Build the render-core options used to turn a [`Section`] into chunk
    /// body text: math conversion follows `self.config.math_config` (`None`
    /// disables it, preserving the pre-render-core behavior of chunk text
    /// carrying unconverted math glyphs), section headers are left out (the
    /// breadcrumb/heading prefix is a chunker-level concern, not render-core's),
    /// and figures/tables are interleaved at their insertion point so chunk
    /// text reaches the same fidelity as markdown/llm_text output.
    fn render_opts(&self) -> RenderOptions<'_> {
        RenderOptions {
            math_config: self.config.math_config.as_ref(),
            escape: EscapeMode::Plain,
            include_headers: false,
            include_figures: true,
            include_tables: true,
            figure_format: FigureTextFormat::Placeholder,
            image_base: String::new(),
        }
    }

    /// Chunk an entire document.
    pub fn chunk(&self, doc: &PaperDocument) -> Vec<Chunk> {
        match self.config.split_strategy {
            SplitStrategy::SectionBoundary => self.chunk_by_section(doc),
            SplitStrategy::TokenCount => self.chunk_by_tokens(doc),
            SplitStrategy::Paragraph => self.chunk_by_paragraph(doc),
        }
    }

    /// Chunk a slice of pre-selected sections.
    pub fn chunk_sections(&self, sections: &[&Section]) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut chunk_id = 0;
        let opts = self.render_opts();

        for section in sections {
            let section_text = render_core::render_section_content(section, &opts);
            let estimated_tokens = self.count_tokens(&section_text);

            if estimated_tokens <= self.config.max_tokens {
                chunks.push(Chunk {
                    chunk_id,
                    paper_id: String::new(),
                    section: section.header_text(),
                    page_range: section.page_range,
                    text: section_text,
                    figures: section.figures.clone(),
                    tables: section.tables.clone(),
                    estimated_tokens,
                    has_continuation: false,
                });
                chunk_id += 1;
            } else {
                // Not tracked in a hierarchy here (a flat, pre-selected slice
                // of sections with no parent chain available), so no
                // breadcrumb prefix is built for this path — see
                // `chunk_section_recursive` for the SectionBoundary strategy,
                // which is where `include_section_context` applies (P2-2).
                let sub = self.split_section_text(
                    &section_text,
                    "",
                    section.header_text(),
                    section.page_range,
                    &section.figures,
                    &section.tables,
                    &mut chunk_id,
                    "",
                );
                chunks.extend(sub);
            }
        }

        chunks
    }

    // ---- Strategy: SectionBoundary ----

    fn chunk_by_section(&self, doc: &PaperDocument) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut chunk_id = 0;

        for section in &doc.sections {
            self.chunk_section_recursive(section, &doc.paper_id, &[], &mut chunk_id, &mut chunks);
        }

        chunks
    }

    /// Recursively chunk `section` and its children, prefixing each chunk's
    /// text with a `[Context: A > B > C]` breadcrumb plus the section's own
    /// heading line when `include_section_context` is enabled.
    ///
    /// `breadcrumb` holds the clean heading text of every ancestor section
    /// (root-to-parent order); it is empty for top-level sections.
    fn chunk_section_recursive(
        &self,
        section: &Section,
        paper_id: &str,
        breadcrumb: &[&str],
        chunk_id: &mut usize,
        out: &mut Vec<Chunk>,
    ) {
        let opts = self.render_opts();
        let section_text = render_core::render_section_content(section, &opts);
        let own = section.header_text();
        let prefix = if self.config.include_section_context {
            Self::build_context_prefix(breadcrumb, &own)
        } else {
            String::new()
        };
        let prefixed_text = format!("{prefix}{section_text}");
        let estimated_tokens = self.count_tokens(&prefixed_text);

        if estimated_tokens <= self.config.max_tokens {
            out.push(Chunk {
                chunk_id: *chunk_id,
                paper_id: paper_id.to_string(),
                section: own.clone(),
                page_range: section.page_range,
                text: prefixed_text,
                figures: section.figures.clone(),
                tables: section.tables.clone(),
                estimated_tokens,
                has_continuation: false,
            });
            *chunk_id += 1;
        } else {
            // Section too large: split by paragraph, carrying the same
            // breadcrumb/heading prefix onto every resulting sub-chunk.
            let sub = self.split_section_text(
                &section_text,
                paper_id,
                own.clone(),
                section.page_range,
                &section.figures,
                &section.tables,
                chunk_id,
                &prefix,
            );
            out.extend(sub);
        }

        // Recurse into children, extending the breadcrumb with this
        // section's own heading (skipped when headerless, so headerless
        // sections don't leave an empty path segment for their descendants).
        let mut child_breadcrumb = breadcrumb.to_vec();
        if !own.is_empty() {
            child_breadcrumb.push(own.as_str());
        }
        for child in &section.children {
            self.chunk_section_recursive(child, paper_id, &child_breadcrumb, chunk_id, out);
        }
    }

    // ---- Shared section flattening (P2-4): TokenCount/Paragraph attribution ----

    /// Flatten the section tree into one [`RenderUnit`] per section (depth
    /// first, parent before children — the same order
    /// [`Self::chunk_section_recursive`] visits sections in), so
    /// `TokenCount`/`Paragraph` can process sections individually instead of
    /// concatenating everything into one untraceable string.
    fn flatten_render_units(&self, doc: &PaperDocument) -> Vec<RenderUnit> {
        let mut units = Vec::new();
        for section in &doc.sections {
            self.flatten_section_recursive(section, &[], &mut units);
        }
        units
    }

    /// Recursive helper for [`Self::flatten_render_units`]. Builds each
    /// section's `RenderUnit` (rich text via `render_opts()`, breadcrumb
    /// prefix via [`Self::build_context_prefix`] — identical logic to
    /// [`Self::chunk_section_recursive`], kept as a separate traversal since
    /// that method produces `Chunk`s directly per section rather than a flat
    /// list `TokenCount`/`Paragraph` can consume).
    fn flatten_section_recursive(
        &self,
        section: &Section,
        breadcrumb: &[&str],
        units: &mut Vec<RenderUnit>,
    ) {
        let opts = self.render_opts();
        let text = render_core::render_section_content(section, &opts);
        let own = section.header_text();
        let prefix = if self.config.include_section_context {
            Self::build_context_prefix(breadcrumb, &own)
        } else {
            String::new()
        };

        units.push(RenderUnit {
            section_name: own.clone(),
            prefix,
            text,
            figures: section.figures.clone(),
            tables: section.tables.clone(),
            page_range: section.page_range,
        });

        let mut child_breadcrumb = breadcrumb.to_vec();
        if !own.is_empty() {
            child_breadcrumb.push(own.as_str());
        }
        for child in &section.children {
            self.flatten_section_recursive(child, &child_breadcrumb, units);
        }
    }

    /// Build the `[Context: ...]` breadcrumb + heading prefix prepended to a
    /// chunk's text when `include_section_context` is enabled.
    ///
    /// `ancestors` are the clean heading text of enclosing sections
    /// (root-to-parent order); `own` is the current section's own clean
    /// heading (empty for headerless sections). Headerless entries (empty
    /// strings) are dropped from the path so they never produce an empty
    /// `" > "`-joined segment. Returns an empty string when the resulting
    /// path is empty (an entirely headerless section with no headed
    /// ancestors) — no prefix is emitted in that case.
    fn build_context_prefix(ancestors: &[&str], own: &str) -> String {
        let mut path: Vec<&str> = ancestors
            .iter()
            .copied()
            .filter(|s| !s.is_empty())
            .collect();
        if !own.is_empty() {
            path.push(own);
        }
        if path.is_empty() {
            return String::new();
        }
        let joined = path.join(" > ");
        if own.is_empty() {
            format!("[Context: {joined}]\n\n")
        } else {
            format!("[Context: {joined}]\n# {own}\n\n")
        }
    }

    // `prefix` (P2-2's breadcrumb/heading text) and `figures`/`tables` (P2-4,
    // replacing a `&Section` borrow so callers can pass a `RenderUnit`'s
    // owned asset lists too) pushed this past clippy's 7-argument default; a
    // dedicated params struct would be overkill for a single private helper.
    #[allow(clippy::too_many_arguments)]
    fn split_section_text(
        &self,
        text: &str,
        paper_id: &str,
        section_name: String,
        page_range: (u32, u32),
        figures: &[FigureInfo],
        tables: &[TableInfo],
        chunk_id: &mut usize,
        prefix: &str,
    ) -> Vec<Chunk> {
        let paragraphs: Vec<&str> = text
            .split("\n\n")
            .filter(|p| !p.trim().is_empty())
            .collect();

        // Expand any paragraph whose token count alone exceeds max_tokens
        // into several within-budget pieces (P2-4: previously such a
        // paragraph was emitted as a single over-budget chunk — see
        // `split_oversized_paragraph`). Every resulting piece then flows
        // through the ordinary accumulate-and-flush loop below exactly like
        // a normal paragraph, so it still gets the same context prefix and
        // token-budgeted overlap as any other sub-chunk.
        let mut expanded: Vec<String> = Vec::with_capacity(paragraphs.len());
        for para in &paragraphs {
            if self.count_tokens(para) > self.config.max_tokens {
                expanded.extend(self.split_oversized_paragraph(para));
            } else {
                expanded.push((*para).to_string());
            }
        }

        let mut chunks = Vec::new();
        let mut current_text = String::new();
        let mut current_tokens = 0usize;
        let mut is_first = true;

        for para in &expanded {
            let para_tokens = self.count_tokens(para);

            if current_tokens + para_tokens > self.config.max_tokens && !current_text.is_empty() {
                // Flush current chunk. Every sub-chunk (first and
                // continuation alike) carries the same context prefix, so
                // positional information survives the split (P2-2).
                let prefixed_text = format!("{prefix}{}", current_text.trim());
                chunks.push(Chunk {
                    chunk_id: *chunk_id,
                    paper_id: paper_id.to_string(),
                    section: section_name.clone(),
                    page_range,
                    estimated_tokens: self.count_tokens(&prefixed_text),
                    text: prefixed_text,
                    // Figures/tables are attached to the first sub-chunk
                    // rather than the (harder to determine, post text-split)
                    // sub-chunk whose span actually covers their insertion
                    // point — a documented judgment call (see P2-4 report);
                    // every asset still lands in exactly one chunk, so
                    // nothing is silently dropped.
                    figures: if is_first { figures.to_vec() } else { vec![] },
                    tables: if is_first { tables.to_vec() } else { vec![] },
                    has_continuation: true,
                });
                *chunk_id += 1;
                is_first = false;

                // Add overlap from end of previous chunk, measured in tokens
                // (not a fixed char multiplier) so it stays accurate for
                // CJK text.
                current_text = self.extract_overlap(&current_text);
                current_tokens = self.count_tokens(&current_text);
            }

            if !current_text.is_empty() {
                current_text.push_str("\n\n");
            }
            current_text.push_str(para);
            current_tokens += para_tokens;
        }

        // Final chunk.
        if !current_text.is_empty() {
            let prefixed_text = format!("{prefix}{}", current_text.trim());
            chunks.push(Chunk {
                chunk_id: *chunk_id,
                paper_id: paper_id.to_string(),
                section: section_name,
                page_range,
                estimated_tokens: self.count_tokens(&prefixed_text),
                text: prefixed_text,
                figures: if is_first { figures.to_vec() } else { vec![] },
                tables: if is_first { tables.to_vec() } else { vec![] },
                has_continuation: false,
            });
            *chunk_id += 1;
        }

        // Ensure the last chunk always has has_continuation = false.
        if let Some(last) = chunks.last_mut() {
            last.has_continuation = false;
        }

        chunks
    }

    /// Split a single paragraph whose token count exceeds `max_tokens` into
    /// several pieces that (with the possible exception noted below) each
    /// fit the budget.
    ///
    /// Splits at sentence boundaries first, greedily packing consecutive
    /// sentences up to the budget so pieces stay as readable as possible. If
    /// a single sentence is itself still oversized, falls back to the same
    /// token-budgeted character window `chunk_by_tokens` uses
    /// ([`Self::token_budget_end`]). No text is ever dropped (No Silent
    /// Drop): concatenating the returned pieces in order reproduces `para`
    /// exactly. The only case that can still exceed `max_tokens` is a single
    /// indivisible run with no sentence boundary and no cuttable prefix
    /// shorter than the whole run's token count under the configured
    /// tokenizer — unavoidable without dropping characters.
    fn split_oversized_paragraph(&self, para: &str) -> Vec<String> {
        let effective_max_tokens = self.config.max_tokens.max(1);
        let mut pieces = Vec::new();
        let mut current = String::new();

        for sentence in Self::split_into_sentences(para) {
            let sentence_tokens = self.count_tokens(sentence);

            if sentence_tokens > effective_max_tokens {
                if !current.is_empty() {
                    pieces.push(std::mem::take(&mut current));
                }
                pieces.extend(self.split_by_char_window(sentence, effective_max_tokens));
                continue;
            }

            // Re-count the actual combined candidate rather than summing
            // per-sentence counts: per-character-class token counts use
            // integer division (see `HeuristicTokenizer`), so
            // `count("a") + count("b")` can under-count relative to
            // `count("ab")` (e.g. two lone CJK punctuation marks each round
            // down to 0 "other" tokens individually, but together round up
            // to 1) — summing would silently let a packed piece exceed
            // `effective_max_tokens`.
            let candidate = format!("{current}{sentence}");
            if self.count_tokens(&candidate) > effective_max_tokens && !current.is_empty() {
                pieces.push(std::mem::take(&mut current));
                current.push_str(sentence);
            } else {
                current = candidate;
            }
        }

        if !current.is_empty() {
            pieces.push(current);
        }

        pieces
    }

    /// Split `text` into sentences, keeping each sentence's terminating
    /// punctuation attached to it. Recognizes ASCII `.`/`!`/`?` followed by
    /// whitespace, and full-width Japanese `。`/`！`/`？` (which need no
    /// following whitespace, matching Japanese typographic convention), as
    /// sentence boundaries.
    ///
    /// Concatenating the returned sentences in order reproduces `text`
    /// exactly (no character is consumed or dropped by the boundary scan).
    fn split_into_sentences(text: &str) -> Vec<&str> {
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let mut sentences = Vec::new();
        let mut start = 0usize;

        for (i, (byte_idx, ch)) in chars.iter().enumerate() {
            let is_boundary = matches!(ch, '。' | '！' | '？')
                || (matches!(ch, '.' | '!' | '?')
                    && chars
                        .get(i + 1)
                        .is_some_and(|(_, next)| next.is_whitespace()));
            if is_boundary {
                let end = byte_idx + ch.len_utf8();
                sentences.push(&text[start..end]);
                start = end;
            }
        }
        if start < text.len() {
            sentences.push(&text[start..]);
        }

        sentences
    }

    /// Token-budgeted character-window split of a single indivisible run of
    /// text, reusing [`Self::token_budget_end`] (the same forward-advancing
    /// window `chunk_by_tokens` uses) so a sentence with no further internal
    /// boundary still lands within `max_tokens` per piece. Always makes
    /// forward progress and never drops a character.
    fn split_by_char_window(&self, text: &str, max_tokens: usize) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        let mut pieces = Vec::new();
        let mut start = 0;
        while start < chars.len() {
            let end = self.token_budget_end(&chars, start, max_tokens);
            pieces.push(chars[start..end].iter().collect());
            start = end;
        }
        pieces
    }

    /// Take a suffix of `text` whose token count (per `self.tokenizer`) fits
    /// within `self.config.overlap_tokens`.
    ///
    /// Replaces the legacy `overlap_tokens * 4` fixed char-per-token
    /// approximation, which overshot the configured overlap budget for CJK
    /// text (see [`crate::output::tokenizer`]).
    fn extract_overlap(&self, text: &str) -> String {
        if self.config.overlap_tokens == 0 {
            return String::new();
        }
        let chars: Vec<char> = text.chars().collect();
        let end = chars.len();
        let start = self.token_overlap_start(&chars, end, self.config.overlap_tokens);
        chars[start..end].iter().collect()
    }

    /// Find the start index (within `chars[..end]`) of the longest suffix
    /// ending at `end` whose token count is within `overlap_tokens`.
    ///
    /// Walks backward from `end` one character at a time, which is simpler
    /// than a binary search and does not depend on token count growing
    /// strictly monotonically with window length (true for
    /// [`HeuristicTokenizer`](crate::output::tokenizer::HeuristicTokenizer),
    /// but not guaranteed for every possible [`Tokenizer`] impl, e.g. a real
    /// BPE tokenizer's merges).
    fn token_overlap_start(&self, chars: &[char], end: usize, overlap_tokens: usize) -> usize {
        if overlap_tokens == 0 || end == 0 {
            return end;
        }
        let mut start = end;
        while start > 0 {
            let candidate: String = chars[start - 1..end].iter().collect();
            if self.count_tokens(&candidate) > overlap_tokens {
                break;
            }
            start -= 1;
        }
        start
    }

    /// Find the end index (within `chars[start..]`) of the longest chunk
    /// starting at `start` whose token count (per `self.tokenizer`) is
    /// within `max_tokens`.
    ///
    /// Starts from a coarse guess (the legacy ~4 ASCII-chars/token ratio)
    /// then walks the guess forward or backward one character at a time
    /// against the real tokenizer count, so CJK-heavy runs — which the
    /// legacy `max_tokens * 4` fixed char budget systematically
    /// overshot — land within `max_tokens` too. Always returns an index
    /// `> start` (forward progress guaranteed even when a single character
    /// already meets or exceeds the budget, so callers never loop forever
    /// and no character is ever silently dropped).
    fn token_budget_end(&self, chars: &[char], start: usize, max_tokens: usize) -> usize {
        let mut end = (start + max_tokens.saturating_mul(4))
            .min(chars.len())
            .max(start + 1);

        // Shrink: the coarse guess can overshoot for CJK-heavy text.
        while end > start + 1 {
            let candidate: String = chars[start..end].iter().collect();
            if self.count_tokens(&candidate) <= max_tokens {
                break;
            }
            end -= 1;
        }

        // Grow: the coarse guess can undershoot for ASCII-heavy text.
        while end < chars.len() {
            let candidate: String = chars[start..end + 1].iter().collect();
            if self.count_tokens(&candidate) > max_tokens {
                break;
            }
            end += 1;
        }

        end
    }

    // ---- Strategy: TokenCount ----

    /// Pack whole [`RenderUnit`]s (sections) into chunks up to the token
    /// budget, splitting only a section whose own text alone exceeds the
    /// budget (via [`Self::split_section_text`]/[`Self::split_oversized_paragraph`]).
    ///
    /// Before P2-4 this flattened every section into one giant string and
    /// windowed it with [`Self::token_budget_end`], which packed tokens as
    /// tightly as possible but dropped section attribution and every
    /// figure/table (`section: String::new()`, `figures: vec![]`,
    /// `tables: vec![]`). Chunking at section granularity instead means each
    /// chunk's `section` names the section its content actually came from
    /// (the leading section's name, when several small sections are packed
    /// together — see the loop below) and carries that section's figures/
    /// tables, with **no asset ever silently dropped**: every unit's
    /// figures/tables are attached exactly once, either to the chunk holding
    /// its full text or (if the unit itself had to be split) to the first
    /// resulting sub-chunk.
    ///
    /// A consequence of chunking at section granularity: unlike the old
    /// char-level window, there is no token-based overlap between two chunks
    /// that both hold whole, unsplit sections (overlap only still applies
    /// *within* an oversized section's sub-chunks, via `split_section_text`).
    /// This matches how the `SectionBoundary` strategy already treats
    /// section-to-section boundaries (no overlap there either), rather than
    /// smearing context across semantically distinct sections.
    fn chunk_by_tokens(&self, doc: &PaperDocument) -> Vec<Chunk> {
        let units = self.flatten_render_units(doc);
        if units.is_empty() {
            return vec![];
        }

        // Guard against max_tokens == 0 which would cause an infinite loop.
        let effective_max_tokens = self.config.max_tokens.max(1);

        let mut chunks = Vec::new();
        let mut chunk_id = 0usize;
        let mut idx = 0usize;

        while idx < units.len() {
            let unit = &units[idx];
            let own_tokens = self.count_tokens(&format!("{}{}", unit.prefix, unit.text));

            if own_tokens > effective_max_tokens {
                // This section alone is too large for one chunk: sub-split
                // it (paragraph, then sentence/char-window for an oversized
                // paragraph) instead of emitting a single over-budget chunk.
                let sub = self.split_section_text(
                    &unit.text,
                    &doc.paper_id,
                    unit.section_name.clone(),
                    unit.page_range,
                    &unit.figures,
                    &unit.tables,
                    &mut chunk_id,
                    &unit.prefix,
                );
                chunks.extend(sub);
                idx += 1;
                continue;
            }

            // Greedily pack whole subsequent sections into the same chunk
            // while staying within budget.
            let mut group_end = idx + 1;
            let mut acc_tokens = own_tokens;
            while group_end < units.len() {
                let candidate = &units[group_end];
                let candidate_tokens = self.count_tokens(&candidate.text);
                if candidate_tokens > effective_max_tokens
                    || acc_tokens + candidate_tokens > effective_max_tokens
                {
                    break;
                }
                acc_tokens += candidate_tokens;
                group_end += 1;
            }

            let mut text = format!("{}{}", unit.prefix, unit.text);
            let mut figures = unit.figures.clone();
            let mut tables = unit.tables.clone();
            let mut page_start = unit.page_range.0;
            let mut page_end = unit.page_range.1;
            for other in &units[idx + 1..group_end] {
                text.push_str("\n\n");
                text.push_str(&other.text);
                figures.extend(other.figures.iter().cloned());
                tables.extend(other.tables.iter().cloned());
                page_start = page_start.min(other.page_range.0);
                page_end = page_end.max(other.page_range.1);
            }
            let estimated_tokens = self.count_tokens(&text);

            chunks.push(Chunk {
                chunk_id,
                paper_id: doc.paper_id.clone(),
                section: unit.section_name.clone(),
                page_range: (page_start, page_end),
                text,
                figures,
                tables,
                estimated_tokens,
                has_continuation: false,
            });
            chunk_id += 1;
            idx = group_end;
        }

        chunks
    }

    // ---- Strategy: Paragraph ----

    /// Split each section independently by paragraph/token budget (via
    /// [`Self::split_section_text`]), carrying its own name, breadcrumb
    /// prefix, figures, and tables onto every resulting chunk.
    ///
    /// Unlike [`Self::chunk_by_tokens`], sections are never merged into one
    /// chunk here — every chunk always traces back to exactly one section.
    /// Before P2-4 this instead concatenated every section into one string
    /// and split it via a placeholder `empty_section` (`section:
    /// String::new()`, no figures/tables), losing attribution entirely.
    fn chunk_by_paragraph(&self, doc: &PaperDocument) -> Vec<Chunk> {
        let units = self.flatten_render_units(doc);
        let mut chunks = Vec::new();
        let mut chunk_id = 0usize;

        for unit in &units {
            let sub = self.split_section_text(
                &unit.text,
                &doc.paper_id,
                unit.section_name.clone(),
                unit.page_range,
                &unit.figures,
                &unit.tables,
                &mut chunk_id,
                &unit.prefix,
            );
            chunks.extend(sub);
        }

        chunks
    }

    // ---- Token estimation ----

    /// Static token count estimate using the default [`HeuristicTokenizer`].
    ///
    /// Kept as a thin wrapper (rather than removed) for callers that need a
    /// standalone estimate with no `Chunker` instance at hand (e.g.
    /// [`crate::selector::SectionSelector::total_estimated_tokens`] and
    /// existing tests). Instance-scoped counting — which respects whatever
    /// [`Tokenizer`] a given `Chunker` was constructed with — goes through
    /// [`Self::count_tokens`] instead; this static function always uses the
    /// heuristic regardless of a `Chunker`'s configured tokenizer.
    pub fn estimate_tokens(text: &str) -> usize {
        HeuristicTokenizer.count(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ChunkConfig, SplitStrategy};
    use crate::types::{BlockType, DocumentMetadata, PaperDocument, Rect, Section, TextBlock};
    use std::path::PathBuf;

    fn make_doc_with_sections(sections: Vec<Section>) -> PaperDocument {
        PaperDocument {
            paper_id: "test".to_string(),
            source_file: PathBuf::from("test.pdf"),
            metadata: DocumentMetadata {
                pages: sections.len() as u32,
                ..Default::default()
            },
            all_figures: vec![],
            all_tables: vec![],
            sections,
        }
    }

    fn make_section(header: &str, text: &str, level: u8) -> Section {
        use crate::types::SectionHeader;
        Section {
            header: Some(SectionHeader {
                text: header.to_string(),
                clean_text: header.to_string(),
                level,
                numbering: None,
                page: 0,
                bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                block_index: 0,
            }),
            level,
            blocks: vec![TextBlock {
                global_index: 0,
                lines: vec![],
                text: text.to_string(),
                bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                page: 0,
                column_index: 0,
                block_type: BlockType::BodyText,
            }],
            figures: vec![],
            tables: vec![],
            children: vec![],
            page_range: (0, 0),
        }
    }

    fn default_config() -> ChunkConfig {
        ChunkConfig {
            max_tokens: 4000,
            overlap_tokens: 200,
            split_strategy: SplitStrategy::SectionBoundary,
            include_section_context: true,
            math_config: None,
        }
    }

    #[test]
    fn small_section_becomes_one_chunk() {
        let config = default_config();
        let chunker = Chunker::new(config);
        let doc = make_doc_with_sections(vec![make_section("INTRO", "Short intro text.", 1)]);
        let chunks = chunker.chunk(&doc);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].section, "INTRO");
        assert!(chunks[0].text.contains("Short intro text."));
    }

    #[test]
    fn multiple_sections_produce_multiple_chunks() {
        let config = default_config();
        let chunker = Chunker::new(config);
        let doc = make_doc_with_sections(vec![
            make_section("INTRO", "Introduction text.", 1),
            make_section("METHODS", "Methods text.", 1),
        ]);
        let chunks = chunker.chunk(&doc);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].section, "INTRO");
        assert_eq!(chunks[1].section, "METHODS");
    }

    #[test]
    fn chunk_ids_are_sequential() {
        let config = default_config();
        let chunker = Chunker::new(config);
        let doc = make_doc_with_sections(vec![
            make_section("SEC1", "Text 1.", 1),
            make_section("SEC2", "Text 2.", 1),
            make_section("SEC3", "Text 3.", 1),
        ]);
        let chunks = chunker.chunk(&doc);
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_id, i, "Chunk IDs should be 0, 1, 2...");
        }
    }

    #[test]
    fn token_estimation_for_ascii() {
        // 100 ASCII chars / 4 = 25 tokens.
        let text = "a".repeat(100);
        assert_eq!(Chunker::estimate_tokens(&text), 25);
    }

    #[test]
    fn last_chunk_has_continuation_false() {
        let config = default_config();
        let chunker = Chunker::new(config);
        let doc = make_doc_with_sections(vec![make_section("SEC", "Some text.", 1)]);
        let chunks = chunker.chunk(&doc);
        assert!(!chunks.last().unwrap().has_continuation);
    }

    #[test]
    fn extract_overlap_handles_non_ascii() {
        // Verify extract_overlap works with multibyte UTF-8 characters, and
        // is measured in tokens (not the legacy `overlap_tokens * 4` char
        // approximation, which — since CJK is ~1 char/token, not
        // ~4 — would have overshot to 8 chars instead of 2).
        let config = ChunkConfig {
            max_tokens: 10,
            overlap_tokens: 2,
            split_strategy: SplitStrategy::SectionBoundary,
            include_section_context: true,
            math_config: None,
        };
        let chunker = Chunker::new(config);
        // 10 Japanese characters (3 bytes each in UTF-8).
        let text = "あいうえおかきくけこ";
        let overlap = chunker.extract_overlap(text);
        // 1 CJK char == 1 token under HeuristicTokenizer, so 2 overlap
        // tokens == the last 2 chars: "けこ".
        assert_eq!(overlap, "けこ");
    }

    #[test]
    fn token_count_strategy_zero_max_tokens_does_not_loop() {
        let config = ChunkConfig {
            max_tokens: 0,
            overlap_tokens: 0,
            split_strategy: SplitStrategy::TokenCount,
            include_section_context: true,
            math_config: None,
        };
        let chunker = Chunker::new(config);
        let doc = make_doc_with_sections(vec![make_section("SEC", "Hello world.", 1)]);
        // Should complete without infinite loop (max_tokens clamped to 1).
        let chunks = chunker.chunk(&doc);
        assert!(!chunks.is_empty());
    }

    // ---- Tokenizer trait / CJK budget fix (P2-3) ----

    #[test]
    fn token_count_strategy_respects_budget_for_cjk() {
        // 1000 CJK characters, split with a small token budget. Under the
        // legacy `max_tokens * 4` char budget (which assumed ~4 ASCII
        // chars/token even for CJK, where the real ratio is closer to
        // 1 char/token) each chunk would have held ~1200 tokens' worth of
        // text against a budget of 300 — a 4x overshoot. The token-budgeted
        // split must keep every chunk within budget.
        let config = ChunkConfig {
            max_tokens: 300,
            overlap_tokens: 0,
            split_strategy: SplitStrategy::TokenCount,
            include_section_context: false,
            math_config: None,
        };
        let chunker = Chunker::new(config);
        let cjk_text: String = "漢字仮名交じり文章例。"
            .chars()
            .cycle()
            .take(1000)
            .collect();
        let doc = make_doc_with_sections(vec![make_section("SEC", &cjk_text, 1)]);

        let chunks = chunker.chunk(&doc);

        assert!(
            chunks.len() >= 4,
            "1000 CJK chars over a 300-token budget should split into >= 4 chunks, got {}",
            chunks.len()
        );
        for chunk in &chunks {
            let actual = Chunker::estimate_tokens(&chunk.text);
            assert!(
                actual <= 300,
                "chunk exceeded the token budget: {actual} tokens (text len {} chars)",
                chunk.text.chars().count()
            );
        }
    }

    #[test]
    fn token_overlap_start_respects_token_budget() {
        // 16 ASCII chars; HeuristicTokenizer counts ASCII at chars/4 (floor),
        // so the largest window whose count stays <= 2 tokens is 11 chars
        // (floor(11/4) == 2, floor(12/4) == 3) — the walk finds the true
        // token-budgeted boundary rather than a fixed `overlap_tokens * 4`
        // multiplication.
        let chunker = Chunker::new(default_config());
        let chars: Vec<char> = "a".repeat(16).chars().collect();
        let start = chunker.token_overlap_start(&chars, chars.len(), 2);
        assert_eq!(
            chars.len() - start,
            11,
            "expected an 11-char overlap window for a 2-token budget"
        );
    }

    #[test]
    fn token_budget_end_keeps_cjk_within_budget() {
        // 50 CJK chars; under HeuristicTokenizer 1 CJK char == 1 token, so a
        // 10-token budget must cut at exactly 10 chars (the legacy
        // `max_tokens * 4` char budget would have cut at 40).
        let chunker = Chunker::new(default_config());
        let chars: Vec<char> = "漢".repeat(50).chars().collect();
        let end = chunker.token_budget_end(&chars, 0, 10);
        assert_eq!(end, 10);
    }

    #[test]
    fn chunk_by_tokens_overlap_is_token_based() {
        // With a token-count strategy and a nonzero overlap, chunk N+1 must
        // begin with the token-budgeted overlap tail of chunk N (computed by
        // `token_overlap_start`), not a fixed char multiplier of
        // `overlap_tokens`.
        let config = ChunkConfig {
            max_tokens: 20,
            overlap_tokens: 5,
            split_strategy: SplitStrategy::TokenCount,
            include_section_context: false,
            math_config: None,
        };
        let chunker = Chunker::new(config);
        let text = "word ".repeat(60); // plenty of ASCII text to force multiple chunks
        let doc = make_doc_with_sections(vec![make_section("SEC", &text, 1)]);

        let chunks = chunker.chunk(&doc);
        assert!(
            chunks.len() >= 2,
            "expected multiple chunks, got {}",
            chunks.len()
        );

        let chars0: Vec<char> = chunks[0].text.chars().collect();
        let overlap_start = chunker.token_overlap_start(&chars0, chars0.len(), 5);
        let expected_overlap: String = chars0[overlap_start..].iter().collect();

        assert!(
            !expected_overlap.is_empty(),
            "expected a nonempty token-based overlap window"
        );
        assert!(
            chunks[1].text.starts_with(&expected_overlap),
            "chunk 1 should begin with chunk 0's token-budgeted overlap tail:\nexpected prefix: {expected_overlap:?}\nchunk1: {}",
            chunks[1].text
        );
    }

    // ---- render-core integration (P2-1): chunk.text must carry the same
    // fidelity as markdown/llm_text output instead of raw full_text(). ----

    /// A section whose single block is a math-font line (CMMI10 "α", all-math
    /// → Display context), so math conversion is exercised end-to-end.
    fn make_math_section(math_text: &str, font_name: &str) -> Section {
        use crate::types::{SectionHeader, TextLine, TextSpan};

        let span = TextSpan {
            text: math_text.to_string(),
            font_name: font_name.to_string(),
            font_size: 10.0,
            is_bold: false,
            is_italic: true,
            bbox: Rect::new(100.0, 700.0, 150.0, 690.0),
            page: 0,
        };
        let line = TextLine {
            text: math_text.to_string(),
            spans: vec![span],
            bbox: Rect::new(100.0, 700.0, 150.0, 690.0),
            page: 0,
            baseline_y: 690.0,
            primary_font_size: 10.0,
            primary_font_name: font_name.to_string(),
            is_bold: false,
        };
        let block = TextBlock {
            global_index: 0,
            lines: vec![line],
            text: math_text.to_string(),
            bbox: Rect::new(100.0, 700.0, 150.0, 690.0),
            page: 0,
            column_index: 0,
            block_type: BlockType::BodyText,
        };
        Section {
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
        }
    }

    #[test]
    fn chunk_text_contains_converted_math() {
        use crate::config::{MathConfig, MathRepresentationPreference};

        let config = ChunkConfig {
            math_config: Some(MathConfig {
                representation: MathRepresentationPreference::LaTeX,
                ..MathConfig::default()
            }),
            ..default_config()
        };
        let chunker = Chunker::new(config);
        let doc = make_doc_with_sections(vec![make_math_section("α", "CMMI10")]);
        let chunks = chunker.chunk(&doc);

        assert_eq!(chunks.len(), 1);
        assert!(
            chunks[0].text.contains("\\alpha"),
            "expected converted math '\\alpha' in chunk text, got: {}",
            chunks[0].text
        );
        assert!(
            !chunks[0].text.contains('α'),
            "raw math glyph should have been converted, got: {}",
            chunks[0].text
        );
    }

    #[test]
    fn chunk_text_contains_table_markdown() {
        use crate::types::{InsertionPoint, TableInfo, TableRepresentation};

        let mut section = make_section("SEC", "Body text.", 1);
        section.tables.push(TableInfo {
            table_id: "Table 1".to_string(),
            table_number: Some(1),
            caption: None,
            representation: TableRepresentation::Markdown {
                header: vec!["A".to_string(), "B".to_string()],
                rows: vec![vec!["1".to_string(), "2".to_string()]],
                caption: None,
                markdown_text: "| A | B |\n| --- | --- |\n| 1 | 2 |\n".to_string(),
                header_rows: vec![],
            },
            insertion_point: InsertionPoint {
                page: 0,
                after_block_index: None,
                y_position: 0.0,
            },
            page: 0,
        });
        let config = default_config();
        let chunker = Chunker::new(config);
        let doc = make_doc_with_sections(vec![section]);
        let chunks = chunker.chunk(&doc);

        assert_eq!(chunks.len(), 1);
        assert!(
            chunks[0].text.contains("| --- |"),
            "expected table markdown in chunk text, got: {}",
            chunks[0].text
        );
    }

    #[test]
    fn chunk_text_contains_figure_placeholder() {
        use crate::types::{FigureInfo, ImageFormat, ImageInfo, InsertionPoint};
        use std::path::PathBuf;

        let mut section = make_section("SEC", "Body text.", 1);
        section.figures.push(FigureInfo {
            figure_id: "Fig. 1".to_string(),
            figure_number: Some(1),
            caption_text: "Fig. 1: A diagram.".to_string(),
            image: ImageInfo {
                path: PathBuf::from("images/p000_img000.png"),
                page: 0,
                raw_bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                normalized_bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                width_px: 10,
                height_px: 10,
                format: ImageFormat::Png,
            },
            context_text: String::new(),
            insertion_point: InsertionPoint {
                page: 0,
                after_block_index: Some(0),
                y_position: 0.0,
            },
        });
        let config = default_config();
        let chunker = Chunker::new(config);
        let doc = make_doc_with_sections(vec![section]);
        let chunks = chunker.chunk(&doc);

        assert_eq!(chunks.len(), 1);
        assert!(
            chunks[0].text.contains("[IMAGE:"),
            "expected figure placeholder in chunk text, got: {}",
            chunks[0].text
        );
    }

    // ---- include_section_context (P2-2): breadcrumb + heading prefix ----

    #[test]
    fn breadcrumb_prefix_for_nested_section() {
        let config = default_config(); // include_section_context: true
        let chunker = Chunker::new(config);

        let mut parent = make_section("METHODS", "Parent body text.", 1);
        parent
            .children
            .push(make_section("Data Collection", "Child body text.", 2));

        let doc = make_doc_with_sections(vec![parent]);
        let chunks = chunker.chunk(&doc);

        assert_eq!(chunks.len(), 2, "expected one chunk per section");
        assert!(
            chunks[0]
                .text
                .starts_with("[Context: METHODS]\n# METHODS\n\n"),
            "top-level chunk should carry its own heading as the sole breadcrumb entry: {}",
            chunks[0].text
        );
        assert!(
            chunks[1]
                .text
                .starts_with("[Context: METHODS > Data Collection]\n# Data Collection\n\n"),
            "nested chunk should show the full ancestor path: {}",
            chunks[1].text
        );
    }

    #[test]
    fn no_prefix_when_context_disabled() {
        let config = ChunkConfig {
            include_section_context: false,
            ..default_config()
        };
        let chunker = Chunker::new(config);

        let mut parent = make_section("METHODS", "Parent body text.", 1);
        parent
            .children
            .push(make_section("Data Collection", "Child body text.", 2));

        let doc = make_doc_with_sections(vec![parent]);
        let chunks = chunker.chunk(&doc);

        assert_eq!(chunks.len(), 2);
        for chunk in &chunks {
            assert!(
                !chunk.text.contains("[Context:"),
                "prefix must be absent when include_section_context is false: {}",
                chunk.text
            );
        }
        // Disabling the flag reproduces the plain (unprefixed) body text.
        assert!(chunks[0].text.starts_with("Parent body text."));
        assert!(chunks[1].text.starts_with("Child body text."));
    }

    #[test]
    fn headerless_section_no_empty_breadcrumb() {
        let config = default_config();
        let chunker = Chunker::new(config);

        let section = Section {
            header: None,
            level: 1,
            blocks: vec![TextBlock {
                global_index: 0,
                lines: vec![],
                text: "Preamble text.".to_string(),
                bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                page: 0,
                column_index: 0,
                block_type: BlockType::BodyText,
            }],
            figures: vec![],
            tables: vec![],
            children: vec![],
            page_range: (0, 0),
        };
        let doc = make_doc_with_sections(vec![section]);
        let chunks = chunker.chunk(&doc);

        assert_eq!(chunks.len(), 1);
        assert!(
            !chunks[0].text.contains("[Context:"),
            "an entirely headerless section (no ancestors either) must have no breadcrumb: {}",
            chunks[0].text
        );
        assert!(
            !chunks[0].text.contains(" >  >") && !chunks[0].text.contains("Context: ]"),
            "must not produce an empty or doubly-separated breadcrumb: {}",
            chunks[0].text
        );
        assert!(chunks[0].text.contains("Preamble text."));
    }

    #[test]
    fn split_chunks_all_carry_prefix() {
        use crate::types::SectionHeader;

        // Small max_tokens forces `split_section_text` to break the section's
        // three paragraphs into multiple sub-chunks.
        let config = ChunkConfig {
            max_tokens: 15,
            overlap_tokens: 0,
            split_strategy: SplitStrategy::SectionBoundary,
            include_section_context: true,
            math_config: None,
        };
        let chunker = Chunker::new(config);

        let make_block = |idx: usize, text: &str| TextBlock {
            global_index: idx,
            lines: vec![],
            text: text.to_string(),
            bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
            page: 0,
            column_index: 0,
            block_type: BlockType::BodyText,
        };
        let section = Section {
            header: Some(SectionHeader {
                text: "BIG".to_string(),
                clean_text: "BIG".to_string(),
                level: 1,
                numbering: None,
                page: 0,
                bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                block_index: 0,
            }),
            level: 1,
            blocks: vec![
                make_block(0, "Paragraph one padding text goes here now."),
                make_block(1, "Paragraph two padding text goes here now."),
                make_block(2, "Paragraph three padding text goes here."),
            ],
            figures: vec![],
            tables: vec![],
            children: vec![],
            page_range: (0, 0),
        };
        let doc = make_doc_with_sections(vec![section]);
        let chunks = chunker.chunk(&doc);

        assert!(
            chunks.len() >= 2,
            "expected the oversized section to split into multiple sub-chunks, got {}",
            chunks.len()
        );
        for chunk in &chunks {
            assert!(
                chunk.text.starts_with("[Context: BIG]\n# BIG\n\n"),
                "every sub-chunk must carry the same section prefix: {}",
                chunk.text
            );
        }
    }

    // ---- P2-4: TokenCount/Paragraph section+figure/table attribution,
    // oversized single-paragraph sub-splitting ----

    fn make_figure(id: &str, after_block_index: Option<usize>) -> crate::types::FigureInfo {
        use crate::types::{ImageFormat, ImageInfo, InsertionPoint};
        use std::path::PathBuf;

        crate::types::FigureInfo {
            figure_id: id.to_string(),
            figure_number: Some(1),
            caption_text: format!("{id}: A diagram."),
            image: ImageInfo {
                path: PathBuf::from(format!("images/{id}.png")),
                page: 0,
                raw_bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                normalized_bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
                width_px: 10,
                height_px: 10,
                format: ImageFormat::Png,
            },
            context_text: String::new(),
            insertion_point: InsertionPoint {
                page: 0,
                after_block_index,
                y_position: 0.0,
            },
        }
    }

    fn make_table(id: &str, after_block_index: Option<usize>) -> crate::types::TableInfo {
        use crate::types::{InsertionPoint, TableRepresentation};

        crate::types::TableInfo {
            table_id: id.to_string(),
            table_number: Some(1),
            caption: None,
            representation: TableRepresentation::Markdown {
                header: vec!["A".to_string()],
                rows: vec![vec!["1".to_string()]],
                caption: None,
                markdown_text: "| A |\n| --- |\n| 1 |\n".to_string(),
                header_rows: vec![],
            },
            insertion_point: InsertionPoint {
                page: 0,
                after_block_index,
                y_position: 0.0,
            },
            page: 0,
        }
    }

    /// A section with a header, one body block, and one figure + one table
    /// anchored to that block.
    fn make_section_with_assets(header: &str, text: &str, id_suffix: &str) -> Section {
        let mut section = make_section(header, text, 1);
        section
            .figures
            .push(make_figure(&format!("Fig. {id_suffix}"), Some(0)));
        section
            .tables
            .push(make_table(&format!("Table {id_suffix}"), Some(0)));
        section
    }

    #[test]
    fn token_strategy_preserves_section_and_figures() {
        // Two sections, each with its own figure/table, packed generously so
        // `TokenCount` doesn't need to split anything — before P2-4 this
        // strategy always produced `section: String::new()`, `figures:
        // vec![]`, `tables: vec![]` regardless of budget.
        let config = ChunkConfig {
            max_tokens: 4000,
            overlap_tokens: 0,
            split_strategy: SplitStrategy::TokenCount,
            include_section_context: false,
            math_config: None,
        };
        let chunker = Chunker::new(config);
        let doc = make_doc_with_sections(vec![
            make_section_with_assets("INTRO", "Introduction body text.", "1"),
            make_section_with_assets("METHODS", "Methods body text.", "2"),
        ]);

        let chunks = chunker.chunk(&doc);

        assert!(!chunks.is_empty());
        for chunk in &chunks {
            assert!(
                !chunk.section.is_empty(),
                "TokenCount chunk must carry a non-empty section name: {chunk:?}"
            );
        }
        let total_figures: usize = chunks.iter().map(|c| c.figures.len()).sum();
        let total_tables: usize = chunks.iter().map(|c| c.tables.len()).sum();
        assert_eq!(
            total_figures, 2,
            "both sections' figures must appear across the produced chunks"
        );
        assert_eq!(
            total_tables, 2,
            "both sections' tables must appear across the produced chunks"
        );
    }

    #[test]
    fn paragraph_strategy_has_section_name() {
        // Before P2-4, `Paragraph` concatenated every section into one
        // string via an `empty_section` placeholder, so `chunk.section` was
        // always empty. Each section must now name the heading it came from.
        let config = ChunkConfig {
            max_tokens: 4000,
            overlap_tokens: 0,
            split_strategy: SplitStrategy::Paragraph,
            include_section_context: false,
            math_config: None,
        };
        let chunker = Chunker::new(config);
        let doc = make_doc_with_sections(vec![
            make_section("INTRO", "Introduction body text.", 1),
            make_section("METHODS", "Methods body text.", 1),
        ]);

        let chunks = chunker.chunk(&doc);

        assert_eq!(chunks.len(), 2, "one chunk per section at this budget");
        for chunk in &chunks {
            assert!(
                !chunk.section.is_empty(),
                "Paragraph strategy chunk must carry a non-empty section name: {chunk:?}"
            );
        }
        assert_eq!(chunks[0].section, "INTRO");
        assert_eq!(chunks[1].section, "METHODS");
    }

    #[test]
    fn oversized_single_paragraph_is_subsplit() {
        // A single paragraph (one block, so `split_section_text` sees it as
        // one "\n\n"-delimited paragraph) whose token count is several times
        // `max_tokens`. Before P2-4's fix, `split_section_text` only ever
        // flushed on paragraph boundaries (`!current_text.is_empty()`
        // guards the flush check), so a lone oversized paragraph was never
        // split and became a single over-budget chunk.
        let config = ChunkConfig {
            max_tokens: 20,
            overlap_tokens: 0,
            split_strategy: SplitStrategy::SectionBoundary,
            include_section_context: false,
            math_config: None,
        };
        let chunker = Chunker::new(config);
        // No sentence punctuation, so this exercises the char-window
        // fallback tier of `split_oversized_paragraph` (~75 tokens' worth of
        // ASCII text against a 20-token budget).
        let text = "word ".repeat(60);
        let doc = make_doc_with_sections(vec![make_section("SEC", &text, 1)]);

        let chunks = chunker.chunk(&doc);

        assert!(
            chunks.len() >= 2,
            "an oversized single paragraph must be sub-split into multiple chunks, got {}",
            chunks.len()
        );
        for chunk in &chunks {
            let tokens = chunker.count_tokens(&chunk.text);
            assert!(
                tokens <= 20,
                "every sub-chunk of an oversized paragraph must stay within max_tokens: {tokens} tokens in {:?}",
                chunk.text
            );
        }
        // No Silent Drop: the char-window fallback tier cuts on raw
        // character boundaries (not word boundaries), so a naive substring
        // count of "word" across chunks would under-count whenever a cut
        // lands mid-word without any character actually being lost. Count
        // the single-character 'w' that starts every occurrence instead —
        // it can never be partially retained, so this exactly verifies no
        // character was dropped by the split.
        let w_count: usize = chunks.iter().map(|c| c.text.matches('w').count()).sum();
        assert_eq!(
            w_count, 60,
            "every 'w' from the 60 \"word \" occurrences must survive the oversized-paragraph sub-split"
        );
    }

    #[test]
    fn all_figures_and_tables_present_across_chunks_for_every_strategy() {
        // No Silent Drop: for every split strategy, the total figure/table
        // count across all produced chunks must equal the input count, even
        // when a small budget forces heavy splitting.
        for strategy in [
            SplitStrategy::SectionBoundary,
            SplitStrategy::TokenCount,
            SplitStrategy::Paragraph,
        ] {
            let config = ChunkConfig {
                max_tokens: 12,
                overlap_tokens: 0,
                split_strategy: strategy,
                include_section_context: false,
                math_config: None,
            };
            let chunker = Chunker::new(config);
            let doc = make_doc_with_sections(vec![
                make_section_with_assets(
                    "INTRO",
                    "Introduction padding text goes here for this test.",
                    "1",
                ),
                make_section_with_assets(
                    "METHODS",
                    "Methods padding text goes here for this test too.",
                    "2",
                ),
                make_section_with_assets(
                    "RESULTS",
                    "Results padding text goes here for this test also.",
                    "3",
                ),
            ]);

            let chunks = chunker.chunk(&doc);
            let total_figures: usize = chunks.iter().map(|c| c.figures.len()).sum();
            let total_tables: usize = chunks.iter().map(|c| c.tables.len()).sum();
            assert_eq!(
                total_figures, 3,
                "all 3 figures must survive splitting under {chunks:?}"
            );
            assert_eq!(
                total_tables, 3,
                "all 3 tables must survive splitting under {chunks:?}"
            );
        }
    }
}
