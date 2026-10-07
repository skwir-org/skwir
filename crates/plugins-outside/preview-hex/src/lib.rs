//! The hexadecimal preview, again, as a WebAssembly module (ADR-046,
//! Phase 6's gate): what the built-in `preview-hex` shows, page for page
//! and match for match, its bytes read through bibli. A page is 4 KiB in
//! rows of 16 bytes; a search goes on from `offset` and puts the row of
//! its match on top.

use bibli_plugin_api::guest::{Host, Plugin};
use bibli_plugin_api::protocol::{InspectParams, Inspection, InspectionKind, Row};

const ROW: u64 = 16;
const GROUP: usize = 8;
const PAGE: u64 = 4096;
const NEEDLE: usize = 4096;
const CHUNK: usize = 64 * 1024;

#[derive(Default)]
pub struct HexView;

impl Plugin for HexView {
    fn inspect(
        &self,
        host: &dyn Host,
        params: InspectParams,
    ) -> Result<Option<Inspection>, String> {
        let file = &params.file;
        let len = file.size.ok_or("the file's size is not known")?;
        let mut position = params.offset.min(len.saturating_sub(1)) / ROW * ROW;
        let mut notice = String::new();
        if let Some(query) = &params.query {
            let needle = needle(query)?;
            if let Some(found) = find(host, file.handle, &needle, params.offset)? {
                position = found;
                notice = format!("Match at 0x{found:X}");
            } else {
                notice = "No further match".into();
            }
        }
        let offset = position / ROW * ROW;
        let bytes = read(host, file.handle, offset, PAGE)?;
        Ok(Some(Inspection {
            kind: InspectionKind::Hex,
            title: format!("Hex · {}", size(len)),
            rows: rows(&bytes, offset),
            columns: columns(),
            position,
            previous: (offset > 0).then(|| offset.saturating_sub(PAGE)),
            next: (offset + PAGE < len).then_some(offset + PAGE),
            notice,
            ..Inspection::default()
        }))
    }
}

/// Up to `length` bytes from `offset`, in reads bibli bounds.
fn read(host: &dyn Host, file: u32, offset: u64, length: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    while (bytes.len() as u64) < length {
        let want = u32::try_from(length - bytes.len() as u64).unwrap_or(u32::MAX);
        let got = host.read(file, offset + bytes.len() as u64, want)?;
        if got.is_empty() {
            break;
        }
        bytes.extend(got);
    }
    Ok(bytes)
}

/// `hex: 6e 65` byte pairs, or the query's UTF-8 bytes.
pub fn needle(query: &str) -> Result<Vec<u8>, String> {
    let needle = if let Some(hex) = query.strip_prefix("hex:") {
        let hex: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
        if !hex.is_ascii() || !hex.len().is_multiple_of(2) {
            return Err("Use pairs of hexadecimal digits".into());
        }
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    } else {
        query.as_bytes().to_vec()
    };
    if needle.is_empty() || needle.len() > NEEDLE {
        return Err(format!("Search must contain 1–{NEEDLE} bytes"));
    }
    Ok(needle)
}

/// Where `needle` is first found from `offset`, read a chunk at a time
/// with the needle's length less one kept across chunks.
fn find(host: &dyn Host, file: u32, needle: &[u8], offset: u64) -> Result<Option<u64>, String> {
    let mut kept: Vec<u8> = Vec::new();
    let mut base = offset;
    let mut at = offset;
    loop {
        let chunk = host.read(file, at, CHUNK as u32)?;
        if chunk.is_empty() {
            return Ok(None);
        }
        at += chunk.len() as u64;
        kept.extend_from_slice(&chunk);
        if let Some(i) = kept.windows(needle.len()).position(|w| w == needle) {
            return Ok(Some(base + i as u64));
        }
        let keep = (needle.len() - 1).min(kept.len());
        base += (kept.len() - keep) as u64;
        kept.drain(..kept.len() - keep);
    }
}

/// `00 01 … 07  08 … 0F`: pairs separated by a space, groups by two.
fn hex_bytes(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            out.push(' ');
            if i % GROUP == 0 {
                out.push(' ');
            }
        }
        let _ = write!(out, "{b:02X}");
    }
    out
}

fn columns() -> Vec<String> {
    vec![
        "Offset".into(),
        hex_bytes(&(0..ROW as u8).collect::<Vec<_>>()),
        "ASCII".into(),
    ]
}

fn rows(bytes: &[u8], base: u64) -> Vec<Row> {
    bytes
        .chunks(ROW as usize)
        .enumerate()
        .map(|(i, chunk)| Row {
            cells: vec![
                format!("{:016X}", base + i as u64 * ROW),
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

/// `512 B`, `1.5 KB`: a size as the preview's title says it.
#[allow(clippy::cast_precision_loss)] // A size shown to one decimal.
fn size(bytes: u64) -> String {
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

bibli_plugin_api::export!(HexView);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needles_parse_hex_pairs_and_reject_empty_or_odd_input() {
        assert_eq!(needle("hex: 6e 65 ").unwrap(), b"ne");
        assert_eq!(needle("né").unwrap(), "né".as_bytes());
        assert!(needle("hex:6").is_err());
        assert!(needle("hex:zz").is_err());
        assert!(needle("").is_err());
        assert!(needle(&"a".repeat(NEEDLE + 1)).is_err());
    }

    #[test]
    fn sizes_and_rows_read_as_the_preview_says_them() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(1536), "1.5 KB");
        let rows = rows(b"Hello, bytes!\x00\x01\x02more", 32);
        assert_eq!(rows[0].cells[0], "0000000000000020");
        assert_eq!(rows[0].cells[2], "Hello, bytes!...");
        assert_eq!(rows[1].cells[1], "6D 6F 72 65");
    }
}
