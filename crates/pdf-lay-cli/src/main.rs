//! `pdf-lay` CLI — PDF layout analysis for academic papers.
//!
//! Subcommands:
//! - `toc <PDF>`       — print the table of contents to stdout.
//! - `markdown <PDF>`  — convert the PDF to Markdown and print to stdout.
//! - `json <PDF>`      — dump the analyzed document as JSON.
//! - `chunks <PDF>`    — split the document into JSONL chunks for RAG.
//! - `llm-text <PDF>`  — render LLM-optimized plain text.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process;

use clap::{Args, Parser, Subcommand};
use pdf_lay::{
    CaptionStyle, Chunk, ChunkConfig, Chunker, Config, FigureInfo, FigureTextFormat, JsonGenerator,
    LlmTextConfig, LlmTextGenerator, MarkdownConfig, MarkdownGenerator, MathConfig,
    MathRepresentationPreference, SplitStrategy, TableInfo, TableRepresentation, TocGenerator,
    Tokenizer, analyze_pdf,
};
use serde::Serialize;

const CLI_LONG_ABOUT: &str = "\
Analyze academic-paper PDFs and emit section-aware outputs for review, conversion,
and downstream LLM pipelines.

This CLI always runs the same core analysis pipeline:
  1. Read the PDF and extract text spans plus embedded images.
  2. Reconstruct lines and detect reading order / column layout.
  3. Build a section hierarchy and lightweight document metadata.
  4. Render either a table of contents or Markdown output.

Operational behavior:
  - Primary output is written to stdout unless a subcommand offers `-o/--output`.
  - Non-fatal analysis issues are reported as warnings on stderr.
  - Fatal failures print `error: ...` to stderr and exit with status code 1.
  - Embedded-image extraction is enabled by default and writes files under
    `--image-dir`.

Use `pdf-lay <COMMAND> --help` for command-specific output semantics, caveats,
and concrete examples.";

const CLI_AFTER_LONG_HELP: &str = "\
Examples:
  pdf-lay toc paper.pdf
  pdf-lay toc paper.pdf --figures-only
  pdf-lay toc paper.pdf --no-extract-images

  pdf-lay markdown paper.pdf
  pdf-lay markdown paper.pdf -o paper.md
  pdf-lay markdown paper.pdf --section introduction --section results
  pdf-lay markdown paper.pdf --image-dir out/images --image-base ./images -o paper.md

  pdf-lay json paper.pdf
  pdf-lay json paper.pdf --content-only -o paper.json

  pdf-lay chunks paper.pdf --strategy section -o paper.chunks.jsonl
  pdf-lay chunks paper.pdf --max-tokens 2000 --overlap 100 --strategy token

  pdf-lay llm-text paper.pdf
  pdf-lay llm-text paper.pdf --section methods --image-base ./img

Command summary:
  toc
    Print a section tree with estimated tokens, page ranges, and figure/table hints.
  markdown
    Render the full document or selected sections as Markdown with image links.
  json
    Dump the analyzed document as JSON (full geometry-carrying dump, or a
    lightweight content-only projection with `--content-only`).
  chunks
    Split the document into RAG-ready chunks, one JSON object per line (JSONL).
  llm-text
    Render the full document or selected sections as plain LLM-optimized text.";

const TOC_LONG_ABOUT: &str = "\
Analyze a PDF and print its inferred section tree.

The output is designed for quick inspection before selecting sections for LLM
processing or Markdown export. Each line corresponds to one section entry and
contains:

  [level] HEADER  p.START-END  ~TOKENS  fig:N  tab:N

Field meanings:
  [level]
    Detected section depth. Top-level sections are usually level 1.
  HEADER
    The normalized section title that pdf-lay inferred from the PDF.
  p.START-END
    Inclusive page range, displayed as 1-based page numbers for humans.
  ~TOKENS
    Rough token estimate for the section body, useful for chunk planning.
  fig:N / tab:N
    Present only when the section contains figures or tables.

Notes:
  - Child sections are indented beneath their parents.
  - `--figures-only` suppresses entries without figures, but descendants are still
    traversed so figure-bearing children are not lost.
  - Image extraction still runs by default because the main analysis pipeline is
    shared; use `--no-extract-images` if you only need the textual structure.";

const TOC_AFTER_LONG_HELP: &str = "\
Examples:
  pdf-lay toc paper.pdf
    Print the full inferred section hierarchy.

  pdf-lay toc paper.pdf --figures-only
    Show only sections associated with figures.

  pdf-lay toc paper.pdf --image-dir tmp/images
    Save extracted images under `tmp/images` while generating the TOC.

Typical use:
  1. Run `pdf-lay toc ...` to inspect section names and approximate token sizes.
  2. Re-run `pdf-lay markdown ... --section ...` with the section names you want.";

const MARKDOWN_LONG_ABOUT: &str = "\
Analyze a PDF and render the result as Markdown.

What the generated Markdown contains:
  - Section headers derived from the detected document hierarchy.
  - Paragraph text in reconstructed reading order.
  - Figures as Markdown image links pointing at `--image-base`.
  - Figure captions rendered in italic text below the image.
  - Tables inlined as Markdown tables when available.
  - HTML comments of the form `<!-- page N -->` by default.

Section filtering:
  - If `--section` is omitted, the entire analyzed document is emitted.
  - `--section` is repeatable.
  - Matching is case-insensitive and uses partial matching against detected
    section headers.
  - Repeating `--section` works like OR: any matching section is included.

Path behavior:
  - `--image-dir` controls where extracted image files are written on disk.
  - `--image-base` controls the path text embedded into Markdown image links.
    These often match, but do not have to.

Output behavior:
  - Without `-o/--output`, Markdown is written to stdout for easy piping.
  - With `-o/--output`, the final Markdown is written to the specified file.";

const MARKDOWN_AFTER_LONG_HELP: &str = "\
Examples:
  pdf-lay markdown paper.pdf
    Emit the full document as Markdown to stdout.

  pdf-lay markdown paper.pdf -o paper.md
    Write the Markdown to `paper.md`.

  pdf-lay markdown paper.pdf --section introduction --section methods -o subset.md
    Emit only matching sections and their descendants.

  pdf-lay markdown paper.pdf --image-dir out/images --image-base ./images -o paper.md
    Save extracted images under `out/images` and write Markdown links as
    `./images/...` inside the document.

Heading examples:
  --heading-offset 1
    Level-1 sections become `##` and level-2 sections become `###`.
  --heading-offset 0
    Level-1 sections become `#`.";

const JSON_LONG_ABOUT: &str = "\
Analyze a PDF and print the result as JSON.

Two output shapes are available:
  - Full dump (default): the entire analyzed document tree, including every
    text block/line/span bounding box and font metadata, plus figure/table
    geometry. Body text is the raw extracted text with no math conversion.
    This is the heaviest, most complete shape — useful for tooling that needs
    exact layout or wants to re-render the document.
  - Content-only (`--content-only`): a lightweight projection with section
    headers, a breadcrumb path per section, math-converted body text, and a
    light figure/table summary (id, caption, image filename, page). No
    `bbox`, font metadata, or per-line/per-span arrays anywhere in the
    output. This is the shape meant for feeding an LLM/RAG pipeline.

