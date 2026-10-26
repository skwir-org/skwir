//! `--probe`, what skwir's settings ask of a helper (`platform::process::probe`),
//! and `--build`, what skwir asks of one whose call failed
//! (`platform::process::explain`).

use std::process::Command;

#[test]
fn started_it_says_whether_it_works() {
    let output = Command::new(env!("CARGO_BIN_EXE_skwir-kmtp-worker"))
        .arg("--probe")
        .output()
        .unwrap();
    // Where FUSE is, it works; elsewhere it says why not.
    let fuse = cfg!(target_os = "linux") && std::path::Path::new("/dev/fuse").exists();
    if fuse && output.status.success() {
        assert!(output.stdout.is_empty(), "nothing on its protocol's output");
    } else {
        assert!(!output.status.success());
        assert!(!output.stderr.is_empty(), "it says why");
    }
}

#[test]
fn asked_it_says_its_build_alone() {
    let output = Command::new(env!("CARGO_BIN_EXE_skwir-kmtp-worker"))
        .arg("--build")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", env!("SKWIR_BUILD"))
    );
}
