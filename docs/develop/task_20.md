# Task 20: CLI (toc + markdown Subcommands)

## Overview

Implement the `pdf-lay-cli` binary using `clap` 4.x. Two subcommands:

- `pdf-lay toc <PDF>` — print the table of contents (section hierarchy with page numbers and
  token estimates) to stdout.
- `pdf-lay markdown <PDF>` — convert the PDF to Markdown and print to stdout.

Both subcommands accept common options:
- `--extract-images` / `--no-extract-images` (default: true)
- `--image-dir <PATH>` (default: `./images`)

`markdown` additionally accepts:
- `--sections <NAME>...` — select specific sections by header name (repeatable flag)
- `--heading-offset <N>` (default: 1)
- `--no-page-numbers` — omit page number comments

## Related Information
- **Plan**: `/Users/sonokawa/.claude/plans/fuzzy-drifting-summit.md` (Task 20)
- **Design doc**: `docs/arch/02_DESIGN.md` § 5.3 CLI
- **Overview**: `docs/develop/overview.md`
- **Dependencies**: Task 18 (public crate finalization) must be completed first

## Files to Modify

- [ ] `crates/pdf-lay-cli/src/main.rs` — replace stub with full implementation

## Implementation Steps

### Step 1: `crates/pdf-lay-cli/Cargo.toml`

Verify dependencies:

```toml
[package]
name = "pdf-lay-cli"
version.workspace = true
edition.workspace = true
license.workspace = true

[[bin]]
name = "pdf-lay"
path = "src/main.rs"

[dependencies]
pdf-lay = { path = "../pdf-lay" }
clap = { workspace = true, features = ["derive"] }
```

### Step 2: Full `crates/pdf-lay-cli/src/main.rs`

```rust
//! CLI for pdf-lay: PDF layout analysis for academic papers.

use std::path::PathBuf;
use std::process;

use clap::{Args, Parser, Subcommand};
use pdf_lay::{
    analyze_pdf,
    config::{CaptionStyle, Config, MarkdownConfig, SplitStrategy},
    selector::TocGenerator,
    selector::SectionSelector,
    AnalysisResult,
};

// ---------------------------------------------------------------------------
// Argument structure
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "pdf-lay",
    about = "PDF layout analysis for academic papers",
    version,
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

#[derive(Args)]
struct CommonArgs {
    /// Path to the PDF file.
    #[arg(value_name = "PDF")]
    path: PathBuf,

    /// Output directory for extracted images.
    #[arg(long, default_value = "./images", value_name = "DIR")]
    image_dir: PathBuf,

    /// Extract embedded images (pass --no-extract-images to disable).
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    extract_images: bool,
}

#[derive(Args)]
struct TocArgs {
    #[command(flatten)]
    common: CommonArgs,

    /// Show only sections with figures.
    #[arg(long)]
    figures_only: bool,
}

#[derive(Args)]
struct MarkdownArgs {
    #[command(flatten)]
    common: CommonArgs,

    /// Select sections by header name (case-insensitive, repeatable).
    /// If not specified, all sections are included.
    #[arg(long = "section", value_name = "NAME", num_args = 1)]
    sections: Vec<String>,

    /// Heading level offset (1 = start at ##, 0 = start at #).
    #[arg(long, default_value_t = 1, value_name = "N")]
    heading_offset: u8,

    /// Omit <!-- page N --> comments.
    #[arg(long)]
    no_page_numbers: bool,

    /// Image base path in generated Markdown links.
    #[arg(long, default_value = "./images", value_name = "PATH")]
    image_base: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_config(common: &CommonArgs) -> Config {
    Config {
        image_output_dir: common.image_dir.clone(),
        extract_images: common.extract_images,
        ..Default::default()
    }
}

fn run_analysis(common: &CommonArgs) -> AnalysisResult {
    let config = build_config(common);
    match analyze_pdf(&common.path, &config) {
        Ok(result) => {
            if !result.warnings.is_empty() {
                for w in &result.warnings {
                    eprintln!("[warning] {:?}", w);
                }
            }
            result
        }
        Err(e) => {
            eprintln!("error: {}", e);
            process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Subcommand handlers
// ---------------------------------------------------------------------------

fn cmd_toc(args: &TocArgs) {
    let result = run_analysis(&args.common);
    let doc = &result.document;
    let toc = TocGenerator::generate(doc);

    // Print flat display of all entries.
    fn print_entries(entries: &[pdf_lay::SectionEntry], indent: usize, figures_only: bool) {
        for entry in entries {
            if figures_only && !entry.has_figures {
                // Still descend in case children have figures.
                print_entries(&entry.children, indent, figures_only);
                continue;
            }
            let prefix = "  ".repeat(indent);
            let fig_marker = if entry.has_figures {
                format!(" [fig:{}]", entry.figure_count)
            } else {
                String::new()
            };
            let tab_marker = if entry.has_tables {
                format!(" [tab:{}]", entry.table_count)
            } else {
                String::new()
            };
            println!(
                "{}{:<3} {}  (p.{}-{}, ~{} tokens){}{}",
                prefix,
                format!("L{}", entry.level),
                entry.header,
                entry.page_range.0 + 1,   // 1-indexed for users
                entry.page_range.1 + 1,
                entry.estimated_tokens,
                fig_marker,
                tab_marker,
            );
            print_entries(&entry.children, indent + 1, figures_only);
        }
    }

    print_entries(&toc, 0, args.figures_only);
}

fn cmd_markdown(args: &MarkdownArgs) {
    let result = run_analysis(&args.common);
    let doc = &result.document;

    let markdown_config = MarkdownConfig {
        image_base_path: args.image_base.clone(),
        include_page_numbers: !args.no_page_numbers,
        heading_offset: args.heading_offset,
        include_metadata_header: false,
        table_as_image: false,
        figure_caption_style: CaptionStyle::Italic,
    };

    let output = if args.sections.is_empty() {
        // Full document.
        pdf_lay::output::markdown::MarkdownGenerator::new(markdown_config).generate(doc)
    } else {
        // Selected sections only.
        let name_refs: Vec<&str> = args.sections.iter().map(|s| s.as_str()).collect();
        let selector = SectionSelector::by_names(doc, &name_refs);
        selector.to_markdown(&markdown_config)
    };

    print!("{}", output);
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();
    match &cli.command {
        Commands::Toc(args) => cmd_toc(args),
        Commands::Markdown(args) => cmd_markdown(args),
    }
}
```

