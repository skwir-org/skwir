//! `KiCad`'s files since version 6 (boards since 4): S-expressions, a
//! list per item, `(footprint "R_0402" (at 10 20 90) (pad "1" …))`. A
//! board is read one top-level item at a time (`Reader`), so what is held
//! at once is the largest item (a filled zone), never the whole tree.
//! Points, the bulk of a zone, are kept as numbers (`Node::Xy`).
use std::borrow::Cow;

/// How deep lists may nest; `KiCad`'s go about ten deep.
const DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub enum Node<'a> {
    /// A word or number, as written.
    Atom(&'a str),
    /// A quoted string, its escapes still in it (`text`).
    Quoted(&'a str),
    /// `(xy x y)`, the points of lines and polygons.
    Xy(f64, f64),
    List(Box<[Node<'a>]>),
}

impl<'a> Node<'a> {
    /// A list's items, its keyword first; nothing for an atom.
    pub fn items(&self) -> &[Node<'a>] {
        match self {
            Node::List(items) => items,
            _ => &[],
        }
    }

    /// A list's keyword: `footprint` for `(footprint …)`.
    pub fn head(&self) -> Option<&'a str> {
        match self.items().first() {
            Some(Node::Atom(word)) => Some(word),
            _ => None,
        }
    }

    /// The word or string itself.
    pub fn word(&self) -> Option<&'a str> {
        match self {
            Node::Atom(word) | Node::Quoted(word) => Some(word),
            _ => None,
        }
    }

    /// The first list among the items with keyword `key`.
    pub fn child(&self, key: &str) -> Option<&Node<'a>> {
        self.items().iter().find(|item| item.head() == Some(key))
    }

    /// Every list among the items with keyword `key`.
    pub fn children<'s>(&'s self, key: &'s str) -> impl Iterator<Item = &'s Node<'a>> + 's {
        self.items()
            .iter()
            .filter(move |item| item.head() == Some(key))
    }

    /// The item at `index` (the keyword is 0) as a word or string.
    pub fn atom(&self, index: usize) -> Option<&'a str> {
        self.items().get(index)?.word()
    }

    /// The item at `index` as a number.
    pub fn number(&self, index: usize) -> Option<f64> {
        self.atom(index)?
            .parse()
            .ok()
            .filter(|n: &f64| n.is_finite())
    }

    /// The item at `index` as text, unescaped.
    pub fn text(&self, index: usize) -> Option<Cow<'a, str>> {
        Some(unescape(self.atom(index)?))
    }

    /// The number after keyword `key`: `(width 0.25)`.
    pub fn value(&self, key: &str) -> Option<f64> {
        self.child(key)?.number(1)
    }

    /// The text after keyword `key`: `(layer "F.Cu")`.
    pub fn string(&self, key: &str) -> Option<Cow<'a, str>> {
        self.child(key)?.text(1)
    }

    /// The point after keyword `key`: `(start 1 2)`.
    pub fn point(&self, key: &str) -> Option<(f64, f64)> {
        let list = self.child(key)?;
        Some((list.number(1)?, list.number(2)?))
    }

    /// Whether a bare word is among the items: `hide`, `locked`.
    pub fn flag(&self, word: &str) -> bool {
        self.items()
            .iter()
            .any(|item| matches!(item, Node::Atom(w) if *w == word))
    }

    /// A yes-or-no setting, as `KiCad` 8 writes it (`(hide yes)`) or as
    /// earlier versions did (a bare `hide`).
    pub fn switch(&self, key: &str) -> bool {
        self.flag(key)
            || self
                .child(key)
                .is_some_and(|list| list.atom(1) != Some("no"))
    }

    /// The points of a `(pts (xy …) …)` list among the items.
    pub fn points(&self) -> Vec<(f64, f64)> {
        self.child("pts")
            .map(|pts| {
                pts.items()
                    .iter()
                    .filter_map(|item| match item {
                        Node::Xy(x, y) => Some((*x, *y)),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// A quoted string's text: `KiCad` escapes quotes, backslashes and line
/// breaks.
pub fn unescape(raw: &str) -> Cow<'_, str> {
    if !raw.contains('\\') {
        return Cow::Borrowed(raw);
    }
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    Cow::Owned(out)
}

/// The items of one list at a time, from text.
pub struct Reader<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Reader<'a> {
    /// Enter the file's list and say its keyword: `kicad_pcb`.
    pub fn open(text: &'a str) -> anyhow::Result<(Reader<'a>, &'a str)> {
        let mut reader = Reader { text, at: 0 };
        anyhow::ensure!(reader.token() == Some(Token::Open), "Not a KiCad file");
        match reader.token() {
            Some(Token::Atom(head)) => Ok((reader, head)),
            _ => anyhow::bail!("Not a KiCad file"),
        }
    }

    /// The next item of the file's list; `None` at its end.
    pub fn next_item(&mut self) -> anyhow::Result<Option<Node<'a>>> {
        let mut stack: Vec<Vec<Node<'a>>> = Vec::new();
        loop {
            let Some(token) = self.token() else {
                anyhow::ensure!(stack.is_empty(), "The file ends inside a list");
                return Ok(None);
            };
            let node = match token {
                Token::Open => {
                    anyhow::ensure!(stack.len() < DEPTH, "Lists nest too deep");
                    stack.push(Vec::new());
                    continue;
                }
                Token::Close => {
                    let Some(items) = stack.pop() else {
                        return Ok(None);
                    };
                    list(items)
                }
                Token::Atom(word) => Node::Atom(word),
                Token::Quoted(text) => Node::Quoted(text),
            };
            match stack.last_mut() {
                Some(items) => items.push(node),
                None => return Ok(Some(node)),
            }
        }
    }

    fn token(&mut self) -> Option<Token<'a>> {
        let bytes = self.text.as_bytes();
        while self.at < bytes.len() && bytes[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
        let start = self.at;
        match *bytes.get(start)? {
            b'(' => {
                self.at += 1;
                Some(Token::Open)
            }
            b')' => {
                self.at += 1;
                Some(Token::Close)
            }
            b'"' => {
                let mut end = start + 1;
                while end < bytes.len() && bytes[end] != b'"' {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                let end = end.min(bytes.len());
                self.at = (end + 1).min(bytes.len());
                Some(Token::Quoted(self.text.get(start + 1..end)?))
            }
            _ => {
                let mut end = start;
                while end < bytes.len()
                    && !bytes[end].is_ascii_whitespace()
                    && !matches!(bytes[end], b'(' | b')' | b'"')
                {
                    end += 1;
                }
                self.at = end;
                Some(Token::Atom(self.text.get(start..end)?))
            }
        }
    }
}

/// A list, or a point when it is one.
fn list(items: Vec<Node<'_>>) -> Node<'_> {
    if let [Node::Atom("xy"), Node::Atom(x), Node::Atom(y)] = items.as_slice()
        && let (Ok(x), Ok(y)) = (x.parse::<f64>(), y.parse::<f64>())
        && x.is_finite()
        && y.is_finite()
    {
        return Node::Xy(x, y);
    }
    Node::List(items.into_boxed_slice())
}

#[derive(Debug, PartialEq)]
enum Token<'a> {
    Open,
    Close,
    Atom(&'a str),
    Quoted(&'a str),
}

/// A whole small file as one list (a footprint, a symbol library).
pub fn parse(text: &str) -> anyhow::Result<(String, Vec<Node<'_>>)> {
    let (mut reader, head) = Reader::open(text)?;
    let mut items = Vec::new();
    while let Some(item) = reader.next_item()? {
        items.push(item);
    }
    Ok((head.to_owned(), items))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_one_at_a_time() {
        let text = r#"(kicad_pcb (version 20241229)
            (gr_text "a \"quoted\"\nline" (at 1 2.5 90) (effects hide))
            (zone (pts (xy 0 0) (xy 1 0) (xy 1 1))))"#;
        let (mut reader, head) = Reader::open(text).unwrap();
        assert_eq!(head, "kicad_pcb");
        let version = reader.next_item().unwrap().unwrap();
        assert_eq!(version.number(1), Some(20_241_229.));
        let text_item = reader.next_item().unwrap().unwrap();
        assert_eq!(text_item.text(1).unwrap(), "a \"quoted\"\nline");
        assert_eq!(text_item.point("at"), Some((1., 2.5)));
        assert!(text_item.child("effects").unwrap().switch("hide"));
        let zone = reader.next_item().unwrap().unwrap();
        assert_eq!(zone.points(), [(0., 0.), (1., 0.), (1., 1.)]);
        assert_eq!(reader.next_item().unwrap(), None);
    }

    #[test]
    fn damage_is_an_error_not_a_hang() {
        let (mut reader, _) = Reader::open("(kicad_sch (a (b").unwrap();
        assert!(reader.next_item().is_err());
        assert!(Reader::open("hello").is_err());
        let deep = format!("(x {}", "(".repeat(100));
        let (mut reader, _) = Reader::open(&deep).unwrap();
        assert!(reader.next_item().is_err());
        // An unterminated string ends with the text.
        let (mut reader, _) = Reader::open("(x \"abc").unwrap();
        assert_eq!(reader.next_item().unwrap(), Some(Node::Quoted("abc")));
        assert_eq!(reader.next_item().unwrap(), None);
    }

    #[test]
    fn switches_old_and_new() {
        let (_, items) = parse("(f (a hide) (b (hide yes)) (c (hide no)))").unwrap();
        let [a, b, c] = items.as_slice() else {
            panic!()
        };
        assert!(a.switch("hide"));
        assert!(b.switch("hide"));
        assert!(!c.switch("hide"));
    }
}