Math conversion:
  - `--math-format` only affects `--content-only` output. The full dump never
    converts math (its body text is always the raw extracted text), so
    `--math-format` is accepted but has no effect without `--content-only`.

Output behavior:
  - Without `-o/--output`, JSON is written to stdout for easy piping.
  - With `-o/--output`, the JSON is written to the specified file.";

const JSON_AFTER_LONG_HELP: &str = "\
Examples:
  pdf-lay json paper.pdf
    Emit the full geometry-carrying JSON dump to stdout.

  pdf-lay json paper.pdf --content-only -o paper.json
    Emit the lightweight, LLM-facing content-only projection to a file.

  pdf-lay json paper.pdf --content-only --math-format unicode
    Content-only projection with Unicode math instead of the LaTeX default.";

const CHUNKS_LONG_ABOUT: &str = "\
Analyze a PDF and split it into LLM-context-window chunks, printed as JSONL
(one JSON object per line) so downstream tooling can stream and parse chunks
independently without loading the whole array into memory.

Each JSONL line contains:
  chunk_id          Sequential chunk index within this document.
  paper_id          Paper identifier (empty when chunking a `--section`
                     subset rather than the whole document).
  section           Header text of the containing section.
  page_range        [first_page, last_page], zero-based, inclusive.
  estimated_tokens  Token count from the configured tokenizer.
  has_continuation  True if this chunk continues in the next chunk.
  text              Chunk body text: math-converted, with inline table
                     Markdown and figure placeholders, and (unless
                     `--no-section-context`) a `[Context: ...]` breadcrumb
                     plus heading line prefix.
  figures           Figures whose insertion point falls in this chunk
                     (id, caption, image filename only, page).
  tables            Tables whose insertion point falls in this chunk
                     (id, caption, rendered text, page).

Split strategy (`--strategy`):
  section    (default) Split at section boundaries first; oversized sections
             are further split by paragraph/sentence/character windows.
  token      Split purely by token count across the whole document.
  paragraph  Split at paragraph boundaries, still attributed to a section.

Tokenizer (`--tokenizer`):
  Omit this flag to use the built-in heuristic tokenizer (no model download,
  CJK-aware character-class estimate). Pass a Hugging Face model id (e.g.
  `Qwen/Qwen2.5-7B`) or a local `tokenizer.json` path to count tokens with a
  real BPE tokenizer instead. This requires a binary built with the
  `real-tokenizer` cargo feature (`cargo build --features real-tokenizer`);
  without it, `--tokenizer` fails fast with an explanatory error rather than
  silently falling back to the heuristic.

Section filtering and math conversion follow the same rules as `markdown`
(see `pdf-lay markdown --help`): `--section` is repeatable and matches by
partial, case-insensitive header text; `--math-format` accepts `latex`
(default), `unicode`, `plain`, or `off`.

Output behavior:
  - Without `-o/--output`, JSONL is written to stdout for easy piping.
  - With `-o/--output`, the JSONL is written to the specified file.";

const CHUNKS_AFTER_LONG_HELP: &str = "\
Examples:
  pdf-lay chunks paper.pdf
    Emit section-boundary chunks (JSONL) to stdout with default sizing.

  pdf-lay chunks paper.pdf --max-tokens 2000 --overlap 100 -o paper.chunks.jsonl
    Smaller chunks with overlap, written to a file.

  pdf-lay chunks paper.pdf --strategy token
    Split purely by token count instead of section boundaries.

  pdf-lay chunks paper.pdf --section methods --no-section-context
    Chunk only the methods section, without the `[Context: ...]` prefix.

  pdf-lay chunks paper.pdf --tokenizer ./tokenizer.json
    Count tokens with a real BPE tokenizer (requires `--features real-tokenizer`).";

const LLM_TEXT_LONG_ABOUT: &str = "\
Analyze a PDF and render LLM-optimized plain text (no Markdown/HTML markup).

What the generated text contains:
  - `#`-style section header lines (one `#` per level, unless disabled).
  - Paragraph text in reconstructed reading order, math-converted.
  - Figures and tables interleaved at their detected insertion point (not
    drained to the end of the section), formatted per `--figure-format`.

Section filtering works exactly like `markdown` (see `pdf-lay markdown
--help`): `--section` is repeatable, matches partially and case-insensitively
against detected headers, and omitting it emits the full document.

Figures and tables:
  - `--no-figures` / `--no-tables` omit figures/tables from the output
    entirely (figures are still detected; they are just not rendered here).
  - `--figure-format` controls how an included figure is rendered:
      placeholder (default) - `[IMAGE: Fig. 1 path/to/img.png]`
      markdown              - `![Fig. 1](path/to/img.png)`
      caption                - caption text only, no image path
      omit                   - omit figures regardless of `--no-figures`
  - `--image-base` is prepended to the image filename in figure references
    (e.g. `./img/p000_img000.png`); the raw on-disk path is never embedded.

Math conversion (`--math-format`, same choices as `markdown`: `latex`
(default), `unicode`, `plain`, `off`): for `llm-text` specifically, `plain`
and `off` currently both disable math conversion (there is no LLM-text path
today that keeps conversion on but renders an ASCII approximation); `latex`
and `unicode` behave as documented for `markdown`.

Output behavior:
  - Without `-o/--output`, text is written to stdout for easy piping.
  - With `-o/--output`, the text is written to the specified file.";

const LLM_TEXT_AFTER_LONG_HELP: &str = "\
Examples:
  pdf-lay llm-text paper.pdf
    Emit the full document as LLM text to stdout.

  pdf-lay llm-text paper.pdf --section methods --section results -o subset.txt
    Emit only matching sections and their descendants.

  pdf-lay llm-text paper.pdf --image-base ./img --figure-format markdown
    Render figures as Markdown image links based at `./img`.

  pdf-lay llm-text paper.pdf --no-tables --no-figures
    Text only, no figure/table content.";

// ---------------------------------------------------------------------------
// Argument structures
// ---------------------------------------------------------------------------

/// PDF layout analysis for academic papers.
#[derive(Parser)]
#[command(
    name = "pdf-lay",
    about = "PDF layout analysis for academic papers",
    long_about = CLI_LONG_ABOUT,
    after_long_help = CLI_AFTER_LONG_HELP,
    arg_required_else_help = true,
    next_line_help = true,
    propagate_version = true,
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(
        about = "Print the inferred table of contents of a PDF",
        long_about = TOC_LONG_ABOUT,
        after_long_help = TOC_AFTER_LONG_HELP
    )]
    Toc(TocArgs),
    #[command(
        about = "Convert a PDF to Markdown",
        long_about = MARKDOWN_LONG_ABOUT,
        after_long_help = MARKDOWN_AFTER_LONG_HELP
    )]
    Markdown(MarkdownArgs),
    #[command(
        about = "Dump the analyzed PDF as JSON",
        long_about = JSON_LONG_ABOUT,
        after_long_help = JSON_AFTER_LONG_HELP
    )]
    Json(JsonArgs),
    #[command(
        name = "chunks",
        about = "Split a PDF into JSONL chunks for RAG/LLM consumption",
        long_about = CHUNKS_LONG_ABOUT,
        after_long_help = CHUNKS_AFTER_LONG_HELP
    )]
    Chunks(ChunksArgs),
    #[command(
        name = "llm-text",
        about = "Render a PDF as LLM-optimized plain text",
        long_about = LLM_TEXT_LONG_ABOUT,
        after_long_help = LLM_TEXT_AFTER_LONG_HELP
    )]
    LlmText(LlmTextArgs),
}

