//! A trigram index for `content:`, built and timed against the scan it
//! would stand in for: the question of ADR-003 (Tantivy or trigrams).
//! Nothing in skwir uses it.
//!
//! It holds what the scan searches (`skwir_plugin_query::lang::
//! content_matches`): every file a search under the root walks to (hidden
//! entries left out, links to folders not followed, a link to a file read
//! as that file), at most `CONTENT_LIMIT` long, read as the searcher reads
//! it: up to its first NUL, a file starting with a UTF-16 byte order mark
//! decoded first. Each file's trigrams are its bytes', ASCII letters
//! folded. A pattern is planned into a query on trigrams that every match
//! satisfies, from the HIR grep-regex builds of it, Unicode case classes
//! included, so the index names every file that can match; each is then
//! matched by `content_matches` itself. `skwir-bench trigram` checks query
//! by query that the answer is the scan's.
//!
//! Folders the user keeps private (`permissions::Privacy`) are left out
//! of the index, and of the scan's answer when the two are compared. What
//! is printed is counts and times, never a path.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, bail};
use rayon::prelude::*;
use regex_syntax::hir::{Class, Hir, HirKind};
use serde_json::json;
use skwir_core::location::ListingEvent;
use skwir_plugin_query::lang::{self, CONTENT_LIMIT, ContentPat, Expr, Term};

/// The queries measured when none is given: common and rare words, case
/// either way, a word with accents, one found nowhere, one too short for
/// a trigram, regexes that leave the index words to look up and one that
/// leaves it none.
pub const QUERIES: &[&str] = &[
    "content:the",
    "content:error",
    "content:\"fn main\"",
    "content:copyright",
    "case:sensitive content:SELECT",
    "content:\"licensed under the apache license\"",
    "content:été",
    "content:xyzzyplugh",
    "content:ab",
    r"content:/fn\s+\w+_dir/",
    r"content:/impl\s+Drop\s+for/",
    r"content:/[0-9]{4}-[0-9]{2}-[0-9]{2}/",
];

/// The query timed cold, its files evicted from the page cache first.
pub const COLD_QUERY: &str = "content:copyright";

/// How long a scan may take before the bench gives up on it.
const SCAN_DEADLINE: Duration = Duration::from_secs(3600);
/// What one read asks for, as the searcher's buffer does.
const CHUNK: usize = 64 << 10;
/// A segment holds the trigrams of about this much text, as read: it bounds
/// what building one holds at once, and few segments keep the trigram
/// tables each repeats few.
const SEGMENT_BYTES: u64 = 32 << 20;
/// Files read at once: at most this many, of at most this many bytes by
/// their sizes (a binary file is read only to its first NUL, so a run of
/// them makes little text).
const READ_FILES: usize = 16_384;
const READ_BYTES: u64 = 32 << 20;
/// Strings a piece of a pattern is kept as before it is only trigrams.
const SET_MAX: usize = 64;
/// Characters a class may hold and still be kept as its strings.
const CLASS_MAX: u32 = 16;

// ------------------------------------------------------------- the walk

/// Folders the user keeps private: the listed ones, and any holding the
/// marker. The walk asks it of each folder before entering it, so the
/// folders above have been asked already.
struct Private {
    listed: Vec<PathBuf>,
}

impl Private {
    fn holds(&self, dir: &Path) -> bool {
        self.listed.iter().any(|f| dir.starts_with(f))
            || std::fs::symlink_metadata(dir.join(skwir_core::permissions::MARKER)).is_ok()
    }
}

/// A file a search under the root walks to.
struct Walked {
    path: PathBuf,
    /// What reading it reads: a link's target's size.
    size: u64,
    /// Its change time (a link's target's): what a check compares.
    changed: Option<SystemTime>,
}

/// The files a `content:` search under `root` walks to, as the query
/// plugin walks (`provider.rs`), each stat-ed through a link to what it
/// opens: what a check of the index costs, as it reads no file.
fn walk(root: &Path, private: &Arc<Private>) -> Vec<Walked> {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(true)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .ignore(false)
        .parents(false)
        .follow_links(false);
    let p = private.clone();
    builder.filter_entry(move |dent| {
        !(dent.depth() > 0 && dent.file_type().is_some_and(|t| t.is_dir()) && p.holds(dent.path()))
    });
    builder.build_parallel().run(|| {
        let tx = tx.clone();
        Box::new(move |result| {
            let Ok(dent) = result else {
                return ignore::WalkState::Continue;
            };
            let Some(kind) = dent.file_type() else {
                return ignore::WalkState::Continue;
            };
            if !(kind.is_file() || kind.is_symlink()) {
                return ignore::WalkState::Continue;
            }
            let path = dent.into_path();
            if let Ok(meta) = std::fs::metadata(&path)
                && meta.is_file()
            {
                let changed = skwir_core::fs::stat::changed(&path, &meta);
                let _ = tx.send(Walked {
                    path,
                    size: meta.len(),
                    changed,
                });
            }
            ignore::WalkState::Continue
        })
    });
    drop(tx);
    rx.into_iter().collect()
}

