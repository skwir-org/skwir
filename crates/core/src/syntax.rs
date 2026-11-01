//! What skwir and `skwir-tree-sitter-worker` say to each other (ADR-110):
//! the grammars a text is parsed with, and the colors, folds and outline
//! the helper answers. Lines are `document::Tagged`, as the readers'
//! (`document::worker`): the greeting is reply 0, request `n` gets reply
//! `n`, once and in order.
//!
//! The helper is told every grammar found once (`Grammars`), then opens
//! texts by id; each text is a file skwir wrote in the helper's working
//! folder (what the preview holds, ADR-101), never one of the user's. It
//! reads nothing else but the grammars' files, which skwir names.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Bytes of one request line, its newline included: a list of grammars
/// can be long.
pub const MAX_REQUEST: usize = 4 * 1024 * 1024;
/// Bytes of one reply line, its newline included.
pub const MAX_REPLY: usize = 32 * 1024 * 1024;
/// Lines a `Colors` request asks at most.
pub const MAX_LINES: u32 = 4096;
/// Bytes of a text the helper parses at most: what a Full preview holds
/// whole (`preview::held::WHOLE_FULL`).
pub const MAX_TEXT: u64 = 128 * 1024 * 1024;

/// A grammar found (`skwir_platform::grammars`), as the helper loads it.
/// Paths are written as `os_path` writes them, a name that is not UTF-8
/// as its bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grammar {
    /// The language's name, as injections name it (`rust`, `javascript`).
    pub name: String,
    /// The library: a shared library (`.so`, `.dylib`, `.dll`) exporting
    /// `symbol`, or a WebAssembly module (`.wasm`) whose language is
    /// `symbol` without its `tree_sitter_`.
    #[serde(
        serialize_with = "crate::os_path::serialize",
        deserialize_with = "crate::os_path::deserialize"
    )]
    pub library: PathBuf,
    /// `tree_sitter_<name>`, unless the grammar says otherwise.
    pub symbol: String,
    /// Each query's files, read in order and joined: a query a grammar
    /// inherits comes before its own.
    #[serde(with = "paths")]
    pub highlights: Vec<PathBuf>,
    #[serde(with = "paths")]
    pub injections: Vec<PathBuf>,
    #[serde(with = "paths")]
    pub locals: Vec<PathBuf>,
    #[serde(with = "paths")]
    pub folds: Vec<PathBuf>,
    /// Definitions for the outline: `tags.scm` (`@definition.*` and
    /// `@name`) or Zed's `outline.scm` (`@item` and `@name`).
    #[serde(with = "paths")]
    pub outline: Vec<PathBuf>,
    /// The queries other sources hold for its language, tried in order
    /// where those before do not compile against its library (a query
    /// written for another version of the grammar names nodes it lacks).
    #[serde(default)]
    pub lent: Vec<Queries>,
    /// What injected text in this language says it is
    /// (`tree-sitter.json`'s `injection-regex`); its name when `None`.
    pub injection_regex: Option<String>,
}

impl Grammar {
    /// Its own queries (the fields above `lent`), then those lent, in the
    /// order they are tried.
    pub fn query_sets(&self) -> Vec<Queries> {
        std::iter::once(Queries {
            highlights: self.highlights.clone(),
            injections: self.injections.clone(),
            locals: self.locals.clone(),
            folds: self.folds.clone(),
            outline: self.outline.clone(),
        })
        .chain(self.lent.iter().cloned())
        .collect()
    }

    /// Every file it names: its library and every query of every set.
    pub fn files(&self) -> impl Iterator<Item = &PathBuf> {
        std::iter::once(&self.library)
            .chain(&self.highlights)
            .chain(&self.injections)
            .chain(&self.locals)
            .chain(&self.folds)
            .chain(&self.outline)
            .chain(self.lent.iter().flat_map(Queries::files))
            .filter(|file| !file.as_os_str().is_empty())
    }
}

/// A grammar's queries as one source holds them: each kind's files, read
/// in order and joined.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queries {
    #[serde(with = "paths")]
    pub highlights: Vec<PathBuf>,
    #[serde(with = "paths")]
    pub injections: Vec<PathBuf>,
    #[serde(with = "paths")]
    pub locals: Vec<PathBuf>,
    #[serde(with = "paths")]
    pub folds: Vec<PathBuf>,
    #[serde(with = "paths")]
    pub outline: Vec<PathBuf>,
}

impl Queries {
    /// Its files, every kind.
    pub fn files(&self) -> impl Iterator<Item = &PathBuf> {
        self.highlights
            .iter()
            .chain(&self.injections)
            .chain(&self.locals)
            .chain(&self.folds)
            .chain(&self.outline)
    }
}

/// A list of paths written as `os_path` writes each.
mod paths {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::path::{Path, PathBuf};

