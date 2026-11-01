//! The helper as skwir talks to it: started as it is built, not isolated,
//! asked over its standard input, answered on its output (`syntax`).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use skwir_core::document::Tagged;
use skwir_core::syntax::{Grammar, MAX_LINES, MAX_REQUEST, Queries, Reply, Request, Span, Symbol};

/// How long one answer may take: a debug build parses slowly.
const ANSWER: Duration = Duration::from_secs(60);

struct Helper {
    child: Child,
    input: ChildStdin,
    /// Each line it answers, read on a thread of its own; closed at its end.
    lines: Receiver<String>,
    sent: u64,
}

impl Helper {
    /// The helper started and its greeting read.
    fn start() -> (Helper, Reply) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_skwir-tree-sitter-worker"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let (sender, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in output.lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let mut helper = Helper {
            child,
            input,
            lines,
            sent: 0,
        };
        let (id, greeting) = helper.reply().expect("it greets");
        assert_eq!(id, 0);
        (helper, greeting)
    }

    /// The helper started, told `grammars`.
    fn with(grammars: Vec<Grammar>) -> Helper {
        let (mut helper, _) = Helper::start();
        assert_eq!(helper.ask(&Request::Grammars(grammars)), Reply::Done);
        helper
    }

    fn send(&mut self, request: &Request) -> u64 {
        self.sent += 1;
        let line = skwir_core::jsonl::encode(&Tagged {
            id: self.sent,
            body: request,
        })
        .unwrap();
        self.input.write_all(&line).unwrap();
        self.sent
    }

    /// The next answer, `None` once the helper ended.
    fn reply(&mut self) -> Option<(u64, Reply)> {
        match self.lines.recv_timeout(ANSWER) {
            Ok(line) => {
                let Tagged { id, body } = serde_json::from_str::<Tagged<Reply>>(&line).unwrap();
                Some((id, body))
            }
            Err(RecvTimeoutError::Disconnected) => None,
            Err(RecvTimeoutError::Timeout) => panic!("no answer in {ANSWER:?}"),
        }
    }

    fn ask(&mut self, request: &Request) -> Reply {
        let id = self.send(request);
        let (answered, reply) = self.reply().expect("it answers");
        assert_eq!(answered, id, "answered in order");
        reply
    }

    fn open(&mut self, doc: u32, name: &str, text: &Path) -> Reply {
        self.ask(&Request::Open {
            doc,
            name: name.into(),
            text: text.to_owned(),
        })
    }

    /// The spans of every line of `doc`, `lines` of them.
    fn colors(&mut self, doc: u32, lines: u32) -> Vec<Vec<Span>> {
        match self.ask(&Request::Colors {
            doc,
            from: 0,
            to: lines,
        }) {
            Reply::Colors { lines } => lines,
            other => panic!("{other:?}"),
        }
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/json")
}

/// The fixture grammar built as a shared library (`build.rs`).
#[cfg(target_os = "linux")]
fn native_library() -> PathBuf {
    match option_env!("SKWIR_TEST_GRAMMAR") {
        Some(path) => PathBuf::from(path),
        None => panic!(
            "the tests' grammar was not built: {}",
            option_env!("SKWIR_TEST_GRAMMAR_MISSING").unwrap_or("no C compiler?")
        ),
    }
}

fn wasm_library() -> PathBuf {
    fixtures().join("tree-sitter-json.wasm")
}

/// tree-sitter-json from `library`, with its own highlights.
fn json(name: &str, library: PathBuf) -> Grammar {
    Grammar {
        name: name.into(),
        library,
        symbol: "tree_sitter_json".into(),
        highlights: vec![fixtures().join("queries/highlights.scm")],
        ..Grammar::default()
    }
}

/// A file of `text` in `dir`.
fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

/// A JSON text of ten lines: a block comment over three (one ending
/// `\r\n`), a key whose value is not ASCII, a nested object, an array.
const TEXT: &str = concat!(
    "/* one\r\n",
    "   two\n",
    "   three */ {\n",
    "  \"name\": \"caf\u{e9}\",\n",
    "  \"count\": 42,\n",
    "  \"nested\": {\n",
    "    \"ok\": true,\n",
    "    \"list\": [1, null]\n",
    "  }\n",
    "}\n",
);

