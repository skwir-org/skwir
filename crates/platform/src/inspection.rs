//! Shared bounded reads and embedded image decoding, called only by workers.
use bibli_core::{
    CancelToken,
    inspection::{Block, Inspection, InspectionKind, Row},
    thumbs::Image,
};
use std::{
    borrow::Cow,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

/// Every preview bound, in one place. Plugins and helpers name these instead
/// of repeating literals, so a limit changes once and stays consistent across
/// the process boundary: the archive worker receives its member bound as an
/// argument rather than compiling its own copy.
pub mod limits {
    use std::time::Duration;

    /// Text inputs that are parsed whole: Markdown, tables, structured code.
    pub const TEXT_INPUT: usize = 2 * 1024 * 1024;
    /// Binary inputs read whole: fonts, mail, embedded images, container members.
    pub const BINARY_INPUT: usize = 16 * 1024 * 1024;
    /// Declared expansion of every resource in one EPUB container.
    pub const CONTAINER_EXPANSION: u64 = 128 * 1024 * 1024;
    /// Members listed from one container: archive entries, EPUB resources.
    pub const CONTAINER_MEMBERS: usize = 4000;
    /// Bytes of all listed member names together.
    pub const MEMBER_NAMES: usize = 256 * 1024;
    /// Fixed JSON fields per listed member (`index`, `size`, three flags, punctuation).
    pub const MEMBER_JSON_OVERHEAD: usize = 160;
    /// Helper listing output, derived from the name bound: JSON escaping expands
    /// a byte to at most six characters (`\u00XX`), plus fixed fields per entry.
    pub const HELPER_OUTPUT: u64 =
        (MEMBER_NAMES * 6 + CONTAINER_MEMBERS * MEMBER_JSON_OVERHEAD) as u64;
    pub const HELPER_TIMEOUT: Duration = Duration::from_secs(15);
    /// One source page and one hexadecimal page.
    pub const SOURCE_PAGE: usize = 32 * 1024;
    /// One page of a text file that has no presentation of its own.
    pub const TEXT_PAGE: usize = 256 * 1024;
    pub const HEX_PAGE: u64 = 4096;
    pub const HEX_NEEDLE: usize = 4096;
    /// Semantic blocks per rich document; rows per contents listing.
    pub const BLOCKS: usize = 2000;
    /// Bytes per logical line: code lines and split rich-text rows. Must match
    /// the row split in `bibli_core::inspection::split_blocks`.
    pub const LINE_BYTES: usize = bibli_core::inspection::LINE_BYTES;
    pub const CODE_LINES: usize = 10000;
    pub const CODE_NODES: usize = 10000;
    /// Nesting depth of parsed documents, contents trees and archive paths.
    pub const NESTING: usize = 64;
    /// Element depth of HTML read in reading mode.
    pub const HTML_DEPTH: usize = 128;
    /// Style rules kept for one page, the built-in ones included; its
    /// stylesheets together are bounded by `TEXT_INPUT`.
    pub const STYLE_RULES: usize = 4000;
    pub const EMBEDDED_IMAGES: usize = 8;
    /// Decoded embedded images: pixel side, decoder allocation and display side.
    pub const IMAGE_SIDE: u32 = 8192;
    pub const IMAGE_ALLOC: u64 = 128 * 1024 * 1024;
    pub const IMAGE_DISPLAY_SIDE: u32 = 1600;
    pub const TABLE_ROWS: usize = 500;
    pub const TABLE_COLUMNS: usize = 64;
    /// Characters kept per table cell; longer cells are marked truncated.
    pub const TABLE_CELL: usize = 1024;
    /// Characters of one fact in the panel's Details.
    pub const DETAIL_CHARS: usize = 256;
    pub const ATTACHMENTS: usize = 100;
    pub const ADDRESSES: usize = 50;
    /// Font specimen text and characters per character-sheet section.
    pub const SPECIMEN_CHARS: usize = 200;
    pub const SHEET_GLYPHS: usize = 192;

    /// Whole binary units for messages: `16 MiB`, `64 KiB`, `500 bytes`.
    pub fn describe(bytes: usize) -> String {
        if bytes > 0 && bytes.is_multiple_of(1024 * 1024) {
            format!("{} MiB", bytes / (1024 * 1024))
        } else if bytes > 0 && bytes.is_multiple_of(1024) {
            format!("{} KiB", bytes / 1024)
        } else {
            format!("{bytes} bytes")
        }
    }
}

pub fn check(cancel: &CancelToken) -> anyhow::Result<()> {
    anyhow::ensure!(!cancel.is_cancelled(), "preview cancelled");
    Ok(())
}
/// A regular file opened for a preview that streams it (a table, a byte
/// search): through `file::open_regular`, so a named pipe fails instead of
/// blocking the worker.
pub fn open(path: &Path) -> anyhow::Result<std::fs::File> {
    crate::file::open_regular(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::InvalidInput {
            anyhow::anyhow!("preview needs a regular file")
        } else {
            error.into()
        }
    })
}
/// Up to `limit` bytes of a regular file from `offset`, opened by `open`.
pub fn read(
    path: &Path,
    offset: u64,
    limit: usize,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<u8>> {
    check(cancel)?;
    let mut file = open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut out = Vec::new();
    file.take(limit as u64).read_to_end(&mut out)?;
    check(cancel)?;
    Ok(out)
}
/// Read a whole file of at most `limit` bytes, or fail with `what` in the message.
pub fn read_bounded(
    path: &Path,
    limit: usize,
    what: &str,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<u8>> {
    let bytes = read(path, 0, limit + 1, cancel)?;
    anyhow::ensure!(
        bytes.len() <= limit,
        "{what} exceeds {} preview limit",
        limits::describe(limit)
    );
    Ok(bytes)
}
/// Copy at most `limit` bytes from a stream; more is an error, never a truncation.
pub fn copy_bounded(
    mut source: impl Read,
    limit: usize,
    what: &str,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<u8>> {
    check(cancel)?;
    let mut out = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let n = source.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        anyhow::ensure!(
            out.len() + n <= limit,
            "{what} exceeds {} preview limit",
            limits::describe(limit)
        );
        out.extend_from_slice(&chunk[..n]);
        check(cancel)?;
    }
    Ok(out)
}

/// How the bytes of a text were read. Anything but UTF-8 was an assumption,
/// and the preview says so (`notice`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// UTF-8, with or without a byte order mark (which is dropped).
    Utf8,
    /// UTF-16, as its byte order mark said.
    Utf16Le,
    Utf16Be,
    /// Not UTF-8 and no mark: the Western legacy code page, which reads any
    /// byte (spreadsheet exports, old playlists and subtitles are written so).
    Windows1252,
}
impl Encoding {
    pub fn name(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf16Le => "UTF-16LE",
            Self::Utf16Be => "UTF-16BE",
            Self::Windows1252 => "Windows-1252",
        }
    }
    /// Bytes read in this encoding, whatever they look like: a stream that
    /// was told its encoding once (a table's cells) reads every piece alike.
    pub fn decode(self, bytes: &[u8]) -> Cow<'_, str> {
        let encoding = match self {
            Self::Utf8 => return String::from_utf8_lossy(bytes),
            Self::Utf16Le => encoding_rs::UTF_16LE,
            Self::Utf16Be => encoding_rs::UTF_16BE,
            Self::Windows1252 => encoding_rs::WINDOWS_1252,
        };
        encoding.decode_without_bom_handling(bytes).0
    }
    /// What a preview says about the encoding, or nothing for UTF-8.
    pub fn notice(self) -> Option<String> {
        (self != Self::Utf8).then(|| format!("Read as {}", self.name()))
    }
}

