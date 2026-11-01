//! The grammars skwir found, each loaded when a text first needs it and
//! kept for the helper's life: its language, its highlights (with its
//! injections and locals) and its folds and outline queries, and what was
//! wrong with any of them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use skwir_core::syntax::{Grammar, Queries};
use tree_sitter::{Language, Query, QueryError, QueryErrorKind, WasmStore, wasmtime};
use tree_sitter_highlight::HighlightConfiguration;

/// Bytes of one query file read at most: the largest editors carry are
/// tens of KiB.
const QUERY_FILE: u64 = 4 * 1024 * 1024;
/// Bytes of one query, its files joined, at most.
const QUERY: usize = 8 * 1024 * 1024;
/// Bytes of a WebAssembly grammar read at most: the largest are a few MiB.
const WASM: u64 = 64 * 1024 * 1024;

/// Captures that name no color but what Neovim does with the text (spell
/// checking, hiding it). The highlighter colors a node by the last capture
/// of it (`(comment) @comment @spell`), so they are left out of the
/// highlights read, or a comment would lose its color.
const UNCOLORED: [&str; 3] = ["spell", "nospell", "conceal"];

/// A grammar loaded: what parses and colors its texts. Loaded once and
/// never let go of while the helper runs, so a text being colored and the
/// languages injected into it share them.
pub struct Loaded {
    pub language: Language,
    pub abi: u32,
    /// Highlights, injections and locals together, configured with every
    /// capture name the grammars loaded so far have (`Grammars::captures`).
    pub highlight: HighlightConfiguration,
    pub folds: Option<Query>,
    pub outline: Option<Query>,
    /// What is wrong with a query, left out for it.
    pub problems: Vec<String>,
    /// Why it colors nothing: no highlights query it was given compiles
    /// against its library. A text is then not colored by it (bat's
    /// grammar colors it), though it still parses what is injected.
    pub colorless: Option<String>,
}

pub struct Grammars {
    list: Vec<Grammar>,
    /// Each grammar's `injection_regex`, compiled once when told (`None`
    /// without one, or for one that does not compile).
    regexes: Vec<Option<regex::Regex>>,
    /// Each grammar of `list` once loaded, or why it could not be.
    loaded: HashMap<usize, Result<&'static Loaded, String>>,
    /// Which grammar an injection names, once asked (lowercase).
    injected: HashMap<String, Option<usize>>,
    /// The capture names of every grammar loaded, in the order first met:
    /// a highlight's index is into this list whatever grammar colored it,
    /// so the spans of a text and of the languages injected into it are
    /// told apart.
    captures: Vec<String>,
    known: HashMap<String, usize>,
    /// Wasmtime, for every WebAssembly grammar, and the store that loads
    /// them: a parser runs a language loaded here in a store of its own.
    engine: Option<wasmtime::Engine>,
    loader: Option<WasmStore>,
}

impl Grammars {
    pub fn new() -> Self {
        Grammars {
            list: Vec::new(),
            regexes: Vec::new(),
            loaded: HashMap::new(),
            injected: HashMap::new(),
            captures: Vec::new(),
            known: HashMap::new(),
            engine: None,
            loader: None,
        }
    }

    /// Take the grammars found in place of those told before. What was
    /// loaded is kept (the texts open were colored with it) but no longer
    /// found by name.
    pub fn set(&mut self, list: Vec<Grammar>) {
        self.regexes = list
            .iter()
            .map(|grammar| {
                regex::RegexBuilder::new(grammar.injection_regex.as_deref()?)
                    .case_insensitive(true)
                    .size_limit(1 << 20)
                    .build()
                    .ok()
            })
            .collect();
        self.list = list;
        self.loaded.clear();
        self.injected.clear();
    }

    /// Whether a grammar told is a WebAssembly module: a parser needs a
    /// store before it parses any text, since one may be injected.
    pub fn any_wasm(&self) -> bool {
        self.list.iter().any(|grammar| is_wasm(&grammar.library))
    }

    /// The engine every store is made with.
    pub fn engine(&mut self) -> &wasmtime::Engine {
        self.engine.get_or_insert_with(wasmtime::Engine::default)
    }

    /// The capture name a highlight's index is.
    pub fn capture(&self, index: usize) -> &str {
        self.captures.get(index).map_or("", String::as_str)
    }

    /// The grammar called `name`: its name as told, else ignoring case.
    pub fn find(&self, name: &str) -> anyhow::Result<usize> {
        self.list
            .iter()
            .position(|grammar| grammar.name == name)
            .or_else(|| {
                self.list
                    .iter()
                    .position(|grammar| grammar.name.eq_ignore_ascii_case(name))
            })
            .with_context(|| format!("no grammar named {name}"))
    }