/// The capture of `line`'s bytes `start..end`, which one span covers.
fn capture_at(spans: &[Vec<Span>], captures: &[String], line: usize, text: &str) -> String {
    let source = TEXT.lines().nth(line).unwrap();
    let start = source.find(text).unwrap() as u32;
    let end = start + text.len() as u32;
    let span = spans[line]
        .iter()
        .find(|span| span.start <= start && end <= span.end)
        .unwrap_or_else(|| panic!("nothing colors {text:?} on line {line}: {:?}", spans[line]));
    captures[usize::from(span.capture)].clone()
}

/// What `grammar` colors `TEXT` with: the spans of each line in order,
/// apart, within the line; comments, strings, numbers and constants each
/// their own capture, a comment over several lines colored on each.
fn colors_json(grammar: Grammar) {
    let dir = tempfile::tempdir().unwrap();
    let text = write(dir.path(), "text", TEXT);
    let mut helper = Helper::with(vec![grammar]);
    let Reply::Opened { lines, captures } = helper.open(1, "json", &text) else {
        panic!("not opened");
    };
    assert_eq!(lines as usize, TEXT.lines().count());
    let spans = helper.colors(1, lines);
    assert_eq!(spans.len(), lines as usize);
    for (line, (spans, source)) in spans.iter().zip(TEXT.lines()).enumerate() {
        let mut at = 0;
        for span in spans {
            assert!(
                at <= span.start && span.start < span.end,
                "line {line}: {spans:?}"
            );
            assert!(
                span.end as usize <= source.trim_end_matches('\r').len(),
                "line {line}"
            );
            assert!(usize::from(span.capture) < captures.len());
            at = span.end;
        }
    }
    let at = |line, text| capture_at(&spans, &captures, line, text);
    // The comment, on each of its lines, its `\r` left out.
    assert_eq!(at(0, "/* one"), "comment");
    assert_eq!(spans[0][0].end, 6);
    assert_eq!(at(1, "   two"), "comment");
    assert_eq!(at(2, "   three */"), "comment");
    // tree-sitter-highlight takes the last pattern that captures a node:
    // a key is a string too (`(string) @string` comes after the key's).
    assert_eq!(at(3, "\"caf\u{e9}\""), "string");
    assert_eq!(at(4, "42"), "number");
    assert_eq!(at(6, "true"), "constant.builtin");
    assert_eq!(at(7, "null"), "constant.builtin");
    for (one, other) in [("string", "number"), ("number", "constant.builtin")] {
        assert_ne!(
            captures.iter().position(|c| c == one),
            captures.iter().position(|c| c == other)
        );
    }
    // Lines past the last are left out; too many at once refused.
    let Reply::Colors { lines: tail } = helper.ask(&Request::Colors {
        doc: 1,
        from: lines - 1,
        to: lines + 5,
    }) else {
        panic!("no colors");
    };
    assert_eq!(tail.len(), 1);
    assert!(matches!(
        helper.ask(&Request::Colors {
            doc: 1,
            from: 0,
            to: MAX_LINES + 1
        }),
        Reply::Error(_)
    ));
    assert_eq!(helper.ask(&Request::Close { doc: 1 }), Reply::Done);
    assert!(matches!(
        helper.ask(&Request::Colors {
            doc: 1,
            from: 0,
            to: 1
        }),
        Reply::Error(_)
    ));
}

#[test]
fn it_greets_with_the_versions_it_parses() {
    let (_helper, greeting) = Helper::start();
    let Reply::Ready { version, abi, wasm } = greeting else {
        panic!("{greeting:?}");
    };
    assert!(version.starts_with("0.27."), "{version}");
    assert_eq!(abi, (13, 15));
    assert!(wasm);
}

#[cfg(target_os = "linux")]
#[test]
fn a_native_grammar_colors_each_line() {
    colors_json(json("json", native_library()));
}

#[test]
fn a_webassembly_grammar_colors_each_line() {
    colors_json(json("json", wasm_library()));
}

