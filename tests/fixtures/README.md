# Test Fixtures

## Committed fixtures

These are synthetic, rights-safe "dummy" papers (no real research content)
generated specifically for regression testing, so they are committed to the
repository:

- `en_twocol_figs.pdf` — English two-column sample paper (~16 pages) with
  figures, tables, equations, and embedded images. Exercises two-column
  reading-order reconstruction, figure/caption matching, and image extraction.
- `ja_twocol.pdf` — Japanese two-column sample paper (~16 pages, A4) with
  CID/ToUnicode CJK text, tables, and section headings. Exercises CJK
  extraction and two-column reading order for Japanese layout.

## Optional local fixtures (not committed)

Larger real-world papers can be dropped in for manual/CI-only checks:

- `sample_ieee_twocol.pdf` — IEEE-style two-column paper (at least 5 pages)
- `sample_single_col.pdf` — Single-column paper (at least 3 pages)

Integration tests that depend on these optional files are marked `#[ignore]`
and are skipped when the files are absent. They run on CI only when
`RUN_INTEGRATION_TESTS=1` is set.