    /// The grammar at `index`, loaded now if it was not.
    pub fn load(&mut self, index: usize) -> anyhow::Result<&'static Loaded> {
        if let Some(loaded) = self.loaded.get(&index) {
            return loaded.clone().map_err(anyhow::Error::msg);
        }
        let grammar = self.list.get(index).context("no such grammar")?.clone();
        let loaded = self
            .language(&grammar)
            .map(|(language, abi)| {
                let loaded: &'static Loaded =
                    Box::leak(Box::new(self.queries(&grammar, language, abi)));
                for problem in &loaded.problems {
                    eprintln!("{}: {problem}", grammar.name);
                }
                loaded
            })
            .map_err(|error| format!("{}: {error:#}", grammar.name));
        self.loaded.insert(index, loaded.clone());
        loaded.map_err(anyhow::Error::msg)
    }

    /// The highlights of the language an injection names, loaded if need
    /// be; `None` when no grammar is for it, or it does not load: the
    /// injected text stays as it is.
    pub fn injected(&mut self, name: &str) -> Option<&'static HighlightConfiguration> {
        let key = name.to_lowercase();
        let index = if let Some(index) = self.injected.get(&key) {
            *index
        } else {
            let index = self.injection(&key);
            self.injected.insert(key, index);
            index
        };
        match self.load(index?) {
            Ok(loaded) => Some(&loaded.highlight),
            Err(error) => {
                eprintln!("injected {name}: {error:#}");
                None
            }
        }
    }

    /// The grammar text injected as `name` (lowercase) is in: the first
    /// whose `injection_regex` matches it, or without one whose name it
    /// is; else the first so named, whatever its regex says.
    fn injection(&self, name: &str) -> Option<usize> {
        let says = |(grammar, regex): (&Grammar, &Option<regex::Regex>)| match (
            &grammar.injection_regex,
            regex,
        ) {
            (Some(_), regex) => regex.as_ref().is_some_and(|regex| regex.is_match(name)),
            (None, _) => grammar.name.eq_ignore_ascii_case(name),
        };
        self.list
            .iter()
            .zip(&self.regexes)
            .position(says)
            .or_else(|| {
                self.list
                    .iter()
                    .position(|grammar| grammar.name.eq_ignore_ascii_case(name))
            })
    }

    /// The grammar's language and its ABI: a shared library's symbol, or a
    /// WebAssembly module's language, refused outside the ABIs this
    /// tree-sitter parses.
    fn language(&mut self, grammar: &Grammar) -> anyhow::Result<(Language, u32)> {
        let language = if is_wasm(&grammar.library) {
            self.wasm_language(grammar)?
        } else {
            native_language(grammar)?
        };
        let abi = language.abi_version();
        let range = tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION..=tree_sitter::LANGUAGE_VERSION;
        anyhow::ensure!(
            range.contains(&abi),
            "its ABI is {abi}, and this tree-sitter reads {} to {}",
            range.start(),
            range.end()
        );
        Ok((language, u32::try_from(abi)?))
    }

    fn wasm_language(&mut self, grammar: &Grammar) -> anyhow::Result<Language> {
        let bytes = skwir_core::fs::file::read_regular(&grammar.library, WASM)
            .with_context(|| format!("read {}", grammar.library.display()))?;
        let name = grammar
            .symbol
            .strip_prefix("tree_sitter_")
            .unwrap_or(&grammar.symbol);
        anyhow::ensure!(!name.contains('\0'), "its symbol holds a NUL");
        if self.loader.is_none() {
            let store =
                WasmStore::new(self.engine()).map_err(|error| anyhow::anyhow!("{error}"))?;
            self.loader = Some(store);
        }
        let loader = self.loader.as_mut().expect("made above");
        loader
            .load_language(name, &bytes)
            .map_err(|error| anyhow::anyhow!("{error}"))
    }

    /// The grammar's queries read and compiled against `language`, its own
    /// first, then each set lent to it (`Grammar::lent`): the first set
    /// whose highlights compile colors it, and folds and an outline come
    /// from the first set whose query of them compiles. A query that does
    /// not compile is said in `problems` and passed over; with no
    /// highlights at all, the grammar is `colorless`.
    fn queries(&mut self, grammar: &Grammar, language: Language, abi: u32) -> Loaded {
        let mut problems = Vec::new();
        let sets = grammar.query_sets();
        let mut highlight = None;
        for set in sets.iter().filter(|set| !set.highlights.is_empty()) {
            highlight = highlight_of(grammar, &language, set, &mut problems);
            if highlight.is_some() {
                break;
            }
        }
        let colorless = highlight
            .is_none()
            .then(|| "none of its highlights queries compiles against it".to_owned());
        let mut highlight = highlight.unwrap_or_else(|| {
            HighlightConfiguration::new(language.clone(), grammar.name.clone(), "", "", "")
                .expect("an empty query compiles")
        });
        self.configure(&mut highlight);
        let mut compiled = |kind: &str, files: fn(&Queries) -> &[PathBuf]| {
            sets.iter().find_map(|set| {
                let files = files(set);
                if files.is_empty() {
                    return None;
                }
                let joined = match Joined::read(files) {
                    Ok(joined) if !joined.text.trim().is_empty() => joined,
                    Ok(_) => return None,
                    Err(error) => {
                        problems.push(format!("{kind}: {error:#}"));
                        return None;
                    }
                };
                Query::new(&language, &joined.text)
                    .map_err(|error| problems.push(joined.explain(kind, &error)))
                    .ok()
            })
        };
        let folds = compiled("folds", |set| &set.folds);
        let outline = compiled("outline", |set| &set.outline);
        Loaded {
            language,
            abi,
            highlight,
            folds,
            outline,
            problems,
            colorless,
        }
    }

    /// Tell `highlight` every capture name known, its own added: each of
    /// its captures is then a highlight, its index into `captures`, which
    /// only grows, so what was configured before stays right.
    fn configure(&mut self, highlight: &mut HighlightConfiguration) {
        for name in highlight.names() {
            if colors(name) && !self.known.contains_key(*name) {
                self.known.insert((*name).to_owned(), self.captures.len());
                self.captures.push((*name).to_owned());
            }
        }
        highlight.configure(&self.captures);
    }
}

