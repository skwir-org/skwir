//! Links found in plain text: what a reader would click if the text were a
//! page (ADR-029).
//!
//! An address is found by its scheme, and only by the schemes skwir
//! follows: `http`, `https` and `mailto` go to the desktop's browser and mail
//! client, `file` into skwir. A name with a dot is never taken for a domain:
//! in a file manager `main.rs`, `setup.py` and `README.md` are files, though
//! `.rs`, `.py` and `.md` are countries' domains. In a terminal the words of
//! a line may also be paths (`paths`), and in a file's text what source and
//! markup write as one (`written`); only the filesystem confirms them
//! (`resolve`, `WrittenPaths`, on a worker).

use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use skwir_core::fs::volume::Here;
use skwir_core::inspection::{Block, Run};

/// The schemes found in text, as they are written before what follows.
const SCHEMES: [&str; 4] = ["https://", "http://", "file://", "mailto:"];

/// The longest word taken for a path.
const PATH_BYTES: usize = 4096;

/// Whether `c` ends an address: a space, a control character, what RFC 3986
/// leaves out of URIs and texts put around them (`<>"{}|\^` and the
/// backtick), and the quotes, brackets and stops of other scripts.
fn ends_address(c: char) -> bool {
    c.is_whitespace()
        || c.is_control()
        || matches!(
            c,
            '<' | '>'
                | '"'
                | '`'
                | '{'
                | '}'
                | '|'
                | '\\'
                | '^'
                | '«'
                | '»'
                | '“'
                | '”'
                | '‘'
                | '’'
                | '⟨'
                | '⟩'
                | '「'
                | '」'
                | '『'
                | '』'
                | '（'
                | '）'
                | '。'
                | '、'
                | '，'
        )
}

/// An address without what a sentence put after it: a full stop, a comma,
/// a closing bracket it does not open (`(see https://a.b/c)`, while
/// Wikipedia's `…/Rust_(language)` keeps its own).
fn trim_end(address: &str) -> &str {
    // The brackets counted once, and as they are dropped: counted again
    // for each one dropped, a run of them was quadratic.
    let count = |c: char| address.matches(c).count().cast_signed();
    let mut parens = count('(') - count(')');
    let mut brackets = count('[') - count(']');
    let mut address = address;
    while let Some(last) = address.chars().next_back() {
        let drop = match last {
            '.' | ',' | ':' | ';' | '!' | '?' | '\'' | '*' => true,
            ')' if parens < 0 => {
                parens += 1;
                true
            }
            ']' if brackets < 0 => {
                brackets += 1;
                true
            }
            _ => false,
        };
        if !drop {
            break;
        }
        address = &address[..address.len() - last.len_utf8()];
    }
    address
}

/// Whether what follows `scheme` makes an address of it: a host for the
/// web, a path for a file, someone at somewhere for mail.
fn complete(scheme: &str, rest: &str) -> bool {
    match scheme {
        "mailto:" => rest
            .split_once('@')
            .is_some_and(|(who, at)| !who.is_empty() && at.contains(char::is_alphanumeric)),
        "file://" => rest.contains('/'),
        _ => {
            let host = rest.split(['/', '?', '#']).next().unwrap_or("");
            host.contains(char::is_alphanumeric)
        }
    }
}

/// The longest an address is taken to be, after its scheme: about what
/// browsers take.
const ADDRESS_MOST: usize = 2048;

