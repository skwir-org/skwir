//! Blender files: the picture Blender keeps near the start of every file it
//! saves (its `TEST` block, what its own thumbnailer shows), the version
//! that saved it, and its scenes. A file saved compressed (gzip before
//! Blender 3.0, Zstandard since) is decoded as a stream, and only as far as
//! that picture: the blocks before it are a few scenes' names.
use crate::Found;
use skwir_core::{CancelToken, thumbs::Image};
use skwir_platform::inspection;
use std::io::Read;
use std::path::Path;

/// Bytes read before the picture: a `REND` block per scene precedes it.
const BEFORE_PICTURE: u64 = 1024 * 1024;
/// The longest side of a picture believed: Blender writes 128 or 256.
const PICTURE_SIDE: u32 = 4096;
/// Scenes named in Properties.
const SCENES: usize = 8;
/// Frames of a Zstandard stream held at once: Blender compresses with a
/// window of a few megabytes; a file that claims more is refused.
const ZSTD_WINDOW: u64 = 32 * 1024 * 1024;

/// How the file's block headers are laid out, from its header.
#[derive(Clone, Copy)]
enum Heads {
    /// 32-bit pointers: `code, len, old, sdna, nr`, 20 bytes.
    Four,
    /// 64-bit pointers: the same with an 8-byte `old`, 24 bytes.
    SmallEight,
    /// Blender 5's: `code, sdna, old, len (i64), nr (i64)`, 32 bytes.
    LargeEight,
}

impl Heads {
    fn size(self) -> usize {
        match self {
            Heads::Four => 20,
            Heads::SmallEight => 24,
            Heads::LargeEight => 32,
        }
    }

    /// A block's code and length.
    fn read(self, head: &[u8]) -> ([u8; 4], i64) {
        let code = [head[0], head[1], head[2], head[3]];
        let len = match self {
            Heads::Four | Heads::SmallEight => i64::from(i32_at(head, 4)),
            Heads::LargeEight => i64::from_le_bytes(head[16..24].try_into().unwrap_or_default()),
        };
        (code, len)
    }
}

fn i32_at(bytes: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap_or_default())
}

/// The file's content, decompressed as it is read.
fn content(path: &Path) -> anyhow::Result<Box<dyn Read>> {
    let mut file = std::io::BufReader::new(inspection::open(path)?);
    let mut magic = [0u8; 4];
    let got = read_up_to(&mut file, &mut magic)?;
    let file = std::io::Cursor::new(magic[..got].to_vec()).chain(file);
    Ok(match magic {
        [0x1f, 0x8b, ..] => Box::new(flate2::read::MultiGzDecoder::new(file)),
        [0x28, 0xb5, 0x2f, 0xfd] => Box::new(Frames::new(file)),
        _ => Box::new(file),
    })
}

/// Fill `buf` as far as the reader goes.
fn read_up_to(reader: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut got = 0;
    while got < buf.len() {
        match reader.read(&mut buf[got..])? {
            0 => break,
            n => got += n,
        }
    }
    Ok(got)
}

/// A Zstandard stream of several frames, as Blender writes one: a frame
/// per chunk, and a skippable frame (its seek table) at the end.
struct Frames<R> {
    source: R,
    decoder: ruzstd::decoding::FrameDecoder,
    /// A frame is being decoded.
    open: bool,
    /// No frame follows.
    over: bool,
}

impl<R: Read> Frames<R> {
    fn new(source: R) -> Self {
        let mut decoder = ruzstd::decoding::FrameDecoder::new();
        decoder.set_max_window_size(ZSTD_WINDOW);
        Frames {
            source,
            decoder,
            open: false,
            over: false,
        }
    }
}

impl<R: Read> Read for Frames<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use ruzstd::decoding::errors::{FrameDecoderError, ReadFrameHeaderError};
        let fail = |error: FrameDecoderError| std::io::Error::other(error.to_string());
        while !buf.is_empty() {
            if self.open {
                if self.decoder.can_collect() > 0 {
                    return self.decoder.read(buf);
                }
                if !self.decoder.is_finished() {
                    self.decoder
                        .decode_blocks(
                            &mut self.source,
                            ruzstd::decoding::BlockDecodingStrategy::UptoBytes(buf.len()),
                        )
                        .map_err(fail)?;
                    continue;
                }
                self.open = false;
            }
            if self.over {
                break;
            }
            match self.decoder.reset(&mut self.source) {
                Ok(()) => self.open = true,
                Err(FrameDecoderError::ReadFrameHeaderError(ReadFrameHeaderError::SkipFrame {
                    length,
                    ..
                })) => {
                    std::io::copy(
                        &mut (&mut self.source).take(u64::from(length)),
                        &mut std::io::sink(),
                    )?;
                }
                // Nothing more to read where a frame would begin.
                Err(FrameDecoderError::ReadFrameHeaderError(
                    ReadFrameHeaderError::MagicNumberReadError(_),
                )) => self.over = true,
                Err(error) => return Err(fail(error)),
            }
        }
        Ok(0)
    }
}

