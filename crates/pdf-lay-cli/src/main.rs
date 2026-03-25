//! `pdf-lay` CLI — PDF layout analysis for academic papers.
//!
//! Subcommands:
//! - `toc <PDF>`      — print the table of contents to stdout.
//! - `markdown <PDF>` — convert the PDF to Markdown and print to stdout.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process;

use clap::{Args, Parser, Subcommand};
use pdf_lay::{CaptionStyle, Config, MarkdownConfig, MarkdownGenerator, TocGenerator, analyze_pdf};

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

Command summary:
  toc
    Print a section tree with estimated tokens, page ranges, and figure/table hints.
  markdown
    Render the full document or selected sections as Markdown with image links.";

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
        default_value = "./images",
        value_name = "PATH",
        help = "Base path written into Markdown image links.",
        long_help = "Base path written into Markdown image links.\n\n\
This string is prefixed to extracted image filenames when pdf-lay emits \
Markdown image links such as `![Fig. 1](PATH/p000_img000.png)`. It does not \
control where image files are saved; use `--image-dir` for that."
    )]
    image_base: String,

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

fn cmd_markdown(args: &MarkdownArgs) {
    let result = run_analysis(&args.common);
    let doc = &result.document;

    let md_config = MarkdownConfig {
        image_base_path: args.image_base.clone(),
        include_page_numbers: !args.no_page_numbers,
        heading_offset: args.heading_offset,
        include_metadata_header: false,
        table_as_image: false,
        figure_caption_style: CaptionStyle::Italic,
        math_config: None,
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
    }
}

#[cfg(test)]
mod tests {
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
    fn no_extract_images_flag_is_accepted() {
        let cli = Cli::try_parse_from(["pdf-lay", "toc", "paper.pdf", "--no-extract-images"])
            .expect("expected --no-extract-images to parse");

        let super::Commands::Toc(args) = cli.command else {
            panic!("expected toc command");
        };

        assert!(args.common.no_extract_images);
        assert!(!args.common.extract_images_enabled());
    }
}
