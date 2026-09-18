//! The freedesktop thumbnail cache
//! (`$XDG_CACHE_HOME/thumbnails/<size>/<md5 of file URI>.png`): lookup,
//! load to pixels, and spec-compliant write-back.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use bibli_core::thumbs::Image;

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

    /// The cache directory for a core size.
    pub fn from_core(s: bibli_core::thumbs::ThumbSize) -> Self {
        match s {
            bibli_core::thumbs::ThumbSize::Normal => ThumbSize::Normal,
            bibli_core::thumbs::ThumbSize::Large => ThumbSize::Large,
        }
    }

    /// The core size, for the two sizes the core requests.
    pub fn to_core(self) -> Option<bibli_core::thumbs::ThumbSize> {
        match self {
            ThumbSize::Normal => Some(bibli_core::thumbs::ThumbSize::Normal),
            ThumbSize::Large => Some(bibli_core::thumbs::ThumbSize::Large),
            _ => None,
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

/// `lookup` then decode: the cached thumbnail as RGBA8 pixels.
pub fn load(path: &Path, size: ThumbSize) -> Option<Image> {
    let png = lookup(path, size)?;
    let img = image::open(&png).ok()?.into_rgba8();
    Some(Image {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw().into(),
    })
}

/// Write `image` into the cache for `path` per the spec: PNG with
/// `Thumb::URI`, `Thumb::MTime`, `Thumb::Size` and `Software` text chunks,
/// written to a temp file in the size directory and renamed into place,
/// file mode 0600, directory 0700. Returns the thumbnail path.
pub fn store(
    path: &Path,
    size: ThumbSize,
    image: &Image,
    mtime: SystemTime,
) -> anyhow::Result<PathBuf> {
    use anyhow::Context;
    let meta =
        std::fs::symlink_metadata(path).with_context(|| format!("stat {}", path.display()))?;
    anyhow::ensure!(meta.is_file(), "{} is not a regular file", path.display());
    anyhow::ensure!(path.is_absolute(), "{} is not absolute", path.display());
    anyhow::ensure!(
        image.width.max(image.height) <= size.pixels(),
        "image {}x{} exceeds the {} size",
        image.width,
        image.height,
        size.dir_name()
    );
    let root = cache_dir().context("no cache directory")?;
    let dir = root.join(size.dir_name());
    std::fs::create_dir_all(&dir)?;
    set_mode(&dir, 0o700);
    let key = key(path);
    let secs = mtime.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let tmp = dir.join(format!(".bibli-{}-{key}.png", std::process::id()));
    {
        let file = std::fs::File::create(&tmp)?;
        set_mode(&tmp, 0o600);
        let w = std::io::BufWriter::new(file);
        let mut enc = png::Encoder::new(w, image.width, image.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.add_text_chunk("Thumb::URI".into(), file_uri(path))?;
        enc.add_text_chunk("Thumb::MTime".into(), secs.to_string())?;
        enc.add_text_chunk("Thumb::Size".into(), meta.len().to_string())?;
        enc.add_text_chunk("Software".into(), "bibli".into())?;
        let mut writer = enc.write_header()?;
        writer.write_image_data(&image.rgba)?;
        writer.finish()?;
    }
    let dest = dir.join(format!("{key}.png"));
    if let Err(e) = std::fs::rename(&tmp, &dest) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(dest)
}

#[cfg(unix)]
fn set_mode(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_mode(_p: &Path, _mode: u32) {}

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
    #[allow(unsafe_code)]
    fn store_then_lookup_and_load_round_trip() {
        let cache = tempfile::tempdir().unwrap();
        // SAFETY: nextest runs each test in its own process; nothing else reads the env here.
        unsafe { std::env::set_var("XDG_CACHE_HOME", cache.path()) };
        let src_dir = tempfile::tempdir().unwrap();
        let src = src_dir.path().join("photo.jpg");
        std::fs::write(&src, b"not really a jpeg, only its mtime matters").unwrap();
        let mtime = std::fs::metadata(&src).unwrap().modified().unwrap();
        let img = Image {
            width: 2,
            height: 3,
            rgba: vec![200u8; 2 * 3 * 4].into(),
        };
        let dest = store(&src, ThumbSize::Normal, &img, mtime).unwrap();
        assert_eq!(
            dest,
            cache
                .path()
                .join("thumbnails/normal")
                .join(format!("{}.png", key(&src)))
        );
        assert_eq!(
            png_text_chunk(&dest, "Thumb::URI").as_deref(),
            Some(file_uri(&src).as_str())
        );
        assert_eq!(png_text_chunk(&dest, "Software").as_deref(), Some("bibli"));
        assert_eq!(
            lookup(&src, ThumbSize::Normal).as_deref(),
            Some(dest.as_path())
        );
        let back = load(&src, ThumbSize::Normal).unwrap();
        assert_eq!((back.width, back.height), (2, 3));
        assert_eq!(&back.rgba[..4], &[200, 200, 200, 200]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        // Touching the source makes the thumbnail stale.
        std::fs::File::options()
            .write(true)
            .open(&src)
            .unwrap()
            .set_modified(mtime + std::time::Duration::from_secs(5))
            .unwrap();
        assert!(lookup(&src, ThumbSize::Normal).is_none());
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