/// The addresses in `text`, as byte ranges, in order.
pub fn urls(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut from = 0;
    // The end of the address last looked for, and where that look began:
    // any colon between them ends there too (`ends_address` is a
    // character's), where each colon of a long run (`mailto:` without an
    // `@`, over and over) looked for its end again.
    let mut ending: Option<(usize, usize)> = None;
    let mut colons = 0usize;
    while let Some(colon) = text[from..].find(':').map(|i| from + i) {
        from = colon + 1;
        // A text of megabytes is long to look through: a preview moved on
        // from stops here (its answer is not taken).
        colons += 1;
        if colons.is_multiple_of(1024) && skwir_core::scheduler::job_cancelled() {
            break;
        }
        // The scheme is the letters right before the colon, and starts a
        // word: `xhttp://` is not `http://`.
        let start = text[..colon]
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_ascii_alphabetic())
            .last()
            .map_or(colon, |(i, _)| i);
        if start == colon
            || text[..start]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
        {
            continue;
        }
        let Some(scheme) = SCHEMES.into_iter().find(|scheme| {
            text.get(start..start + scheme.len())
                .is_some_and(|written| written.eq_ignore_ascii_case(scheme))
        }) else {
            continue;
        };
        let body = start + scheme.len();
        let end = match ending {
            Some((looked, end)) if looked <= body && body <= end => end,
            _ => {
                let end = text[body..]
                    .find(ends_address)
                    .map_or(text.len(), |i| body + i);
                ending = Some((body, end));
                end
            }
        };
        // No longer than an address can be: each colon of a long run looks
        // at so much at most, however long the run.
        let mut end = end.min(body + ADDRESS_MOST);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let end = start + trim_end(&text[start..end]).len();
        if end > body && complete(scheme, &text[body..end]) {
            found.push(start..end);
            from = end;
        }
    }
    found
}

/// A word of a terminal's line that may name a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathWord {
    /// What is drawn as the link: the path, and the line and column after it.
    pub range: Range<usize>,
    /// The path as written, without the quotes around it.
    pub path: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// The words of `text` that may name a file, as a terminal shows them: a
/// path (`/etc/hosts`, `~/notes`, `src/main.rs`), a name (`Cargo.toml`, as
/// `ls` lists them) or a quoted one (`'my file.txt'`, as `ls` quotes a name
/// with a space), each maybe followed by the line and column a message
/// points at (`src/main.rs:12:5`). Addresses are not paths: `urls` finds
/// them. Whether a word names a file is the filesystem's to say (`resolve`).
pub fn paths(text: &str) -> Vec<PathWord> {
    let mut words = Vec::new();
    let mut at = 0;
    while let Some(start) = text[at..]
        .find(|c: char| !c.is_whitespace())
        .map(|i| at + i)
    {
        let quote = text[start..]
            .chars()
            .next()
            .filter(|c| matches!(c, '\'' | '"'));
        if let Some(quote) = quote
            && let Some(len) = text[start + 1..].find(quote)
        {
            let inner = start + 1..start + 1 + len;
            words.extend(path_word(text, inner.clone(), true));
            at = inner.end + 1;
            continue;
        }
        let end = text[start..]
            .find(char::is_whitespace)
            .map_or(text.len(), |i| start + i);
        words.extend(path_word(text, start..end, false));
        at = end;
    }
    words
}

/// `range` of `text` as a path word, if it can be one: a sentence's
/// brackets and stops around it are not part of it (a quoted name keeps
/// everything), nor is grep's rest of the line after `path:12:`.
fn path_word(text: &str, range: Range<usize>, quoted: bool) -> Option<PathWord> {
    let mut start = range.start;
    let mut word = &text[range];
    if !quoted {
        let trimmed = word.trim_start_matches(['(', '[', '{', '<', '"', '\'', '`']);
        start += word.len() - trimmed.len();
        word = trimmed.trim_end_matches(['(', ')', ']', '}', '>', '"', '\'', '`', ',', ';', '.']);
    }
    if word.is_empty()
        || word.len() > PATH_BYTES
        || word.starts_with('-')
        || word.contains("://")
        || word.chars().any(char::is_control)
        || word
            .get(..7)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("mailto:"))
    {
        return None;
    }
    let digits = |s: &str| s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let position = word
        .match_indices(':')
        .map(|(i, _)| i)
        .find(|&i| i > 0 && word[i + 1..].starts_with(|c: char| c.is_ascii_digit()));
    let (path, line, column, len) = if let Some(colon) = position {
        let after = colon + 1;
        let line_end = after + digits(&word[after..]);
        let line = word[after..line_end].parse().ok();
        let (column, len) = match word[line_end..].strip_prefix(':') {
            Some(rest) if rest.starts_with(|c: char| c.is_ascii_digit()) => {
                let end = line_end + 1 + digits(rest);
                (word[line_end + 1..end].parse().ok(), end)
            }
            _ => (None, line_end),
        };
        (&word[..colon], line, column, len)
    } else {
        let path = if quoted {
            word
        } else {
            word.trim_end_matches(':')
        };
        (path, None, None, path.len())
    };
    // A number, a time or a stop is not a name.
    if !(path.chars().any(char::is_alphabetic) || path.len() > 1 && path.contains('/'))
        || path == "~"
    {
        return None;
    }
    Some(PathWord {
        range: start..start + len,
        path: path.to_owned(),
        line,
        column,
    })
}