/// Text from bytes: a byte order mark (UTF-8 or UTF-16) decides, else UTF-8,
/// else Windows-1252. Never fails: every byte means something in the last.
pub fn decode(bytes: &[u8]) -> (Cow<'_, str>, Encoding) {
    if let Some((encoding, _)) = encoding_rs::Encoding::for_bom(bytes) {
        let (text, _) = encoding.decode_with_bom_removal(bytes);
        let which = if encoding == encoding_rs::UTF_16LE {
            Encoding::Utf16Le
        } else if encoding == encoding_rs::UTF_16BE {
            Encoding::Utf16Be
        } else {
            Encoding::Utf8
        };
        return (text, which);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => (Cow::Borrowed(text), Encoding::Utf8),
        Err(_) => (
            encoding_rs::WINDOWS_1252
                .decode_without_bom_handling(bytes)
                .0,
            Encoding::Windows1252,
        ),
    }
}

/// A whole text file parsed by a format (Markdown, tables, structured data),
/// decoded by `decode`, with the encoding it was read in.
pub fn read_text(path: &Path, cancel: &CancelToken) -> anyhow::Result<(String, Encoding)> {
    let bytes = read(path, 0, limits::TEXT_INPUT + 1, cancel)?;
    anyhow::ensure!(
        bytes.len() <= limits::TEXT_INPUT,
        "Text preview limited to {}; use Source or Hex for ranges",
        limits::describe(limits::TEXT_INPUT)
    );
    let (text, encoding) = decode(&bytes);
    Ok((text.into_owned(), encoding))
}
/// `read_text` without the encoding.
pub fn text(path: &Path, cancel: &CancelToken) -> anyhow::Result<String> {
    read_text(path, cancel).map(|(text, _)| text)
}
/// A file's own text behind a rendered presentation, one page at a time.
pub fn source(path: &Path, offset: u64, cancel: &CancelToken) -> anyhow::Result<Inspection> {
    let mut data = lines(path, offset, limits::SOURCE_PAGE, cancel)?;
    data.title = format!("Source · byte {}", data.position);
    data.source_available = true;
    Ok(data)
}