### Step 3: Update `crates/pdf-lay/src/lib.rs` if needed

The CLI imports `pdf_lay::output::markdown::MarkdownGenerator` and `pdf_lay::selector::TocGenerator`
and `pdf_lay::selector::SectionSelector` and `pdf_lay::SectionEntry`. Ensure these are all
publicly accessible through the `pdf-lay` facade crate. Add re-exports if missing:

```rust
// In crates/pdf-lay/src/lib.rs — add if not already present:
pub use pdf_lay_core::output;      // exposes output::markdown::MarkdownGenerator
pub use pdf_lay_core::selector;    // exposes selector::TocGenerator, SectionSelector
```

Alternatively, re-export the specific types:

```rust
pub use pdf_lay_core::output::markdown::MarkdownGenerator;
pub use pdf_lay_core::selector::{SectionSelector, SectionEntry, TocGenerator};
```

### Step 4: Smoke Test (manual)

Build and test manually using any PDF:

```bash
cargo build -p pdf-lay-cli

# Test toc subcommand.
./target/debug/pdf-lay toc path/to/paper.pdf

# Test markdown subcommand (full document).
./target/debug/pdf-lay markdown path/to/paper.pdf

# Test markdown with section filter.
./target/debug/pdf-lay markdown path/to/paper.pdf --section INTRODUCTION --section METHODS

# Test with figures-only filter.
./target/debug/pdf-lay toc path/to/paper.pdf --figures-only
```

Expected output of `toc` for a typical IEEE paper:
```
L1  ABSTRACT  (p.1-1, ~120 tokens)
L1  INTRODUCTION  (p.1-2, ~450 tokens) [fig:1]
L1  RELATED WORK  (p.2-3, ~380 tokens)
L1  METHODS  (p.3-5, ~820 tokens) [fig:3]
  L2  Data Collection  (p.3-4, ~310 tokens)
  L2  Model Architecture  (p.4-5, ~510 tokens) [fig:2]
L1  RESULTS  (p.5-7, ~620 tokens) [tab:2]
L1  CONCLUSION  (p.7-8, ~280 tokens)
```

## Acceptance Criteria

- [ ] `cargo build -p pdf-lay-cli` succeeds
- [ ] `pdf-lay --help` prints help without error
- [ ] `pdf-lay toc --help` and `pdf-lay markdown --help` print subcommand help
- [ ] `pdf-lay toc <any-existing-file>` exits 0 (even if result has warnings)
- [ ] `pdf-lay toc <nonexistent-file>` exits with non-zero status and error message on stderr
- [ ] `pdf-lay markdown <PDF> --section INTRODUCTION` filters to matching sections only
- [ ] `--no-page-numbers` suppresses `<!-- page N -->` comments
- [ ] `cargo clippy -p pdf-lay-cli -- -D warnings` passes

## Dependencies

- Task 18 (public crate + full pipeline) must be completed first.

## Commit Message

```
feat(cli): add pdf-lay CLI with toc and markdown subcommands using clap 4
```