/// The highlights of one set of queries, with its injections and locals:
/// `None` when its highlights do not read or compile (said in `problems`),
/// so the next set is tried. An injections or locals query that does not
/// compile is left out, the highlights kept.
fn highlight_of(
    grammar: &Grammar,
    language: &Language,
    set: &Queries,
    problems: &mut Vec<String>,
) -> Option<HighlightConfiguration> {
    let mut read = |kind: &str, files: &[PathBuf]| match Joined::read(files) {
        Ok(joined) => Some(joined),
        Err(error) => {
            problems.push(format!("{kind}: {error:#}"));
            None
        }
    };
    let mut highlights = read("highlights", &set.highlights)?;
    highlights.text = without_uncolored(&highlights.text);
    if let Err(error) = Query::new(language, &highlights.text) {
        problems.push(highlights.explain("highlights", &error));
        return None;
    }
    let mut parts = [
        (
            "injections",
            read("injections", &set.injections).unwrap_or_default(),
        ),
        ("locals", read("locals", &set.locals).unwrap_or_default()),
    ];
    let configure = |parts: &[(&str, Joined); 2]| {
        HighlightConfiguration::new(
            language.clone(),
            grammar.name.clone(),
            &highlights.text,
            &parts[0].1.text,
            &parts[1].1.text,
        )
    };
    if let Ok(highlight) = configure(&parts) {
        return Some(highlight);
    }
    // Which one does not compile, each alone: it is left out.
    for (kind, joined) in &mut parts {
        if let Err(error) = Query::new(language, &joined.text) {
            problems.push(joined.explain(kind, &error));
            *joined = Joined::default();
        }
    }
    configure(&parts)
        .or_else(|together| {
            // Each compiles, not together: the highlights alone.
            problems.push(format!(
                "highlights, injections and locals together: {together}"
            ));
            for (_, joined) in &mut parts {
                *joined = Joined::default();
            }
            configure(&parts)
        })
        .map_err(|error| problems.push(format!("highlights: {error}")))
        .ok()
}

/// Whether a capture of a highlights query is a color: not one of those
/// the highlighter uses itself (injections, locals) nor one a query keeps
/// for its predicates (`@_name`).
fn colors(name: &str) -> bool {
    !(name.starts_with('_') || name.starts_with("injection.") || name.starts_with("local."))
}

fn is_wasm(library: &Path) -> bool {
    library
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("wasm"))
}

/// A shared library's language: its symbol, a function the tree-sitter CLI
/// generated, called once. The library stays loaded for the helper's life:
/// the language is its static data.
fn native_language(grammar: &Grammar) -> anyhow::Result<Language> {
    // SAFETY: loading a library runs its initializers. A grammar is code
    // skwir was pointed at by an editor's or the system's folders, which
    // is why it is loaded here, in an isolated helper, and never in skwir.
    #[allow(unsafe_code)]
    let library = unsafe { libloading::Library::new(&grammar.library) }
        .with_context(|| format!("load {}", grammar.library.display()))?;
    // SAFETY: the symbol is taken as what the grammar's parser.c defines,
    // `const TSLanguage *tree_sitter_<name>(void)`; the language it
    // returns is static data of the library, which is never unloaded.
    #[allow(unsafe_code)]
    let language = unsafe {
        let symbol = library
            .get::<unsafe extern "C" fn() -> *const ()>(grammar.symbol.as_bytes())
            .with_context(|| format!("find {} in {}", grammar.symbol, grammar.library.display()))?;
        tree_sitter::Language::new(tree_sitter_language::LanguageFn::from_raw(*symbol))
    };
    std::mem::forget(library);
    Ok(language)
}

