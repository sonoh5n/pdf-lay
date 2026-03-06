# Task 01: Workspace Skeleton + CI

## Overview

Create the complete Cargo workspace skeleton with all four crates, shared configuration files,
and a GitHub Actions CI pipeline. This task produces a compilable (though mostly empty) project
that enforces `cargo fmt`, `cargo clippy -D warnings`, and `cargo test` on every push.

## Related Information
- **Plan**: `/Users/sonokawa/.claude/plans/fuzzy-drifting-summit.md` (Task 1)
- **Overview**: `docs/develop/overview.md`
- **Dependencies**: None — this is the first task

## Files to Create

- [ ] `Cargo.toml` (workspace root)
- [ ] `crates/pdf-lay-core/Cargo.toml`
- [ ] `crates/pdf-lay-core/src/lib.rs`
- [ ] `crates/pdf-lay/Cargo.toml`
- [ ] `crates/pdf-lay/src/lib.rs`
- [ ] `crates/pdf-lay-cli/Cargo.toml`
- [ ] `crates/pdf-lay-cli/src/main.rs`
- [ ] `crates/pdflay-python/Cargo.toml`
- [ ] `crates/pdflay-python/src/lib.rs`
- [ ] `crates/pdflay-python/pyproject.toml`
- [ ] `rustfmt.toml`
- [ ] `.gitignore`
- [ ] `.github/workflows/ci.yml`

## Implementation Steps

### Step 1: Root `Cargo.toml` (workspace)

```toml
[workspace]
members = [
    "crates/pdf-lay-core",
    "crates/pdf-lay",
    "crates/pdf-lay-cli",
    "crates/pdflay-python",
]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"
repository = "https://github.com/xxx/pdf-lay"

[workspace.dependencies]
# Core dependencies — all crates pin versions here
serde       = { version = "1", features = ["derive"] }
serde_json  = "1"
regex       = "1"
thiserror   = "2"
log         = "0.4"
env_logger  = "0.11"
image       = "0.25"
rayon       = "1"

# Python bindings
pyo3        = { version = "0.23", features = ["extension-module"] }

# CLI
clap        = { version = "4", features = ["derive"] }

# Test/dev
tempfile    = "3"

# NOTE: pdf_oxide pinned when exact version confirmed in Task 3
# pdf_oxide = { git = "https://github.com/yfedoseev/pdf_oxide" }
```

### Step 2: `crates/pdf-lay-core/Cargo.toml`

```toml
[package]
name = "pdf-lay-core"
version.workspace = true
edition.workspace = true
license.workspace = true
publish = false   # internal crate, not published to crates.io

[dependencies]
serde.workspace      = true
serde_json.workspace = true
regex.workspace      = true
thiserror.workspace  = true
log.workspace        = true
image.workspace      = true
rayon.workspace      = true

[dev-dependencies]
tempfile.workspace = true
```

### Step 3: `crates/pdf-lay-core/src/lib.rs`

```rust
//! pdf-lay-core: internal PDF layout analysis library.
//!
//! This crate is not published to crates.io. Use `pdf-lay` for the public API.

#![warn(missing_docs)]

// Module declarations will be added as tasks complete:
// pub mod types;
// pub mod extract;
// pub mod layout;
// pub mod structure;
// pub mod figure;
// pub mod selector;
// pub mod output;
// pub mod config;
// pub(crate) mod pipeline;
```

### Step 4: `crates/pdf-lay/Cargo.toml`

```toml
[package]
name = "pdf-lay"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "PDF Layout Analysis for Academic Papers"

[dependencies]
pdf-lay-core = { path = "../pdf-lay-core" }
```

### Step 5: `crates/pdf-lay/src/lib.rs`

```rust
//! pdf-lay: PDF Layout Analysis for Academic Papers.
//!
//! This crate re-exports the public API from `pdf-lay-core`.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use pdf_lay::{analyze_pdf, Config};
//! use std::path::Path;
//!
//! let config = Config::default();
//! let doc = analyze_pdf(Path::new("paper.pdf"), &config).unwrap();
//! println!("{}", doc.toc().len());
//! ```

// Re-exports will be added as pdf-lay-core modules are completed.
// pub use pdf_lay_core::*;
```

### Step 6: `crates/pdf-lay-cli/Cargo.toml`

```toml
[package]
name = "pdf-lay-cli"
version.workspace = true
edition.workspace = true
license.workspace = true
publish = false

[[bin]]
name = "pdf-lay"
path = "src/main.rs"

[dependencies]
pdf-lay-core = { path = "../pdf-lay-core" }
clap.workspace = true
log.workspace = true
env_logger.workspace = true
```

### Step 7: `crates/pdf-lay-cli/src/main.rs`

```rust
//! pdf-lay CLI binary.

fn main() {
    eprintln!("pdf-lay CLI — not yet implemented");
    std::process::exit(1);
}
```

### Step 8: `crates/pdflay-python/Cargo.toml`

```toml
[package]
name = "pdflay-python"
version.workspace = true
edition.workspace = true
license.workspace = true
publish = false

[lib]
name = "pdflay"
crate-type = ["cdylib"]

[dependencies]
pdf-lay-core = { path = "../pdf-lay-core" }
pyo3.workspace = true
```

### Step 9: `crates/pdflay-python/src/lib.rs`

```rust
//! PyO3 Python bindings for pdf-lay.

use pyo3::prelude::*;

/// Python module registration — functions added in Task 19.
#[pymodule]
fn pdflay(_m: &Bound<'_, PyModule>) -> PyResult<()> {
    Ok(())
}
```

### Step 10: `crates/pdflay-python/pyproject.toml`

```toml
[build-system]
requires = ["maturin>=1.0,<2.0"]
build-backend = "maturin"

[project]
name = "pdflay"
requires-python = ">=3.9"
description = "PDF Layout Analysis for Academic Papers"
license = { text = "MIT OR Apache-2.0" }

[tool.maturin]
features = ["pyo3/extension-module"]
module-name = "pdflay"
python-source = "python"
```

### Step 11: `rustfmt.toml`

```toml
edition = "2024"
max_width = 100
use_small_heuristics = "Default"
```

### Step 12: `.gitignore`

```
/target/
**/*.rs.bk
Cargo.lock
*.pyc
__pycache__/
.venv/
dist/
*.egg-info/
.pytest_cache/
```

### Step 13: `.github/workflows/ci.yml`

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

env:
  CARGO_TERM_COLOR: always

jobs:
  check:
    name: Check (fmt + clippy + test)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy

      - name: Cache cargo registry
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target/
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}

      - name: Check formatting
        run: cargo fmt --all -- --check

      - name: Clippy
        run: cargo clippy --all-targets -- -D warnings

      - name: Test
        run: cargo test --all
```

## Acceptance Criteria

- [ ] `cargo build` succeeds with zero errors
- [ ] `cargo clippy --all-targets -- -D warnings` passes with zero warnings
- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo test --all` passes (no tests yet, but command succeeds)
- [ ] Directory structure matches AGENTS.md spec exactly
- [ ] All four crate `Cargo.toml` files reference `workspace.package` for version/edition/license

## Dependencies

None — this is Task 01 (the root task).

## Commit Message

```
feat(workspace): initialize cargo workspace skeleton with 4 crates and CI pipeline
```
