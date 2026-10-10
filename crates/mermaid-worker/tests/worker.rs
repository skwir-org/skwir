use std::io::Write;
use std::process::{Command, Stdio};

fn draw(source: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_skwir-mermaid-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A worker that refuses a long diagram stops reading it: the rest of
    // the write meets a closed pipe, which is its answer, not a failure.
    let _ = child.stdin.take().unwrap().write_all(source.as_bytes());
    child.wait_with_output().unwrap()
}

#[test]
fn a_diagram_is_drawn_as_svg_resvg_can_read() {
    let output = draw("flowchart LR\n  A[Read the file] --> B[Show it]\n");
    assert!(output.status.success());
    let svg = String::from_utf8(output.stdout).unwrap();
    assert!(svg.starts_with("<svg"), "{}", &svg[..svg.len().min(80)]);
    assert!(svg.contains("Read the file"), "labels are SVG text");
    assert!(!svg.contains("<foreignObject"));
}

#[test]
fn what_is_not_a_diagram_is_refused_with_a_reason() {
    let output = draw("not a diagram at all");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
    let long = draw(&"%% a comment\n".repeat(10_000));
    assert!(!long.status.success());
}