/// The threads the scan reads with: the walker's default (`ignore` caps it
/// at twelve), so the index reads no faster for having more.
fn readers() -> rayon::ThreadPool {
    let n = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    rayon::ThreadPoolBuilder::new()
        .num_threads(n.min(12))
        .build()
        .expect("reader threads")
}

// ------------------------------------------------------------ the files

/// Read what the scan searches of `path` into `buf`: `None` when it reads
/// nothing (no longer a regular file, larger than the limit), else
/// whether the file was UTF-16, decoded as the searcher decodes it.
fn read_searched(path: &Path, buf: &mut Vec<u8>) -> Option<bool> {
    buf.clear();
    let mut file = skwir_platform::file::open_regular(path).ok()?;
    if file.metadata().ok()?.len() > CONTENT_LIMIT {
        return None;
    }
    let mut utf16 = None;
    loop {
        let at = buf.len();
        buf.resize(at + CHUNK, 0);
        let n = loop {
            match file.read(&mut buf[at..]) {
                Ok(n) => break n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break 0,
            }
        };
        buf.truncate(at + n);
        if at == 0 {
            // grep-searcher sniffs UTF-16's marks and decodes the file;
            // UTF-8's goes through as it is (`utf8_passthru`).
            utf16 = match buf.as_slice() {
                [0xFF, 0xFE, ..] => Some(true),
                [0xFE, 0xFF, ..] => Some(false),
                _ => None,
            };
        }
        if n == 0 {
            break;
        }
        // The searcher stops at the first NUL as if the file ended there
        // (`BinaryDetection::quit`), of the decoded text for UTF-16.
        if utf16.is_none()
            && let Some(i) = memchr::memchr(0, &buf[at..])
        {
            buf.truncate(at + i);
            return Some(false);
        }
    }
    let Some(le) = utf16 else {
        return Some(false);
    };
    let body = &buf[2..];
    let units = body.as_chunks::<2>().0.iter().map(|&pair| {
        if le {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        }
    });
    let mut text: String = char::decode_utf16(units)
        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect();
    if body.len() % 2 == 1 {
        text.push(char::REPLACEMENT_CHARACTER);
    }
    let mut bytes = text.into_bytes();
    if let Some(i) = memchr::memchr(0, &bytes) {
        bytes.truncate(i);
    }
    *buf = bytes;
    Some(true)
}

fn fold(b: u8) -> u8 {
    b.to_ascii_lowercase()
}

fn trigram(s: &[u8]) -> u32 {
    u32::from(fold(s[0])) << 16 | u32::from(fold(s[1])) << 8 | u32::from(fold(s[2]))
}

thread_local! {
    /// One bit per trigram: those of the file at hand already counted.
    static SEEN: RefCell<Vec<u64>> = RefCell::new(vec![0; (1 << 24) / 64]);
}

/// The sorted distinct trigrams of `text`, ASCII letters folded.
fn trigrams(text: &[u8], out: &mut Vec<u32>) {
    out.clear();
    if text.len() < 3 {
        return;
    }
    SEEN.with_borrow_mut(|seen| {
        let mut t = u32::from(fold(text[0])) << 8 | u32::from(fold(text[1]));
        for &b in &text[2..] {
            t = ((t << 8) | u32::from(fold(b))) & 0xFF_FFFF;
            let (w, bit) = ((t >> 6) as usize, 1u64 << (t & 63));
            if seen[w] & bit == 0 {
                seen[w] |= bit;
                out.push(t);
            }
        }
        for &t in out.iter() {
            seen[(t >> 6) as usize] = 0;
        }
    });
    out.sort_unstable();
}

// ------------------------------------------------------------ the index

struct Doc {
    path: PathBuf,
    /// Bytes the scan searches of it.
    text: u32,
}

