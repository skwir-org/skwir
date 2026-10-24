//! An archive is read for what it holds, never for what it names: an
//! mtree file lists files on disk, which libarchive's reader opens and
//! hands over as members once asked to check them (`checkfs`). The helper
//! never asks, so a file named so is neither listed at its size, nor
//! previewed, nor extracted.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const WORKER: &str = env!("CARGO_BIN_EXE_skwir-archive-worker");

/// A folder of its own under `~/.cache/skwir-tests`, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let home = std::env::home_dir().expect("a home folder");
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = Path::new(&home).join(".cache/skwir-tests").join(format!(
            "archive-worker-mtree-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(dir: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(WORKER)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(input);
    child.wait_with_output().unwrap()
}

const SECRET: &str = "what a private file holds";

#[test]
fn an_mtree_file_hands_over_none_of_the_files_it_names() {
    let scratch = Scratch::new();
    let secret = scratch.0.join("it's-private.txt");
    std::fs::write(&secret, SECRET).unwrap();
    // A file of words that libarchive takes for an archive: `notes.tar`.
    let archive = scratch.0.join("notes.tar");
    std::fs::write(
        &archive,
        format!(
            "#mtree\n./x type=file contents={}\n./y type=file size=25\n",
            secret.display()
        ),
    )
    .unwrap();
    let archive = archive.to_str().unwrap();

    let listed = run(
        &scratch.0,
        &[archive, "--members", "10", "--names", "4096"],
        b"",
    );
    let said = String::from_utf8_lossy(&listed.stdout);
    assert!(listed.status.success(), "{said}");
    let listing: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    let x = &listing["members"][0];
    assert_eq!(
        (x["name"].as_str(), x["size"].as_u64()),
        (Some("./x"), Some(0)),
        "{said}"
    );

    let preview = scratch.0.join("preview");
    std::fs::create_dir(&preview).unwrap();
    let output = preview.join("member.txt");
    run(
        &preview,
        &[
            archive,
            "0",
            output.to_str().unwrap(),
            "--name",
            "./x",
            "--bytes",
            "4096",
        ],
        b"",
    );
    let shown = std::fs::read_to_string(&output).unwrap_or_default();
    assert!(!shown.contains(SECRET), "previewed: {shown}");

    let into = scratch.0.join("into");
    std::fs::create_dir(&into).unwrap();
    run(&into, &[archive, "--extract"], &[1; 64]);
    for entry in std::fs::read_dir(&into).unwrap() {
        let path = entry.unwrap().path();
        let made = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(!made.contains(SECRET), "extracted: {}", path.display());
    }
}
