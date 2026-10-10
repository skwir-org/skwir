//! `--probe`, what bibli's settings ask of a helper (`platform::process::probe`),
//! and `--build`, what bibli asks of one whose call failed
//! (`platform::process::explain`).

use std::process::Command;

#[test]
fn started_it_says_it_works() {
    let output = Command::new(env!("CARGO_BIN_EXE_bibli-poppler-worker"))
        .arg("--probe")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty(), "nothing on its protocol's output");
}

#[test]
fn asked_it_says_its_build_alone() {
    let output = Command::new(env!("CARGO_BIN_EXE_bibli-poppler-worker"))
        .arg("--build")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", env!("BIBLI_BUILD"))
    );
}