/// The trigrams of a run of docs: for each, the docs holding it (numbers
/// from `base`, ascending, delta varints).
struct Segment {
    base: u32,
    len: u32,
    keys: Vec<u32>,
    offs: Vec<u32>,
    post: Vec<u8>,
}

impl Segment {
    fn postings(&self, tri: u32) -> Vec<u32> {
        let Ok(k) = self.keys.binary_search(&tri) else {
            return Vec::new();
        };
        let mut bytes = &self.post[self.offs[k] as usize..self.offs[k + 1] as usize];
        let mut out = Vec::new();
        let mut last = 0u32;
        while !bytes.is_empty() {
            let (v, n) = varint(bytes);
            bytes = &bytes[n..];
            last += v;
            out.push(last);
        }
        out
    }

    fn bytes(&self) -> usize {
        self.keys.len() * 4 + self.offs.len() * 4 + self.post.len()
    }
}

fn varint(b: &[u8]) -> (u32, usize) {
    let mut v = 0u32;
    for (i, &x) in b.iter().enumerate().take(5) {
        v |= u32::from(x & 0x7f) << (7 * i);
        if x & 0x80 == 0 {
            return (v, i + 1);
        }
    }
    (v, b.len().min(5))
}

fn put_varint(out: &mut Vec<u8>, mut v: u32) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

pub struct Index {
    docs: Vec<Doc>,
    segments: Vec<Segment>,
}

impl Index {
    /// The postings, the trigram tables and their offsets.
    fn postings_bytes(&self) -> usize {
        self.segments.iter().map(Segment::bytes).sum()
    }

    /// The docs as a stored index would keep them: each path's bytes, its
    /// offset, the bytes searched and the change time a check compares.
    fn table_bytes(&self) -> usize {
        self.docs
            .iter()
            .map(|d| d.path.as_os_str().len() + 4 + 4 + 12)
            .sum()
    }
}

#[derive(Default)]
struct BuildStats {
    walked: usize,
    docs: usize,
    with_text: usize,
    cut_at_nul: usize,
    utf16: usize,
    text_bytes: u64,
    postings: u64,
    walk: Duration,
    read: Duration,
    encode: Duration,
}

/// Index what a search under `root` reads.
fn build(root: &Path, private: &Arc<Private>, pool: &rayon::ThreadPool) -> (Index, BuildStats) {
    let mut stats = BuildStats::default();
    let t = Instant::now();
    let files = walk(root, private);
    stats.walk = t.elapsed();
    stats.walked = files.len();
    let mut index = Index {
        docs: Vec::new(),
        segments: Vec::new(),
    };
    // The segment being filled: its first doc, its text so far, and its
    // (trigram, doc within it) pairs.
    let (mut base, mut seg_text, mut pairs) = (0u32, 0u64, Vec::new());
    let mut start = 0;
    while start < files.len() {
        let (mut end, mut bytes) = (start, 0u64);
        while end < files.len() && end - start < READ_FILES && bytes < READ_BYTES {
            bytes += files[end].size.min(CONTENT_LIMIT);
            end += 1;
        }
        let t = Instant::now();
        let read: Vec<Option<(u32, bool, bool, Vec<u32>)>> = pool.install(|| {
            files[start..end]
                .par_iter()
                .map_init(Vec::new, |buf, w| {
                    let utf16 = read_searched(&w.path, buf)?;
                    let cut = !utf16 && (buf.len() as u64) < w.size;
                    let mut tris = Vec::new();
                    trigrams(buf, &mut tris);
                    Some((buf.len() as u32, utf16, cut, tris))
                })
                .collect()
        });
        stats.read += t.elapsed();
        let t = Instant::now();
        for (w, r) in files[start..end].iter().zip(read) {
            let Some((text, utf16, cut, tris)) = r else {
                continue;
            };
            let local = index.docs.len() as u32 - base;
            pairs.extend(tris.iter().map(|&t| u64::from(t) << 32 | u64::from(local)));
            seg_text += u64::from(text);
            stats.with_text += usize::from(text > 0);
            stats.utf16 += usize::from(utf16);
            stats.cut_at_nul += usize::from(cut);
            stats.text_bytes += u64::from(text);
            index.docs.push(Doc {
                path: w.path.clone(),
                text,
            });
        }
        start = end;
        let last = start == files.len();
        if seg_text >= SEGMENT_BYTES || (last && (index.docs.len() as u32 > base)) {
            stats.postings += pairs.len() as u64;
            let len = index.docs.len() as u32 - base;
            index
                .segments
                .push(encode(std::mem::take(&mut pairs), base, len, pool));
            (base, seg_text) = (index.docs.len() as u32, 0);
        }
        stats.encode += t.elapsed();
    }
    stats.docs = index.docs.len();
    (index, stats)
}