/// Any text file that has no presentation of its own. It is the same view as
/// `source`, so every text file is read, searched and copied the same way.
pub fn plain(path: &Path, offset: u64, cancel: &CancelToken) -> anyhow::Result<Inspection> {
    let mut data = lines(path, offset, limits::TEXT_PAGE, cancel)?;
    let count = data.blocks.len();
    data.title = match (data.position, count) {
        (0, 1) => "1 line".into(),
        (0, _) => format!("{count} lines"),
        (position, _) => format!("{count} lines from byte {position}"),
    };
    if data.next.is_some() || data.position > 0 {
        let paged = format!(
            "Shown {} at a time · [ and ] move through the file",
            limits::describe(limits::TEXT_PAGE)
        );
        data.notice = if data.notice.is_empty() {
            paged
        } else {
            format!("{} · {paged}", data.notice)
        };
    }
    Ok(data)
}

/// The code view of `text`: a block per line in `language` (a file
/// extension or name, as the highlighter takes it). Text the view's bounds
/// do not hold (a minified file is one endless line) stays one block of
/// plain text. Formats that show a text of their own as source (a book's
/// chapter) use this rather than one big block.
pub fn code_lines(text: &str, language: Option<String>) -> Inspection {
    let bounded = text.lines().count() <= limits::CODE_LINES
        && text.lines().all(|line| line.len() <= limits::LINE_BYTES);
    let (kind, blocks) = if bounded {
        let blocks = text
            .lines()
            .map(|line| Block {
                text: line.to_owned(),
                code: true,
                language: language.clone(),
                ..Block::default()
            })
            .collect();
        (InspectionKind::Code, blocks)
    } else {
        let block = Block {
            text: text.to_owned(),
            code: true,
            language,
            ..Block::default()
        };
        (InspectionKind::Text, vec![block])
    };
    Inspection {
        kind,
        blocks,
        ..Inspection::default()
    }
}