/// A query's files read and joined in order, and where each begins, to
/// say where a problem is.
#[derive(Default)]
struct Joined {
    text: String,
    /// Each file and the line of `text` it begins on.
    files: Vec<(PathBuf, usize)>,
}

impl Joined {
    fn read(files: &[PathBuf]) -> anyhow::Result<Joined> {
        let mut joined = Joined::default();
        for file in files {
            let text = skwir_core::fs::file::read_regular_text(file, QUERY_FILE)
                .with_context(|| format!("read {}", file.display()))?;
            anyhow::ensure!(
                joined.text.len() + text.len() < QUERY,
                "its files exceed {} MiB",
                QUERY >> 20
            );
            let line = joined.text.lines().count();
            joined.files.push((file.clone(), line));
            for line in text.lines() {
                // An editor's modeline (Neovim's `; inherits:` and
                // `; extends`) is a comment: skwir resolved what it says.
                if !is_modeline(line) {
                    joined.text.push_str(line);
                }
                // Kept as an empty line, so lines are counted as in the file.
                joined.text.push('\n');
            }
        }
        Ok(joined)
    }

    /// `error`, which compiling the query of `kind` gave, said with the
    /// file and line it is at.
    fn explain(&self, kind: &str, error: &QueryError) -> String {
        let what = match error.kind {
            QueryErrorKind::Field => "invalid field name ",
            QueryErrorKind::NodeType => "invalid node type ",
            QueryErrorKind::Capture => "invalid capture name ",
            QueryErrorKind::Predicate => "invalid predicate: ",
            QueryErrorKind::Structure => "impossible pattern: ",
            QueryErrorKind::Syntax => "invalid syntax: ",
            QueryErrorKind::Language => "",
        };
        let message = error
            .message
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        match self
            .files
            .iter()
            .rev()
            .find(|(_, start)| *start <= error.row)
        {
            Some((file, start)) => format!(
                "{kind}: {}, line {}: {what}{message}",
                file.display(),
                error.row - start + 1
            ),
            None => format!("{kind}: {what}{message}"),
        }
    }
}

fn is_modeline(line: &str) -> bool {
    let comment = line.trim_start();
    let Some(said) = comment.strip_prefix(';') else {
        return false;
    };
    let said = said.trim_start_matches(';').trim_start();
    said.starts_with("inherits:") || said == "extends" || said.starts_with("extends ")
}

/// `query` without the captures in `UNCOLORED`, outside strings and
/// comments: `(comment) @comment @spell` reads `(comment) @comment`.
fn without_uncolored(query: &str) -> String {
    let mut kept = String::with_capacity(query.len());
    let mut rest = query;
    while let Some(at) = rest.find(['"', ';', '@']) {
        kept.push_str(&rest[..at]);
        rest = &rest[at..];
        let taken = match rest.as_bytes()[0] {
            b'"' => string_length(rest),
            b';' => rest.find('\n').unwrap_or(rest.len()),
            _ => {
                let name_end = rest[1..]
                    .find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.' | '-')))
                    .map_or(rest.len(), |end| end + 1);
                if UNCOLORED.contains(&&rest[1..name_end]) {
                    rest = &rest[name_end..];
                    continue;
                }
                name_end
            }
        };
        kept.push_str(&rest[..taken]);
        rest = &rest[taken..];
    }
    kept.push_str(rest);
    kept
}

/// Bytes of the string literal `text` begins with, its quotes included
/// (all of `text` when it is not closed).
fn string_length(text: &str) -> usize {
    let mut escaped = false;
    for (at, c) in text.char_indices().skip(1) {
        match c {
            '\\' if !escaped => escaped = true,
            '"' if !escaped => return at + 1,
            _ => escaped = false,
        }
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncolored_captures_are_left_out_of_highlights() {
        assert_eq!(
            without_uncolored("(comment) @comment @spell\n(string) @string @nospell"),
            "(comment) @comment \n(string) @string "
        );
        // Not in a string, nor a comment, nor a longer name.
        let kept = "((x) @spelling (#eq? @spelling \"@spell\")) ; @spell\n(y) @conceal.x";
        assert_eq!(without_uncolored(kept), kept);
        assert_eq!(
            without_uncolored("(a \"\\\"@spell\") @conceal"),
            "(a \"\\\"@spell\") "
        );
    }

    #[test]
    fn modelines_are_told_from_comments() {
        assert!(is_modeline("; inherits: c,cpp"));
        assert!(is_modeline(";; extends"));
        assert!(!is_modeline("; a comment"));
        assert!(!is_modeline("(identifier) @variable"));
    }
}