/// The segment of `len` docs from `base` whose (trigram, doc within it)
/// pairs these are.
fn encode(mut pairs: Vec<u64>, base: u32, len: u32, pool: &rayon::ThreadPool) -> Segment {
    pool.install(|| pairs.par_sort_unstable());
    let mut seg = Segment {
        base,
        len,
        keys: Vec::new(),
        offs: Vec::new(),
        post: Vec::new(),
    };
    let (mut key, mut last) = (u32::MAX, 0u32);
    for p in pairs {
        let (t, d) = ((p >> 32) as u32, p as u32);
        if t == key {
            put_varint(&mut seg.post, d - last);
        } else {
            seg.keys.push(t);
            seg.offs.push(seg.post.len() as u32);
            put_varint(&mut seg.post, d);
            key = t;
        }
        last = d;
    }
    seg.offs.push(seg.post.len() as u32);
    seg
}

// ------------------------------------------------------------- planning

/// A query on the trigrams a file holds.
#[derive(Clone, Debug)]
enum Tq {
    All,
    Tri(u32),
    And(Vec<Tq>),
    Or(Vec<Tq>),
}

fn and(a: Tq, b: Tq) -> Tq {
    match (a, b) {
        (Tq::All, x) | (x, Tq::All) => x,
        (Tq::And(mut v), Tq::And(w)) => {
            v.extend(w);
            Tq::And(v)
        }
        (Tq::And(mut v), x) | (x, Tq::And(mut v)) => {
            v.push(x);
            Tq::And(v)
        }
        (a, b) => Tq::And(vec![a, b]),
    }
}

fn or(a: Tq, b: Tq) -> Tq {
    match (a, b) {
        (Tq::All, _) | (_, Tq::All) => Tq::All,
        (Tq::Or(mut v), Tq::Or(w)) => {
            v.extend(w);
            Tq::Or(v)
        }
        (Tq::Or(mut v), x) | (x, Tq::Or(mut v)) => {
            v.push(x);
            Tq::Or(v)
        }
        (a, b) => Tq::Or(vec![a, b]),
    }
}

/// Folded strings one of which is a piece of every match.
type Strings = BTreeSet<Vec<u8>>;

/// What every match of a piece of a pattern satisfies: `tq`, and when
/// `whole` is known, its folded text is one of these strings.
struct Info {
    whole: Option<Strings>,
    tq: Tq,
}

impl Info {
    fn string(s: Vec<u8>) -> Info {
        Info {
            whole: Some(Strings::from([s])),
            tq: Tq::All,
        }
    }

    fn anything() -> Info {
        Info {
            whole: None,
            tq: Tq::All,
        }
    }

    /// What a match must hold, as trigrams alone.
    fn required(self) -> Tq {
        and(self.tq, holds_one(self.whole.as_ref()))
    }
}

/// The files that hold one of `strings`: any, when one is too short to
/// have a trigram (or none is known).
fn holds_one(strings: Option<&Strings>) -> Tq {
    let Some(strings) = strings.filter(|s| !s.is_empty()) else {
        return Tq::All;
    };
    let mut q: Option<Tq> = None;
    for s in strings {
        if s.len() < 3 {
            return Tq::All;
        }
        let all = s.windows(3).map(|w| Tq::Tri(trigram(w))).fold(Tq::All, and);
        q = Some(match q {
            None => all,
            Some(q) => or(q, all),
        });
    }
    q.unwrap_or(Tq::All)
}

fn product(a: &Strings, b: &Strings) -> Strings {
    a.iter()
        .flat_map(|x| b.iter().map(move |y| [x.as_slice(), y].concat()))
        .collect()
}