/// One page of a text file as the code view wants it (`code_lines`), the
/// language taken from the name. A page starts and ends on a line where it
/// can: going back a page lands on an arbitrary byte, so a page whose start
/// is not at a line start begins after the first newline and says where
/// (`position`); a page ends after its last newline.
fn lines(
    path: &Path,
    offset: u64,
    page: usize,
    cancel: &CancelToken,
) -> anyhow::Result<Inspection> {
    // One byte before the page tells whether it starts a line.
    let before = u64::from(offset > 0);
    let bytes = read(path, offset - before, page + 1 + before as usize, cancel)?;
    let mut start = before as usize;
    if bytes
        .first()
        .is_some_and(|byte| before == 1 && *byte != b'\n')
        && let Some(newline) = bytes[1..bytes.len().min(page)]
            .iter()
            .position(|byte| *byte == b'\n')
    {
        start = newline + 2;
    } else {
        // No line to start on: at least do not start inside a character.
        while bytes.get(start).is_some_and(|byte| byte & 0xC0 == 0x80) && start < 4 {
            start += 1;
        }
    }
    let position = offset - before + start as u64;
    let bytes = &bytes[start.min(bytes.len())..];
    let limit = page - (position - offset) as usize;
    let more = bytes.len() > limit;
    let mut end = bytes.len().min(limit);
    if more && let Some(newline) = bytes[..end].iter().rposition(|byte| *byte == b'\n') {
        end = newline + 1;
    }
    let (text, encoding) = page_text(&bytes[..end], position == 0);
    let language = path
        .extension()
        .or_else(|| path.file_name())
        .map(|name| name.to_string_lossy().into_owned());
    Ok(Inspection {
        position,
        previous: (position > 0).then(|| position.saturating_sub(page as u64)),
        next: more.then_some(position + end as u64),
        notice: encoding.notice().unwrap_or_default(),
        ..code_lines(&text, language)
    })
}

/// A page's text. UTF-8 cut through its last character is still UTF-8;
/// otherwise `decode` decides, except that UTF-16 is recognised only by the
/// mark at the start of the file: a page elsewhere cannot tell.
fn page_text(bytes: &[u8], first: bool) -> (Cow<'_, str>, Encoding) {
    let marked = first && encoding_rs::Encoding::for_bom(bytes).is_some();
    if !marked && text_prefix(bytes).is_some() {
        return (String::from_utf8_lossy(bytes), Encoding::Utf8);
    }
    match decode(bytes) {
        (_, Encoding::Utf16Le | Encoding::Utf16Be) if !first => {
            (String::from_utf8_lossy(bytes), Encoding::Utf8)
        }
        decoded => decoded,
    }
}
pub fn image(bytes: &[u8]) -> anyhow::Result<Image> {
    anyhow::ensure!(
        bytes.len() <= limits::BINARY_INPUT,
        "Embedded image exceeds {}",
        limits::describe(limits::BINARY_INPUT)
    );
    if is_svg(bytes) {
        return svg(bytes);
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let mut image_limits = image::Limits::default();
    image_limits.max_image_width = Some(limits::IMAGE_SIDE);
    image_limits.max_image_height = Some(limits::IMAGE_SIDE);
    image_limits.max_alloc = Some(limits::IMAGE_ALLOC);
    reader.limits(image_limits);
    let mut img = reader.decode()?;
    // Only shrink: `thumbnail` would otherwise scale a tiny icon up to the
    // display side and hand the UI megabytes of pixels per embedded image.
    let side = limits::IMAGE_DISPLAY_SIDE;
    if img.width() > side || img.height() > side {
        img = img.thumbnail(side, side);
    }
    let img = img.into_rgba8();
    Ok(Image {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw().into(),
    })
}
/// Whether `bytes` are an SVG document rather than a raster image.
fn is_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(1024)];
    let head = String::from_utf8_lossy(head);
    let head = head.trim_start_matches('\u{feff}').trim_start();
    (head.starts_with("<?xml") || head.starts_with("<svg") || head.starts_with("<!--"))
        && head.contains("<svg")
}
/// Rasterize an SVG picture at its own size, no larger than the display
/// side. It is inert: nothing it names outside itself (a file, a URL) is
/// read, only pictures embedded in it as data.
fn svg(bytes: &[u8]) -> anyhow::Result<Image> {
    use resvg::{tiny_skia, usvg};
    let mut options = usvg::Options::default();
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    let tree = usvg::Tree::from_data(bytes, &options)?;
    let size = tree.size();
    let limit = limits::IMAGE_DISPLAY_SIDE as f32;
    let scale = (limit / size.width()).min(limit / size.height()).min(1.);
    let width = (size.width() * scale).round().max(1.) as u32;
    let height = (size.height() * scale).round().max(1.) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| anyhow::anyhow!("SVG picture has no size"))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let rgba: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect();
    Ok(Image {
        width,
        height,
        rgba: rgba.into(),
    })
}
/// One fact for the panel's Details, or nothing when the format left it
/// blank. Bounded: a title can be a whole abstract.
pub fn detail(label: &str, value: impl AsRef<str>) -> Option<bibli_core::inspection::Detail> {
    let value = value.as_ref().trim();
    (!value.is_empty()).then(|| {
        let mut text: String = value.chars().take(limits::DETAIL_CHARS).collect();
        if text.len() < value.len() {
            text.push('…');
        }
        bibli_core::inspection::Detail::new(label, text)
    })
}
/// What a rich document cut at `limits::BLOCKS` says in its notice.
pub fn blocks_notice() -> String {
    format!("First {} blocks · Source shows the rest", limits::BLOCKS)
}
/// The facts of `Inspection::details` from `(label, value)` pairs, in order,
/// leaving out what is blank or `None` (`count` makes a zero blank).
pub fn details<'a, V: Into<Option<String>>>(
    facts: impl IntoIterator<Item = (&'a str, V)>,
) -> Vec<bibli_core::inspection::Detail> {
    facts
        .into_iter()
        .filter_map(|(label, value)| detail(label, value.into()?))
        .collect()
}
/// A count worth stating: nothing for zero, where a Details row would only
/// say that something is absent.
pub fn count(n: usize) -> Option<String> {
    (n > 0).then(|| n.to_string())
}
/// A size the way the file list shows one: `512 B`, `1.5 KB`, `3.2 GB`, in
/// binary multiples.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}
pub fn extension(path: &Path) -> String {
    path.extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase()
}

