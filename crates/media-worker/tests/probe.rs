//! `--probe`, what bibli's settings ask of a helper (`platform::process::probe`).

use std::process::Command;

/// `GStreamer` without its playback plugins cannot play: the helper says so.
#[test]
fn without_its_plugins_it_says_it_does_not_work() {
    let empty = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_bibli-media-worker"))
        .arg("--probe")
        .env("GST_PLUGIN_SYSTEM_PATH_1_0", empty.path())
        .env("GST_PLUGIN_PATH_1_0", empty.path())
        .env("GST_REGISTRY_1_0", empty.path().join("registry.bin"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("playback plugins"), "{said}");
}