/// What every match of `h` satisfies. Each case keeps to what is true of
/// every match, so the files the query names are all that can match.
fn info(h: &Hir) -> Info {
    match h.kind() {
        HirKind::Empty | HirKind::Look(_) => Info::string(Vec::new()),
        HirKind::Literal(lit) => Info::string(lit.0.iter().map(|&b| fold(b)).collect()),
        HirKind::Class(Class::Unicode(c)) => {
            let n: u32 = c
                .ranges()
                .iter()
                .map(|r| u32::from(r.end()) - u32::from(r.start()) + 1)
                .sum();
            if n > CLASS_MAX {
                return Info::anything();
            }
            let whole = c
                .ranges()
                .iter()
                .flat_map(|r| r.start()..=r.end())
                .map(|ch| {
                    let mut b = [0; 4];
                    ch.encode_utf8(&mut b).bytes().map(fold).collect()
                })
                .collect();
            Info {
                whole: Some(whole),
                tq: Tq::All,
            }
        }
        HirKind::Class(Class::Bytes(c)) => {
            let n: u32 = c
                .ranges()
                .iter()
                .map(|r| u32::from(r.end()) - u32::from(r.start()) + 1)
                .sum();
            if n > CLASS_MAX {
                return Info::anything();
            }
            let whole = c
                .ranges()
                .iter()
                .flat_map(|r| r.start()..=r.end())
                .map(|b| vec![fold(b)])
                .collect();
            Info {
                whole: Some(whole),
                tq: Tq::All,
            }
        }
        HirKind::Repetition(r) => {
            let sub = info(&r.sub);
            match (r.min, r.max) {
                (1, Some(1)) => sub,
                (0, Some(1)) => match sub.whole {
                    Some(mut whole) => {
                        whole.insert(Vec::new());
                        Info {
                            whole: Some(whole),
                            tq: Tq::All,
                        }
                    }
                    None => Info::anything(),
                },
                (0, _) => Info::anything(),
                // At least one of them: what one holds, the match holds.
                _ => Info {
                    whole: None,
                    tq: sub.required(),
                },
            }
        }
        HirKind::Capture(c) => info(&c.sub),
        HirKind::Concat(subs) => {
            // Pieces known as strings run together while their product
            // stays small; a run that ends becomes what the match holds.
            let mut tq = Tq::All;
            let mut run = Strings::from([Vec::new()]);
            let mut whole = true;
            for sub in subs.iter().map(info) {
                tq = and(tq, sub.tq);
                match sub.whole {
                    Some(s) if run.len() * s.len() <= SET_MAX => run = product(&run, &s),
                    Some(s) => {
                        tq = and(tq, holds_one(Some(&run)));
                        whole = false;
                        run = s;
                    }
                    None => {
                        tq = and(tq, holds_one(Some(&run)));
                        whole = false;
                        run = Strings::from([Vec::new()]);
                    }
                }
            }
            if whole {
                Info {
                    whole: Some(run),
                    tq,
                }
            } else {
                Info {
                    whole: None,
                    tq: and(tq, holds_one(Some(&run))),
                }
            }
        }
        HirKind::Alternation(subs) => {
            let infos: Vec<Info> = subs.iter().map(info).collect();
            let strings: usize = infos
                .iter()
                .map(|i| i.whole.as_ref().map_or(usize::MAX, BTreeSet::len))
                .fold(0, usize::saturating_add);
            if infos.is_empty() {
                Info::anything()
            } else if strings <= SET_MAX {
                let mut whole = Strings::new();
                let mut tq: Option<Tq> = None;
                for i in infos {
                    whole.extend(i.whole.into_iter().flatten());
                    tq = Some(match tq {
                        None => i.tq,
                        Some(q) => or(q, i.tq),
                    });
                }
                Info {
                    whole: Some(whole),
                    tq: tq.unwrap_or(Tq::All),
                }
            } else {
                let tq = infos.into_iter().map(Info::required).reduce(or);
                Info {
                    whole: None,
                    tq: tq.unwrap_or(Tq::All),
                }
            }
        }
    }
}

/// The trigram query of `pat`, from the HIR grep-regex builds of it
/// (`ConfiguredHIR::new`: escaped when it is text, case folded by
/// Unicode's rules unless `case:sensitive`).
fn plan(pat: &ContentPat) -> Result<Tq> {
    let pattern = if pat.regex {
        format!("(?:{})", pat.text)
    } else {
        format!("(?:{})", regex_syntax::escape(&pat.text))
    };
    let hir = regex_syntax::ParserBuilder::new()
        .utf8(false)
        .unicode(true)
        .octal(false)
        .nest_limit(250)
        .case_insensitive(!pat.case_sensitive)
        .build()
        .parse(&pattern)
        .with_context(|| format!("planning `{}`", pat.text))?;
    Ok(info(&hir).required())
}

