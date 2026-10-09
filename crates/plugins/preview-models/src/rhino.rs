//! Rhino models (`.3dm`, openNURBS): the preview Rhino stores when it
//! saves, and the application that wrote the file. A 3dm file is a
//! 32-byte header then chunks, each a type code and a length (4 bytes
//! before version 5, 8 since); the properties table near the start holds
//! the preview, a Windows bitmap whose pixels are zlib-compressed.
use crate::Found;
use bibli_core::CancelToken;
use bibli_platform::inspection::{self, limits};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const HEADER: &[u8; 24] = b"3D Geometry File Format ";
const PROPERTIES_TABLE: u32 = 0x1000_0014;
const APPLICATION: u32 = 0x2000_8024;
const COMPRESSED_PREVIEW: u32 = 0x2000_8025;
const END_OF_TABLE: u32 = 0xFFFF_FFFF;
const END_OF_FILE: u32 = 0x0000_7FFF;
/// A chunk whose length field is its value: nothing follows it.
const SHORT: u32 = 0x8000_0000;
/// Top-level chunks walked before the properties table, which Rhino
/// writes second, after a comment.
const CHUNKS_BEFORE: usize = 8;

/// A reader of chunks within bytes already read.
struct Chunks<'a> {
    bytes: &'a [u8],
    /// The length field's width: 8 bytes since version 5.
    wide: bool,
}

impl<'a> Chunks<'a> {
    /// The next chunk's type code and body (empty for a short chunk).
    fn next(&mut self) -> Option<(u32, &'a [u8])> {
        let width = if self.wide { 8 } else { 4 };
        let code = u32::from_le_bytes(self.bytes.get(..4)?.try_into().ok()?);
        let length = length(self.bytes.get(4..4 + width)?)?;
        let rest = &self.bytes[4 + width..];
        if code & SHORT != 0 {
            self.bytes = rest;
            return Some((code, &[]));
        }
        let body = rest.get(..length)?;
        self.bytes = &rest[length..];
        Some((code, body))
    }
}

/// A chunk length, 4 or 8 little-endian bytes.
fn length(bytes: &[u8]) -> Option<usize> {
    let value = match bytes.len() {
        4 => u64::from(u32::from_le_bytes(bytes.try_into().ok()?)),
        _ => u64::from_le_bytes(bytes.try_into().ok()?),
    };
    usize::try_from(value).ok()
}

/// A Rhino model.
pub fn read(path: &Path, side: u32, cancel: &CancelToken) -> anyhow::Result<Found> {
    let mut file = inspection::open(path)?;
    let mut header = [0u8; 32];
    file.read_exact(&mut header)?;
    anyhow::ensure!(header.starts_with(HEADER), "Not a Rhino model");
    let version: u32 = std::str::from_utf8(&header[24..])?.trim().parse()?;
    // Version 5 onwards is written 50, 60…
    let major = if version >= 10 { version / 10 } else { version };
    let wide = version >= 50;
    let mut found = Found {
        title: "Rhino model".into(),
        details: vec![("Format".into(), format!("3dm version {major}"))],
        ..Found::default()
    };
    let width: u64 = if wide { 8 } else { 4 };
    let mut head = vec![0u8; 4 + usize::try_from(width)?];
    for _ in 0..CHUNKS_BEFORE {
        inspection::check(cancel)?;
        if file.read_exact(&mut head).is_err() {
            break;
        }
        let code = u32::from_le_bytes(head[..4].try_into()?);
        let bytes = length(&head[4..]).ok_or_else(|| anyhow::anyhow!("Damaged Rhino model"))?;
        if code == END_OF_FILE {
            break;
        }
        if code & SHORT != 0 {
            continue;
        }
        if code != PROPERTIES_TABLE {
            file.seek(SeekFrom::Current(i64::try_from(bytes)?))?;
            continue;
        }
        anyhow::ensure!(
            bytes <= limits::BINARY_INPUT,
            "The model's properties are larger than {} MiB",
            limits::BINARY_INPUT >> 20
        );
        let mut table = vec![0u8; bytes];
        file.read_exact(&mut table)?;
        let mut chunks = Chunks {
            bytes: &table,
            wide,
        };
        while let Some((code, body)) = chunks.next() {
            match code {
                APPLICATION => {
                    if let Some(name) = application(body) {
                        found.details.insert(0, ("Made with".into(), name));
                    }
                }
                COMPRESSED_PREVIEW => found.picture = preview(body, wide, side),
                END_OF_TABLE => break,
                _ => {}
            }
        }
        break;
    }
    Ok(found)
}

