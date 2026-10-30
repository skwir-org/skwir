//! Mobipocket books (ADR-106): MOBI 6, KF8 (AZW3) and `PalmDOC` texts, all
//! Palm databases. A file holding MOBI 6 and KF8 is read through its KF8
//! part. A book whose text is encrypted (DRM) is refused, saying so, and so
//! is Amazon's KFX, which is not one.
//!
//! The text is decompressed once (`PalmDOC`'s LZ77 or Mobipocket's
//! Huffman coding, at most `TEXT` bytes) and kept, cut in chapters, while
//! the preview shows the book (`book::Kept`); pictures are read from their
//! records when shown.
use std::sync::Arc;

use skwir_core::{
    CancelToken,
    inspection::{Picture, Row},
    preview::FileSource,
};
use skwir_platform::{
    inspection::{self, limits},
    rich_preview::{self, Markup},
};

use crate::book::{Book, ChapterRef, Sheets, Sizes, member_key};

mod huffcdic;
mod index;
mod kf8;
mod mobi6;
pub(crate) mod palmdb;
mod palmdoc;

use palmdb::{Compression, Database, Header, Kind, exth};

/// Bytes of a book's first record read to learn how it is stored: its
/// `PalmDOC` header and the start of its MOBI header.
const FIRST: usize = 64;
/// A book's text decompressed, at most: more than any novel, a large
/// dictionary's worth.
const TEXT: usize = 4 * limits::BINARY_INPUT;
/// Bytes one text record decompresses to, at most (its own are 4 KiB).
const RECORD: usize = 1024 * 1024;

/// Whether `MuPDF` reads the book in `file` (ADR-106): a MOBI 6 book or a
/// `PalmDOC` text, stored as is or with `PalmDOC`'s compression, and not
/// encrypted. A file holding MOBI 6 and KF8 is read through its MOBI 6 part;
/// `MuPDF` reads nothing of a KF8 book (AZW3) and refuses Huffman-coded
/// text. Told from the file's first record, before anything opens it.
pub(crate) fn mupdf_reads<S: FileSource + ?Sized>(file: &S, cancel: &CancelToken) -> bool {
    let read = || -> anyhow::Result<bool> {
        let database = Database::open(file, cancel)?;
        if !matches!(database.kind, Kind::Mobi | Kind::Text) {
            return Ok(false);
        }
        let header = Header::parse(&database.record_head(file, 0, FIRST, cancel)?)?;
        Ok(!header.encrypted
            && matches!(header.compression, Compression::None | Compression::PalmDoc)
            && header.mobi.as_ref().is_none_or(|mobi| mobi.version < 8))
    };
    read().unwrap_or(false)
}

/// Whether `head`, a file's first bytes, is a Mobipocket book or a `PalmDOC`
/// text: what a Palm database (`.pdb`, `.prc`) must be to be read as one.
pub(crate) fn is_book(head: &[u8]) -> bool {
    matches!(Kind::of(head), Kind::Mobi | Kind::Text)
}

/// A MOBI 6 or `PalmDOC` chapter's name: what links and the contents call
/// it.
pub(crate) fn chapter_href(index: usize) -> String {
    format!("chapter-{}", index + 1)
}

/// A KF8 file's name.
pub(crate) fn part_href(index: usize) -> String {
    format!("part-{}", index + 1)
}

/// `text` escaped for HTML text.
pub(crate) fn escape(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

/// How a book's text is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Text {
    /// MOBI 6's HTML.
    Mobi6,
    /// KF8's XHTML files.
    Kf8,
    /// A `PalmDOC` text.
    Plain,
}