fn describe(tq: &Tq) -> String {
    fn count(tq: &Tq) -> usize {
        match tq {
            Tq::All => 0,
            Tq::Tri(_) => 1,
            Tq::And(v) | Tq::Or(v) => v.iter().map(count).sum(),
        }
    }
    match tq {
        Tq::All => "every file".to_owned(),
        Tq::Tri(_) => "1 trigram".to_owned(),
        Tq::And(_) => format!("all of {} trigrams", count(tq)),
        Tq::Or(v) => format!("one of {} alternatives ({} trigrams)", v.len(), count(tq)),
    }
}

/// The docs of `seg` that `tq` holds, numbered within it: `None` for all.
fn eval(seg: &Segment, tq: &Tq) -> Option<Vec<u32>> {
    match tq {
        Tq::All => None,
        Tq::Tri(t) => Some(seg.postings(*t)),
        Tq::And(v) => {
            let mut acc: Option<Vec<u32>> = None;
            for q in v {
                if let Some(list) = eval(seg, q) {
                    let next = match acc {
                        None => list,
                        Some(a) => intersect(&a, &list),
                    };
                    let empty = next.is_empty();
                    acc = Some(next);
                    if empty {
                        break;
                    }
                }
            }
            acc
        }
        Tq::Or(v) => {
            let mut out = Vec::new();
            for q in v {
                out.extend(eval(seg, q)?);
            }
            out.sort_unstable();
            out.dedup();
            Some(out)
        }
    }
}

fn intersect(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                out.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    out
}

// ------------------------------------------------------------ answering

struct Answer {
    plan: String,
    planned: Duration,
    candidates: usize,
    looked_up: Duration,
    verified: Duration,
    matched: HashSet<PathBuf>,
}

fn answer(index: &Index, pat: &ContentPat, pool: &rayon::ThreadPool) -> Result<Answer> {
    let t = Instant::now();
    let tq = plan(pat)?;
    let planned = t.elapsed();
    let t = Instant::now();
    let candidates: Vec<u32> = index
        .segments
        .par_iter()
        .flat_map_iter(|seg| match eval(seg, &tq) {
            None => (seg.base..seg.base + seg.len)
                .filter(|&d| index.docs[d as usize].text > 0)
                .collect::<Vec<_>>(),
            Some(list) => list.into_iter().map(|d| seg.base + d).collect(),
        })
        .collect();
    let looked_up = t.elapsed();
    let t = Instant::now();
    let matched = pool.install(|| {
        candidates
            .par_iter()
            .map(|&d| &index.docs[d as usize].path)
            .filter(|p| lang::content_matches(p, pat))
            .cloned()
            .collect()
    });
    Ok(Answer {
        plan: describe(&tq),
        planned,
        candidates: candidates.len(),
        looked_up,
        verified: t.elapsed(),
        matched,
    })
}

/// What the query plugin's search of `expr` under `root` answers, and
/// how long it took to complete.
fn scan(app: &skwir_core::App, expr: &str, root: &Path) -> Result<(Duration, HashSet<PathBuf>)> {
    let t = Instant::now();
    let location = skwir_plugin_query::location_for(expr, Some(root));
    let open = app.open_location(&location)?;
    let mut found = HashSet::new();
    loop {
        match open
            .events()
            .recv_timeout(SCAN_DEADLINE.saturating_sub(t.elapsed()))
        {
            Ok(ListingEvent::Added(batch)) => {
                found.extend(batch.iter().filter_map(|e| open.path_of(e.id)));
            }
            Ok(ListingEvent::Complete) => break,
            Ok(ListingEvent::Error(e)) => bail!("search error: {e}"),
            Ok(_) => {}
            Err(e) => bail!("search did not complete: {e}"),
        }
    }
    let took = t.elapsed();
    open.close();
    Ok((took, found))
}

/// The one `content:` term of `expr`.
fn content_term(expr: &str) -> Result<ContentPat> {
    let q = lang::parse(expr).map_err(|e| anyhow::anyhow!("`{expr}`: {}", e.0))?;
    match q.expr {
        Some(Expr::Term(Term::Content(p))) => Ok(p),
        _ => bail!("`{expr}` is not one content: term"),
    }
}

// ------------------------------------------------------- the machine's state

/// Drop `files` from the page cache, so the next read of them is the
/// disk's (their folders' entries and inodes stay cached: that needs
/// root). The number of files dropped, `None` where there is no
/// `fadvise` (the other systems' version), hence the `Option` here.
#[cfg(target_os = "linux")]
#[allow(clippy::unnecessary_wraps)]
fn evict(files: &[Walked]) -> Option<usize> {
    Some(
        files
            .par_iter()
            .filter(|w| {
                skwir_platform::file::open_regular(&w.path).is_ok_and(|f| {
                    rustix::fs::fadvise(&f, 0, None, rustix::fs::Advice::DontNeed).is_ok()
                })
            })
            .count(),
    )
}

