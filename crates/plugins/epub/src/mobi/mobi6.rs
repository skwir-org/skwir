//! A MOBI 6 book's text: one HTML stream, its chapters split at its page
//! breaks (`<mbp:pagebreak/>`), its links and its contents' entries naming
//! a byte of it (`filepos`), its pictures a record (`recindex`). Each named
//! byte becomes an anchor, put before the tag it would cut, and each link
//! leads to the chapter holding it. A `PalmDOC` text is paragraphs.
use std::collections::{BTreeMap, HashMap};

use memchr::memmem;
use skwir_platform::inspection::{self, limits};

use super::{chapter_href, escape};

/// A MOBI 6 book's chapters, and the chapter holding each byte named.
pub(crate) struct Built {
    pub chapters: Vec<String>,
    pub places: HashMap<u32, usize>,
}

/// The chapters of `raw`, a MOBI 6 book's text, with an anchor at each
/// byte its links and `named` (its contents' entries) name.
pub(crate) fn chapters(raw: &[u8], named: &[u32], utf8: bool) -> Built {
    let lower = raw.to_ascii_lowercase();
    let mut targets: Vec<u32> = memmem::find_iter(&lower, b"filepos=")
        .filter_map(|at| number(raw, at + 8).map(|(value, _)| value))
        .chain(named.iter().copied())
        .filter(|&target| target as usize <= raw.len())
        .collect();
    targets.sort_unstable();
    targets.dedup();
    let mut breaks: Vec<usize> = memmem::find_iter(&lower, b"<mbp:pagebreak").collect();
    breaks.truncate(limits::BLOCKS);
    // Where each anchor goes: at the byte named, or before the tag it is in.
    let mut anchors: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
    for &target in &targets {
        anchors
            .entry(outside_tag(raw, target as usize))
            .or_default()
            .push(target);
    }
    let mut pieces: Vec<Vec<u8>> = vec![Vec::new()];
    let mut raw_chapter: HashMap<u32, usize> = HashMap::new();
    let mut events: Vec<usize> = anchors
        .keys()
        .copied()
        .chain(breaks.iter().copied())
        .collect();
    events.sort_unstable();
    events.dedup();
    let mut at = 0;
    for event in events {
        pieces
            .last_mut()
            .expect("a chapter")
            .extend_from_slice(&raw[at..event]);
        at = event;
        if breaks.binary_search(&event).is_ok() && !pieces.last().expect("a chapter").is_empty() {
            pieces.push(Vec::new());
        }
        if let Some(named) = anchors.get(&event) {
            let piece = pieces.last_mut().expect("a chapter");
            for target in named {
                piece.extend_from_slice(format!(r#"<a id="filepos{target}"></a>"#).as_bytes());
            }
            let index = pieces.len() - 1;
            for target in named {
                raw_chapter.insert(*target, index);
            }
        }
    }
    pieces
        .last_mut()
        .expect("a chapter")
        .extend_from_slice(&raw[at..]);
    let texts: Vec<String> = pieces
        .into_iter()
        .map(|piece| {
            if utf8 {
                String::from_utf8_lossy(&piece).into_owned()
            } else {
                inspection::Encoding::Windows1252
                    .decode(&piece)
                    .into_owned()
            }
        })
        .collect();
    // A piece with nothing to read (what stands before the first page
    // break, the head) opens the next one.
    let mut chapters: Vec<String> = Vec::new();
    let mut merged = Vec::with_capacity(texts.len());
    let mut pending = String::new();
    for text in texts {
        let read = readable(&text);
        pending.push_str(&text);
        merged.push(chapters.len());
        if read {
            chapters.push(std::mem::take(&mut pending));
        }
    }
    if !pending.is_empty() || chapters.is_empty() {
        match chapters.last_mut() {
            Some(last) if !pending.is_empty() => last.push_str(&pending),
            _ => chapters.push(pending),
        }
    }
    let last = chapters.len() - 1;
    let places: HashMap<u32, usize> = raw_chapter
        .into_iter()
        .map(|(target, piece)| (target, merged.get(piece).copied().unwrap_or(last).min(last)))
        .collect();
    let chapters = chapters
        .iter()
        .enumerate()
        .map(|(index, text)| links(text, index, &places))
        .collect();
    Built { chapters, places }
}

/// The number written at `at`, quoted or not, and where it ends.
fn number(text: &[u8], at: usize) -> Option<(u32, usize)> {
    let mut at = at;
    let quote = text.get(at).copied().filter(|b| matches!(b, b'"' | b'\''));
    if quote.is_some() {
        at += 1;
    }
    let digits = text
        .get(at..)?
        .iter()
        .take(10)
        .take_while(|b| b.is_ascii_digit())
        .count();
    let value = std::str::from_utf8(&text[at..at + digits])
        .ok()?
        .parse()
        .ok()?;
    let mut end = at + digits;
    if quote.is_some() && text.get(end) == quote.as_ref() {
        end += 1;
    }
    Some((value, end))
}

/// `at`, or the start of the tag it falls in.
fn outside_tag(raw: &[u8], at: usize) -> usize {
    match memchr::memrchr2(b'<', b'>', &raw[..at.min(raw.len())]) {
        Some(open) if raw[open] == b'<' => open,
        _ => at.min(raw.len()),
    }
}

/// Whether `html` has anything to read: text outside its tags, or a
/// picture.
fn readable(html: &str) -> bool {
    let mut inside = false;
    for c in html.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            c if !inside && !c.is_whitespace() => return true,
            _ => {}
        }
    }
    memmem::find_iter(html.as_bytes(), b"<").any(|at| {
        html.as_bytes()
            .get(at + 1..at + 4)
            .is_some_and(|tag| tag.eq_ignore_ascii_case(b"img"))
    })
}

