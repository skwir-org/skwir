//! `--probe`, what skwir's settings ask of a helper (`platform::process::probe`),
//! and `--build`, what skwir asks of one whose call failed
//! (`platform::process::explain`).

use std::process::Command;

/// It opens nothing of its own before a text needs a grammar: started,
/// it works.
#[test]
fn asked_it_says_it_works() {
    let output = Command::new(env!("CARGO_BIN_EXE_skwir-tree-sitter-worker"))
        .arg("--probe")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn asked_it_says_its_build_alone() {
    let output = Command::new(env!("CARGO_BIN_EXE_skwir-tree-sitter-worker"))
        .arg("--build")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", env!("SKWIR_BUILD"))
    );
}
