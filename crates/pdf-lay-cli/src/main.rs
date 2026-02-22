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

// ---------------------------------------------------------------------------
// Argument structures
// ---------------------------------------------------------------------------

/// PDF layout analysis for academic papers.
#[derive(Parser)]
#[command(
    name = "pdf-lay",
    about = "PDF layout analysis for academic papers",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Print the table of contents of a PDF.
    Toc(TocArgs),
    /// Convert a PDF to Markdown.
    Markdown(MarkdownArgs),
}

/// Arguments common to all subcommands.
#[derive(Args)]
struct CommonArgs {
    /// Path to the PDF file.
    #[arg(value_name = "PDF")]
    path: PathBuf,

    /// Output directory for extracted images.
    #[arg(long, default_value = "./images", value_name = "DIR")]
    image_dir: PathBuf,

    /// Extract embedded images (use --no-extract-images to disable).
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    extract_images: bool,
}

/// Arguments for the `toc` subcommand.
#[derive(Args)]
struct TocArgs {
    #[command(flatten)]
    common: CommonArgs,

    /// Show only sections that contain figures.
    #[arg(long)]
    figures_only: bool,
}

/// Arguments for the `markdown` subcommand.
#[derive(Args)]
struct MarkdownArgs {
    #[command(flatten)]
    common: CommonArgs,

    /// Select sections by header name (case-insensitive, repeatable).
    /// If omitted, all sections are included.
    #[arg(long = "section", value_name = "NAME", num_args = 1)]
    sections: Vec<String>,

    /// Heading level offset added to the section level (default 1 → level-1 becomes ##).
    #[arg(long, default_value_t = 1, value_name = "N")]
    heading_offset: u8,

    /// Omit <!-- page N --> comments from the output.
    #[arg(long)]
    no_page_numbers: bool,

    /// Base path used for image links in the generated Markdown.
    #[arg(long, default_value = "./images", value_name = "PATH")]
    image_base: String,

    /// Write output to a file instead of stdout.
    #[arg(long, short = 'o', value_name = "FILE")]
    output: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a `Config` from the common CLI arguments.
fn build_config(common: &CommonArgs) -> Config {
    Config {
        image_output_dir: common.image_dir.clone(),
        extract_images: common.extract_images,
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
                eprintln!("[warning] {w:?}");
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