/// Arguments common to all subcommands.
#[derive(Args)]
struct CommonArgs {
    /// Path to the PDF file.
    #[arg(
        value_name = "PDF",
        help = "Path to the input PDF file.",
        long_help = "Path to the input PDF file to analyze.\n\n\
The path may be relative or absolute. The file must already exist and be \
readable. Parsing failures are reported as fatal errors and cause a non-zero \
exit status."
    )]
    path: PathBuf,

    /// Output directory for extracted images.
    #[arg(
        long,
        default_value = "./images",
        value_name = "DIR",
        help = "Directory where extracted embedded images are written.",
        long_help = "Directory where extracted embedded images are written.\n\n\
If image extraction is enabled, pdf-lay creates this directory when needed and \
saves files such as `p000_img000.png` into it. This affects files written to \
disk, not the Markdown link text. For Markdown link paths, use `--image-base` \
on the `markdown` subcommand."
    )]
    image_dir: PathBuf,

    /// Extract embedded images.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set, hide = true)]
    extract_images: bool,

    /// Skip embedded-image extraction.
    #[arg(
        long,
        action = clap::ArgAction::SetTrue,
        help = "Skip embedded-image extraction.",
        long_help = "Skip embedded-image extraction.\n\n\
Use this when you only need text structure or section metadata and do not want \
image files written to disk. Figure references may still be detected from text, \
but no embedded image assets are exported."
    )]
    no_extract_images: bool,
}

impl CommonArgs {
    fn extract_images_enabled(&self) -> bool {
        self.extract_images && !self.no_extract_images
    }
}

/// Arguments for the `toc` subcommand.
#[derive(Args)]
struct TocArgs {
    #[command(flatten)]
    common: CommonArgs,

    /// Show only sections that contain figures.
    #[arg(
        long,
        help = "Show only sections associated with figures.",
        long_help = "Show only sections associated with figures.\n\n\
Entries without figures are filtered from the printed output. The traversal \
still descends into child sections, so figure-bearing descendants remain \
visible even when their parent section is omitted."
    )]
    figures_only: bool,
}

/// Arguments for the `markdown` subcommand.
#[derive(Args)]
struct MarkdownArgs {
    #[command(flatten)]
    common: CommonArgs,

    /// Select sections by header name (case-insensitive, repeatable).
    /// If omitted, all sections are included.
    #[arg(
        long = "section",
        value_name = "NAME",
        num_args = 1,
        help = "Include only sections whose headers match this name.",
        long_help = "Include only sections whose headers match this name.\n\n\
This option is repeatable. Matching is case-insensitive and uses partial \
matching, so values such as `intro`, `Introduction`, or `RESULTS` can match the \
detected section headers. If any `--section` options are supplied, only matching \
sections are emitted. When omitted, the full document is rendered."
    )]
    sections: Vec<String>,

    /// Heading level offset added to the section level (default 1 → level-1 becomes ##).
    #[arg(
        long,
        default_value_t = 1,
        value_name = "N",
        help = "Add an offset to generated Markdown heading levels.",
        long_help = "Add an offset to generated Markdown heading levels.\n\n\
The detected section level is converted to a Markdown heading by adding this \
offset. With the default `1`, a level-1 section becomes `##` and a level-2 \
section becomes `###`. Set this to `0` if you want top-level sections to use \
`#`."
    )]
    heading_offset: u8,

    /// Omit <!-- page N --> comments from the output.
    #[arg(
        long,
        help = "Omit HTML page-marker comments from the Markdown.",
        long_help = "Omit HTML page-marker comments from the Markdown.\n\n\
By default, pdf-lay inserts comments such as `<!-- page 3 -->` to preserve a \
lightweight mapping back to the source PDF. Use this flag when you want cleaner \
Markdown without page annotations."
    )]
    no_page_numbers: bool,

    /// Base path used for image links in the generated Markdown.
    #[arg(
        long,
        value_name = "PATH",
        help = "Base path written into Markdown image links.",
        long_help = "Base path written into Markdown image links.\n\n\
When set, this string is prefixed to extracted image filenames in Markdown image \
links such as `![Fig. 1](PATH/p000_img000.png)`. When omitted, and output is \
written to a file with `-o`, pdf-lay instead computes the link path relative to \
that output file's directory using `--image-dir`, so links resolve no matter \
where the .md is written. This option does not control where image files are \
saved; use `--image-dir` for that."
    )]
    image_base: Option<String>,

    /// Write output to a file instead of stdout.
    #[arg(
        long,
        short = 'o',
        value_name = "FILE",
        help = "Write Markdown to a file instead of stdout.",
        long_help = "Write Markdown to a file instead of stdout.\n\n\
When omitted, the generated Markdown is printed to stdout so it can be piped to \
other tools. When provided, pdf-lay writes the final Markdown bytes directly to \
the specified file path."
    )]
    output: Option<PathBuf>,

    /// How to render mathematical expressions in the Markdown output.
    #[arg(
        long,
        default_value = "latex",
        value_name = "FORMAT",
        value_parser = ["latex", "unicode", "plain", "off"],
        help = "How to render math: latex (default), unicode, plain, or off.",
        long_help = "How to render mathematical expressions detected in the PDF.\n\n\
  latex   - LaTeX notation such as `$E = mc^{2}$` (default; best for LLMs).\n\
  unicode - Unicode math characters such as `E = mc²`.\n\
  plain   - Plain ASCII approximation such as `E = mc^2`.\n\
  off     - No conversion; math spans are emitted as raw extracted glyphs.\n\n\
Math conversion is enabled by default. Use `off` to reproduce the previous \
raw-text behavior."
    )]
    math_format: String,
}

/// Arguments for the `json` subcommand.
#[derive(Args)]
struct JsonArgs {
    #[command(flatten)]
    common: CommonArgs,

    /// Emit the lightweight content-only projection instead of the full dump.
    #[arg(
        long,
        help = "Emit the lightweight, LLM-facing content-only projection.",
        long_help = "Emit the lightweight, LLM-facing content-only projection instead of the \
full geometry-carrying dump.\n\n\
The content-only shape drops `bbox`/font metadata and per-line/per-span \
arrays, keeps math-converted section body text, and summarizes figures/tables \
(id, caption, image filename, page) instead of embedding their full geometry. \
`--math-format` only has an effect when this flag is set; the full dump never \
converts math."
    )]
    content_only: bool,

    /// How to render mathematical expressions (only applies with `--content-only`).
    #[arg(
        long,
        default_value = "latex",
        value_name = "FORMAT",
        value_parser = ["latex", "unicode", "plain", "off"],
        help = "How to render math with --content-only: latex (default), unicode, plain, or off.",
        long_help = "How to render mathematical expressions detected in the PDF, when \
`--content-only` is also set.\n\n\
  latex   - LaTeX notation such as `$E = mc^{2}$` (default; best for LLMs).\n\
  unicode - Unicode math characters such as `E = mc²`.\n\
  plain   - Plain ASCII approximation such as `E = mc^2`.\n\
  off     - No conversion; math spans are emitted as raw extracted glyphs.\n\n\
Ignored (has no effect) without `--content-only`: the full JSON dump always \
carries the raw, unconverted extracted text."
    )]
    math_format: String,

    /// Write output to a file instead of stdout.
    #[arg(
        long,
        short = 'o',
        value_name = "FILE",
        help = "Write JSON to a file instead of stdout.",
        long_help = "Write JSON to a file instead of stdout.\n\n\
When omitted, the generated JSON is printed to stdout so it can be piped to \
other tools. When provided, pdf-lay writes the final JSON bytes directly to \
the specified file path."
    )]
    output: Option<PathBuf>,
}

