use std::io::Write;
use std::process::{Command, Stdio};

fn typeset(input: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_skwir-math-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A worker that refuses a long batch stops reading it: the rest of the
    // write meets a closed pipe, which is its answer, not a failure.
    let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
    child.wait_with_output().unwrap()
}

#[test]
fn each_formula_is_drawn_or_refused_alone_in_order() {
    let batch = serde_json::json!({"formulas": [
        {"tex": r"\frac{a}{b} + \sqrt{x^2}", "display": true},
        {"tex": r"\frac{1}{"},
        {"tex": r"\def\a{\a\a}\a"},
        {"tex": "x_i"},
    ]});
    let output = typeset(&batch.to_string());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let answer: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(answer["padding"], 0.25);
    let formulas = answer["formulas"].as_array().unwrap();
    assert_eq!(formulas.len(), 4);
    let drawn = &formulas[0];
    let svg = drawn["svg"].as_str().unwrap();
    assert!(svg.starts_with("<svg"), "{}", &svg[..svg.len().min(80)]);
    assert!(!svg.contains("<text"), "glyphs are outlines: {svg}");
    assert!(!svg.contains("href"), "nothing to fetch");
    let em = |key: &str| drawn[key].as_f64().unwrap();
    assert!(em("width") > 1. && em("height") > 0.5 && em("depth") > 0.2);
    assert!(formulas[1]["error"].as_str().is_some_and(|e| !e.is_empty()));
    assert!(
        formulas[2]["error"].as_str().is_some(),
        "a macro that expands for ever is refused"
    );
    assert!(formulas[3]["svg"].as_str().is_some(), "the next is drawn");
}

/// No equation numbers itself (MathJax's default, Jupyter's): a number is
/// a `\tag`'s, and comes apart for the caller to set at the line's end.
#[test]
fn an_equation_is_numbered_by_its_tag_alone_and_its_number_comes_apart() {
    let batch = serde_json::json!({"formulas": [
        {"tex": r"\begin{equation}x = 1\end{equation}", "display": true},
        {"tex": r"\begin{align}a &= b \tag{3}\\ c &= d\end{align}", "display": true},
        {"tex": r"E = mc^2 \tag{A}", "display": true},
        {"tex": r"x \tag{B}", "display": false},
    ]});
    let output = typeset(&batch.to_string());
    assert!(output.status.success());
    let answer: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let formulas = answer["formulas"].as_array().unwrap();
    assert!(formulas[0].get("tag").is_none(), "{}", formulas[0]);
    for tagged in &formulas[1..3] {
        let tag = &tagged["tag"];
        assert!(
            tag["svg"]
                .as_str()
                .is_some_and(|svg| svg.starts_with("<svg"))
        );
        assert!(
            tag["width"].as_f64().unwrap() > 1.,
            "the parentheses and the number"
        );
        assert!(
            tag["gap"].as_f64().unwrap() >= 1.,
            "an em clear of the formula"
        );
    }
    assert!(
        formulas[2]["width"].as_f64().unwrap() < 4.,
        "the formula's width is its own, not its number's"
    );
    assert!(
        formulas[3].get("tag").is_none(),
        "a formula in a sentence keeps it"
    );
}

#[test]
fn what_is_not_a_batch_is_refused_with_a_reason() {
    let output = typeset("x^2");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
    let long = typeset(&" ".repeat(2 * 1024 * 1024));
    assert!(!long.status.success());
}

/// What skwir writes from MathML's markup is TeX the typesetter draws:
/// text mode's escapes, a root's index braced, a fence left open.
#[test]
fn the_tex_written_from_mathml_is_drawn() {
    let batch = serde_json::json!({"formulas": [
        {"tex": r"\text{a\textbackslash b \textasciicircum c \textasciitilde d \{e\} \#\$\%\&\_}"},
        {"tex": r"\sqrt[{]}]{x}"},
        {"tex": r"\left\{ a\&b\right."},
    ]});
    let output = typeset(&batch.to_string());
    assert!(output.status.success());
    let answer: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    for typeset in answer["formulas"].as_array().unwrap() {
        assert!(typeset["svg"].is_string(), "{typeset}");
    }
}