/// `path` with `.` and `..` worked out by its text: following links would
/// show a folder the path never named.
fn normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// The file a path word names, read from `base`, the folder the shell was
/// in when the word was printed: an absolute path as it is, `~/…` in the
/// home folder, anything else in `base`, or, when it has a folder in it
/// (`crates/ui/src/x.rs`), in the nearest folder above `base` that has it,
/// as cargo writes paths from its workspace's root wherever it ran. A bare
/// name, `./x` and `../x` are looked for in `base` only. Only paths whose
/// lookup stays on this machine are looked up (`here`): never a server's
/// share named by the path, nor what an automount point would mount. It
/// asks the filesystem: call it on a worker.
pub fn resolve(word: &str, base: &Path, here: &Here) -> Option<PathBuf> {
    let there = |path: PathBuf| {
        let path = normal(&path);
        (here.holds(&path, base) && path.exists()).then_some(path)
    };
    let path = Path::new(word);
    if path.is_absolute() {
        return there(path.to_owned());
    }
    if let Some(rest) = word.strip_prefix("~/") {
        return there(skwir_core::os_path::home()?.join(rest));
    }
    if !base.is_absolute() {
        return None;
    }
    let walks = word.contains('/') && !word.starts_with("./") && !word.starts_with("../");
    if !walks {
        return there(base.join(path));
    }
    base.ancestors().find_map(|dir| there(dir.join(path)))
}

/// The words of a line of a file that may name a file, as source and
/// markup write them: what quotes hold (a string, `href="page.html"`,
/// `#include "util.h"`, Markdown's inline code), a Markdown link's target
/// (`[a](notes/a.md)`), what `url(…)` holds, and a word with a slash after
/// a name (`see docs/guide.md`, `src/main.rs:12`). A bare word is not: names in
/// code are mostly not files, and each would be a lookup. An apostrophe
/// inside a word (`don't`, `l'été`) opens no quote. Whether a word names
/// a file is the filesystem's to say (`WrittenPaths`).
pub fn written(text: &str) -> Vec<PathWord> {
    let mut words = Vec::new();
    let mut at = 0;
    while let Some(open) = text[at..].find(['"', '\'', '`']).map(|i| at + i) {
        let quote = text[open..].chars().next().unwrap_or('"');
        at = open + 1;
        let in_word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
        if quote == '\'' && in_word(text[..open].chars().next_back()) {
            continue;
        }
        let Some(len) = text[at..].find(quote) else {
            continue;
        };
        let close = at + len;
        if quote == '\'' && in_word(text[close + 1..].chars().next()) {
            continue;
        }
        words.extend(path_word(text, at..close, true));
        at = close + 1;
    }
    for opening in ["](", "url("] {
        for (open, _) in text.match_indices(opening) {
            let from = open + opening.len();
            if let Some(len) = text[from..].find([')', ' ']) {
                words.extend(path_word(text, from..from + len, true));
            }
        }
    }
    let marks = |c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\''
                    | '`'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '<'
                    | '>'
                    | '='
                    | ','
                    | ';'
                    | '|'
            )
    };
    let mut start = None;
    for (i, c) in text.char_indices().chain([(text.len(), ' ')]) {
        match (marks(c), start) {
            (true, Some(from)) => {
                start = None;
                // A slash after a name, not a comment's `//` or a closing
                // tag's `/p`.
                let word = &text[from..i];
                if word.trim_start_matches('/').contains('/')
                    && word.contains(char::is_alphanumeric)
                {
                    words.extend(path_word(text, from..i, false));
                }
            }
            (false, None) => start = Some(i),
            _ => {}
        }
    }
    words.sort_by_key(|word| (word.range.start, std::cmp::Reverse(word.range.end)));
    words.dedup_by(|later, kept| later.range == kept.range);
    words
}

