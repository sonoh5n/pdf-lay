//! Optional OCR seam for scanned/image-only pages (P4-2).
//!
//! Compiled only under the `ocr` cargo feature (see `pdf-lay-core/Cargo.toml`
//! and `crate::extract`'s `mod ocr` declaration). The default build never
//! includes this module at all — scanned-page *detection* and the
//! `PdfLayWarning::PageTextMissing`/`PageTextRecovered` warnings are always
//! on (see `pipeline.rs`), but the actual OCR attempt only exists when this
//! feature is compiled in.
//!
//! # Why shell out to `tesseract` instead of pdf_oxide's built-in `ocr` feature
//!
//! pdf_oxide 0.3.8 has its own `ocr` cargo feature (`ort` + PaddleOCR-style
//! ONNX det/rec/dict models), but enabling it pulls in ONNX Runtime as a
//! build dependency and requires locating model files at runtime — a much
//! heavier default than this crate wants to impose (see
//! `docs/refactor/phase4_findings.md` P4-1 §6, which recommends the
//! tesseract shell-out as the safer default per
//! `docs/refactor/00_REVIEW_POLICY.md` §7's "do not add a heavy mandatory
//! dependency" guidance). Shelling out to a `tesseract` binary that the
//! operator already has on `PATH` adds zero new Rust dependencies to this
//! crate, at the cost of requiring that external binary to be present.
//!
//! This module never panics: a missing `tesseract` binary, a page with no
//! usable image, or a non-zero tesseract exit status all become `Err`
//! (or `Ok(vec![])` when tesseract runs but reads no text), which the caller
//! (`pipeline.rs`) turns into a `PdfLayWarning::PageTextMissing` rather than
//! aborting the analysis.

use std::process::{Command, Stdio};

use crate::config::{OcrConfig, OcrEngineKind};
use crate::extract::PdfReader;
use crate::types::{Rect, TextSpan};

/// Best-effort check for whether the configured OCR engine looks runnable on
/// this machine. `false` means the caller should not call [`ocr_page`] and
/// should record a `PdfLayWarning::PageTextMissing` instead. Never panics.
pub(crate) fn engine_available(cfg: &OcrConfig) -> bool {
    match cfg.engine {
        OcrEngineKind::Tesseract => Command::new("tesseract")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false),
        // Not wired up: see the module docs for why pdf_oxide's built-in
        // `ocr` feature is not enabled for this crate.
        OcrEngineKind::Builtin => false,
    }
}

/// Attempt to recover text for `page` via OCR.
///
/// Returns the recovered spans (`Ok(vec![])` if OCR ran successfully but
/// found no text) or an `Err` with a human-readable reason nothing could be
/// recovered. Callers should check [`engine_available`] first; this function
/// still degrades gracefully (never panics) even if it is not.
pub(crate) fn ocr_page(
    reader: &mut PdfReader,
    page: u32,
    cfg: &OcrConfig,
) -> Result<Vec<TextSpan>, String> {
    match cfg.engine {
        OcrEngineKind::Builtin => Err(
            "OCR engine \"builtin\" (pdf_oxide's own ocr feature) is not wired up in this build"
                .to_string(),
        ),
        OcrEngineKind::Tesseract => ocr_page_via_tesseract(reader, page, cfg),
    }
}

