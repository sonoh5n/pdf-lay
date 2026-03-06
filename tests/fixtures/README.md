# Test Fixtures

Place PDF files here for integration testing.

Required files:
- `sample_ieee_twocol.pdf` — IEEE-style two-column paper (at least 5 pages)
- `sample_single_col.pdf` — Single-column paper (at least 3 pages)

These files are NOT committed to the repository. Download them manually:
1. Any IEEE conference paper (e.g., from IEEE Xplore)
2. Any arXiv preprint in single-column format

The integration tests are marked `#[ignore]` and will be skipped if the files
are not present. They run on CI only when `RUN_INTEGRATION_TESTS=1` is set.
