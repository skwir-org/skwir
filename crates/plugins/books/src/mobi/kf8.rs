//! A KF8 book's text (AZW3, or the KF8 part of a MOBI file): flows (the
//! FDST table), the first the book's XHTML files cut in pieces, each file a
//! skeleton (SKEL index) with its fragments (FRAG index) put back where
//! they go; the others its stylesheets. A link names a fragment and an
//! offset in it (`kindle:pos:fid:`), a resource its number
//! (`kindle:embed:`, `kindle:flow:`). Each place a link or the contents
//! name becomes an anchor, as in a MOBI 6 book.
use std::collections::BTreeMap;

use memchr::memmem;
use skwir_core::{CancelToken, inspection::Row};
use skwir_platform::inspection::{self, limits};

use super::{index::Index, part_href};

/// A KF8 book's files, its flows (stylesheets), and its contents.
pub(crate) struct Built {
    pub parts: Vec<String>,
    pub flows: Vec<String>,
    pub contents: Vec<Row>,
}

/// A fragment: where it goes in the text as put together.
struct Fragment {
    insert: usize,
}

/// A number in Kindle's base 32 (`0`–`9`, `A`–`V`).
pub(crate) fn base32(text: &str) -> Option<u32> {
    text.chars().try_fold(0u32, |value, c| {
        let digit = c.to_digit(32)?;
        value.checked_mul(32)?.checked_add(digit)
    })
}

/// A link to a place: `kindle:pos:fid:XXXX:off:YYYYYYYYYY`.
const LINK: usize = 34;

/// The links to places in `file`: where each starts, its fragment and its
/// offset.
fn positions(file: &[u8]) -> impl Iterator<Item = (usize, u32, u32)> + '_ {
    memmem::find_iter(file, b"kindle:pos:fid:").filter_map(|at| {
        let link = std::str::from_utf8(file.get(at..at + LINK)?).ok()?;
        let fid = link.get(15..19).and_then(base32)?;
        let offset = link.get(24..34).and_then(base32)?;
        (link.get(19..24) == Some(":off:")).then_some((at, fid, offset))
    })
}

/// The anchor put at a place.
fn anchor(fid: u32, offset: u32) -> String {
    format!("pos{fid}-{offset}")
}

/// The flows of `raw` the FDST record `fdst` cuts it in, the whole text
/// one flow without one.
fn flows<'a>(raw: &'a [u8], fdst: Option<&[u8]>) -> Vec<&'a [u8]> {
    let starts = fdst
        .filter(|record| record.get(..4) == Some(b"FDST"))
        .and_then(|record| {
            let count = u32::from_be_bytes(record.get(8..12)?.try_into().ok()?) as usize;
            (0..count.min(limits::BLOCKS))
                .map(|k| {
                    let at = 12 + 8 * k;
                    Some(u32::from_be_bytes(record.get(at..at + 4)?.try_into().ok()?) as usize)
                })
                .collect::<Option<Vec<usize>>>()
        })
        .filter(|starts| starts.first() == Some(&0) && starts.is_sorted())
        .unwrap_or_else(|| vec![0]);
    let mut ends: Vec<usize> = starts.iter().skip(1).copied().collect();
    ends.push(raw.len());
    starts
        .iter()
        .zip(ends)
        .map(|(&start, end)| {
            raw.get(start.min(raw.len())..end.min(raw.len()))
                .unwrap_or_default()
        })
        .collect()
}