/// How many words a call of [`WrittenPaths::link`] looks up that it was
/// not told of before: a list of thousands of names links what these
/// found, and the rest of a long file is linked as it is read.
const LOOKUPS: usize = 256;
/// How many answers a file's words keep before they are asked again.
const KNOWN: usize = 4096;

/// The paths a text read from a file writes (`written`), looked up from
/// that file's folder as [`resolve`] reads a path printed there, and only
/// where that stays on this machine. Each word is looked up once for the
/// file; a call looks up [`LOOKUPS`] new words at most.
pub struct WrittenPaths {
    base: PathBuf,
    known: std::collections::HashMap<String, Option<PathBuf>>,
}

impl WrittenPaths {
    /// For the text of the file at `file`; `None` when it is in no folder
    /// it can be named from.
    pub fn new(file: &Path) -> Option<Self> {
        let base = file.parent().filter(|base| base.is_absolute())?;
        Some(Self {
            base: base.to_owned(),
            known: std::collections::HashMap::new(),
        })
    }

    /// Give `blocks` the paths their text writes as links followed with
    /// Ctrl (`Run::found`), each to the file as a `file://` address with
    /// the line or the mark the word names (`#L12`, `#setup`), where no run
    /// links already or is code. It asks the filesystem: on a worker.
    pub fn link(&mut self, blocks: &mut [Block]) {
        let here = Here::now();
        let mut lookups = LOOKUPS;
        if self.known.len() > KNOWN {
            self.known.clear();
        }
        for block in blocks {
            if skwir_core::scheduler::job_cancelled() {
                return;
            }
            let mut found: Vec<(Range<usize>, String)> = written(&block.text)
                .into_iter()
                .filter_map(|word| {
                    Some((word.range.clone(), self.target(&word, &here, &mut lookups)?))
                })
                .collect();
            // Words that overlap: the first, and the longest of those.
            let mut end = 0;
            found.retain(|(range, _)| {
                let apart = range.start >= end;
                if apart {
                    end = range.end;
                }
                apart
            });
            if found.is_empty() {
                continue;
            }
            let runs = std::mem::take(&mut block.runs);
            block.runs = linked(runs, &found, true);
        }
    }

    /// Where `word` leads, as a link's target: the file's address, and the
    /// line or the mark it names.
    fn target(&mut self, word: &PathWord, here: &Here, lookups: &mut usize) -> Option<String> {
        // A link in markup names a place in the file after `#`, and what
        // follows `?` is a page's, not the file's.
        let (path, mark) = match word.path.split_once('#') {
            Some((path, mark)) if !path.is_empty() => (path, Some(mark)),
            _ => (word.path.as_str(), None),
        };
        let path = path.split('?').next().unwrap_or(path);
        let file = match self.known.get(path) {
            Some(known) => known.clone(),
            None if *lookups == 0 => return None,
            None => {
                *lookups -= 1;
                // Markup escapes what a URL may not hold (`my%20file.md`).
                let found = resolve(path, &self.base, here).or_else(|| {
                    let decoded = crate::uri::percent_decode(path);
                    let decoded = std::str::from_utf8(&decoded).ok()?;
                    (decoded != path)
                        .then(|| resolve(decoded, &self.base, here))
                        .flatten()
                });
                self.known.insert(path.to_owned(), found.clone());
                found
            }
        }?;
        let mut target = crate::uri::file_uri(&file);
        match (word.line, mark) {
            (Some(line), _) => {
                target.push_str("#L");
                target.push_str(&line.to_string());
            }
            // A fragment, escaped as markup writes it.
            (None, Some(mark)) if !mark.is_empty() => {
                target.push('#');
                target.push_str(mark);
            }
            _ => {}
        }
        Some(target)
    }
}

