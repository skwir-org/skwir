//! Links found in plain text: what a reader would click if the text were a
//! page (ADR-029).
//!
//! An address is found by its scheme, and only by the schemes bibli
//! follows: `http`, `https` and `mailto` go to the desktop's browser and mail
//! client, `file` into bibli. A name with a dot is never taken for a domain:
//! in a file manager `main.rs`, `setup.py` and `README.md` are files, though
//! `.rs`, `.py` and `.md` are countries' domains. In a terminal the words of
//! a line may also be paths (`paths`), which only the filesystem confirms
//! (`resolve`, on a worker).

use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use bibli_core::inspection::{Block, Run};

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
    let mut address = address;
    while let Some(last) = address.chars().next_back() {
        let drop = match last {
            '.' | ',' | ':' | ';' | '!' | '?' | '\'' | '*' => true,
            ')' => address.matches('(').count() < address.matches(')').count(),
            ']' => address.matches('[').count() < address.matches(']').count(),
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

/// The addresses in `text`, as byte ranges, in order.
pub fn urls(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(colon) = text[from..].find(':').map(|i| from + i) {
        from = colon + 1;
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
        let end = text[body..]
            .find(ends_address)
            .map_or(text.len(), |i| body + i);
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
/// name, `./x` and `../x` are looked for in `base` only. It asks the
/// filesystem: call it on a worker.
pub fn resolve(word: &str, base: &Path) -> Option<PathBuf> {
    let there = |path: PathBuf| path.exists().then(|| normal(&path));
    let path = Path::new(word);
    if path.is_absolute() {
        return there(path.to_owned());
    }
    if let Some(rest) = word.strip_prefix("~/") {
        return there(bibli_core::os_path::home()?.join(rest));
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

/// Give `block` the addresses in its text as links/// Give `block` the addresses in its text as links, where no run links
/// already or is code: a link the text was written with stays as it was.
/// `found` says how they are followed (`Run::found`): with Ctrl in raw
/// text, where a click selects; on a click in a document read as a page.
pub fn link_block(block: &mut Block, found: bool) {
    let addresses = urls(&block.text);
    if addresses.is_empty() {
        return;
    }
    let runs = std::mem::take(&mut block.runs);
    block.runs = linked(&block.text, runs, &addresses, found);
}

/// `runs` (in order, apart) with `addresses` linked, those that cross a
/// link or code left out. A run is cut where an address starts or ends,
/// and text no run covered inside an address gets a run of its own.
fn linked(text: &str, runs: Vec<Run>, addresses: &[Range<usize>], found: bool) -> Vec<Run> {
    let taken = |range: &Range<usize>| {
        runs.iter().any(|run| {
            run.range.start < range.end
                && range.start < run.range.end
                && (run.link.is_some() || run.code)
        })
    };
    let addresses: Vec<&Range<usize>> = addresses.iter().filter(|a| !taken(a)).collect();
    if addresses.is_empty() {
        return runs;
    }
    let mut edges: Vec<usize> = runs
        .iter()
        .flat_map(|run| [run.range.start, run.range.end])
        .chain(addresses.iter().flat_map(|a| [a.start, a.end]))
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
        while addresses.get(address).is_some_and(|a| a.end <= piece.start) {
            address += 1;
        }
        let covering = runs.get(run).filter(|r| r.range.start <= piece.start);
        let inside = addresses
            .get(address)
            .filter(|a| a.start <= piece.start)
            .copied();
        let mut piece_run = match (covering, inside) {
            (None, None) => continue,
            (Some(r), _) => r.clone(),
            (None, Some(_)) => Run::default(),
        };
        piece_run.range = piece;
        if let Some(a) = inside {
            piece_run.link = Some(text[a.clone()].to_owned());
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
        // Schemes bibli does not follow, and schemes inside a word.
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
        // cargo, run in a member, writes from the workspace's root.
        assert_eq!(resolve("crates/ui/src/x.rs", &deep), Some(file.clone()));
        assert_eq!(resolve("src/x.rs", &deep), Some(file.clone()));
        // A bare name and `./` are read where they were printed only.
        assert_eq!(resolve("top.txt", &deep), None);
        assert_eq!(
            resolve("top.txt", root.path()),
            Some(root.path().join("top.txt"))
        );
        assert_eq!(
            resolve("../../top.txt", &deep),
            Some(root.path().join("top.txt"))
        );
        assert_eq!(resolve("./x.rs", &deep), None);
        assert_eq!(resolve(file.to_str().unwrap(), Path::new("/")), Some(file));
        assert_eq!(resolve("x.rs", Path::new("relative")), None);
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