/// The application's name: after a version byte, a string of UTF-16
/// units counted with their terminating zero.
fn application(body: &[u8]) -> Option<String> {
    let count = usize::try_from(u32::from_le_bytes(body.get(1..5)?.try_into().ok()?)).ok()?;
    let units: Vec<u16> = body
        .get(5..5 + count.checked_mul(2)?)?
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|&unit| unit != 0)
        .collect();
    let name = String::from_utf16(&units).ok()?;
    let name = name.trim();
    (!name.is_empty()).then(|| name.chars().take(limits::DETAIL_CHARS).collect())
}

/// The preview: a bitmap's 40-byte header written field by field, then
/// its palette (for 8 bits or fewer) and its pixels, each a buffer.
fn preview(body: &[u8], wide: bool, side: u32) -> Option<bibli_core::thumbs::Image> {
    let header = body.get(..40)?;
    let int = |at: usize| i32::from_le_bytes(header[at..at + 4].try_into().unwrap_or_default());
    let (width, height) = (int(4), int(8));
    let bits = u16::from_le_bytes([header[14], header[15]]);
    let (w, h) = (width.unsigned_abs(), height.unsigned_abs());
    // A bitmap's own depths only: `bits` is the file's, and the buffer it
    // sizes is allocated before a byte of it is read.
    if w == 0
        || h == 0
        || w > limits::IMAGE_SIDE
        || h > limits::IMAGE_SIDE
        || ![1, 4, 8, 16, 24, 32].contains(&bits)
    {
        return None;
    }
    let stride = (u64::from(w) * u64::from(bits)).div_ceil(32) * 4;
    if stride * u64::from(h) > limits::IMAGE_ALLOC {
        return None;
    }
    let pixels = usize::try_from(stride * u64::from(h)).ok()?;
    let colors = match int(32) {
        0 if bits <= 8 => 1usize << bits,
        0 => 0,
        used => usize::try_from(used).ok()?.min(256),
    };
    let mut rest = &body[40..];
    let mut dib = header.to_vec();
    if colors > 0 {
        dib.extend(buffer(&mut rest, wide, colors * 4)?);
    }
    dib.extend(buffer(&mut rest, wide, pixels)?);
    crate::cad::bitmap(&dib, side)
}