/// Folds from a folds query, outlines from a `tags.scm` and from Zed's
/// `outline.scm`, from one grammar's library under three names.
#[test]
fn folds_and_outlines_come_from_their_queries() {
    let dir = tempfile::tempdir().unwrap();
    let text = write(dir.path(), "text", TEXT);
    let folds = write(dir.path(), "folds.scm", "[(object) (array)] @fold\n");
    let tags = write(
        dir.path(),
        "tags.scm",
        "(pair key: (string (string_content) @name)) @definition.field\n",
    );
    let zed = write(dir.path(), "outline.scm", "(pair key: (_) @name) @item\n");
    let grammar = |name: &str, outline: &Path| Grammar {
        folds: vec![folds.clone()],
        outline: vec![outline.to_owned()],
        ..json(name, wasm_library())
    };
    let mut helper = Helper::with(vec![grammar("json", &tags), grammar("zed", &zed)]);
    assert!(matches!(
        helper.open(1, "json", &text),
        Reply::Opened { .. }
    ));
    // The object from its `{` to its `}`, the nested one; the array is on
    // one line.
    assert_eq!(
        helper.ask(&Request::Folds { doc: 1 }),
        Reply::Folds(vec![(2, 9), (5, 8)])
    );
    let symbol = |name: &str, kind: &str, line, depth| Symbol {
        name: name.into(),
        kind: kind.into(),
        line,
        depth,
    };
    assert_eq!(
        helper.ask(&Request::Outline { doc: 1 }),
        Reply::Outline(vec![
            symbol("name", "field", 3, 0),
            symbol("count", "field", 4, 0),
            symbol("nested", "field", 5, 0),
            symbol("ok", "field", 6, 1),
            symbol("list", "field", 7, 1),
        ])
    );
    assert!(matches!(helper.open(2, "zed", &text), Reply::Opened { .. }));
    let Reply::Outline(outline) = helper.ask(&Request::Outline { doc: 2 }) else {
        panic!("no outline");
    };
    let said: Vec<(&str, &str, u32)> = outline
        .iter()
        .map(|symbol| (symbol.name.as_str(), symbol.kind.as_str(), symbol.depth))
        .collect();
    assert_eq!(
        said,
        [
            ("\"name\"", "item", 0),
            ("\"count\"", "item", 0),
            ("\"nested\"", "item", 0),
            ("\"ok\"", "item", 1),
            ("\"list\"", "item", 1),
        ]
    );
    // A grammar without them has none.
    let mut plain = Helper::with(vec![json("json", wasm_library())]);
    assert!(matches!(plain.open(1, "json", &text), Reply::Opened { .. }));
    assert_eq!(
        plain.ask(&Request::Folds { doc: 1 }),
        Reply::Folds(Vec::new())
    );
    assert_eq!(
        plain.ask(&Request::Outline { doc: 1 }),
        Reply::Outline(Vec::new())
    );
}

/// A string's content in another language, found by that grammar's
/// `injection_regex`, is colored by its highlights, its captures among
/// the document's.
#[test]
fn injected_text_is_colored_by_its_own_grammar() {
    let dir = tempfile::tempdir().unwrap();
    let injections = write(
        dir.path(),
        "injections.scm",
        "((string (string_content) @injection.content)\n (#set! injection.language \"in\"))\n",
    );
    let inner = write(dir.path(), "inner.scm", "(number) @number.inner\n");
    let outer = Grammar {
        injections: vec![injections],
        ..json("json", wasm_library())
    };
    let inner = Grammar {
        name: "inner".into(),
        highlights: vec![inner],
        injection_regex: Some("^in(ner)?$".into()),
        ..json("inner", wasm_library())
    };
    let text = "{\"a\": \"[7]\", \"b\": 8}";
    let path = write(dir.path(), "text", text);
    let mut helper = Helper::with(vec![outer, inner]);
    let Reply::Opened { lines, captures } = helper.open(1, "json", &path) else {
        panic!("not opened");
    };
    let spans = helper.colors(1, lines);
    let capture = |at: usize| {
        let at = at as u32;
        let span = spans[0]
            .iter()
            .find(|span| span.start <= at && at < span.end)
            .unwrap();
        captures[usize::from(span.capture)].as_str()
    };
    assert_eq!(capture(text.find('7').unwrap()), "number.inner");
    assert_eq!(capture(text.find('8').unwrap()), "number");
    assert_eq!(capture(text.find('[').unwrap()), "string");
}