/// Arguments for the `chunks` subcommand.
#[derive(Args)]
struct ChunksArgs {
    #[command(flatten)]
    common: CommonArgs,

    /// Select sections by header name (case-insensitive, repeatable).
    /// If omitted, the whole document is chunked.
    #[arg(
        long = "section",
        value_name = "NAME",
        num_args = 1,
        help = "Chunk only sections whose headers match this name.",
        long_help = "Chunk only sections whose headers match this name.\n\n\
This option is repeatable and matches like `markdown`'s `--section`: \
case-insensitive, partial matching against detected section headers. When \
omitted, the full document is chunked."
    )]
    sections: Vec<String>,

    /// Maximum tokens per chunk.
    #[arg(
        long,
        default_value_t = 4000,
        value_name = "N",
        help = "Maximum tokens per chunk.",
        long_help = "Maximum tokens per chunk, measured with the configured tokenizer \
(see `--tokenizer`). Sections (or paragraphs/token windows, depending on \
`--strategy`) larger than this budget are split further; no chunk exceeds it \
except unavoidably-oversized single tokens."
    )]
    max_tokens: usize,

    /// Number of tokens of overlap between adjacent chunks.
    #[arg(
        long,
        default_value_t = 200,
        value_name = "N",
        help = "Token overlap between adjacent split chunks.",
        long_help = "Number of tokens of overlap carried from the end of one chunk into the \
start of the next, when a section/strategy has to split a chunk to stay \
within `--max-tokens`. Helps preserve context across a chunk boundary for \
retrieval."
    )]
    overlap: usize,

    /// Chunk-splitting strategy.
    #[arg(
        long,
        default_value = "section",
        value_name = "STRAT",
        value_parser = ["section", "token", "paragraph"],
        help = "How to split the document: section (default), token, or paragraph.",
        long_help = "How to split the document into chunks.\n\n\
  section   - (default) Split at section boundaries first; oversized \
sections are further split by paragraph/sentence/character windows.\n\
  token     - Split purely by token count across the whole document.\n\
  paragraph - Split at paragraph boundaries, still attributed to a section."
    )]
    strategy: String,

    /// Real tokenizer model id or `tokenizer.json` path (default: built-in heuristic).
    #[arg(
        long,
        value_name = "SPEC",
        help = "Tokenizer model id or tokenizer.json path (needs `real-tokenizer` feature).",
        long_help = "Count tokens with a real BPE tokenizer instead of the built-in \
heuristic.\n\n\
`SPEC` is either a Hugging Face Hub model id (e.g. `Qwen/Qwen2.5-7B`, \
downloaded on first use) or a path to a local `tokenizer.json` file. Requires \
a binary built with the `real-tokenizer` cargo feature \
(`cargo build --features real-tokenizer`); without it, this flag fails with \
an explanatory error instead of silently falling back to the heuristic. \
Omit this flag to use the default heuristic tokenizer (no model download, \
CJK-aware)."
    )]
    tokenizer: Option<String>,

    /// Disable the `[Context: ...]` breadcrumb + heading prefix on each chunk.
    #[arg(
        long,
        help = "Disable the breadcrumb/heading prefix prepended to each chunk.",
        long_help = "Disable the `[Context: A > B > C]` breadcrumb plus heading line that is \
otherwise prepended to each chunk's text, restoring plain chunk bodies with \
no positional context."
    )]
    no_section_context: bool,

    /// How to render mathematical expressions in chunk text.
    #[arg(
        long,
        default_value = "latex",
        value_name = "FORMAT",
        value_parser = ["latex", "unicode", "plain", "off"],
        help = "How to render math: latex (default), unicode, plain, or off.",
        long_help = "How to render mathematical expressions detected in the PDF, applied to \
every chunk's body text.\n\n\
  latex   - LaTeX notation such as `$E = mc^{2}$` (default; best for LLMs).\n\
  unicode - Unicode math characters such as `E = mc²`.\n\
  plain   - Plain ASCII approximation such as `E = mc^2`.\n\
  off     - No conversion; math spans are emitted as raw extracted glyphs."
    )]
    math_format: String,

    /// Write output to a file instead of stdout.
    #[arg(
        long,
        short = 'o',
        value_name = "FILE",
        help = "Write JSONL to a file instead of stdout.",
        long_help = "Write JSONL to a file instead of stdout.\n\n\
When omitted, the generated JSONL is printed to stdout (one JSON object per \
line) so it can be piped to other tools. When provided, pdf-lay writes the \
final JSONL bytes directly to the specified file path."
    )]
    output: Option<PathBuf>,
}

/// Arguments for the `llm-text` subcommand.
#[derive(Args)]
struct LlmTextArgs {
    #[command(flatten)]
    common: CommonArgs,