#[cfg(not(target_os = "linux"))]
fn evict(_: &[Walked]) -> Option<usize> {
    None
}

/// A field of `/proc/self/status`, in kB.
fn status_kb(field: &str) -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = s.lines().find(|l| l.starts_with(field))?;
    line[field.len()..]
        .trim()
        .trim_end_matches("kB")
        .trim()
        .parse()
        .ok()
}

/// Start measuring the peak resident memory afresh.
fn reset_peak() {
    let _ = std::fs::write("/proc/self/clear_refs", "5");
}

/// The process's CPU time, user and system, from `/proc/self/stat` (in
/// the kernel's user clock ticks, a hundred a second).
fn cpu() -> Option<Duration> {
    let s = std::fs::read_to_string("/proc/self/stat").ok()?;
    let rest = &s[s.rfind(')')? + 2..];
    let f: Vec<&str> = rest.split(' ').collect();
    let ticks: u64 = f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?;
    Some(Duration::from_millis(ticks * 10))
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1e5).round() / 100.0
}

fn cpu_since(before: Option<Duration>) -> Option<f64> {
    Some(ms(cpu()?.checked_sub(before?)?))
}

fn emit(v: &serde_json::Value) {
    println!("{v}");
}

/// What a file the two answers disagree on was, without its name.
fn describe_file(path: &Path, since: SystemTime, index: &Index) -> serde_json::Value {
    let meta = std::fs::metadata(path).ok();
    let changed = meta
        .as_ref()
        .and_then(|m| skwir_core::fs::stat::changed(path, m))
        .is_some_and(|c| c >= since);
    let doc = index.docs.iter().find(|d| d.path == path);
    json!({
        "ext": path.extension().map(|e| e.to_string_lossy().into_owned()),
        "size": meta.as_ref().map(std::fs::Metadata::len),
        "changed_since_built": changed,
        "indexed": doc.is_some(),
        "text": doc.map(|d| d.text),
    })
}

// ------------------------------------------------------------- the run

pub struct Options {
    pub queries: Vec<String>,
    pub cold: bool,
    pub scan: bool,
    /// Runs of each query, by the index and by the scan.
    pub runs: usize,
}

/// `runs`' wall times (each, the least and the median) and the CPU time
/// of the median one, under `name_*` keys.
fn runs_into(line: &mut serde_json::Value, name: &str, runs: &[(Duration, Option<f64>)]) {
    let mut sorted: Vec<&(Duration, Option<f64>)> = runs.iter().collect();
    sorted.sort_by_key(|r| r.0);
    let Some(median) = sorted.get(sorted.len() / 2) else {
        return;
    };
    line[format!("{name}_ms")] = json!(runs.iter().map(|r| ms(r.0)).collect::<Vec<_>>());
    line[format!("{name}_min_ms")] = json!(ms(sorted[0].0));
    line[format!("{name}_median_ms")] = json!(ms(median.0));
    line[format!("{name}_median_cpu_ms")] = json!(median.1);
}

