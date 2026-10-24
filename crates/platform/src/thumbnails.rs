//! The freedesktop thumbnail cache
//! (`$XDG_CACHE_HOME/thumbnails/<size>/<md5 of file URI>.png`): lookup,
//! load to pixels, and spec-compliant write-back.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use skwir_core::thumbs::Image;

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
    pub fn from_core(s: skwir_core::thumbs::ThumbSize) -> Self {
        match s {
            skwir_core::thumbs::ThumbSize::Normal => ThumbSize::Normal,
            skwir_core::thumbs::ThumbSize::Large => ThumbSize::Large,
        }
    }
}

/// The text chunk of a failure skwir wrote that stamps the tools it had.
const TOOLS_CHUNK: &str = "X-Skwir-Tools";

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
        crate::xdg::cache_home().map(|cache| Self::at(cache.join("thumbnails")))
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

    /// Whether skwir made no thumbnail of `path` as it is now (its
    /// modification time), with the tools there are now (`tools`, a stamp
    /// of them): remembered in the specification's `fail/skwir` folder,
    /// so a file nothing can draw is not tried again at every launch, and
    /// is once a tool that might draw it is installed.
    pub fn failed(&self, path: &Path, tools: &str) -> bool {
        let candidate = self.fail_dir().join(format!("{}.png", key(path)));
        let Some(mtime) = std::fs::symlink_metadata(path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        else {
            return false;
        };
        png_text_chunk(&candidate, "Thumb::MTime")
            .is_some_and(|m| m.trim().parse::<u64>().ok() == Some(mtime.as_secs()))
            && png_text_chunk(&candidate, TOOLS_CHUNK).as_deref() == Some(tools)
    }

    /// Remember that no thumbnail could be made of `path` as of `mtime`,
    /// with the tools `tools` says: a pixel, as the specification has the
    /// failures written.
    ///
    /// # Errors
    /// When it cannot be written.
    pub fn store_failed(&self, path: &Path, mtime: SystemTime, tools: &str) -> anyhow::Result<()> {
        let pixel = Image {
            width: 1,
            height: 1,
            rgba: vec![0; 4].into(),
        };
        self.write(&self.fail_dir(), path, &pixel, mtime, Some(tools))
            .map(drop)
    }

    fn fail_dir(&self) -> PathBuf {
        self.root.join("fail").join("skwir")
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
        anyhow::ensure!(
            image.width.max(image.height) <= size.pixels(),
            "image {}x{} exceeds the {} size",
            image.width,
            image.height,
            size.dir_name()
        );
        self.write(&self.root.join(size.dir_name()), path, image, mtime, None)
    }

    /// Whether `path` is one of the cache's own files, named through the
    /// cache's folder as it is given or as it resolves.
    fn holds(&self, path: &Path) -> bool {
        path.starts_with(&self.root)
            || skwir_core::os_path::canonical(&self.root).is_ok_and(|root| path.starts_with(root))
    }

    /// `image` written for `path` into `dir`, as `store` says. Never for
    /// one of the cache's own files, as the specification says: shown in a
    /// folder of the cache, each thumbnail stored there would be listed,
    /// and stored in turn, without end.
    fn write(
        &self,
        dir: &Path,
        path: &Path,
        image: &Image,
        mtime: SystemTime,
        tools: Option<&str>,
    ) -> anyhow::Result<PathBuf> {
        use anyhow::Context;
        let meta =
            std::fs::symlink_metadata(path).with_context(|| format!("stat {}", path.display()))?;
        anyhow::ensure!(meta.is_file(), "{} is not a regular file", path.display());
        anyhow::ensure!(path.is_absolute(), "{} is not absolute", path.display());
        anyhow::ensure!(
            !self.holds(path),
            "{} is a thumbnail of the cache's own",
            path.display()
        );
        let dir = dir.to_path_buf();
        std::fs::create_dir_all(&dir)?;
        set_mode(&dir, 0o700);
        let key = key(path);
        let secs = mtime.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        // Created 0600 with a unique name: never readable by others, and two
        // stores of one key do not write into each other's file.
        let staged = tempfile::Builder::new()
            .prefix(".skwir-")
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
            enc.add_text_chunk("Software".into(), "skwir".into())?;
            if let Some(tools) = tools {
                enc.add_text_chunk(TOOLS_CHUNK.into(), tools.into())?;
            }
            let mut writer = enc.write_header()?;
            writer.write_image_data(&image.rgba)?;
            writer.finish()?;
        }
        let dest = dir.join(format!("{key}.png"));
        crate::file::take_place(staged, &dest)?;
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

    // The freedesktop thumbnailers and cache (step G, ADR-035).
    #[cfg(unix)]
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
        assert_eq!(png_text_chunk(&dest, "Software").as_deref(), Some("skwir"));
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
    fn the_caches_own_files_are_never_thumbnailed() {
        let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::Builder::new()
            .prefix("thumbnails-")
            .tempdir_in(base)
            .unwrap();
        let root = tmp.path().join("it's the cache");
        let thumbs = ThumbCache::at(&root);
        let src = tmp.path().join("photo café.jpg");
        std::fs::write(&src, b"only its mtime matters").unwrap();
        let mtime = std::fs::metadata(&src).unwrap().modified().unwrap();
        let img = Image {
            width: 2,
            height: 2,
            rgba: vec![9u8; 2 * 2 * 4].into(),
        };
        let stored = thumbs.store(&src, ThumbSize::Normal, &img, mtime).unwrap();
        // Shown in the cache's folder, its thumbnail is not stored again,
        // nor a failure for it, nor through a link to the cache's folder.
        assert!(
            thumbs
                .store(&stored, ThumbSize::Normal, &img, mtime)
                .is_err()
        );
        assert!(thumbs.store_failed(&stored, mtime, "tools").is_err());
        #[cfg(unix)]
        {
            let alias = tmp.path().join("alias");
            std::os::unix::fs::symlink(&root, &alias).unwrap();
            let through = ThumbCache::at(&alias);
            let named = root.join("normal").join(stored.file_name().unwrap());
            assert!(
                through
                    .store(&named, ThumbSize::Normal, &img, mtime)
                    .is_err()
            );
        }
        let normal: Vec<_> = std::fs::read_dir(root.join("normal"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(normal, [stored]);
        assert!(!root.join("fail").exists());
    }

    #[test]
    fn a_failure_is_remembered_for_the_file_as_it_is_and_the_tools_there_are() {
        let cache = tempfile::tempdir().unwrap();
        let thumbs = ThumbCache::at(cache.path().join("thumbnails"));
        let src_dir = tempfile::tempdir().unwrap();
        let src = src_dir.path().join("broken.jpg");
        std::fs::write(&src, b"nothing draws this").unwrap();
        let mtime = std::fs::metadata(&src).unwrap().modified().unwrap();
        assert!(!thumbs.failed(&src, "tools-a"));
        thumbs.store_failed(&src, mtime, "tools-a").unwrap();
        assert!(
            cache
                .path()
                .join("thumbnails/fail/skwir")
                .join(format!("{}.png", key(&src)))
                .is_file()
        );
        assert!(thumbs.failed(&src, "tools-a"));
        assert!(
            !thumbs.failed(&src, "tools-b"),
            "a tool installed since may draw it"
        );
        assert!(
            thumbs.lookup(&src, ThumbSize::Normal).is_none(),
            "a failure is no thumbnail"
        );
        std::fs::File::options()
            .write(true)
            .open(&src)
            .unwrap()
            .set_modified(mtime + std::time::Duration::from_secs(5))
            .unwrap();
        assert!(!thumbs.failed(&src, "tools-a"), "edited since: tried again");
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