/// A Mobipocket book or `PalmDOC` text, read and cut in chapters.
pub(crate) struct Mobi {
    text: Text,
    /// Whether the file is KF8 alone (an AZW3).
    azw3: bool,
    database: Database,
    refs: Vec<ChapterRef>,
    chapters: Vec<String>,
    /// KF8's flows: its stylesheets, by number.
    flows: Vec<String>,
    contents: Vec<Row>,
    details: Vec<(&'static str, Option<String>)>,
    /// The first picture's record: what `recindex` and `kindle:embed`
    /// count from (in a file holding both, the MOBI 6 part's).
    pictures: Option<usize>,
    /// The size each picture is shown at, read once per book.
    sizes: Sizes,
}

impl Mobi {
    /// The book in `file`.
    pub(crate) fn open<S: FileSource + ?Sized>(
        file: &S,
        cancel: &CancelToken,
    ) -> anyhow::Result<Self> {
        let head = file.first_bytes()?;
        match Kind::of(&head) {
            Kind::Kfx => {
                anyhow::bail!("This is a KFX book, Amazon's own format, which is not read")
            }
            Kind::Other => anyhow::bail!("This is not a Mobipocket book"),
            Kind::Mobi | Kind::Text => {}
        }
        drop(head);
        let database = Database::open(file, cancel)?;
        let first = Header::parse(&database.record(file, 0, cancel)?)?;
        anyhow::ensure!(
            !first.encrypted,
            "This book is protected: its text is encrypted (DRM)"
        );
        // A KF8 book, or a file holding MOBI 6 then KF8: its KF8 part.
        let version = |header: &Header| header.mobi.as_ref().map_or(0, |mobi| mobi.version);
        let kf8 = if version(&first) >= 8 {
            Some((0, first.clone()))
        } else {
            first
                .number(exth::KF8_BOUNDARY)
                .map(|record| record as usize)
                .filter(|&record| record > 0 && record < database.len())
                .and_then(|record| {
                    let header =
                        Header::parse(&database.record(file, record, cancel).ok()?).ok()?;
                    (version(&header) >= 8 && !header.encrypted).then_some((record, header))
                })
        };
        let (pictures, _) = pictures(&first, &database);
        let details = details(&first, &database);
        let mut book = Self {
            text: Text::Mobi6,
            azw3: version(&first) >= 8,
            database,
            refs: Vec::new(),
            chapters: Vec::new(),
            flows: Vec::new(),
            contents: Vec::new(),
            details,
            pictures,
            sizes: Sizes::default(),
        };
        if let Some((base, header)) = kf8 {
            let raw = text(&book.database, file, base, &header, cancel)?;
            let mobi = header.mobi.as_ref().expect("a KF8 header is a MOBI header");
            let record = |relative: Option<u32>| {
                relative
                    .map(|r| base + r as usize)
                    .filter(|&r| r < book.database.len())
            };
            let index = |relative: Option<u32>| -> anyhow::Result<Option<index::Index>> {
                record(relative)
                    .map(|first| index::read(&book.database, file, first, true, cancel))
                    .transpose()
            };
            let skeletons = index(mobi.skeleton)?
                .ok_or_else(|| anyhow::anyhow!("This KF8 book has no skeleton index"))?;
            let fragments = index(mobi.fragment)?
                .ok_or_else(|| anyhow::anyhow!("This KF8 book has no fragment index"))?;
            // A book reads without its contents.
            let ncx = index(mobi.ncx).ok().flatten();
            let fdst = record(mobi.fdst)
                .map(|r| book.database.record(file, r, cancel))
                .transpose()?;
            let built = kf8::build(
                &raw,
                fdst.as_deref(),
                &skeletons,
                &fragments,
                ncx.as_ref(),
                cancel,
            )?;
            book.text = Text::Kf8;
            book.refs = (0..built.parts.len())
                .map(|index| ChapterRef {
                    href: part_href(index),
                    markup: Markup::Xhtml,
                })
                .collect();
            book.chapters = built.parts;
            book.flows = built.flows;
            book.contents = built.contents;
            return Ok(book);
        }
        let raw = text(&book.database, file, 0, &first, cancel)?;
        if let Some(mobi) = first
            .mobi
            .as_ref()
            .filter(|_| book.database.kind != Kind::Text)
        {
            let ncx = mobi
                .ncx
                .map(|r| r as usize)
                .filter(|&r| r < book.database.len())
                .and_then(|r| index::read(&book.database, file, r, mobi.utf8, cancel).ok());
            let entries: Vec<(String, u32, usize)> = ncx
                .iter()
                .flat_map(|ncx| {
                    ncx.in_order().into_iter().filter_map(|(entry, depth)| {
                        let position = entry.first(1)?;
                        let label = entry
                            .first(3)
                            .and_then(|offset| ncx.label(offset))
                            .unwrap_or_default()
                            .trim()
                            .to_owned();
                        Some((label, position, depth))
                    })
                })
                .collect();
            let positions: Vec<u32> = entries.iter().map(|(_, position, _)| *position).collect();
            let built = mobi6::chapters(&raw, &positions, mobi.utf8);
            book.contents = entries
                .into_iter()
                .take(limits::BLOCKS)
                .map(|(label, position, depth)| Row {
                    cells: vec![label],
                    target: built
                        .places
                        .get(&position)
                        .map(|&chapter| format!("{}#filepos{position}", chapter_href(chapter))),
                    depth: depth.min(limits::NESTING),
                    ..Row::default()
                })
                .collect();
            book.chapters = built.chapters;
        } else {
            book.text = Text::Plain;
            book.chapters = mobi6::text_chapters(&raw);
        }
        book.refs = (0..book.chapters.len())
            .map(|index| ChapterRef {
                href: chapter_href(index),
                markup: Markup::Html,
            })
            .collect();
        Ok(book)
    }