    /// Select sections by header name (case-insensitive, repeatable).
    /// If omitted, all sections are included.
    #[arg(
        long = "section",
        value_name = "NAME",
        num_args = 1,
        help = "Include only sections whose headers match this name.",
        long_help = "Include only sections whose headers match this name.\n\n\
This option is repeatable and matches like `markdown`'s `--section`: \
case-insensitive, partial matching against detected section headers. When \
omitted, the full document is rendered."
    )]
    sections: Vec<String>,

    /// Omit figures from the output entirely.
    #[arg(
        long,
        help = "Omit figures from the output entirely.",
        long_help = "Omit figures from the output entirely. Figures are still detected \
during analysis; they are simply not rendered into the LLM text."
    )]
    no_figures: bool,

    /// Omit tables from the output entirely.
    #[arg(
        long,
        help = "Omit tables from the output entirely.",
        long_help = "Omit tables from the output entirely. Tables are still detected \
during analysis; they are simply not rendered into the LLM text."
    )]
    no_tables: bool,

    /// How an included figure is rendered.
    #[arg(
        long,
        default_value = "placeholder",
        value_name = "FMT",
        value_parser = ["placeholder", "markdown", "caption", "omit"],
        help = "How to render figures: placeholder (default), markdown, caption, or omit.",
        long_help = "How an included figure is rendered.\n\n\
  placeholder - (default) `[IMAGE: Fig. 1 path/to/img.png]`\n\
  markdown    - `![Fig. 1](path/to/img.png)`\n\
  caption     - Caption text only, no image path.\n\
  omit        - Omit figures regardless of `--no-figures`."
    )]
    figure_format: String,

    /// Base path prepended to figure image filenames.
    #[arg(
        long,
        default_value = "./images",
        value_name = "PATH",
        help = "Base path prepended to figure image filenames.",
        long_help = "Base path prepended to extracted image filenames in figure references \
such as `[IMAGE: Fig. 1 PATH/p000_img000.png]`. The raw on-disk path (which \
may be absolute) is never embedded."
    )]
    image_base: String,

    /// How to render mathematical expressions in the LLM text output.
    #[arg(
        long,
        default_value = "latex",
        value_name = "FORMAT",
        value_parser = ["latex", "unicode", "plain", "off"],
        help = "How to render math: latex (default), unicode, plain, or off.",
        long_help = "How to render mathematical expressions detected in the PDF.\n\n\
  latex   - LaTeX notation such as `$E = mc^{2}$` (default; best for LLMs).\n\
  unicode - Unicode math characters such as `E = mc²`.\n\
  plain   - Plain ASCII approximation (see caveat below).\n\
  off     - No conversion; math spans are emitted as raw extracted glyphs.\n\n\
Caveat: for `llm-text` specifically, `plain` and `off` currently produce the \
same result (no math conversion) — there is no LLM-text path today that \
keeps conversion on but renders an ASCII approximation."
    )]
    math_format: String,

    /// Write output to a file instead of stdout.
    #[arg(
        long,
        short = 'o',
        value_name = "FILE",
        help = "Write LLM text to a file instead of stdout.",
        long_help = "Write LLM text to a file instead of stdout.\n\n\
When omitted, the generated text is printed to stdout so it can be piped to \
other tools. When provided, pdf-lay writes the final text bytes directly to \
the specified file path."
    )]
    output: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a `Config` from the common CLI arguments.
fn build_config(common: &CommonArgs) -> Config {
    Config {
        image_output_dir: common.image_dir.clone(),
        extract_images: common.extract_images_enabled(),
        ..Default::default()
    }
}

