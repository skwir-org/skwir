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

/// The spec's cache key for an absolute path: the MD5 of its URI, escaped
/// as `GLib` escapes it (`crate::uri::file_uri`).
pub fn key(path: &Path) -> String {
    format!("{:x}", md5::compute(crate::uri::file_uri(path)))
}

/// A thumbnail cache directory: the desktop's, shared with every other
/// application, or an explicit one for tests and the bench, so nobody has
/// to rewrite `XDG_CACHE_HOME` in a running process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThumbCache {
    root: PathBuf,
}

impl ThumbCache {
    /// `$XDG_CACHE_HOME/thumbnails`.
    pub fn desktop() -> Option<Self> {
        dirs::cache_dir().map(|cache| Self::at(cache.join("thumbnails")))
    }

    /// A cache rooted at `root`, which holds the `normal`, `large`, …
    /// directories.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Find a fresh cached thumbnail for `path`, at `size` or larger.
    /// `path` must be absolute. Stale (mtime mismatch) thumbnails are ignored.
    pub fn lookup(&self, path: &Path, size: ThumbSize) -> Option<PathBuf> {
        let key = key(path);
        let mtime = std::fs::symlink_metadata(path)
            .ok()?
            .modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_secs();
        ThumbSize::ALL.iter().filter(|s| **s >= size).find_map(|s| {
            let candidate = self.root.join(s.dir_name()).join(format!("{key}.png"));
            let thumb_mtime = png_text_chunk(&candidate, "Thumb::MTime")?;
            (thumb_mtime.trim().parse::<u64>().ok() == Some(mtime)).then_some(candidate)
        })
    }

    /// `lookup` then decode: the cached thumbnail as RGBA8 pixels, at most
    /// `size` on its longest side. A larger size class serves a smaller
    /// request (a desktop on a high-density screen writes `x-large`), shrunk here so
    /// callers never hold 16 times the pixels they asked for.
    pub fn load(&self, path: &Path, size: ThumbSize) -> Option<Image> {
        let png = self.lookup(path, size)?;
        let file = crate::file::open_regular(&png).ok()?;
        let img =
            image::ImageReader::with_format(std::io::BufReader::new(file), image::ImageFormat::Png)
                .decode()
                .ok()?;
        let px = size.pixels();
        let img = if img.width().max(img.height()) > px {
            img.thumbnail(px, px)
        } else {
            img
        };
        let img = img.into_rgba8();
        Some(Image {
            width: img.width(),
            height: img.height(),
            rgba: img.into_raw().into(),
        })
    }

    /// Write `image` into the cache for `path` per the spec: PNG with
    /// `Thumb::URI`, `Thumb::MTime`, `Thumb::Size` and `Software` text chunks,
    /// written to a private temp file in the size directory and renamed into
    /// place, file mode 0600, directory 0700. Returns the thumbnail path.
    pub fn store(
        &self,
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
        let dir = self.root.join(size.dir_name());
        std::fs::create_dir_all(&dir)?;
        set_mode(&dir, 0o700);
        let key = key(path);
        let secs = mtime.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        // Created 0600 with a unique name: never readable by others, and two
        // stores of one key do not write into each other's file.
        let staged = tempfile::Builder::new()
            .prefix(".bibli-")
            .suffix(".png")
            .tempfile_in(&dir)?;
        {
            let w = std::io::BufWriter::new(staged.as_file());
            let mut enc = png::Encoder::new(w, image.width, image.height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.add_text_chunk("Thumb::URI".into(), crate::uri::file_uri(path))?;
            enc.add_text_chunk("Thumb::MTime".into(), secs.to_string())?;
            enc.add_text_chunk("Thumb::Size".into(), meta.len().to_string())?;
            enc.add_text_chunk("Software".into(), "bibli".into())?;
            let mut writer = enc.write_header()?;
            writer.write_image_data(&image.rgba)?;
            writer.finish()?;
        }
        let dest = dir.join(format!("{key}.png"));
        staged.persist(&dest).map_err(|e| e.error)?;
        Ok(dest)
    }
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
    let mut f = crate::file::open_regular(png).ok()?;
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
        assert_eq!(crate::uri::file_uri(p), "file:///home/jens/photos/me.png");
        assert_eq!(key(p), "c6ee772d9e49320e97ec29a7eb5b1697");
    }

    #[test]
    fn store_then_lookup_and_load_round_trip() {
        let cache = tempfile::tempdir().unwrap();
        let thumbs = ThumbCache::at(cache.path().join("thumbnails"));
        let src_dir = tempfile::tempdir().unwrap();
        let src = src_dir.path().join("photo.jpg");
        std::fs::write(&src, b"not really a jpeg, only its mtime matters").unwrap();
        let mtime = std::fs::metadata(&src).unwrap().modified().unwrap();
        let img = Image {
            width: 2,
            height: 3,
            rgba: vec![200u8; 2 * 3 * 4].into(),
        };
        let dest = thumbs.store(&src, ThumbSize::Normal, &img, mtime).unwrap();
        assert_eq!(
            dest,
            cache
                .path()
                .join("thumbnails/normal")
                .join(format!("{}.png", key(&src)))
        );
        assert_eq!(
            png_text_chunk(&dest, "Thumb::URI").as_deref(),
            Some(crate::uri::file_uri(&src).as_str())
        );
        assert_eq!(png_text_chunk(&dest, "Software").as_deref(), Some("bibli"));
        assert_eq!(
            thumbs.lookup(&src, ThumbSize::Normal).as_deref(),
            Some(dest.as_path())
        );
        let back = thumbs.load(&src, ThumbSize::Normal).unwrap();
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
        // A larger size class serves a smaller request, shrunk to it.
        let large = Image {
            width: 256,
            height: 128,
            rgba: vec![7u8; 256 * 128 * 4].into(),
        };
        std::fs::remove_file(&dest).unwrap();
        thumbs.store(&src, ThumbSize::Large, &large, mtime).unwrap();
        let back = thumbs.load(&src, ThumbSize::Normal).unwrap();
        assert_eq!((back.width, back.height), (128, 64));
        // Touching the source makes the thumbnail stale.
        std::fs::File::options()
            .write(true)
            .open(&src)
            .unwrap()
            .set_modified(mtime + std::time::Duration::from_secs(5))
            .unwrap();
        assert!(thumbs.lookup(&src, ThumbSize::Normal).is_none());
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
