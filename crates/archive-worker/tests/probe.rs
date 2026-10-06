//! `--probe`, what bibli's settings ask of a helper (`platform::process::probe`).

use std::process::Command;

#[test]
fn started_it_says_it_works() {
    let output = Command::new(env!("CARGO_BIN_EXE_bibli-archive-worker"))
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