/// Native grammars that cannot be loaded: a library missing, a symbol it
/// lacks, a language of an ABI no tree-sitter reads.
#[cfg(target_os = "linux")]
fn native_failures(dir: &Path) -> Vec<Grammar> {
    vec![
        json("absent", dir.join("libtree-sitter-absent.so")),
        Grammar {
            symbol: "tree_sitter_nothing".into(),
            ..json("unnamed", native_library())
        },
        Grammar {
            symbol: "tree_sitter_wrong_abi".into(),
            ..json("future", native_library())
        },
    ]
}

#[cfg(not(target_os = "linux"))]
fn native_failures(_: &Path) -> Vec<Grammar> {
    Vec::new()
}

/// A grammar that cannot be loaded, or whose query does not compile, is
/// said so; the helper goes on with the next request.
#[test]
fn a_grammar_that_fails_is_said_and_the_helper_goes_on() {
    let dir = tempfile::tempdir().unwrap();
    let text = write(dir.path(), "text", TEXT);
    let broken = write(
        dir.path(),
        "broken.scm",
        "(string) @string\n(number) @@not-a-capture\n",
    );
    let good = json("json", wasm_library());
    let mut grammars = vec![
        good.clone(),
        json("gone", dir.path().join("gone.wasm")),
        Grammar {
            highlights: vec![broken.clone()],
            ..json("broken", wasm_library())
        },
        // Its own highlights do not compile; another source's do.
        Grammar {
            highlights: vec![broken.clone()],
            lent: vec![
                Queries {
                    highlights: vec![broken.clone()],
                    ..Queries::default()
                },
                Queries {
                    highlights: good.highlights.clone(),
                    ..Queries::default()
                },
            ],
            ..json("lent", wasm_library())
        },
    ];
    grammars.extend(native_failures(dir.path()));
    let mut helper = Helper::with(grammars);
    let error = |reply: Reply| match reply {
        Reply::Error(error) => error,
        other => panic!("{other:?}"),
    };
    let gone = error(helper.open(1, "gone", &text));
    assert!(gone.starts_with("gone: "), "{gone}");
    #[cfg(target_os = "linux")]
    {
        let absent = error(helper.open(1, "absent", &text));
        assert!(absent.starts_with("absent: load "), "{absent}");
        let unnamed = error(helper.open(1, "unnamed", &text));
        assert!(unnamed.contains("tree_sitter_nothing"), "{unnamed}");
        let future = error(helper.ask(&Request::Check {
            name: "future".into(),
        }));
        assert!(future.contains("ABI is 99"), "{future}");
    }
    assert!(error(helper.open(1, "nobody", &text)).contains("no grammar named nobody"));
    // A query that does not compile is passed over, where it is said; a
    // grammar no highlights query of compiles colors no text (bat's
    // grammar does).
    let Reply::Checked { abi, problems } = helper.ask(&Request::Check {
        name: "broken".into(),
    }) else {
        panic!("not checked");
    };
    assert_eq!(abi, 14);
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems[0].starts_with(&format!("highlights: {}, line 2", broken.display())),
        "{problems:?}"
    );
    assert!(
        problems[1].contains("none of its highlights"),
        "{problems:?}"
    );
    let colorless = error(helper.open(1, "broken", &text));
    assert!(colorless.contains("colors nothing"), "{colorless}");
    // The queries lent are tried in turn: the first that compiles colors.
    let Reply::Opened { lines, captures } = helper.open(1, "lent", &text) else {
        panic!("not opened");
    };
    assert!(
        captures.iter().any(|capture| capture == "number"),
        "{captures:?}"
    );
    assert!(helper.colors(1, lines).iter().any(|line| !line.is_empty()));
    let Reply::Checked { problems, .. } = helper.ask(&Request::Check {
        name: "lent".into(),
    }) else {
        panic!("not checked");
    };
    assert_eq!(
        problems.len(),
        2,
        "its own and the first lent: {problems:?}"
    );
    // And the next works.
    let Reply::Checked { problems, .. } = helper.ask(&Request::Check {
        name: "JSON".into(),
    }) else {
        panic!("not checked");
    };
    assert!(problems.is_empty(), "{problems:?}");
    assert!(matches!(
        helper.open(2, "json", &text),
        Reply::Opened { .. }
    ));
}

