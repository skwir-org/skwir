//! `--probe`, what skwir's settings ask of a helper (`platform::process::probe`),
//! and `--build`, what skwir asks of one whose call failed
//! (`platform::process::explain`).

use std::process::Command;

/// `PDFium`'s library is the helper's to open: without it, it says so.
#[test]
fn without_its_library_it_says_it_does_not_work() {
    let dir = std::env::temp_dir().join("skwir-no-pdfium-here");
    let output = Command::new(env!("CARGO_BIN_EXE_skwir-pdf-worker"))
        .arg("--probe")
        .env("SKWIR_PDFIUM_LIBRARY", dir.join("libpdfium.so"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("PDFium library unavailable"), "{said}");
}

/// The library a package carries beside the helper is found without any
/// setting (ADR-041). Needs a `PDFium` to copy: the `pdf` dev shell names
/// one in `SKWIR_PDFIUM_LIBRARY`; without it the test says so and passes.
#[test]
fn the_library_beside_it_is_found_without_a_setting() {
    let name = if cfg!(windows) {
        "pdfium.dll"
    } else if cfg!(target_os = "macos") {
        "libpdfium.dylib"
    } else {
        "libpdfium.so"
    };
    // One for this system (Wine is handed Linux's).
    let library = std::env::var_os("SKWIR_PDFIUM_LIBRARY").filter(|library| {
        std::path::Path::new(library).extension() == std::path::Path::new(name).extension()
    });
    let Some(library) = library else {
        eprintln!(
            "skipped: no PDFium library of this system's to copy (the pdf dev shell names one)"
        );
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let helper = dir.path().join(
        std::path::Path::new(env!("CARGO_BIN_EXE_skwir-pdf-worker"))
            .file_name()
            .unwrap(),
    );
    std::fs::copy(env!("CARGO_BIN_EXE_skwir-pdf-worker"), &helper).unwrap();
    std::fs::copy(&library, dir.path().join(name)).unwrap();
    let output = Command::new(&helper)
        .arg("--probe")
        .env_remove("SKWIR_PDFIUM_LIBRARY")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn asked_it_says_its_build_alone() {
    let output = Command::new(env!("CARGO_BIN_EXE_skwir-pdf-worker"))
        .arg("--build")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", env!("SKWIR_BUILD"))
    );
}