/// The longest prefix of `bytes` that is valid UTF-8 text without NUL bytes,
/// so a cut through a multibyte character does not turn text into a hex dump.
pub fn text_prefix(bytes: &[u8]) -> Option<&str> {
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            // Only an incomplete trailing sequence is a cut; other errors are binary.
            if error.error_len().is_some() {
                return None;
            }
            std::str::from_utf8(&bytes[..error.valid_up_to()]).ok()?
        }
    };
    (!text.contains('\0')).then_some(text)
}

/// Bytes per hexadecimal row and the group separator used by every byte viewer.
pub const HEX_ROW: usize = 16;
const HEX_GROUP: usize = 8;

/// `00 01 … 07  08 … 0F`: pairs separated by a space, groups by two.
pub fn hex_bytes(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            out.push(' ');
            if i % HEX_GROUP == 0 {
                out.push(' ');
            }
        }
        let _ = write!(out, "{b:02X}");
    }
    out
}

/// Header cells: offset, byte positions in row layout, ASCII.
pub fn hex_columns() -> Vec<String> {
    vec![
        "Offset".into(),
        hex_bytes(&(0..HEX_ROW as u8).collect::<Vec<_>>()),
        "ASCII".into(),
    ]
}

/// One row per 16 bytes, offsets counted from `base`.
pub fn hex_rows(bytes: &[u8], base: u64) -> Vec<Row> {
    bytes
        .chunks(HEX_ROW)
        .enumerate()
        .map(|(i, chunk)| Row {
            cells: vec![
                format!("{:016X}", base + (i * HEX_ROW) as u64),
                hex_bytes(chunk),
                chunk
                    .iter()
                    .map(|b| {
                        if b.is_ascii_graphic() || *b == b' ' {
                            char::from(*b)
                        } else {
                            '.'
                        }
                    })
                    .collect(),
            ],
            ..Row::default()
        })
        .collect()
}