    struct Lossless<'a>(&'a Path);

    impl Serialize for Lossless<'_> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            crate::os_path::serialize(&self.0, s)
        }
    }

    pub fn serialize<S: Serializer>(paths: &[PathBuf], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(paths.iter().map(|path| Lossless(path)))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<PathBuf>, D::Error> {
        Ok(Vec::<crate::os_path::PathArg>::deserialize(d)?
            .into_iter()
            .map(crate::os_path::PathArg::into_inner)
            .collect())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Request {
    /// Every grammar found, by which injections are colored. Sent once,
    /// after the greeting; a grammar is loaded only when first needed.
    Grammars(Vec<Grammar>),
    /// Load the grammar named `name` (one of `Grammars`) and check its
    /// queries, without parsing anything: the Reading page's question.
    Check { name: String },
    /// Parse `text` (a UTF-8 file in the working folder) with the grammar
    /// named `name` and keep its colors as `doc`, in place of what `doc`
    /// held. Answered `Opened`.
    Open {
        doc: u32,
        name: String,
        #[serde(
            serialize_with = "crate::os_path::serialize",
            deserialize_with = "crate::os_path::deserialize"
        )]
        text: PathBuf,
    },
    /// The colors of lines `from..to` of `doc` (`to - from` at most
    /// `MAX_LINES`). Answered `Colors`.
    Colors { doc: u32, from: u32, to: u32 },
    /// Where `doc` folds. Answered `Folds`.
    Folds { doc: u32 },
    /// The definitions `doc` holds, for the outline. Answered `Outline`.
    Outline { doc: u32 },
    /// Forget `doc`. Answered `Done`.
    Close { doc: u32 },
    /// Stop what runs now: a parse notices soon. Answered, like any
    /// request, by an `Error`.
    Cancel,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Reply {
    /// The greeting: the tree-sitter library's version and the language
    /// versions (ABI) it takes, and whether it runs WebAssembly grammars.
    Ready {
        version: String,
        abi: (u32, u32),
        wasm: bool,
    },
    /// `Check`: the grammar's ABI, and what is wrong with it or a query
    /// (the query left out, the grammar used without it).
    Checked {
        abi: u32,
        problems: Vec<String>,
    },
    /// `Open`: how many lines the text holds, and the capture names its
    /// colors index (`Span::capture`): the grammar's highlights and those
    /// of the languages injected, in the order first met.
    Opened {
        lines: u32,
        captures: Vec<String>,
    },
    /// `Colors`: for each line asked, its spans, in order, not
    /// overlapping, in the line's own bytes (its ending left out).
    Colors {
        lines: Vec<Vec<Span>>,
    },
    /// `Folds`: lines `start..end` a fold hides after `start`, nested or
    /// apart, in order of `start`.
    Folds(Vec<(u32, u32)>),
    /// `Outline`: the definitions, in the text's order.
    Outline(Vec<Symbol>),
    Done,
    Unavailable(String),
    Error(String),
}

/// A run of a line colored by one capture.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    /// An index into `Opened::captures`: the innermost capture.
    pub capture: u16,
}

/// A definition a grammar's outline query found.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Symbol {
    /// Its name, as the text writes it.
    pub name: String,
    /// What it is: the capture's last part (`function`, `class`,
    /// `method`), else `item`.
    pub kind: String,
    /// The line it starts on, from 0.
    pub line: u32,
    /// How many definitions hold it.
    pub depth: u32,
}

// The names a test writes are Unix's bytes.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A grammar whose files are not named in UTF-8 is told whole, as are
    /// the queries lent to it.
    #[test]
    fn names_that_are_not_utf8_are_told() {
        use std::os::unix::ffi::OsStrExt;
        let odd = PathBuf::from(std::ffi::OsStr::from_bytes(b"/g/\xff.so"));
        let grammar = Grammar {
            name: "odd".into(),
            library: odd.clone(),
            highlights: vec![odd.clone(), "/g/plain.scm".into()],
            lent: vec![Queries {
                folds: vec![odd.clone()],
                ..Queries::default()
            }],
            ..Grammar::default()
        };
        let line = crate::jsonl::encode(&Request::Grammars(vec![grammar.clone()])).unwrap();
        let back: Request = serde_json::from_slice(&line).unwrap();
        assert_eq!(back, Request::Grammars(vec![grammar.clone()]));
        assert_eq!(grammar.files().count(), 4);
        // Told before `lent` was: none lent.
        let old = r#"{"name":"x","library":"/x.so","symbol":"tree_sitter_x",
            "highlights":[],"injections":[],"locals":[],"folds":[],"outline":[],
            "injection_regex":null}"#;
        let old: Grammar = serde_json::from_str(old).unwrap();
        assert!(old.lent.is_empty());
    }
}