    /// The picture in record `key` (a number the book's chapters name), at
    /// most `limit` bytes of it: only a record after the text, never one
    /// of the book's own.
    pub(crate) fn picture<S: FileSource + ?Sized>(
        &self,
        file: &S,
        key: &str,
        limit: usize,
        cancel: &CancelToken,
    ) -> anyhow::Result<Vec<u8>> {
        let record = key
            .parse::<usize>()
            .ok()
            .filter(|&record| self.pictures.is_some_and(|first| record >= first))
            .filter(|&record| record < self.database.len())
            .ok_or_else(|| anyhow::anyhow!("Missing picture {key}"))?;
        Ok(self
            .database
            .record_head(file, record, limit.min(limits::BINARY_INPUT), cancel)?
            .into_owned())
    }

    /// The record a chapter's picture `src` names: MOBI 6's `recindex`
    /// (decimal, from 1), KF8's `kindle:embed:` (base 32, from 1).
    fn picture_record(&self, src: &str) -> Option<usize> {
        let number = match self.text {
            Text::Kf8 => {
                let code = src.strip_prefix("kindle:embed:")?;
                kf8::base32(code.split(['?', '#']).next()?)?
            }
            Text::Mobi6 => src.trim().parse::<u32>().ok()?,
            Text::Plain => return None,
        };
        Some(self.pictures? + (number as usize).checked_sub(1)?)
    }

    fn chapter(&self, href: &str) -> anyhow::Result<&str> {
        let key = member_key(href);
        let number = key.as_deref().and_then(|key| {
            key.strip_prefix("chapter-")
                .or_else(|| key.strip_prefix("part-"))?
                .parse::<usize>()
                .ok()
        });
        number
            .and_then(|number| self.chapters.get(number.checked_sub(1)?))
            .map(String::as_str)
            .ok_or_else(|| anyhow::anyhow!("Missing chapter {href}"))
    }
}

/// The first picture's record, and the cover's, as the first header names
/// them.
fn pictures(first: &Header, database: &Database) -> (Option<usize>, Option<usize>) {
    let pictures = first
        .mobi
        .as_ref()
        .and_then(|mobi| mobi.first_image)
        .map(|record| record as usize)
        .filter(|&record| record < database.len());
    let cover = first
        .number(exth::COVER)
        .zip(pictures)
        .map(|(offset, pictures)| pictures + offset as usize)
        .filter(|&record| record < database.len());
    (pictures, cover)
}

/// The cover of the book in `file`, read from its first record and the
/// cover's alone: a thumbnail decompresses no text.
pub(crate) fn cover_picture<S: FileSource + ?Sized>(
    file: &S,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Vec<u8>>> {
    if Kind::of(&file.first_bytes()?) != Kind::Mobi {
        return Ok(None);
    }
    let database = Database::open(file, cancel)?;
    let first = Header::parse(&database.record(file, 0, cancel)?)?;
    let (_, Some(cover)) = pictures(&first, &database) else {
        return Ok(None);
    };
    Ok(Some(database.record(file, cover, cancel)?.into_owned()))
}

/// The text of the book whose header (`header`) is record `base`: its
/// text records, what trails each removed, decompressed.
fn text<S: FileSource + ?Sized>(
    database: &Database,
    file: &S,
    base: usize,
    header: &Header,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<u8>> {
    let mobi = header.mobi.as_ref();
    let flags = mobi.map_or(0, |mobi| mobi.extra_flags);
    let limit = (header.text_length as usize).min(TEXT);
    let mut huffman = match header.compression {
        Compression::HuffCdic => {
            let (first, count) = mobi
                .and_then(|mobi| mobi.huffman)
                .ok_or_else(|| anyhow::anyhow!("This book names no Huffman tables"))?;
            let first = base + first as usize;
            let huff = database.record(file, first, cancel)?;
            let cdics = (1..count as usize)
                .map(|k| database.record(file, first + k, cancel))
                .collect::<anyhow::Result<Vec<_>>>()?;
            // The dictionary keeps no more than the text bound: the length
            // the book declares is the file's word, no bound of its own.
            Some(huffcdic::Huffman::new(
                &huff,
                cdics.iter().map(AsRef::as_ref),
                TEXT,
            )?)
        }
        _ => None,
    };
    let mut out = Vec::with_capacity(limit.min(limits::BINARY_INPUT));
    for k in 1..=usize::from(header.text_records) {
        inspection::check(cancel)?;
        if out.len() >= limit || base + k >= database.len() {
            break;
        }
        let record = database.record(file, base + k, cancel)?;
        let body = &record[..record.len() - trailing(&record, flags)];
        let room = (limit - out.len()).min(RECORD);
        let decoded = match (&mut huffman, header.compression) {
            (Some(huffman), _) => huffman.decode(body, room, cancel)?,
            (None, Compression::PalmDoc) => palmdoc::decompress(body, room)?,
            _ => body[..body.len().min(room)].to_vec(),
        };
        out.extend_from_slice(&decoded);
    }
    out.truncate(limit);
    Ok(out)
}

/// How many bytes trail a text record (MOBI's extra data flags): one entry
/// for each flag past the first, its size written backwards at the end,
/// then, for the first, the bytes of a character the next record goes on
/// with.
fn trailing(record: &[u8], flags: u16) -> usize {
    let mut size = 0;
    for bit in 1..16 {
        if flags & (1 << bit) == 0 {
            continue;
        }
        let end = record.len().saturating_sub(size);
        let mut value = 0usize;
        let mut shift = 0;
        for &byte in record[..end].iter().rev().take(4) {
            value |= usize::from(byte & 0x7F) << shift;
            shift += 7;
            if byte & 0x80 != 0 {
                break;
            }
        }
        size += value;
    }
    if flags & 1 != 0
        && let Some(&byte) = record
            .len()
            .checked_sub(size + 1)
            .and_then(|at| record.get(at))
    {
        size += usize::from(byte & 3) + 1;
    }
    size.min(record.len())
}

/// What the book says of itself, for Properties.
fn details(header: &Header, database: &Database) -> Vec<(&'static str, Option<String>)> {
    let title = header
        .text(exth::TITLE)
        .or_else(|| header.mobi.as_ref().and_then(|mobi| mobi.title.clone()))
        .or_else(|| (!database.name.is_empty()).then(|| database.name.clone()));
    vec![
        ("Title", title),
        ("Author", header.texts(exth::AUTHOR)),
        ("Publisher", header.texts(exth::PUBLISHER)),
        ("Subject", header.texts(exth::SUBJECT)),
        (
            "Published",
            header
                .text(exth::DATE)
                .map(|date| date.chars().take(10).collect()),
        ),
        ("ISBN", header.text(exth::ISBN)),
        ("Language", header.text(exth::LANGUAGE)),
    ]
}

/// A Mobipocket book as reading mode reads it, its pictures read from
/// `file`.
pub(crate) struct Reading<'a, S: FileSource + ?Sized> {
    pub book: &'a Mobi,
    pub file: &'a S,
}

impl<S: FileSource + ?Sized> Book for Reading<'_, S> {
    type Resources<'r>
        = Resources<'r, S>
    where
        Self: 'r;
    fn format(&self) -> &'static str {
        match self.book.text {
            Text::Plain => "PalmDOC",
            Text::Kf8 if self.book.azw3 => "AZW3",
            Text::Kf8 | Text::Mobi6 => "MOBI",
        }
    }
    const SOURCE: &'static str = "html";
    fn chapters(&self) -> &[ChapterRef] {
        &self.book.refs
    }
    fn text(
        &mut self,
        href: &str,
        _cancel: &CancelToken,
    ) -> anyhow::Result<(String, Option<String>)> {
        Ok((self.book.chapter(href)?.to_owned(), None))
    }
    fn resources<'r>(
        &'r mut self,
        _base: &'r str,
        images: bool,
        _sheets: &'r mut Sheets,
        cancel: &'r CancelToken,
    ) -> Resources<'r, S> {
        Resources {
            book: self.book,
            file: self.file,
            images,
            cancel,
        }
    }
    fn contents(&mut self) -> Vec<Row> {
        self.book.contents.clone()
    }
    fn details(&mut self) -> Vec<(&'static str, Option<String>)> {
        self.book.details.clone()
    }
}