/// A scene the file lists before its picture.
struct Scene {
    name: String,
    frames: (i32, i32),
}

/// The version saved in the header (`402`, `0500`) as Blender says it.
fn version(number: u32) -> String {
    format!("{}.{}", number / 100, number % 100)
}

/// What the file says of itself: its picture (at most `side` on its longer
/// side), the version that saved it and its scenes.
pub fn read(path: &Path, side: u32, cancel: &CancelToken) -> anyhow::Result<Found> {
    let mut content = content(path)?.take(BEFORE_PICTURE + u64::from(PICTURE_SIDE).pow(2) * 4);
    let mut header = [0u8; 17];
    read_exact(&mut content, &mut header[..12])?;
    anyhow::ensure!(&header[..7] == b"BLENDER", "Not a Blender file");
    let digits = |bytes: &[u8]| -> anyhow::Result<u32> {
        let text = std::str::from_utf8(bytes)?;
        anyhow::ensure!(
            text.bytes().all(|b| b.is_ascii_digit()),
            "Unknown Blender header"
        );
        Ok(text.parse()?)
    };
    // Before Blender 5: `BLENDER`, the pointer size (`_` 4, `-` 8), the
    // byte order (`v` little, `V` big) and three digits of version. Since:
    // `BLENDER17-01v0500`, the header's length, the block format, then
    // four digits.
    let (heads, little, number) = match header[7] {
        b'_' | b'-' => (
            if header[7] == b'_' {
                Heads::Four
            } else {
                Heads::SmallEight
            },
            header[8] == b'v',
            digits(&header[9..12])?,
        ),
        _ => {
            anyhow::ensure!(
                &header[7..9] == b"17",
                "Saved by a Blender newer than this reader"
            );
            read_exact(&mut content, &mut header[12..17])?;
            anyhow::ensure!(
                &header[9..13] == b"-01v",
                "Saved by a Blender newer than this reader"
            );
            (Heads::LargeEight, true, digits(&header[13..17])?)
        }
    };
    let mut found = Found {
        title: "Blender file".into(),
        details: vec![("Saved by".into(), format!("Blender {}", version(number)))],
        ..Found::default()
    };
    // Blender refuses these too: no picture before 2.50, and none it can
    // read from a big-endian machine's file.
    if number < 250 || !little {
        return Ok(found);
    }
    let mut scenes = Vec::new();
    let mut head = vec![0u8; heads.size()];
    let mut read_before = 0u64;
    loop {
        inspection::check(cancel)?;
        if read_up_to(&mut content, &mut head)? < head.len() {
            break;
        }
        let (code, len) = heads.read(&head);
        let Ok(len) = u64::try_from(len) else { break };
        match &code {
            b"REND" => {
                read_before += len;
                if read_before > BEFORE_PICTURE {
                    break;
                }
                let mut block = vec![0u8; usize::try_from(len)?];
                read_exact(&mut content, &mut block)?;
                // `RenderInfo`: the first and last frame, then the name.
                if block.len() > 8 && scenes.len() < SCENES {
                    let name = &block[8..];
                    let name = &name[..name.iter().position(|b| *b == 0).unwrap_or(name.len())];
                    scenes.push(Scene {
                        name: String::from_utf8_lossy(name).into_owned(),
                        frames: (i32_at(&block, 0), i32_at(&block, 4)),
                    });
                }
            }
            b"TEST" => {
                found.picture = picture(&mut content, len, side)?;
                break;
            }
            // Blender writes the picture before any other block.
            _ => break,
        }
    }
    if let Some(first) = scenes.first() {
        let names: Vec<&str> = scenes.iter().map(|s| s.name.as_str()).collect();
        found.details.push((
            if scenes.len() == 1 { "Scene" } else { "Scenes" }.into(),
            names.join(", "),
        ));
        found.details.push((
            "Frames".into(),
            format!("{}–{}", first.frames.0, first.frames.1),
        ));
    }
    Ok(found)
}

fn read_exact(reader: &mut impl Read, buf: &mut [u8]) -> anyhow::Result<()> {
    anyhow::ensure!(
        read_up_to(reader, buf)? == buf.len(),
        "The Blender file ends early"
    );
    Ok(())
}