/// Build the index of `dir`, cold then warm, time a check of it, and
/// answer each query from it and by the scan, comparing the two.
pub fn run(dir: &Path, opts: &Options) -> Result<()> {
    let dir = skwir_core::os_path::canonical(dir)?;
    let app = crate::measure::tags_app();
    let privacy = app.privacy();
    if privacy.holds(&dir) {
        bail!("the folder is private");
    }
    let private = Arc::new(Private {
        listed: privacy.listed().to_vec(),
    });
    let queries: Vec<String> = if opts.queries.is_empty() {
        QUERIES.iter().map(|&q| q.to_owned()).collect()
    } else {
        opts.queries.clone()
    };
    let pats = queries
        .iter()
        .map(|q| content_term(q).map(|p| (q.clone(), p)))
        .collect::<Result<Vec<_>>>()?;
    let pool = readers();
    emit(&json!({
        "phase": "start",
        "threads": pool.current_num_threads(),
        "load": std::fs::read_to_string("/proc/loadavg").ok().map(|s| s.trim().to_owned()),
    }));

    let t = Instant::now();
    let files = walk(&dir, &private);
    emit(&json!({"phase": "walk", "files": files.len(), "ms": ms(t.elapsed())}));

    if opts.cold {
        let dropped = evict(&files);
        let c = cpu();
        let t = Instant::now();
        let (_, s) = build(&dir, &private, &pool);
        emit(&json!({
            "phase": "build_cold", "dropped": dropped, "ms": ms(t.elapsed()), "cpu_ms": cpu_since(c),
            "walk_ms": ms(s.walk), "read_ms": ms(s.read), "encode_ms": ms(s.encode),
        }));
    }

    reset_peak();
    let rss_before = status_kb("VmRSS:");
    let since = SystemTime::now();
    let c = cpu();
    let t = Instant::now();
    let (index, s) = build(&dir, &private, &pool);
    let took = t.elapsed();
    emit(&json!({
        "phase": "build_warm", "ms": ms(took), "cpu_ms": cpu_since(c),
        "walk_ms": ms(s.walk), "read_ms": ms(s.read), "encode_ms": ms(s.encode),
        "walked": s.walked, "docs": s.docs, "with_text": s.with_text,
        "cut_at_nul": s.cut_at_nul, "utf16": s.utf16, "text_bytes": s.text_bytes,
        "postings": s.postings, "segments": index.segments.len(),
        "postings_bytes": index.postings_bytes(), "table_bytes": index.table_bytes(),
        "rss_before_kb": rss_before, "peak_kb": status_kb("VmHWM:"), "rss_after_kb": status_kb("VmRSS:"),
    }));
    drop(files);

    // What a search would stat to know which files changed since the
    // index was built: every file under the root, reading none.
    let c = cpu();
    let t = Instant::now();
    let again = walk(&dir, &private);
    let changed = again
        .iter()
        .filter(|w| w.changed.is_none_or(|c| c >= since))
        .count();
    emit(&json!({
        "phase": "check", "ms": ms(t.elapsed()), "cpu_ms": cpu_since(c),
        "files": again.len(), "changed_since_built": changed,
    }));

    let mut checker = privacy.checker();
    for (expr, pat) in &pats {
        // Alternated, so the machine's load weighs on both alike.
        let (mut index_runs, mut scan_runs) = (Vec::new(), Vec::new());
        let mut last = None;
        let mut found = None;
        for _ in 0..opts.runs {
            let c = cpu();
            let t = Instant::now();
            let a = answer(&index, pat, &pool)?;
            index_runs.push((t.elapsed(), cpu_since(c)));
            last = Some(a);
            if opts.scan {
                let c = cpu();
                let (took, f) = scan(&app, expr, &dir)?;
                scan_runs.push((took, cpu_since(c)));
                found = Some(f);
            }
        }
        let a = last.context("no run")?;
        let mut line = json!({
            "phase": "query", "query": expr, "plan": a.plan,
            "plan_us": a.planned.as_micros(), "lookup_ms": ms(a.looked_up),
            "candidates": a.candidates, "verify_ms": ms(a.verified),
            "matched": a.matched.len(),
        });
        runs_into(&mut line, "index", &index_runs);
        if let Some(mut found) = found {
            found.retain(|p| !checker.holds(p));
            let missing: Vec<&PathBuf> = found.difference(&a.matched).collect();
            let extra: Vec<&PathBuf> = a.matched.difference(&found).collect();
            runs_into(&mut line, "scan", &scan_runs);
            line["scan_matched"] = json!(found.len());
            line["same"] = json!(missing.is_empty() && extra.is_empty());
            if !missing.is_empty() || !extra.is_empty() {
                line["missing"] = json!(missing.len());
                line["extra"] = json!(extra.len());
                line["missing_files"] = missing
                    .iter()
                    .take(5)
                    .map(|p| describe_file(p, since, &index))
                    .collect();
                line["extra_files"] = extra
                    .iter()
                    .take(5)
                    .map(|p| describe_file(p, since, &index))
                    .collect();
            }
        }
        emit(&line);
    }

    if opts.cold {
        let pat = content_term(COLD_QUERY)?;
        let dropped = evict(&again);
        let t = Instant::now();
        let a = answer(&index, &pat, &pool)?;
        let mut line = json!({
            "phase": "cold", "query": COLD_QUERY, "dropped": dropped,
            "index_ms": ms(t.elapsed()), "candidates": a.candidates, "matched": a.matched.len(),
        });
        if opts.scan {
            evict(&again);
            let (took, found) = scan(&app, COLD_QUERY, &dir)?;
            line["scan_ms"] = json!(ms(took));
            line["scan_matched"] = json!(found.len());
        }
        emit(&line);
    }
    Ok(())
}
