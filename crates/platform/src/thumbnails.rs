//! Read-only lookup in the freedesktop thumbnail cache
//! (`$XDG_CACHE_HOME/thumbnails/<size>/<md5 of file URI>.png`).

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Thumbnail size classes of the spec.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThumbSize {
    /// 128 px
    Normal,
    /// 256 px
    Large,
    /// 512 px
    XLarge,
    /// 1024 px
    XXLarge,
}

impl ThumbSize {
    pub const ALL: [ThumbSize; 4] = [
        ThumbSize::Normal,
        ThumbSize::Large,
        ThumbSize::XLarge,
        ThumbSize::XXLarge,
    ];

    pub fn dir_name(self) -> &'static str {
        match self {
            ThumbSize::Normal => "normal",
            ThumbSize::Large => "large",
            ThumbSize::XLarge => "x-large",
            ThumbSize::XXLarge => "xx-large",
        }
    }

    pub fn pixels(self) -> u32 {
        match self {
            ThumbSize::Normal => 128,
            ThumbSize::Large => 256,
            ThumbSize::XLarge => 512,
            ThumbSize::XXLarge => 1024,
        }
    }
}

/// Percent-encode a path for a `file://` URI as the spec (and `GLib`) does:
/// unreserved characters and `/` pass through, everything else is `%XX`.
pub fn file_uri(path: &Path) -> String {
    use std::fmt::Write;
    let bytes = os_bytes(path.as_os_str());
    let mut s = String::with_capacity(bytes.len() + 7);
    s.push_str("file://");
    for &b in bytes {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                s.push(b as char);
            }
            _ => {
                let _ = write!(s, "%{b:02X}");
            }
        }
    }
    s
}

#[cfg(unix)]
fn os_bytes(s: &std::ffi::OsStr) -> &[u8] {
    std::os::unix::ffi::OsStrExt::as_bytes(s)
}

#[cfg(not(unix))]
fn os_bytes(s: &std::ffi::OsStr) -> &[u8] {
    // Lossy on non-Unix; thumbnails are looked up by the platform crate there anyway.
    s.to_str().map_or(&[], str::as_bytes)
}

/// The spec's cache key for an absolute path.
pub fn key(path: &Path) -> String {
    format!("{:x}", md5::compute(file_uri(path)))
}

/// Root of the thumbnail cache.
pub fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|c| c.join("thumbnails"))
}

/// Find a fresh cached thumbnail for `path`, at `size` or larger.
/// `path` must be absolute. Stale (mtime mismatch) thumbnails are ignored.
pub fn lookup(path: &Path, size: ThumbSize) -> Option<PathBuf> {
    let root = cache_dir()?;
    let key = key(path);
    let mtime = std::fs::symlink_metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs();
    ThumbSize::ALL.iter().filter(|s| **s >= size).find_map(|s| {
        let candidate = root.join(s.dir_name()).join(format!("{key}.png"));
        let thumb_mtime = png_text_chunk(&candidate, "Thumb::MTime")?;
        (thumb_mtime.trim().parse::<u64>().ok() == Some(mtime)).then_some(candidate)
    })
}

/// Read one `tEXt` chunk's value from a PNG without decoding the image.
/// Only the chunk headers up to `IDAT` are visited.
pub fn png_text_chunk(png: &Path, keyword: &str) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(png).ok()?;
    let mut sig = [0u8; 8];
    f.read_exact(&mut sig).ok()?;
    if sig != [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        return None;
    }
    let mut header = [0u8; 8];
    loop {
        f.read_exact(&mut header).ok()?;
        let len = u64::from(u32::from_be_bytes([
            header[0], header[1], header[2], header[3],
        ]));
        let kind = &header[4..8];
        if kind == b"IDAT" || kind == b"IEND" {
            return None;
        }
        if kind == b"tEXt" && len < 64 * 1024 {
            let mut data = vec![0u8; len as usize];
            f.read_exact(&mut data).ok()?;
            f.seek(SeekFrom::Current(4)).ok()?; // CRC
            if let Some(nul) = data.iter().position(|b| *b == 0)
                && &data[..nul] == keyword.as_bytes()
            {
                return Some(String::from_utf8_lossy(&data[nul + 1..]).into_owned());
            }
            continue;
        }
        f.seek(SeekFrom::Current(i64::try_from(len).ok()? + 4))
            .ok()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_example_hash() {
        // From the freedesktop thumbnail spec, "Thumbnail Filename".
        let p = Path::new("/home/jens/photos/me.png");
        assert_eq!(file_uri(p), "file:///home/jens/photos/me.png");
        assert_eq!(key(p), "c6ee772d9e49320e97ec29a7eb5b1697");
    }

    #[test]
    fn uri_encoding_escapes_spaces_and_unicode() {
        let p = Path::new("/tmp/a b/é.png");
        assert_eq!(file_uri(p), "file:///tmp/a%20b/%C3%A9.png");
    }

    #[test]
    fn text_chunk_walker() {
        // Minimal PNG: signature, a tEXt chunk, IEND.
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let text = b"Thumb::MTime\x001700000000";
        png.extend_from_slice(&(text.len() as u32).to_be_bytes());
        png.extend_from_slice(b"tEXt");
        png.extend_from_slice(text);
        png.extend_from_slice(&[0, 0, 0, 0]);
        png.extend_from_slice(&0u32.to_be_bytes());
        png.extend_from_slice(b"IEND");
        png.extend_from_slice(&[0, 0, 0, 0]);
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("t.png");
        std::fs::write(&f, &png).unwrap();
        assert_eq!(
            png_text_chunk(&f, "Thumb::MTime").as_deref(),
            Some("1700000000")
        );
        assert_eq!(png_text_chunk(&f, "Thumb::URI"), None);
    }
}