/// What a chapter names: the book's pictures, by record, and KF8's
/// stylesheets, by flow.
pub(crate) struct Resources<'a, S: FileSource + ?Sized> {
    book: &'a Mobi,
    file: &'a S,
    images: bool,
    cancel: &'a CancelToken,
}

impl<S: FileSource + ?Sized> rich_preview::Resources for Resources<'_, S> {
    /// Named by its record for the view to decode when it shows it, with
    /// the size read once per book, from its first bytes where they say it.
    fn picture(&mut self, src: &str) -> Option<Picture> {
        if !self.images {
            return None;
        }
        let (book, file, cancel) = (self.book, self.file, self.cancel);
        let key = book.picture_record(src)?.to_string();
        book.sizes.picture(key, cancel, |key, limit| {
            book.picture(file, key, limit, cancel).ok()
        })
    }
    fn stylesheet(&mut self, href: &str) -> Option<String> {
        let number = href
            .strip_prefix("kindle:flow:")?
            .split(['?', '#'])
            .next()?;
        let flow = self.book.flows.get(kf8::base32(number)? as usize)?;
        (flow.len() <= limits::TEXT_INPUT).then(|| flow.clone())
    }
    fn parsed_stylesheet(&mut self, href: &str, base: &str) -> Option<Arc<rich_preview::Parsed>> {
        let css = self.stylesheet(href)?;
        Some(Arc::new(rich_preview::Parsed::new(&css, base)))
    }
}

#[cfg(test)]
mod tests;