/// Chapter `index`'s links and pictures as reading mode reads them: a
/// `filepos` a link to the anchor of the byte it names, in the chapter
/// holding it; a `recindex` the picture's `src`.
fn links(text: &str, index: usize, places: &HashMap<u32, usize>) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    let found = memmem::find_iter(lower.as_bytes(), b"filepos=")
        .map(|at| (at, true))
        .chain(memmem::find_iter(lower.as_bytes(), b"recindex=").map(|at| (at, false)));
    let mut found: Vec<(usize, bool)> = found.collect();
    found.sort_unstable();
    for (start, link) in found {
        if start < at {
            continue;
        }
        out.push_str(&text[at..start]);
        if link {
            let Some((target, end)) = number(text.as_bytes(), start + 8) else {
                out.push_str("filepos=");
                at = start + 8;
                continue;
            };
            let href = match places.get(&target) {
                Some(&chapter) if chapter == index => format!("#filepos{target}"),
                Some(&chapter) => format!("{}#filepos{target}", chapter_href(chapter)),
                None => String::new(),
            };
            out.push_str(r#"href=""#);
            out.push_str(&href);
            out.push('"');
            at = end;
        } else {
            out.push_str("src=");
            at = start + 9;
        }
    }
    out.push_str(&text[at..]);
    out
}

/// A `PalmDOC` text: its paragraphs (lines apart by an empty one, else each
/// line) in chapters of about `CHAPTER` bytes.
pub(crate) fn text_chapters(raw: &[u8]) -> Vec<String> {
    const CHAPTER: usize = 64 * 1024;
    let (text, _) = inspection::decode(raw);
    let text = text.replace("\r\n", "\n");
    let paragraphs: Vec<&str> = if text.contains("\n\n") {
        text.split("\n\n").collect()
    } else {
        text.split('\n').collect()
    };
    let mut chapters = vec![String::new()];
    for paragraph in paragraphs.iter().filter(|p| !p.trim().is_empty()) {
        let chapter = chapters.last_mut().expect("a chapter");
        chapter.push_str("<p>");
        for (k, line) in paragraph.trim_matches('\n').split('\n').enumerate() {
            if k > 0 {
                chapter.push_str("<br>");
            }
            escape(chapter, line);
        }
        chapter.push_str("</p>");
        if chapter.len() >= CHAPTER && chapters.len() < limits::BLOCKS {
            chapters.push(String::new());
        }
    }
    if chapters.len() > 1 && chapters.last().is_some_and(String::is_empty) {
        chapters.pop();
    }
    chapters
}
