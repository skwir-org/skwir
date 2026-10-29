//! An archive read while it is shown (`--serve`): members read in order go
//! on from one to the next, each header read once; one before the last read
//! reads the archive again from its start; an archive replaced since its
//! listing is refused by name, never read for another member. The archive
//! is made by the helper itself (`--create`), a compressed tar: read from
//! its start, every member before is decompressed again.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const WORKER: &str = env!("CARGO_BIN_EXE_skwir-archive-worker");

/// A folder of its own under `~/.cache/skwir-tests`, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let home = std::env::home_dir().expect("a home folder");
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = Path::new(&home).join(".cache/skwir-tests").join(format!(
            "archive-worker-{name}-{}-{nanos}",
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

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// A compressed tar at `archive` of `members`, made by the helper.
fn make(archive: &Path, members: &[(String, Vec<u8>)]) {
    let mut child = Command::new(WORKER)
        .arg(archive)
        .args(["--create", "tar.gz"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for (name, bytes) in members {
        let header = format!(
            "{{\"name\":\"{}\",\"kind\":\"file\",\"size\":{},\"mode\":420}}\n",
            hex(name.as_bytes()),
            bytes.len()
        );
        input.write_all(header.as_bytes()).unwrap();
        input.write_all(bytes).unwrap();
    }
    input.write_all(b"{\"end\":true}\n").unwrap();
    drop(input);
    let made = child.wait_with_output().unwrap();
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
}

/// A session of the helper, asked one request at a time.
struct Served {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    next: u64,
}

impl Served {
    fn start(archive: &Path, work: &Path) -> Self {
        let mut child = Command::new(WORKER)
            .arg(archive)
            .arg("--serve")
            .arg(work)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut served = Served {
            child,
            input: Some(input),
            output,
            next: 0,
        };
        let greeting = served.reply();
        assert_eq!(greeting["id"], 0);
        assert_eq!(greeting["body"], "Ready");
        served
    }

    fn reply(&mut self) -> serde_json::Value {
        let mut line = String::new();
        assert!(self.output.read_line(&mut line).unwrap() > 0, "it ended");
        serde_json::from_str(&line).unwrap()
    }

    fn ask(&mut self, body: &serde_json::Value) -> serde_json::Value {
        self.next += 1;
        let request = serde_json::json!({"id": self.next, "body": body});
        writeln!(self.input.as_mut().unwrap(), "{request}").unwrap();
        let reply = self.reply();
        assert_eq!(reply["id"], self.next, "answered in order");
        reply["body"].clone()
    }

    fn read(&mut self, index: u64, name: &str) -> serde_json::Value {
        self.ask(&serde_json::json!({"Read": {"index": index, "name": name, "bytes": 1 << 20}}))
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn name(n: usize) -> String {
    format!("pages/page {n:02}.txt")
}

fn bytes(n: usize) -> Vec<u8> {
    format!("member {n}\n").repeat(200 + n).into_bytes()
}

#[test]
fn members_read_in_order_read_each_header_once() {
    let scratch = Scratch::new("serve");
    let archive = scratch.0.join("fifty pages.tar.gz");
    let members: Vec<(String, Vec<u8>)> = (0..50).map(|n| (name(n), bytes(n))).collect();
    make(&archive, &members);
    let work = scratch.0.join("work");
    std::fs::create_dir(&work).unwrap();
    let mut served = Served::start(&archive, &work);

    // Its listing, from its start.
    let listing =
        served.ask(&serde_json::json!({"List": {"members": 100, "names": 1 << 20, "seconds": 10}}));
    let listed = listing["Listing"]["members"].as_array().unwrap();
    assert_eq!(listed.len(), 50);
    assert_eq!(listing["Listing"]["complete"], true);
    assert_eq!(listed[7]["name"], name(7));

    // Read in order: opened once, each header read once.
    let mut headers = 0;
    for (n, (name, bytes)) in members.iter().enumerate() {
        let written = &served.read(n as u64, name)["Written"];
        assert_eq!(written["len"], bytes.len(), "{written}");
        assert_eq!(written["from_start"], n == 0, "member {n}");
        headers += written["headers"].as_u64().unwrap();
        assert_eq!(std::fs::read(work.join("member")).unwrap(), *bytes);
    }
    assert_eq!(headers, 50, "fifty members, fifty headers");

    // One before: from the start again, its header the eleventh read.
    let written = &served.read(10, &name(10))["Written"];
    assert_eq!(written["from_start"], true);
    assert_eq!(written["headers"], 11);
    assert_eq!(std::fs::read(work.join("member")).unwrap(), bytes(10));
    // And on from there.
    let written = &served.read(12, &name(12))["Written"];
    assert_eq!(written["from_start"], false);
    assert_eq!(written["headers"], 2);

    // A member past the end is no member; the session goes on.
    let gone = served.read(80, "pages/page 80.txt");
    assert!(
        gone["Error"].as_str().unwrap().contains("no longer exists"),
        "{gone}"
    );
    let written = &served.read(3, &name(3))["Written"];
    assert_eq!(written["len"], bytes(3).len());

    // Replaced since its listing: a member read again from the start is
    // asked for by the name it had, and refused.
    let renamed: Vec<(String, Vec<u8>)> = (0..50)
        .map(|n| (format!("other/{n}.txt"), bytes(n)))
        .collect();
    let replacement = scratch.0.join("replacement.tar.gz");
    make(&replacement, &renamed);
    std::fs::rename(&replacement, &archive).unwrap();
    let refused = served.read(2, &name(2));
    assert!(
        refused["Error"]
            .as_str()
            .unwrap()
            .contains("Archive changed since it was listed"),
        "{refused}"
    );
    assert!(
        !work.join("member").exists() || std::fs::read(work.join("member")).unwrap() != bytes(2)
    );

    // The end of its input ends it.
    drop(served.input.take());
    assert!(served.child.wait().unwrap().success());
}