/// Run `analyze_pdf` and return the result, printing warnings to stderr.
/// Exits with status 1 on error.
fn run_analysis(common: &CommonArgs) -> pdf_lay::AnalysisResult {
    let config = build_config(common);
    match analyze_pdf(&common.path, &config) {
        Ok(result) => {
            for w in &result.warnings {
                eprintln!("[warning] {w}");
            }
            result
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Subcommand: toc
// ---------------------------------------------------------------------------

fn cmd_toc(args: &TocArgs) {
    let result = run_analysis(&args.common);
    let toc = TocGenerator::generate(&result.document);

    print_entries(&toc, 0, args.figures_only);
}

fn print_entries(entries: &[pdf_lay::SectionEntry], indent: usize, figures_only: bool) {
    for entry in entries {
        if figures_only && !entry.has_figures {
            // Descend into children even if the parent has no figures.
            print_entries(&entry.children, indent, figures_only);
            continue;
        }

        let prefix = "  ".repeat(indent);
        let fig_marker = if entry.has_figures {
            format!("  fig:{}", entry.figure_count)
        } else {
            String::new()
        };
        let tab_marker = if entry.has_tables {
            format!("  tab:{}", entry.table_count)
        } else {
            String::new()
        };

        println!(
            "{}[{}] {}  p.{}-{}  ~{} tokens{}{}",
            prefix,
            entry.level,
            entry.header,
            entry.page_range.0 + 1, // display 1-indexed page numbers
            entry.page_range.1 + 1,
            entry.estimated_tokens,
            fig_marker,
            tab_marker,
        );

        print_entries(&entry.children, indent + 1, figures_only);
    }
}

// ---------------------------------------------------------------------------
// Subcommand: markdown
// ---------------------------------------------------------------------------

/// Build the optional math configuration from the `--math-format` flag.
///
/// Returns `None` for `"off"` (raw glyphs, no conversion); otherwise a
/// `MathConfig` with the requested representation.
fn math_config_from_flag(format: &str) -> Option<MathConfig> {
    let representation = match format {
        "off" => return None,
        "unicode" => MathRepresentationPreference::UnicodeMath,
        "plain" => MathRepresentationPreference::PlainText,
        _ => MathRepresentationPreference::LaTeX,
    };
    Some(MathConfig {
        representation,
        ..MathConfig::default()
    })
}

fn cmd_markdown(args: &MarkdownArgs) {
    let result = run_analysis(&args.common);
    let doc = &result.document;

    // If --image-base was given explicitly, honor it (prefix behavior). Otherwise
    // compute image links relative to the output file's directory when writing to
    // a file, so links resolve regardless of where the .md lives.
    let image_base_explicit = args.image_base.is_some();
    let image_base_path = args
        .image_base
        .clone()
        .unwrap_or_else(|| "./images".to_string());
    let (image_dir, output_dir) = if image_base_explicit {
        (None, None)
    } else {
        let output_dir = args.output.as_ref().and_then(|p| p.parent()).map(|par| {
            if par.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                par.to_path_buf()
            }
        });
        (Some(args.common.image_dir.clone()), output_dir)
    };

    let md_config = MarkdownConfig {
        image_base_path,
        include_page_numbers: !args.no_page_numbers,
        heading_offset: args.heading_offset,
        include_metadata_header: false,
        table_as_image: false,
        figure_caption_style: CaptionStyle::Italic,
        math_config: math_config_from_flag(&args.math_format),
        image_dir,
        output_dir,
    };

    let output = if args.sections.is_empty() {
        // Full document — use MarkdownGenerator directly.
        MarkdownGenerator::new(md_config).generate(doc)
    } else {
        // Selected sections only — use SectionSelector.
        let name_refs: Vec<&str> = args.sections.iter().map(String::as_str).collect();
        doc.select_sections(&name_refs).to_markdown(&md_config)
    };

    write_output(output.as_bytes(), args.output.as_deref());
}

// ---------------------------------------------------------------------------
// Subcommand: json
// ---------------------------------------------------------------------------

fn cmd_json(args: &JsonArgs) {
    let result = run_analysis(&args.common);
    let doc = &result.document;

    let json = if args.content_only {
        JsonGenerator::generate_content_only(doc, math_config_from_flag(&args.math_format).as_ref())
    } else {
        JsonGenerator::generate(doc)
    };

    match json {
        Ok(s) => write_output(s.as_bytes(), args.output.as_deref()),
        Err(e) => {
            eprintln!("error: failed to serialize JSON: {e}");
            process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Subcommand: chunks
// ---------------------------------------------------------------------------

/// Lightweight, serializable view of a figure carried on a JSONL chunk line.
///
/// Mirrors `output::content_ir::ContentFigure` (P2-7): identity, caption, and
/// the image *filename* only — never the raw on-disk path (which may be
/// absolute), matching the no-path-leak convention used everywhere else
/// figures are rendered for an LLM (markdown/llm-text image links,
/// `json --content-only`).
#[derive(Serialize)]
struct ChunkFigureView {
    figure_id: String,
    caption: String,
    image_path: Option<String>,
    page: u32,
}

/// Lightweight, serializable view of a table carried on a JSONL chunk line.
///
/// Mirrors `output::content_ir::ContentTable`: identity, caption, and a
/// single flattened textual representation, not the raw per-cell grid.
#[derive(Serialize)]
struct ChunkTableView {
    table_id: String,
    caption: Option<String>,
    text: String,
    page: u32,
}

/// One JSONL line emitted by `pdf-lay chunks`.
///
/// `Chunk` itself already derives `Serialize`, but its `figures`/`tables`
/// fields carry the full `FigureInfo`/`TableInfo` (bounding boxes, the raw
/// on-disk image path, `insertion_point`, ~500 chars of `context_text`, …) —
/// too heavy, and path-leaking, for a RAG-facing JSONL line. `ChunkLine`
/// re-serializes each chunk with those two fields projected down to
/// `ChunkFigureView`/`ChunkTableView` instead, while passing every other
/// field through unchanged.
#[derive(Serialize)]
struct ChunkLine {
    chunk_id: usize,
    paper_id: String,
    section: String,
    page_range: (u32, u32),
    estimated_tokens: usize,
    has_continuation: bool,
    text: String,
    figures: Vec<ChunkFigureView>,
    tables: Vec<ChunkTableView>,
}

impl From<&Chunk> for ChunkLine {
    fn from(chunk: &Chunk) -> Self {
        ChunkLine {
            chunk_id: chunk.chunk_id,
            paper_id: chunk.paper_id.clone(),
            section: chunk.section.clone(),
            page_range: chunk.page_range,
            estimated_tokens: chunk.estimated_tokens,
            has_continuation: chunk.has_continuation,
            text: chunk.text.clone(),
            figures: chunk.figures.iter().map(figure_view).collect(),
            tables: chunk.tables.iter().map(table_view).collect(),
        }
    }
}

fn figure_view(fig: &FigureInfo) -> ChunkFigureView {
    ChunkFigureView {
        figure_id: fig.figure_id.clone(),
        caption: fig.caption_text.clone(),
        image_path: figure_image_filename(fig),
        page: fig.image.page,
    }
}

fn table_view(table: &TableInfo) -> ChunkTableView {
    ChunkTableView {
        table_id: table.table_id.clone(),
        caption: table.caption.clone(),
        text: table_representation_text(table),
        page: table.page,
    }
}

/// Reduce a figure's image reference to its filename, never the raw on-disk
/// path (which may be absolute) — same fallback as `render_core::write_figure`
/// and `output::content_ir::project_figure` when the path has no file name
/// component. `None` for a vector figure (no raster image was extracted).
fn figure_image_filename(fig: &FigureInfo) -> Option<String> {
    fig.image.filename()
}

/// Flatten whichever `TableRepresentation` tier a table resolved to into a
/// single text string (same projection as `output::content_ir::project_table`).
fn table_representation_text(table: &TableInfo) -> String {
    match &table.representation {
        TableRepresentation::Markdown { markdown_text, .. } => markdown_text.clone(),
        TableRepresentation::Csv { csv_text, .. } => csv_text.clone(),
        TableRepresentation::PlainText { text, .. } => text.clone(),
    }
}

/// Load a `Tokenizer` for `--tokenizer <SPEC>`, or `None` if the flag was not
/// given (the caller then falls back to `Chunker::new`'s default
/// `HeuristicTokenizer`).
///
/// Behind the `real-tokenizer` feature: loads a real BPE tokenizer from a
/// local `tokenizer.json` path (if `SPEC` names an existing file) or a
/// Hugging Face Hub model id otherwise.
#[cfg(feature = "real-tokenizer")]
fn load_tokenizer(spec: Option<&str>) -> Option<Box<dyn Tokenizer>> {
    let spec = spec?;
    let path = std::path::Path::new(spec);
    let result = if path.exists() {
        pdf_lay::HfTokenizer::from_file(path)
    } else {
        pdf_lay::HfTokenizer::from_pretrained(spec)
    };
    match result {
        Ok(t) => Some(Box::new(t) as Box<dyn Tokenizer>),
        Err(e) => {
            eprintln!("error: failed to load tokenizer '{spec}': {e}");
            process::exit(1);
        }
    }
}

/// `--tokenizer` under the default build (no `real-tokenizer` feature): fail
/// fast with an actionable error rather than silently ignoring the flag and
/// falling back to the heuristic tokenizer (No Silent Drop in spirit — a
/// user-requested tokenizer must not be quietly swapped out).
#[cfg(not(feature = "real-tokenizer"))]
fn load_tokenizer(spec: Option<&str>) -> Option<Box<dyn Tokenizer>> {
    let spec = spec?;
    eprintln!(
        "error: --tokenizer '{spec}' requires a binary built with the `real-tokenizer` \
cargo feature (cargo build --features real-tokenizer); this binary was built without it \
and only supports the built-in heuristic tokenizer. Omit --tokenizer to use it."
    );
    process::exit(1);
}

fn cmd_chunks(args: &ChunksArgs) {
    if args.max_tokens == 0 {
        eprintln!("error: --max-tokens must be greater than 0");
        process::exit(1);
    }

    let result = run_analysis(&args.common);
    let doc = &result.document;

    let split_strategy = match args.strategy.as_str() {
        "token" => SplitStrategy::TokenCount,
        "paragraph" => SplitStrategy::Paragraph,
        _ => SplitStrategy::SectionBoundary,
    };

    let chunk_config = ChunkConfig {
        max_tokens: args.max_tokens,
        overlap_tokens: args.overlap,
        split_strategy,
        include_section_context: !args.no_section_context,
        math_config: math_config_from_flag(&args.math_format),
    };

    let chunker = match load_tokenizer(args.tokenizer.as_deref()) {
        Some(tokenizer) => Chunker::with_tokenizer(chunk_config, tokenizer),
        None => Chunker::new(chunk_config),
    };

    let chunks = if args.sections.is_empty() {
        chunker.chunk(doc)
    } else {
        let name_refs: Vec<&str> = args.sections.iter().map(String::as_str).collect();
        let selector = doc.select_sections(&name_refs);
        chunker.chunk_sections(selector.sections())
    };

    let mut buf = String::new();
    for chunk in &chunks {
        match serde_json::to_string(&ChunkLine::from(chunk)) {
            Ok(line) => {
                buf.push_str(&line);
                buf.push('\n');
            }
            Err(e) => {
                eprintln!("error: failed to serialize chunk {}: {e}", chunk.chunk_id);
                process::exit(1);
            }
        }
    }

    write_output(buf.as_bytes(), args.output.as_deref());
}

// ---------------------------------------------------------------------------
// Subcommand: llm-text
// ---------------------------------------------------------------------------

/// Map `--figure-format` to a `FigureTextFormat` (same mapping as the Python
/// binding's `to_llm_text`).
fn figure_format_from_flag(format: &str) -> FigureTextFormat {
    match format {
        "markdown" => FigureTextFormat::MarkdownLink,
        "caption" => FigureTextFormat::CaptionOnly,
        "omit" => FigureTextFormat::Omit,
        _ => FigureTextFormat::Placeholder,
    }
}

/// Map the shared `--math-format` flag to a `MathRepresentationPreference`
/// for the `llm-text` subcommand.
///
/// `LlmTextConfig` (unlike `MarkdownConfig`/`ChunkConfig`) has no
/// `Option<MathConfig>` escape hatch to fully disable conversion — only a
/// `MathRepresentationPreference` enum. Per
/// `selector::llm_text::math_config_from_llm` (see its
/// `test_plain_text_representation_no_delimiters` test), requesting
/// `PlainText` is how that layer already spells "no math conversion" (raw
/// glyphs). That collapses this CLI's `off` and `plain` choices onto the
/// same underlying behavior for `llm-text` specifically — there is currently
/// no way to request "keep math conversion on but render an ASCII
/// approximation" through `LlmTextConfig`. `markdown`/`json --content-only`/
/// `chunks` do not have this limitation (they build an `Option<MathConfig>`
/// directly via `math_config_from_flag`).
fn llm_math_representation_from_flag(format: &str) -> MathRepresentationPreference {
    match format {
        "off" | "plain" => MathRepresentationPreference::PlainText,
        "unicode" => MathRepresentationPreference::UnicodeMath,
        _ => MathRepresentationPreference::LaTeX,
    }
}

fn cmd_llm_text(args: &LlmTextArgs) {
    let result = run_analysis(&args.common);
    let doc = &result.document;

    let config = LlmTextConfig {
        include_figures: !args.no_figures,
        include_tables: !args.no_tables,
        include_section_headers: true,
        math_representation: llm_math_representation_from_flag(&args.math_format),
        figure_format: figure_format_from_flag(&args.figure_format),
        image_base: args.image_base.clone(),
    };

    let output = if args.sections.is_empty() {
        // Full document — every top-level section (children are appended
        // recursively by `LlmTextGenerator::generate`).
        let top_level: Vec<&pdf_lay::Section> = doc.sections.iter().collect();
        LlmTextGenerator::new(config).generate(&top_level)
    } else {
        let name_refs: Vec<&str> = args.sections.iter().map(String::as_str).collect();
        doc.select_sections(&name_refs).to_llm_text(&config)
    };

    write_output(output.as_bytes(), args.output.as_deref());
}

/// Write bytes to the configured output destination (file or stdout).
fn write_output(data: &[u8], path: Option<&std::path::Path>) {
    match path {
        Some(p) => {
            if let Err(e) = fs::write(p, data) {
                eprintln!("error: failed to write to {}: {e}", p.display());
                process::exit(1);
            }
        }
        None => {
            let stdout = io::stdout();
            if let Err(e) = stdout.lock().write_all(data) {
                eprintln!("error: failed to write to stdout: {e}");
                process::exit(1);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    env_logger::init();
    let cli = Cli::parse();
    match &cli.command {
        Commands::Toc(args) => cmd_toc(args),
        Commands::Markdown(args) => cmd_markdown(args),
        Commands::Json(args) => cmd_json(args),
        Commands::Chunks(args) => cmd_chunks(args),
        Commands::LlmText(args) => cmd_llm_text(args),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::{CommandFactory, Parser};

    use super::Cli;

    #[test]
    fn root_long_help_includes_pipeline_and_examples() {
        let mut cmd = Cli::command();
        let help = cmd.render_long_help().to_string();

        assert!(help.contains("core analysis pipeline"));
        assert!(help.contains("Examples:"));
        assert!(help.contains("pdf-lay markdown paper.pdf -o paper.md"));
    }

    #[test]
    fn toc_long_help_explains_entry_format() {
        let mut cmd = Cli::command();
        let help = cmd
            .find_subcommand_mut("toc")
            .expect("missing toc subcommand")
            .render_long_help()
            .to_string();

        assert!(help.contains("[level] HEADER  p.START-END  ~TOKENS"));
        assert!(help.contains("--figures-only"));
        assert!(help.contains("--no-extract-images"));
    }

    #[test]
    fn markdown_long_help_explains_image_dir_and_image_base() {
        let mut cmd = Cli::command();
        let help = cmd
            .find_subcommand_mut("markdown")
            .expect("missing markdown subcommand")
            .render_long_help()
            .to_string();

        assert!(help.contains("--image-dir"));
        assert!(help.contains("--image-base"));
        assert!(help.contains("Section filtering:"));
    }

    #[test]
    fn markdown_math_format_defaults_to_latex() {
        let cli = Cli::try_parse_from(["pdf-lay", "markdown", "paper.pdf"])
            .expect("markdown should parse with defaults");
        let super::Commands::Markdown(args) = cli.command else {
            panic!("expected markdown command");
        };
        assert_eq!(args.math_format, "latex");
        assert!(super::math_config_from_flag(&args.math_format).is_some());
    }

    #[test]
    fn markdown_math_format_off_disables_conversion() {
        let cli = Cli::try_parse_from(["pdf-lay", "markdown", "paper.pdf", "--math-format", "off"])
            .expect("--math-format off should parse");
        let super::Commands::Markdown(args) = cli.command else {
            panic!("expected markdown command");
        };
        assert_eq!(args.math_format, "off");
        assert!(super::math_config_from_flag(&args.math_format).is_none());
    }

    #[test]
    fn markdown_math_format_rejects_unknown() {
        let result =
            Cli::try_parse_from(["pdf-lay", "markdown", "paper.pdf", "--math-format", "bogus"]);
        assert!(result.is_err(), "unknown math format should be rejected");
    }

    #[test]
    fn no_extract_images_flag_is_accepted() {
        let cli = Cli::try_parse_from(["pdf-lay", "toc", "paper.pdf", "--no-extract-images"])
            .expect("expected --no-extract-images to parse");

        let super::Commands::Toc(args) = cli.command else {
            panic!("expected toc command");
        };

        assert!(args.common.no_extract_images);
        assert!(!args.common.extract_images_enabled());
    }

    // ---- json ----

    #[test]
    fn json_subcommand_parses_with_defaults() {
        let cli = Cli::try_parse_from(["pdf-lay", "json", "paper.pdf"])
            .expect("json should parse with defaults");
        let super::Commands::Json(args) = cli.command else {
            panic!("expected json command");
        };
        assert!(!args.content_only);
        assert_eq!(args.math_format, "latex");
        assert!(args.output.is_none());
    }

    #[test]
    fn json_content_only_flag_parses() {
        let cli = Cli::try_parse_from([
            "pdf-lay",
            "json",
            "paper.pdf",
            "--content-only",
            "--math-format",
            "unicode",
            "-o",
            "out.json",
        ])
        .expect("--content-only should parse");
        let super::Commands::Json(args) = cli.command else {
            panic!("expected json command");
        };
        assert!(args.content_only);
        assert_eq!(args.math_format, "unicode");
        assert_eq!(args.output, Some(PathBuf::from("out.json")));
    }

    #[test]
    fn json_math_format_rejects_unknown() {
        let result =
            Cli::try_parse_from(["pdf-lay", "json", "paper.pdf", "--math-format", "bogus"]);
        assert!(result.is_err(), "unknown math format should be rejected");
    }

    #[test]
    fn json_long_help_explains_content_only() {
        let mut cmd = Cli::command();
        let help = cmd
            .find_subcommand_mut("json")
            .expect("missing json subcommand")
            .render_long_help()
            .to_string();

        assert!(help.contains("--content-only"));
        assert!(help.contains("Content-only"));
    }

    // ---- chunks ----

    #[test]
    fn chunks_subcommand_parses_defaults() {
        let cli = Cli::try_parse_from(["pdf-lay", "chunks", "paper.pdf"])
            .expect("chunks should parse with defaults");
        let super::Commands::Chunks(args) = cli.command else {
            panic!("expected chunks command");
        };
        assert_eq!(args.max_tokens, 4000);
        assert_eq!(args.overlap, 200);
        assert_eq!(args.strategy, "section");
        assert!(!args.no_section_context);
        assert_eq!(args.math_format, "latex");
        assert!(args.tokenizer.is_none());
        assert!(args.sections.is_empty());
        assert!(args.output.is_none());
    }

    #[test]
    fn chunks_subcommand_parses_all_flags() {
        let cli = Cli::try_parse_from([
            "pdf-lay",
            "chunks",
            "paper.pdf",
            "--max-tokens",
            "2000",
            "--overlap",
            "100",
            "--strategy",
            "token",
            "--section",
            "methods",
            "--section",
            "results",
            "--tokenizer",
            "./tokenizer.json",
            "--no-section-context",
            "--math-format",
            "off",
            "-o",
            "out.jsonl",
        ])
        .expect("chunks should parse with all flags");
        let super::Commands::Chunks(args) = cli.command else {
            panic!("expected chunks command");
        };
        assert_eq!(args.max_tokens, 2000);
        assert_eq!(args.overlap, 100);
        assert_eq!(args.strategy, "token");
        assert_eq!(
            args.sections,
            vec!["methods".to_string(), "results".to_string()]
        );
        assert_eq!(args.tokenizer.as_deref(), Some("./tokenizer.json"));
        assert!(args.no_section_context);
        assert_eq!(args.math_format, "off");
        assert_eq!(args.output, Some(PathBuf::from("out.jsonl")));
    }

    #[test]
    fn chunks_strategy_rejects_unknown() {
        let result = Cli::try_parse_from(["pdf-lay", "chunks", "paper.pdf", "--strategy", "bogus"]);
        assert!(result.is_err(), "unknown strategy should be rejected");
    }

    #[test]
    fn chunks_long_help_lists_strategy() {
        let mut cmd = Cli::command();
        let help = cmd
            .find_subcommand_mut("chunks")
            .expect("missing chunks subcommand")
            .render_long_help()
            .to_string();

        assert!(help.contains("--strategy"));
        assert!(help.contains("section"));
        assert!(help.contains("token"));
        assert!(help.contains("paragraph"));
        assert!(help.contains("--tokenizer"));
        assert!(help.contains("real-tokenizer"));
        assert!(help.contains("chunk_id"));
    }

    // ---- llm-text ----

    #[test]
    fn llm_text_subcommand_parses_with_defaults() {
        let cli = Cli::try_parse_from(["pdf-lay", "llm-text", "paper.pdf"])
            .expect("llm-text should parse with defaults");
        let super::Commands::LlmText(args) = cli.command else {
            panic!("expected llm-text command");
        };
        assert!(args.sections.is_empty());
        assert!(!args.no_figures);
        assert!(!args.no_tables);
        assert_eq!(args.figure_format, "placeholder");
        assert_eq!(args.image_base, "./images");
        assert_eq!(args.math_format, "latex");
        assert!(args.output.is_none());
    }

    #[test]
    fn llm_text_subcommand_parses_all_flags() {
        let cli = Cli::try_parse_from([
            "pdf-lay",
            "llm-text",
            "paper.pdf",
            "--section",
            "intro",
            "--no-figures",
            "--no-tables",
            "--figure-format",
            "markdown",
            "--image-base",
            "./img",
            "--math-format",
            "unicode",
            "-o",
            "out.txt",
        ])
        .expect("llm-text should parse with all flags");
        let super::Commands::LlmText(args) = cli.command else {
            panic!("expected llm-text command");
        };
        assert_eq!(args.sections, vec!["intro".to_string()]);
        assert!(args.no_figures);
        assert!(args.no_tables);
        assert_eq!(args.figure_format, "markdown");
        assert_eq!(args.image_base, "./img");
        assert_eq!(args.math_format, "unicode");
        assert_eq!(args.output, Some(PathBuf::from("out.txt")));
    }

    #[test]
    fn llm_text_figure_format_rejects_unknown() {
        let result = Cli::try_parse_from([
            "pdf-lay",
            "llm-text",
            "paper.pdf",
            "--figure-format",
            "bogus",
        ]);
        assert!(result.is_err(), "unknown figure format should be rejected");
    }

    #[test]
    fn llm_text_long_help_documents_math_caveat() {
        let mut cmd = Cli::command();
        let help = cmd
            .find_subcommand_mut("llm-text")
            .expect("missing llm-text subcommand")
            .render_long_help()
            .to_string();

        assert!(help.contains("--figure-format"));
        assert!(help.contains("--image-base"));
        assert!(help.contains("--math-format"));
    }

    // ---- math/figure-format flag mapping helpers ----

    #[test]
    fn llm_math_representation_off_and_plain_both_disable_conversion() {
        assert!(matches!(
            super::llm_math_representation_from_flag("off"),
            super::MathRepresentationPreference::PlainText
        ));
        assert!(matches!(
            super::llm_math_representation_from_flag("plain"),
            super::MathRepresentationPreference::PlainText
        ));
    }

    #[test]
    fn llm_math_representation_maps_latex_and_unicode() {
        assert!(matches!(
            super::llm_math_representation_from_flag("latex"),
            super::MathRepresentationPreference::LaTeX
        ));
        assert!(matches!(
            super::llm_math_representation_from_flag("unicode"),
            super::MathRepresentationPreference::UnicodeMath
        ));
    }

    #[test]
    fn figure_format_from_flag_maps_all_variants() {
        assert!(matches!(
            super::figure_format_from_flag("markdown"),
            super::FigureTextFormat::MarkdownLink
        ));
        assert!(matches!(
            super::figure_format_from_flag("caption"),
            super::FigureTextFormat::CaptionOnly
        ));
        assert!(matches!(
            super::figure_format_from_flag("omit"),
            super::FigureTextFormat::Omit
        ));
        assert!(matches!(
            super::figure_format_from_flag("placeholder"),
            super::FigureTextFormat::Placeholder
        ));
    }

    // ---- CLI-only tokenizer flag: default build must not silently ignore it ----

    #[cfg(not(feature = "real-tokenizer"))]
    #[test]
    fn tokenizer_flag_parses_even_without_real_tokenizer_feature() {
        // Parsing always succeeds regardless of the feature (clap has no
        // opinion on it); the fail-fast behavior lives in `load_tokenizer`,
        // which calls `process::exit` and so cannot be unit-tested directly
        // here — this only guards that the flag itself is recognized.
        let cli = Cli::try_parse_from([
            "pdf-lay",
            "chunks",
            "paper.pdf",
            "--tokenizer",
            "Qwen/Qwen2.5-7B",
        ])
        .expect("--tokenizer should parse regardless of the real-tokenizer feature");
        let super::Commands::Chunks(args) = cli.command else {
            panic!("expected chunks command");
        };
        assert_eq!(args.tokenizer.as_deref(), Some("Qwen/Qwen2.5-7B"));
    }
}
