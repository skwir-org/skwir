//! `--probe`, what bibli's settings ask of a helper (`platform::process::probe`).

use std::process::Command;

/// `PDFium`'s library is the helper's to open: without it, it says so.
#[test]
fn without_its_library_it_says_it_does_not_work() {
    let dir = std::env::temp_dir().join("bibli-no-pdfium-here");
    let output = Command::new(env!("CARGO_BIN_EXE_bibli-pdf-worker"))
        .arg("--probe")
        .env("BIBLI_PDFIUM_LIBRARY", dir.join("libpdfium.so"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("PDFium library unavailable"), "{said}");
}