/// The `TEST` block of `len` bytes: its width and height, then its rows
/// of RGBA from the bottom up.
fn picture(content: &mut impl Read, len: u64, side: u32) -> anyhow::Result<Option<Image>> {
    let mut shape = [0u8; 8];
    read_exact(content, &mut shape)?;
    let (width, height) = (i32_at(&shape, 0), i32_at(&shape, 4));
    let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
        return Ok(None);
    };
    if width == 0 || height == 0 || width > PICTURE_SIDE || height > PICTURE_SIDE {
        return Ok(None);
    }
    let expected = u64::from(width) * u64::from(height) * 4;
    if len.checked_sub(8) != Some(expected) {
        return Ok(None);
    }
    let mut rgba = vec![0u8; usize::try_from(expected)?];
    read_exact(content, &mut rgba)?;
    let row = width as usize * 4;
    let rows: Vec<u8> = rgba.chunks_exact(row).rev().flatten().copied().collect();
    let image = image::RgbaImage::from_raw(width, height, rows)
        .ok_or_else(|| anyhow::anyhow!("The Blender file's picture is damaged"))?;
    Ok(Some(crate::fit(image, side)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A block as Blender 4 writes it with 64-bit pointers.
    fn small_block(code: [u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = code.to_vec();
        out.extend(i32::try_from(data.len()).unwrap().to_le_bytes());
        out.extend(0x7f00_1234_u64.to_le_bytes());
        out.extend(0i32.to_le_bytes());
        out.extend(1i32.to_le_bytes());
        out.extend(data);
        out
    }

    /// And as Blender 5 does.
    fn large_block(code: [u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = code.to_vec();
        out.extend(0i32.to_le_bytes());
        out.extend(0x7f00_1234_u64.to_le_bytes());
        out.extend(i64::try_from(data.len()).unwrap().to_le_bytes());
        out.extend(1i64.to_le_bytes());
        out.extend(data);
        out
    }

    fn scene(name: &str) -> Vec<u8> {
        let mut out = 1i32.to_le_bytes().to_vec();
        out.extend(250i32.to_le_bytes());
        let mut field = [0u8; 64];
        field[..name.len()].copy_from_slice(name.as_bytes());
        out.extend(field);
        out
    }

    /// A 2 × 2 picture, rows from the bottom up: red and green below,
    /// blue and white above.
    fn thumbnail() -> Vec<u8> {
        let mut out = 2i32.to_le_bytes().to_vec();
        out.extend(2i32.to_le_bytes());
        out.extend([
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ]);
        out
    }

    fn blender4() -> Vec<u8> {
        let mut file = b"BLENDER-v402".to_vec();
        file.extend(small_block(*b"REND", &scene("Scène café")));
        file.extend(small_block(*b"TEST", &thumbnail()));
        file.extend(small_block(*b"GLOB", &[0; 16]));
        file
    }

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn check(found: &Found, version: &str) {
        assert_eq!(
            found.details[0],
            ("Saved by".into(), format!("Blender {version}"))
        );
        let picture = found.picture.as_ref().expect("the stored picture");
        assert_eq!((picture.width, picture.height), (2, 2));
        assert_eq!(
            &picture.rgba[..8],
            &[0, 0, 255, 255, 255, 255, 255, 255],
            "the top row first"
        );
    }

    #[test]
    fn the_picture_and_scenes_of_a_blender_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "a scene’s file.blend", &blender4());
        let found = read(&path, 256, &CancelToken::new()).unwrap();
        check(&found, "4.2");
        assert_eq!(found.details[1], ("Scene".into(), "Scène café".into()));
        assert_eq!(found.details[2], ("Frames".into(), "1–250".into()));
    }

    #[test]
    fn a_compressed_file_is_read_as_far_as_its_picture() {
        let dir = tempfile::tempdir().unwrap();
        let plain = blender4();
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gzip.write_all(&plain).unwrap();
        let path = write(dir.path(), "gzip.blend", &gzip.finish().unwrap());
        check(&read(&path, 256, &CancelToken::new()).unwrap(), "4.2");
        // Blender's Zstandard: a frame per chunk, a seek table after.
        let (first, rest) = plain.split_at(30);
        let mut zstd = Vec::new();
        for chunk in [first, rest] {
            zstd.extend(ruzstd::encoding::compress_to_vec(
                chunk,
                ruzstd::encoding::CompressionLevel::Fastest,
            ));
        }
        zstd.extend(0x184D_2A5E_u32.to_le_bytes());
        zstd.extend(4u32.to_le_bytes());
        zstd.extend([0; 4]);
        let path = write(dir.path(), "zstd.blend1", &zstd);
        check(&read(&path, 256, &CancelToken::new()).unwrap(), "4.2");
    }

    #[test]
    fn blender_5_headers_and_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = b"BLENDER17-01v0501".to_vec();
        file.extend(large_block(*b"REND", &scene("Scene")));
        file.extend(large_block(*b"TEST", &thumbnail()));
        let path = write(dir.path(), "five.blend", &file);
        check(&read(&path, 256, &CancelToken::new()).unwrap(), "5.1");
    }

    #[test]
    fn a_picture_that_does_not_add_up_is_not_shown() {
        let dir = tempfile::tempdir().unwrap();
        let mut picture = thumbnail();
        picture.truncate(picture.len() - 4);
        let mut file = b"BLENDER-v402".to_vec();
        file.extend(small_block(*b"TEST", &picture));
        let path = write(dir.path(), "damaged.blend", &file);
        let found = read(&path, 256, &CancelToken::new()).unwrap();
        assert!(found.picture.is_none());
        assert_eq!(found.details[0].1, "Blender 4.2");
        let path = write(dir.path(), "old.blend", b"BLENDER_v249REND");
        assert!(
            read(&path, 256, &CancelToken::new())
                .unwrap()
                .picture
                .is_none()
        );
        let path = write(dir.path(), "not.blend", b"PK\x03\x04 a zip");
        assert!(read(&path, 256, &CancelToken::new()).is_err());
    }
}