/// A buffer of at most `bound` bytes: its size, its CRC, then either the
/// bytes (method 0) or a chunk of zlib data (method 1).
fn buffer(rest: &mut &[u8], wide: bool, bound: usize) -> Option<Vec<u8>> {
    let size = usize::try_from(u32::from_le_bytes(rest.get(..4)?.try_into().ok()?)).ok()?;
    let method = *rest.get(8)?;
    *rest = &rest[9..];
    if size > bound {
        return None;
    }
    match method {
        0 => {
            let bytes = rest.get(..size)?.to_vec();
            *rest = &rest[size..];
            Some(bytes)
        }
        1 => {
            let mut chunks = Chunks { bytes: rest, wide };
            let (_, body) = chunks.next()?;
            *rest = chunks.bytes;
            // The chunk ends with its own CRC.
            let data = body.get(..body.len().checked_sub(4)?)?;
            let mut bytes = Vec::with_capacity(size);
            flate2::read::ZlibDecoder::new(data)
                .take(size as u64)
                .read_to_end(&mut bytes)
                .ok()?;
            (bytes.len() == size).then_some(bytes)
        }
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A chunk with an 8-byte length.
    fn chunk(code: u32, body: &[u8]) -> Vec<u8> {
        let mut out = code.to_le_bytes().to_vec();
        out.extend((body.len() as u64).to_le_bytes());
        out.extend(body);
        out
    }

    /// A version 5 model whose preview is `width` × `height` pixels, the
    /// top row red and the rest blue, written as Rhino writes it.
    pub(crate) fn model(width: u32, height: u32) -> Vec<u8> {
        use std::io::Write as _;
        let stride = (width * 3).div_ceil(4) * 4;
        let mut pixels = Vec::new();
        // Bottom-up rows of blue, green, red.
        for row in 0..height {
            let color = if row == height - 1 {
                [0, 0, 255]
            } else {
                [255, 0, 0]
            };
            let mut line: Vec<u8> = (0..width).flat_map(|_| color).collect();
            line.resize(stride as usize, 0);
            pixels.extend(line);
        }
        let mut deflated = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        deflated.write_all(&pixels).unwrap();
        let mut data = deflated.finish().unwrap();
        data.extend([0; 4]);
        let mut preview = Vec::new();
        for field in [40, width, height] {
            preview.extend(field.to_le_bytes());
        }
        preview.extend(1u16.to_le_bytes());
        preview.extend(24u16.to_le_bytes());
        preview.extend(0u32.to_le_bytes());
        preview.extend((pixels.len() as u32).to_le_bytes());
        preview.extend([0; 16]);
        preview.extend((pixels.len() as u32).to_le_bytes());
        preview.extend([0; 4]);
        preview.push(1);
        preview.extend(chunk(0x4000_8000, &data));
        let mut application = vec![0x10];
        let name: Vec<u16> = "Rhinoceros 5.0\0".encode_utf16().collect();
        application.extend((name.len() as u32).to_le_bytes());
        application.extend(name.iter().flat_map(|u| u.to_le_bytes()));
        let mut table = Vec::new();
        table.extend(0xA000_0026u32.to_le_bytes());
        table.extend(201_502_035u64.to_le_bytes());
        table.extend(chunk(COMPRESSED_PREVIEW, &preview));
        table.extend(chunk(APPLICATION, &application));
        table.extend(chunk(END_OF_TABLE, &[]));
        let mut out = b"3D Geometry File Format       50".to_vec();
        out.extend(chunk(1, b"A comment"));
        out.extend(chunk(PROPERTIES_TABLE, &table));
        out.extend(chunk(END_OF_FILE, &[0; 8]));
        out
    }

    #[test]
    fn the_stored_preview_and_its_application() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a part’s model.3dm");
        std::fs::write(&path, model(5, 3)).unwrap();
        let found = read(&path, 1600, &CancelToken::new()).unwrap();
        let picture = found.picture.as_ref().expect("the preview");
        assert_eq!((picture.width, picture.height), (5, 3));
        // Rows run bottom-up: the last one written is the top.
        assert_eq!(&picture.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&picture.rgba[picture.rgba.len() - 4..], &[0, 0, 255, 255]);
        assert_eq!(
            found.details,
            [
                ("Made with".to_owned(), "Rhinoceros 5.0".to_owned()),
                ("Format".to_owned(), "3dm version 5".to_owned())
            ]
        );
    }

    #[test]
    fn damage_is_refused_without_a_picture() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cut.3dm");
        let mut bytes = model(5, 3);
        bytes.truncate(bytes.len() / 2);
        std::fs::write(&path, &bytes).unwrap();
        // The properties table ends past the file: nothing is read.
        assert!(read(&path, 1600, &CancelToken::new()).is_err());
        std::fs::write(&path, b"3D Geometry File Format  nothing").unwrap();
        assert!(read(&path, 1600, &CancelToken::new()).is_err());
        std::fs::write(&path, b"not a model at all, not a model at all").unwrap();
        assert!(read(&path, 1600, &CancelToken::new()).is_err());
    }
}