/// A parse of several MiB stops when cancelled while it runs; the helper
/// serves on. How long a parse takes here is measured first, on a text a
/// sixteenth as long: the cancel is sent once the long one is surely
/// parsing, and answered long before it could have ended.
#[cfg(target_os = "linux")]
#[test]
fn a_parse_is_cancelled_and_the_helper_serves_on() {
    let dir = tempfile::tempdir().unwrap();
    let json_of = |bytes: usize| {
        let mut text = String::from("[\n");
        while text.len() < bytes {
            text.push_str("  {\"key\": \"value\", \"n\": [1, 2.5, true, null]},\n");
        }
        text.push_str("  {}\n]\n");
        text
    };
    let large = write(dir.path(), "large", &json_of(4 << 20));
    let sample = write(dir.path(), "sample", &json_of(256 << 10));
    let small = write(dir.path(), "small", TEXT);
    let mut helper = Helper::with(vec![json("json", native_library())]);
    let started = std::time::Instant::now();
    assert!(matches!(
        helper.open(1, "json", &sample),
        Reply::Opened { .. }
    ));
    let sample = started.elapsed();
    let open = helper.send(&Request::Open {
        doc: 2,
        name: "json".into(),
        text: large,
    });
    let started = std::time::Instant::now();
    // Read, and parsing.
    std::thread::sleep(sample * 2);
    let cancel = helper.send(&Request::Cancel);
    for id in [open, cancel] {
        assert_eq!(
            helper.reply(),
            Some((id, Reply::Error("cancelled".into()))),
            "request {id}"
        );
    }
    // Sixteen times the sample's text would take about sixteen times as long.
    assert!(
        started.elapsed() < sample * 8,
        "{:?} for a sample of {sample:?}",
        started.elapsed()
    );
    assert!(matches!(
        helper.open(3, "json", &small),
        Reply::Opened { .. }
    ));
    // What was cancelled was never kept; what was before it, is.
    assert!(matches!(
        helper.ask(&Request::Folds { doc: 2 }),
        Reply::Error(_)
    ));
    assert_eq!(
        helper.ask(&Request::Folds { doc: 1 }),
        Reply::Folds(Vec::new())
    );
}

/// A request longer than the protocol's bound is refused, and ends the
/// helper: what follows it cannot be told apart.
#[test]
fn a_request_over_the_limit_is_refused() {
    let (mut helper, _) = Helper::start();
    let mut line = format!(
        "{{\"id\":1,\"body\":{{\"Check\":{{\"name\":\"{}\"}}}}}}",
        "x".repeat(MAX_REQUEST)
    );
    line.push('\n');
    let mut input = helper.input.try_clone_writer();
    let writer = std::thread::spawn(move || {
        // The helper stops reading at the bound: the rest is not taken.
        let _ = input.write_all(line.as_bytes());
    });
    let (id, reply) = helper.reply().expect("it says why it ends");
    assert_eq!(id, 0);
    let Reply::Error(said) = reply else {
        panic!("{reply:?}");
    };
    assert!(said.contains("exceeds limit"), "{said}");
    assert_eq!(helper.reply(), None, "it ended");
    let status = helper.child.wait().unwrap();
    assert!(!status.success());
    writer.join().unwrap();
}

/// The standard input of a child, written from another thread.
trait CloneWriter {
    fn try_clone_writer(&self) -> std::fs::File;
}

impl CloneWriter for ChildStdin {
    #[cfg(unix)]
    fn try_clone_writer(&self) -> std::fs::File {
        use std::os::fd::AsFd;
        std::fs::File::from(self.as_fd().try_clone_to_owned().unwrap())
    }
    #[cfg(windows)]
    fn try_clone_writer(&self) -> std::fs::File {
        use std::os::windows::io::AsHandle;
        std::fs::File::from(self.as_handle().try_clone_to_owned().unwrap())
    }
}