/// A paged hexadecimal presentation of `page` (the bytes at `offset` of a
/// `len`-byte source). `offset` must be row aligned; `position` reports where
/// the caller landed (a search match keeps its exact byte) so the next search
/// can resume after it.
pub fn hex_page(title: String, page: &[u8], len: u64, offset: u64, position: u64) -> Inspection {
    let size = limits::HEX_PAGE;
    Inspection {
        kind: InspectionKind::Hex,
        title,
        rows: hex_rows(page, offset),
        columns: hex_columns(),
        position,
        previous: (offset > 0).then(|| offset.saturating_sub(size)),
        next: (offset + size < len).then_some(offset + size),
        ..Inspection::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_rows_group_bytes_and_align_headers() {
        let bytes: Vec<u8> = (0u8..20).collect();
        let rows = hex_rows(&bytes, 4096);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].cells[0], "0000000000001000");
        assert_eq!(rows[1].cells[0], "0000000000001010");
        assert_eq!(
            rows[0].cells[1],
            "00 01 02 03 04 05 06 07  08 09 0A 0B 0C 0D 0E 0F"
        );
        assert_eq!(rows[1].cells[1], "10 11 12 13");
        assert_eq!(rows[0].cells[2], "................");
        assert_eq!(hex_columns()[1], rows[0].cells[1]);
        assert_eq!(hex_rows(b"Hi there!\0\xff", 0)[0].cells[2], "Hi there!..");
    }

    #[test]
    fn text_is_a_line_per_block_and_pages_end_on_a_line() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = CancelToken::new();
        let path = dir.path().join("it's a café.rs");
        std::fs::write(&path, "fn main() {\r\n    let x = 1;\n}\n").unwrap();
        let data = plain(&path, 0, &cancel).unwrap();
        assert_eq!(data.kind, InspectionKind::Code);
        assert_eq!(data.title, "3 lines");
        assert_eq!(data.blocks[0].text, "fn main() {");
        assert_eq!(data.blocks[1].language.as_deref(), Some("rs"));
        assert!(data.notice.is_empty() && !data.source_available);
        assert!(source(&path, 0, &cancel).unwrap().source_available);

        // A file of several pages: every page ends on a line, none is lost.
        let long = dir.path().join("Makefile");
        let line = "target: dependency # comment\n";
        std::fs::write(&long, line.repeat(limits::TEXT_PAGE / line.len() + 50)).unwrap();
        let first = plain(&long, 0, &cancel).unwrap();
        assert_eq!(first.blocks[0].language.as_deref(), Some("Makefile"));
        let next = first.next.unwrap();
        assert_eq!(next as usize % line.len(), 0, "cut between two lines");
        let second = plain(&long, next, &cancel).unwrap();
        assert_eq!(second.blocks.len(), 50);
        assert!(
            second
                .blocks
                .iter()
                .all(|block| block.text == line.trim_end())
        );
        assert!(!first.notice.is_empty());

        // One endless line is not code: it stays a block of plain text.
        let minified = dir.path().join("app.min.js");
        std::fs::write(&minified, "x".repeat(limits::LINE_BYTES + 1)).unwrap();
        let data = plain(&minified, 0, &cancel).unwrap();
        assert_eq!((data.kind, data.blocks.len()), (InspectionKind::Text, 1));
    }

    #[test]
    fn going_back_a_page_lands_on_a_line_and_meets_the_page_after() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = CancelToken::new();
        let path = dir.path().join("notes caf\u{e9}.txt");
        // Lines of varying length, with characters of several bytes, so a
        // page boundary falls in the middle of a line and of a character.
        let text = (0..60_000).fold(String::new(), |mut text, i| {
            use std::fmt::Write as _;
            let _ = writeln!(text, "{i} {}", "\u{e9}t\u{e9}".repeat(i % 7 + 3));
            text
        });
        std::fs::write(&path, &text).unwrap();
        let first = plain(&path, 0, &cancel).unwrap();
        let second = plain(&path, first.next.unwrap(), &cancel).unwrap();
        let third = plain(&path, second.next.unwrap(), &cancel).unwrap();
        let back = plain(&path, third.previous.unwrap(), &cancel).unwrap();
        let at = back.position as usize;
        assert!(at > 0 && text.as_bytes()[at - 1] == b'\n', "starts a line");
        assert_eq!(back.next, Some(third.position), "ends where it was left");
        let lines: std::collections::HashSet<&str> = text.lines().collect();
        assert_eq!(back.kind, InspectionKind::Code);
        assert!(
            back.blocks.iter().all(|b| lines.contains(b.text.as_str())),
            "every line whole"
        );
        assert!(back.notice.contains("at a time") && !back.notice.contains("Read as"));
        // A requested line start is kept as it is.
        assert_eq!(
            plain(&path, second.position, &cancel).unwrap().position,
            second.position
        );
    }

    #[test]
    fn text_is_decoded_by_its_mark_else_utf8_else_windows_1252() {
        assert_eq!(decode(b"plain"), (Cow::Borrowed("plain"), Encoding::Utf8));
        assert_eq!(
            decode("\u{feff}# T\u{ed}tulo".as_bytes()).0,
            "# T\u{ed}tulo"
        );
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("a,\u{e9}".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(
            decode(&utf16),
            (Cow::Owned("a,\u{e9}".into()), Encoding::Utf16Le)
        );
        assert_eq!(
            decode(b"caf\xe9;\x80"),
            (
                Cow::Owned("caf\u{e9};\u{20ac}".into()),
                Encoding::Windows1252
            )
        );
        assert_eq!(Encoding::Utf8.notice(), None);
        assert_eq!(
            Encoding::Windows1252.notice().as_deref(),
            Some("Read as Windows-1252")
        );

        let dir = tempfile::tempdir().unwrap();
        let cancel = CancelToken::new();
        let path = dir.path().join("export.csv");
        std::fs::write(&path, b"name;price\ncaf\xe9;3\n").unwrap();
        assert_eq!(
            read_text(&path, &cancel).unwrap(),
            ("name;price\ncaf\u{e9};3\n".into(), Encoding::Windows1252)
        );
        let data = plain(&path, 0, &cancel).unwrap();
        assert_eq!(data.blocks[1].text, "caf\u{e9};3");
        assert_eq!(data.notice, "Read as Windows-1252");
    }

    #[test]
    fn code_lines_and_details() {
        let code = code_lines("a\n\nb", Some("rs".into()));
        assert_eq!(code.kind, InspectionKind::Code);
        assert_eq!(code.blocks.len(), 3);
        assert!(code_lines("", None).blocks.is_empty());
        let facts = details([
            ("Words", count(12)),
            ("Headings", count(0)),
            ("Author", Some("  ".into())),
            ("Title", None),
        ]);
        assert_eq!(facts, [bibli_core::inspection::Detail::new("Words", "12")]);
        assert_eq!(details([("Size", size(1536))])[0].value, "1.5 KB");
        assert_eq!(size(512), "512 B");
    }

    #[cfg(unix)]
    #[test]
    fn a_named_pipe_is_refused_not_waited_on() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("x.csv");
        rustix::fs::mknodat(
            rustix::fs::CWD,
            &fifo,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(0o600),
            0,
        )
        .unwrap();
        let error = text(&fifo, &CancelToken::new()).unwrap_err();
        assert_eq!(error.to_string(), "preview needs a regular file");
    }

    #[test]
    fn hex_page_navigates_by_row_aligned_offsets() {
        let page = hex_page("Hex".into(), &[0; 4096], 10000, 4096, 4100);
        assert_eq!(page.kind, InspectionKind::Hex);
        assert_eq!(page.previous, Some(0));
        assert_eq!(page.next, Some(8192));
        assert_eq!(page.position, 4100);
        assert_eq!(page.rows.len(), 256);
        assert_eq!(hex_page("Hex".into(), &[1], 1, 0, 0).next, None);
    }

    #[test]
    fn text_prefix_keeps_a_cut_multibyte_character_out_of_hex() {
        let text = "café".repeat(3);
        let cut = &text.as_bytes()[..text.len() - 1];
        assert_eq!(text_prefix(cut), Some("cafécafécaf"));
        assert_eq!(text_prefix(b"plain"), Some("plain"));
        assert_eq!(text_prefix(b"nul\0byte"), None);
        assert_eq!(text_prefix(&[0xff, 0xfe, b'a']), None);
    }

    #[test]
    fn copy_bounded_rejects_streams_beyond_the_limit() {
        let cancel = CancelToken::new();
        let ok = copy_bounded(&b"12345"[..], 5, "Sample", &cancel).unwrap();
        assert_eq!(ok, b"12345");
        let error = copy_bounded(&b"123456"[..], 5, "Sample", &cancel).unwrap_err();
        assert!(error.to_string().contains("Sample exceeds"));
        let big = std::io::repeat(0).take(200_000);
        assert!(copy_bounded(big, 100_000, "Sample", &cancel).is_err());
        cancel.cancel();
        assert!(copy_bounded(&b"1"[..], 5, "Sample", &cancel).is_err());
    }
}