/// Give `block` the addresses in its text as links, where no run links
/// already or is code: a link the text was written with stays as it was.
/// `found` says how they are followed (`Run::found`): with Ctrl in raw
/// text, where a click selects; on a click in a document read as a page.
pub fn link_block(block: &mut Block, found: bool) {
    // A preview moved on from links nothing more (its answer is not
    // taken): a document of many blocks stops here.
    if skwir_core::scheduler::job_cancelled() {
        return;
    }
    let addresses = urls(&block.text);
    if addresses.is_empty() {
        return;
    }
    let addresses: Vec<(Range<usize>, String)> = addresses
        .into_iter()
        .map(|range| (range.clone(), block.text[range].to_owned()))
        .collect();
    let runs = std::mem::take(&mut block.runs);
    block.runs = linked(runs, &addresses, found);
}

/// `runs` (in order, apart) with `addresses` (in order, apart: each a
/// range and where it leads) linked, those that cross a link or code left
/// out. A run is cut where an address starts or ends, and text no run
/// covered inside an address gets a run of its own.
fn linked(runs: Vec<Run>, addresses: &[(Range<usize>, String)], found: bool) -> Vec<Run> {
    let taken = |range: &Range<usize>| {
        runs.iter().any(|run| {
            run.range.start < range.end
                && range.start < run.range.end
                && (run.link.is_some() || run.code)
        })
    };
    let addresses: Vec<&(Range<usize>, String)> =
        addresses.iter().filter(|(a, _)| !taken(a)).collect();
    if addresses.is_empty() {
        return runs;
    }
    let mut edges: Vec<usize> = runs
        .iter()
        .flat_map(|run| [run.range.start, run.range.end])
        .chain(addresses.iter().flat_map(|(a, _)| [a.start, a.end]))
        .collect();
    edges.sort_unstable();
    edges.dedup();
    let mut out = Vec::with_capacity(runs.len() + 2 * addresses.len());
    let (mut run, mut address) = (0, 0);
    for edge in edges.windows(2) {
        let piece = edge[0]..edge[1];
        while runs.get(run).is_some_and(|r| r.range.end <= piece.start) {
            run += 1;
        }
        while addresses
            .get(address)
            .is_some_and(|(a, _)| a.end <= piece.start)
        {
            address += 1;
        }
        let covering = runs.get(run).filter(|r| r.range.start <= piece.start);
        let inside = addresses
            .get(address)
            .filter(|(a, _)| a.start <= piece.start)
            .copied();
        let mut piece_run = match (covering, inside) {
            (None, None) => continue,
            (Some(r), _) => r.clone(),
            (None, Some(_)) => Run::default(),
        };
        piece_run.range = piece;
        if let Some((_, target)) = inside {
            piece_run.link = Some(target.clone());
            piece_run.found = found;
        }
        out.push(piece_run);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<&str> {
        urls(text).into_iter().map(|r| &text[r]).collect()
    }

    /// Text made to be slow (a run of schemes without addresses, an
    /// address followed by a run of brackets) is read in time linear in
    /// its length: each colon looks at an address's length at most.
    #[test]
    fn crafted_runs_are_read_in_linear_time() {
        let schemes = "mailto:".repeat(20_000);
        let started = std::time::Instant::now();
        assert!(found(&schemes).is_empty());
        let brackets = format!("https://example.org/a{}", ")".repeat(20_000));
        assert_eq!(found(&brackets), ["https://example.org/a"]);
        let balanced = "see (https://en.wikipedia.org/wiki/Rust_(language)).";
        assert_eq!(
            found(balanced),
            ["https://en.wikipedia.org/wiki/Rust_(language)"]
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn addresses_are_found_by_their_scheme_only() {
        assert_eq!(
            found("see https://example.org/a?b=c#d and http://x.y."),
            ["https://example.org/a?b=c#d", "http://x.y"]
        );
        assert_eq!(
            found("mail mailto:someone@example.org, now"),
            ["mailto:someone@example.org"]
        );
        assert_eq!(found("file:///etc/hosts"), ["file:///etc/hosts"]);
        assert_eq!(found("HTTPS://EXAMPLE.ORG"), ["HTTPS://EXAMPLE.ORG"]);
        // Names with a dot are files, not domains.
        assert!(found("main.rs setup.py README.md www.example.org").is_empty());
        // Schemes skwir does not follow, and schemes inside a word.
        assert!(found("ssh://host ftp://host xhttps://a.b javascript:alert(1)").is_empty());
        // Nothing after the scheme.
        assert!(found("https:// http://. mailto: mailto:@x file://").is_empty());
    }

    #[test]
    fn a_sentence_keeps_its_punctuation() {
        assert_eq!(found("(see https://a.b/c)"), ["https://a.b/c"]);
        assert_eq!(
            found("(https://en.wikipedia.org/wiki/Rust_(programming_language))"),
            ["https://en.wikipedia.org/wiki/Rust_(programming_language)"]
        );
        assert_eq!(found("<https://a.b/c>"), ["https://a.b/c"]);
        assert_eq!(found("'https://a.b/c'"), ["https://a.b/c"]);
        assert_eq!(found("\"https://a.b/c\","), ["https://a.b/c"]);
        assert_eq!(found("[link](https://a.b/c)."), ["https://a.b/c"]);
        assert_eq!(found("«https://a.b/é»"), ["https://a.b/é"]);
        assert_eq!(found("https://a.b/c?x=[1]"), ["https://a.b/c?x=[1]"]);
        assert_eq!(found("https://[::1]:8080/x"), ["https://[::1]:8080/x"]);
    }

    /// Each word as drawn (path and position), its path, line and column.
    fn words(text: &str) -> Vec<(&str, &str, Option<u32>, Option<u32>)> {
        paths(text)
            .into_iter()
            .map(|w| {
                let shown = &text[w.range.clone()];
                assert!(shown.starts_with(&w.path), "{shown} starts with {}", w.path);
                (shown, &shown[..w.path.len()], w.line, w.column)
            })
            .collect()
    }

    #[test]
    fn a_compiler_message_points_at_a_line() {
        assert_eq!(
            words("  --> crates/ui/src/x.rs:12:5"),
            [(
                "crates/ui/src/x.rs:12:5",
                "crates/ui/src/x.rs",
                Some(12),
                Some(5)
            )]
        );
        // grep -n: the rest of the line is not the link.
        assert_eq!(
            words("src/a.rs:7:fn main() {"),
            [
                ("src/a.rs:7", "src/a.rs", Some(7), None),
                ("main", "main", None, None),
            ]
        );
        assert_eq!(
            words("error: src/b.rs:3:"),
            [
                ("error", "error", None, None),
                ("src/b.rs:3", "src/b.rs", Some(3), None),
            ]
        );
    }

    #[test]
    fn names_paths_and_quoted_names_are_words() {
        let all: Vec<_> = words("Cargo.toml  'my file.txt'  ~/notes  /etc/hosts  ./run.sh,")
            .into_iter()
            .map(|(shown, ..)| shown)
            .collect();
        assert_eq!(
            all,
            [
                "Cargo.toml",
                "my file.txt",
                "~/notes",
                "/etc/hosts",
                "./run.sh"
            ]
        );
        // Numbers, times, options and addresses are not.
        assert!(words("12 10:30 --hyperlink -x https://a.b mailto:a@b ~ 3.").is_empty());
    }

    #[test]
    fn a_relative_path_is_looked_for_above_where_it_was_printed() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("crates/ui/src/x.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "").unwrap();
        std::fs::write(root.path().join("top.txt"), "").unwrap();
        let deep = root.path().join("crates/ui");
        let here = Here::now();
        let at = |word: &str, base: &Path| resolve(word, base, &here);
        // cargo, run in a member, writes from the workspace's root.
        assert_eq!(at("crates/ui/src/x.rs", &deep), Some(file.clone()));
        assert_eq!(at("src/x.rs", &deep), Some(file.clone()));
        // A bare name and `./` are read where they were printed only.
        assert_eq!(at("top.txt", &deep), None);
        assert_eq!(
            at("top.txt", root.path()),
            Some(root.path().join("top.txt"))
        );
        assert_eq!(
            at("../../top.txt", &deep),
            Some(root.path().join("top.txt"))
        );
        assert_eq!(at("./x.rs", &deep), None);
        assert_eq!(at(file.to_str().unwrap(), Path::new("/")), Some(file));
        assert_eq!(at("x.rs", Path::new("relative")), None);
    }

    #[test]
    fn source_writes_paths_in_strings_link_targets_and_slashed_words() {
        let words = |text: &str| -> Vec<String> {
            written(text).into_iter().map(|word| word.path).collect()
        };
        assert_eq!(words(r#"<a href="page.html">the page</a>"#), ["page.html"]);
        assert_eq!(words(r#"#include "util.h""#), ["util.h"]);
        assert_eq!(
            words("body { background: url(img/bg.png) }"),
            ["img/bg.png"]
        );
        let markdown =
            words(r#"See [the guide](docs/guide.md#setup "Guide") and `notes/l'été.md`."#);
        for path in ["docs/guide.md#setup", "notes/l'été.md"] {
            assert!(markdown.iter().any(|p| p == path), "{path} in {markdown:?}");
        }
        let comment = written("// see crates/ui/src/x.rs:12 and </p> */");
        assert_eq!(comment.len(), 1, "{comment:?}");
        assert_eq!(
            (comment[0].path.as_str(), comment[0].line),
            ("crates/ui/src/x.rs", Some(12))
        );
        // Words, apostrophes in them and addresses are not paths.
        assert!(words("don't stop, it's fine: main.rs").is_empty());
        assert!(words("https://example.org/a/b").is_empty());
    }

    #[test]
    fn the_paths_a_file_writes_link_to_what_is_there() {
        let root = tempfile::tempdir().unwrap();
        let docs = root.path().join("docs");
        for file in [
            "docs/guide.md",
            "docs/l'été/notes.md",
            "docs/my file.md",
            "docs/page.html",
            "src/main.rs",
        ] {
            let file = root.path().join(file);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "").unwrap();
        }
        let mut blocks: Vec<Block> = [
            "[guide](guide.md#set%20up) and [notes](l%27%C3%A9t%C3%A9/notes.md)",
            r#"<a href="page.html">x</a> <img src="missing.png">"#,
            "see ../src/main.rs:12 or `my file.md`",
        ]
        .into_iter()
        .map(|text| Block {
            text: text.to_owned(),
            code: true,
            ..Block::default()
        })
        .collect();
        let mut paths = WrittenPaths::new(&docs.join("index.md")).unwrap();
        paths.link(&mut blocks);
        let links: Vec<(&str, &str)> = blocks
            .iter()
            .flat_map(|block| {
                block.runs.iter().filter_map(|run| {
                    assert!(run.found, "followed with Ctrl");
                    Some((&block.text[run.range.clone()], run.link.as_deref()?))
                })
            })
            .collect();
        let uri = |path: &str| crate::uri::file_uri(&root.path().join(path));
        assert_eq!(
            links,
            [
                (
                    "guide.md#set%20up",
                    format!("{}#set%20up", uri("docs/guide.md")).as_str()
                ),
                (
                    "l%27%C3%A9t%C3%A9/notes.md",
                    uri("docs/l'été/notes.md").as_str()
                ),
                ("page.html", uri("docs/page.html").as_str()),
                (
                    "../src/main.rs:12",
                    format!("{}#L12", uri("src/main.rs")).as_str()
                ),
                ("my file.md", uri("docs/my file.md").as_str()),
            ]
        );
    }

    /// A path is not looked up where the lookup would mount something,
    /// from another machine for `/net/host`: not as written, nor through
    /// `..`, nor above where it was printed.
    #[test]
    fn nothing_an_automount_point_would_mount_is_looked_up() {
        let root = tempfile::tempdir().unwrap();
        let net = root.path().join("net");
        let file = net.join("host/notes.md");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "").unwrap();
        let deep = root.path().join("a/b");
        std::fs::create_dir_all(&deep).unwrap();
        let open = Here::with_automounts([]);
        let automount = Here::with_automounts([net.clone()]);
        let words = [
            (file.to_str().unwrap().to_owned(), root.path()),
            ("net/host/notes.md".to_owned(), root.path()),
            ("net/host/notes.md".to_owned(), deep.as_path()),
            ("../../net/host/notes.md".to_owned(), deep.as_path()),
        ];
        for (word, base) in &words {
            assert_eq!(resolve(word, base, &open), Some(file.clone()), "{word}");
            assert_eq!(resolve(word, base, &automount), None, "{word}");
        }
        // The point itself is read without mounting anything.
        assert_eq!(resolve("net", root.path(), &automount), Some(net));
    }

    fn run(range: Range<usize>, color: u8) -> Run {
        Run {
            range,
            color: Some([color; 3]),
            ..Run::default()
        }
    }

    #[test]
    fn links_cut_runs_and_keep_their_colours() {
        let text = "let u = \"https://a.b/c\"; // ok";
        let start = text.find("https").unwrap();
        let end = start + "https://a.b/c".len();
        let mut block = Block {
            text: text.into(),
            // The string's colour starts at its quote; the comment's after.
            runs: vec![run(0..8, 1), run(8..25, 2), run(26..text.len(), 3)],
            ..Block::default()
        };
        link_block(&mut block, true);
        let linked: Vec<_> = block.runs.iter().filter(|r| r.link.is_some()).collect();
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].range, start..end);
        assert_eq!(linked[0].color, Some([2; 3]));
        assert!(linked[0].found);
        assert_eq!(linked[0].link.as_deref(), Some("https://a.b/c"));
        // Every byte a run covered is still covered, once, in order.
        let covered: Vec<_> = block.runs.iter().map(|r| r.range.clone()).collect();
        assert_eq!(
            covered,
            [0..8, 8..start, start..end, end..25, 26..text.len()]
        );
    }

    #[test]
    fn text_no_run_covers_gets_a_link_run_and_written_links_stay() {
        let text = "write to mailto:a@b.c or https://x.y";
        let mut block = Block {
            text: text.into(),
            runs: vec![Run {
                range: 25..text.len(),
                link: Some("https://elsewhere.example".into()),
                ..Run::default()
            }],
            ..Block::default()
        };
        link_block(&mut block, false);
        assert_eq!(block.runs.len(), 2);
        assert_eq!(block.runs[0].range, 9..21);
        assert_eq!(block.runs[0].link.as_deref(), Some("mailto:a@b.c"));
        assert!(!block.runs[0].found);
        assert_eq!(
            block.runs[1].link.as_deref(),
            Some("https://elsewhere.example")
        );
    }
}