fn ocr_page_via_tesseract(
    reader: &mut PdfReader,
    page: u32,
    cfg: &OcrConfig,
) -> Result<Vec<TextSpan>, String> {
    // Page-level rasterization needs pdf_oxide's `rendering` feature, which
    // this crate does not enable (see `docs/refactor/phase4_findings.md`
    // P4-1, the `get_page_info` entry). Instead, OCR the largest raster
    // Image XObject on the page: a scanned page's common shape is a single
    // full-page image (findings §2.5).
    let images = reader
        .inner_doc()
        .extract_images(page as usize)
        .map_err(|e| format!("could not read page images for OCR: {e}"))?;

    let image = images
        .iter()
        .max_by_key(|img| u64::from(img.width()) * u64::from(img.height()))
        .ok_or_else(|| "no image on the page to OCR".to_string())?;

    let tmp_dir =
        tempfile::tempdir().map_err(|e| format!("could not create OCR temp directory: {e}"))?;
    let image_path = tmp_dir.path().join("page.png");
    image
        .save_as_png(&image_path)
        .map_err(|e| format!("failed to rasterize image for OCR: {e}"))?;

    let output = Command::new("tesseract")
        .arg(&image_path)
        .arg("stdout")
        .arg("-l")
        .arg(&cfg.lang)
        .output()
        .map_err(|e| format!("failed to run tesseract: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "tesseract exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        return Ok(Vec::new());
    }

    // Best-effort whole-page bbox: prefer the OCR'd image's own bbox
    // (pdf_oxide's Rect is {x, y, width, height} in Y-up space with (x, y) at
    // the lower-left corner — same convention documented in
    // `ImageExtractor::save_one_image`), else the page's real MediaBox, else
    // a Letter-size default. A single whole-page span is a coarse but honest
    // approximation of OCR'd text — per-line layout is out of scope (see
    // `docs/refactor/phase4_extraction.md` P4-2 non-scope: "OCR の精度
    // チューニング").
    let bbox = image
        .bbox()
        .filter(|b| b.width > 0.0 && b.height > 0.0)
        .map(|b| {
            Rect::new(
                b.x as f64,
                (b.y + b.height) as f64,
                (b.x + b.width) as f64,
                b.y as f64,
            )
        })
        .or_else(|| {
            reader
                .page_media_box(page)
                .map(|(width, height, _rotation)| Rect::new(0.0, height, width, 0.0))
        })
        .unwrap_or_else(|| Rect::new(0.0, 792.0, 612.0, 0.0));

    Ok(vec![TextSpan {
        text,
        font_name: "ocr:tesseract".to_string(),
        font_size: 10.0,
        is_bold: false,
        is_italic: false,
        bbox,
        page,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OcrConfig;

    #[test]
    fn engine_available_is_false_when_tesseract_binary_is_absent() {
        // This sandbox/CI environment does not have `tesseract` installed
        // (verified: `which tesseract` finds nothing) — exercising the
        // real "engine not available" path rather than mocking it. Must
        // return `false`, not panic or error.
        let cfg = OcrConfig::default();
        assert!(
            !engine_available(&cfg),
            "tesseract is not expected to be on PATH in this environment"
        );
    }

    #[test]
    fn engine_available_is_false_for_builtin_engine() {
        // The `Builtin` engine is a reserved-but-unimplemented placeholder
        // (see module docs / `OcrEngineKind::Builtin`); it must always
        // report unavailable rather than panicking or silently claiming
        // support it does not have.
        let cfg = OcrConfig {
            engine: crate::config::OcrEngineKind::Builtin,
            ..OcrConfig::default()
        };
        assert!(!engine_available(&cfg));
    }

    /// Minimal xref-correct single-page single-`Tj` PDF, just enough to open
    /// a `PdfReader` for the test below. Mirrors (but does not import, since
    /// that helper is private to its own module — see the precedent at
    /// `pipeline.rs`'s `build_image_only_pdf_bytes`) `pdf_reader::tests`'s
    /// `build_text_sanity_pdf`.
    fn minimal_text_pdf() -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();
        let mut offsets: Vec<usize> = Vec::new();
        buf.extend_from_slice(b"%PDF-1.4\n");
        let push_obj = |buf: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &[u8]| {
            offsets.push(buf.len());
            let num = offsets.len();
            buf.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            buf.extend_from_slice(body);
            buf.extend_from_slice(b"\nendobj\n");
        };
        push_obj(&mut buf, &mut offsets, b"<< /Type /Catalog /Pages 2 0 R >>");
        push_obj(
            &mut buf,
            &mut offsets,
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        );
        push_obj(
            &mut buf,
            &mut offsets,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
              /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        );
        let content = b"BT /F1 12 Tf 72 700 Td (Hello) Tj ET";
        offsets.push(buf.len());
        buf.extend_from_slice(b"4 0 obj\n");
        buf.extend_from_slice(format!("<< /Length {} >>\nstream\n", content.len()).as_bytes());
        buf.extend_from_slice(content);
        buf.extend_from_slice(b"\nendstream\nendobj\n");
        offsets.push(buf.len());
        buf.extend_from_slice(
            b"5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
        );
        let xref_offset = buf.len();
        let size = offsets.len() + 1;
        buf.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
        buf.extend_from_slice(b"0000000000 65535 f \n");
        for off in &offsets {
            buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        buf.extend_from_slice(
            format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n")
                .as_bytes(),
        );
        buf
    }

    #[test]
    fn ocr_page_returns_err_for_builtin_engine_without_panicking() {
        let pdf = minimal_text_pdf();
        let mut reader = PdfReader::from_bytes(&pdf).expect("PDF should open");
        let cfg = OcrConfig {
            engine: crate::config::OcrEngineKind::Builtin,
            ..OcrConfig::default()
        };
        let result = ocr_page(&mut reader, 0, &cfg);
        assert!(result.is_err(), "builtin engine must never claim success");
    }

    #[test]
    fn ocr_page_fails_gracefully_without_a_tesseract_binary() {
        // No image on the page at all, and no `tesseract` binary in this
        // environment either way — either failure mode must be a plain
        // `Err`, never a panic.
        let pdf = minimal_text_pdf();
        let mut reader = PdfReader::from_bytes(&pdf).expect("PDF should open");
        let cfg = OcrConfig::default();
        let result = ocr_page(&mut reader, 0, &cfg);
        assert!(result.is_err());
    }
}