/// The book's files put together from `raw` (its text), the FDST record,
/// the skeleton and fragment indexes, and the contents (`ncx`).
pub(crate) fn build(
    raw: &[u8],
    fdst: Option<&[u8]>,
    skeletons: &Index,
    fragments: &Index,
    ncx: Option<&Index>,
    cancel: &CancelToken,
) -> anyhow::Result<Built> {
    let damaged = || anyhow::anyhow!("This book's KF8 text is damaged");
    let flows = flows(raw, fdst);
    let text = flows[0];
    let frags: Vec<Fragment> = fragments
        .entries
        .iter()
        .map(|entry| {
            let insert = std::str::from_utf8(&entry.name).ok()?.trim().parse().ok()?;
            Some(Fragment { insert })
        })
        .collect::<Option<_>>()
        .ok_or_else(damaged)?;
    // Each file: its skeleton with its fragments put in, and where it is
    // in the text (start, end), which positions are counted in.
    let mut parts: Vec<(Vec<u8>, usize, usize)> = Vec::new();
    let mut next = 0;
    for skeleton in &skeletons.entries {
        inspection::check(cancel)?;
        // A skeleton's numbers may come twice: the first are its own.
        let count = skeleton.first(1).ok_or_else(damaged)? as usize;
        let &[start, length, ..] = skeleton.all(6) else {
            anyhow::bail!(damaged());
        };
        let (start, length) = (start as usize, length as usize);
        let skeleton_bytes = text.get(start..start + length).ok_or_else(damaged)?;
        // A fragment goes at its insert point in the file as it grows. The
        // file is the bytes put together so far and the skeleton's rest:
        // while each fragment goes at or after the end of what is put
        // together, as books write them, one ordered pass appends the
        // skeleton up to it, then the fragment. One that goes back puts the
        // skeleton's rest in, and each from then on is inserted where it
        // goes, as calibre's reader does, cancellation checked each time.
        let mut file = Vec::with_capacity(length);
        let mut base = start + length;
        // Where the skeleton's bytes not yet in `file` start; `None` once
        // all of it is.
        let mut rest = Some(0);
        for entry in fragments
            .entries
            .get(next..next + count)
            .ok_or_else(damaged)?
        {
            let &[_, length, ..] = entry.all(6) else {
                anyhow::bail!(damaged());
            };
            let length = length as usize;
            let piece = text.get(base..base + length).ok_or_else(damaged)?;
            let at = frags[next].insert.checked_sub(start).ok_or_else(damaged)?;
            match rest {
                Some(from) if at >= file.len() => {
                    let to = from + (at - file.len());
                    let skipped = skeleton_bytes.get(from..to).ok_or_else(damaged)?;
                    file.extend_from_slice(skipped);
                    file.extend_from_slice(piece);
                    rest = Some(to);
                }
                _ => {
                    if let Some(from) = rest.take() {
                        file.extend_from_slice(&skeleton_bytes[from..]);
                    }
                    inspection::check(cancel)?;
                    anyhow::ensure!(at <= file.len(), damaged());
                    file.splice(at..at, piece.iter().copied());
                }
            }
            base += length;
            next += 1;
        }
        if let Some(from) = rest {
            file.extend_from_slice(&skeleton_bytes[from..]);
        }
        parts.push((file, start, base));
        anyhow::ensure!(parts.len() <= limits::BLOCKS, damaged());
    }
    anyhow::ensure!(!parts.is_empty(), damaged());
    // A place a link or the contents name: a fragment and an offset in it.
    let place = |fid: u32, offset: u32| -> Option<(usize, usize)> {
        let position = frags
            .get(fid as usize)?
            .insert
            .checked_add(offset as usize)?;
        let part = parts
            .partition_point(|(_, start, _)| *start <= position)
            .checked_sub(1)?;
        let (_, start, end) = &parts[part];
        (position < *end).then(|| (part, position - start))
    };
    let mut anchors: Vec<BTreeMap<usize, String>> = vec![BTreeMap::new(); parts.len()];
    for (file, _, _) in &parts {
        inspection::check(cancel)?;
        for (_, fid, offset) in positions(file) {
            if let Some((part, local)) = place(fid, offset) {
                anchors[part].insert(local, anchor(fid, offset));
            }
        }
    }
    let mut contents = Vec::new();
    for (entry, depth) in ncx.map(Index::in_order).unwrap_or_default() {
        let label = entry
            .first(3)
            .and_then(|offset| ncx?.label(offset))
            .unwrap_or_default()
            .trim()
            .to_owned();
        let target = match entry.all(6) {
            &[fid, offset, ..] => place(fid, offset).map(|(part, local)| {
                anchors[part].insert(local, anchor(fid, offset));
                format!("{}#{}", part_href(part), anchor(fid, offset))
            }),
            _ => None,
        };
        if !label.is_empty() || target.is_some() {
            contents.push(Row {
                cells: vec![label],
                target,
                depth,
                ..Row::default()
            });
        }
    }
    contents.truncate(limits::BLOCKS);
    // Each link in one pass over its file: to its anchor, in the file
    // holding it.
    let parts = parts
        .iter()
        .zip(&anchors)
        .enumerate()
        .map(|(index, ((file, _, _), anchors))| {
            let file = with_anchors(file, anchors);
            let mut out = String::with_capacity(file.len());
            let mut at = 0;
            for (start, fid, offset) in positions(file.as_bytes()) {
                out.push_str(&file[at..start]);
                match place(fid, offset) {
                    Some((part, _)) if part == index => {
                        out.push('#');
                        out.push_str(&anchor(fid, offset));
                    }
                    Some((part, _)) => {
                        out.push_str(&part_href(part));
                        out.push('#');
                        out.push_str(&anchor(fid, offset));
                    }
                    None => out.push_str(&file[start..start + LINK]),
                }
                at = start + LINK;
            }
            out.push_str(&file[at..]);
            out
        })
        .collect();
    let flows = flows
        .iter()
        .map(|flow| String::from_utf8_lossy(flow).into_owned())
        .collect();
    Ok(Built {
        parts,
        flows,
        contents,
    })
}

/// `file` with an empty anchor of each id in `anchors` at its place:
/// before the tag it would cut, never before the body.
fn with_anchors(file: &[u8], anchors: &BTreeMap<usize, String>) -> String {
    let lower = file.to_ascii_lowercase();
    let body = memmem::find(&lower, b"<body")
        .and_then(|at| memchr::memchr(b'>', &file[at..]).map(|end| at + end + 1))
        .unwrap_or(0);
    let mut placed: BTreeMap<usize, Vec<&str>> = BTreeMap::new();
    for (&at, id) in anchors {
        let at = at.min(file.len());
        let at = match memchr::memrchr2(b'<', b'>', &file[..at]) {
            Some(open) if file[open] == b'<' => open,
            _ => at,
        };
        placed.entry(at.max(body)).or_default().push(id);
    }
    let mut out = Vec::with_capacity(file.len() + 32 * placed.len());
    let mut at = 0;
    for (place, ids) in placed {
        out.extend_from_slice(&file[at..place]);
        for id in ids {
            out.extend_from_slice(format!(r#"<a id="{id}"></a>"#).as_bytes());
        }
        at = place;
    }
    out.extend_from_slice(&file[at..]);
    String::from_utf8_lossy(&out).into_owned()
}
