//! Shared bounded reads and embedded image decoding, called only by workers.
use bibli_core::{
    CancelToken,
    inspection::{Block, Inspection, InspectionKind, Row},
    thumbs::Image,
};
use std::{
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
    /// Text prefix shown for a container member or attachment.
    pub const MEMBER_TEXT: usize = 64 * 1024;
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
    pub const EMBEDDED_IMAGES: usize = 8;
    /// Decoded embedded images: pixel side, decoder allocation and display side.
    pub const IMAGE_SIDE: u32 = 8192;
    pub const IMAGE_ALLOC: u64 = 128 * 1024 * 1024;
    pub const IMAGE_DISPLAY_SIDE: u32 = 1600;
    pub const TABLE_ROWS: usize = 500;
    pub const TABLE_COLUMNS: usize = 64;
    /// Characters kept per table cell; longer cells are marked truncated.
    pub const TABLE_CELL: usize = 1024;
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
pub fn read(
    path: &Path,
    offset: u64,
    limit: usize,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<u8>> {
    check(cancel)?;
    let mut file = std::fs::File::open(path)?;
    anyhow::ensure!(file.metadata()?.is_file(), "preview needs a regular file");
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
pub fn text(path: &Path, cancel: &CancelToken) -> anyhow::Result<String> {
    let bytes = read(path, 0, limits::TEXT_INPUT + 1, cancel)?;
    anyhow::ensure!(
        bytes.len() <= limits::TEXT_INPUT,
        "Text preview limited to {}; use Source or Hex for ranges",
        limits::describe(limits::TEXT_INPUT)
    );
    Ok(String::from_utf8(bytes)?)
}
/// A file's own text behind a rendered presentation, one page at a time.
pub fn source(path: &Path, offset: u64, cancel: &CancelToken) -> anyhow::Result<Inspection> {
    let mut data = lines(path, offset, limits::SOURCE_PAGE, cancel)?;
    data.title = format!("Source · byte {offset}");
    data.source_available = true;
    Ok(data)
}

/// Any text file that has no presentation of its own. It is the same view as
/// `source`, so every text file is read, searched and copied the same way.
pub fn plain(path: &Path, offset: u64, cancel: &CancelToken) -> anyhow::Result<Inspection> {
    let mut data = lines(path, offset, limits::TEXT_PAGE, cancel)?;
    let count = data.blocks.len();
    data.title = match (offset, count) {
        (0, 1) => "1 line".into(),
        (0, _) => format!("{count} lines"),
        _ => format!("{count} lines from byte {offset}"),
    };
    if data.next.is_some() || offset > 0 {
        data.notice = format!(
            "Shown {} at a time · [ and ] move through the file",
            limits::describe(limits::TEXT_PAGE)
        );
    }
    Ok(data)
}

/// One page of a text file as the code view wants it: a block per line, the
/// language taken from the name. A page ends on a line where it can. Text the
/// code view's bounds do not hold (a minified file is one endless line) stays
/// one block of plain text.
fn lines(
    path: &Path,
    offset: u64,
    page: usize,
    cancel: &CancelToken,
) -> anyhow::Result<Inspection> {
    let bytes = read(path, offset, page + 1, cancel)?;
    let more = bytes.len() > page;
    let mut end = bytes.len().min(page);
    if more && let Some(newline) = bytes[..end].iter().rposition(|byte| *byte == b'\n') {
        end = newline + 1;
    }
    let text = String::from_utf8_lossy(&bytes[..end]);
    let language = path
        .extension()
        .or_else(|| path.file_name())
        .map(|name| name.to_string_lossy().into_owned());
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
        (bibli_core::inspection::InspectionKind::Code, blocks)
    } else {
        let block = Block {
            text: text.into_owned(),
            code: true,
            language,
            ..Block::default()
        };
        (bibli_core::inspection::InspectionKind::Text, vec![block])
    };
    Ok(Inspection {
        kind,
        blocks,
        position: offset,
        previous: (offset > 0).then(|| offset.saturating_sub(page as u64)),
        next: more.then_some(offset + end as u64),
        ..Inspection::default()
    })
}
pub fn image(bytes: &[u8]) -> anyhow::Result<Image> {
    anyhow::ensure!(
        bytes.len() <= limits::BINARY_INPUT,
        "Embedded image exceeds {}",
        limits::describe(limits::BINARY_INPUT)
    );
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
