//! The freedesktop thumbnail cache
//! (`$XDG_CACHE_HOME/thumbnails/<size>/<md5 of file URI>.png`): lookup,
//! load to pixels, and spec-compliant write-back.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
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
        self.find(path, size).map(|(png, _)| png)
    }

    /// [`ThumbCache::lookup`], the thumbnail found held open: its text read
    /// through the reader, which is at its start again.
    fn find(&self, path: &Path, size: ThumbSize) -> Option<(PathBuf, BufReader<File>)> {
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
            let mut png = open_png(&candidate)?;
            let [thumb_mtime] =
                <[_; 1]>::try_from(png_text_chunks(&mut png, &["Thumb::MTime"])).ok()?;
            (thumb_mtime?.trim().parse::<u64>().ok() == Some(mtime)).then_some(())?;
            png.seek(SeekFrom::Start(0)).ok()?;
            Some((candidate, png))
        })
    }

    /// `lookup` then decode: the cached thumbnail as RGBA8 pixels, at most
    /// `size` on its longest side. A larger size class serves a smaller
    /// request (a desktop on a high-density screen writes `x-large`), shrunk here so
    /// callers never hold 16 times the pixels they asked for. The file is
    /// opened once: its time read, then its pixels, through one buffer.
    pub fn load(&self, path: &Path, size: ThumbSize) -> Option<Image> {
        let (_, png) = self.find(path, size)?;
        let img = image::ImageReader::with_format(png, image::ImageFormat::Png)
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
        let Some(mut png) = open_png(&candidate) else {
            return false;
        };
        match png_text_chunks(&mut png, &["Thumb::MTime", TOOLS_CHUNK]).as_slice() {
            [Some(then), Some(stamp)] => {
                then.trim().parse::<u64>().ok() == Some(mtime.as_secs()) && stamp == tools
            }
            _ => false,
        }
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
        let mut chunks = vec![
            ("Thumb::URI", crate::uri::file_uri(path)),
            ("Thumb::MTime", secs.to_string()),
            ("Thumb::Size", meta.len().to_string()),
            ("Software", "skwir".to_owned()),
        ];
        if let Some(tools) = tools {
            chunks.push((TOOLS_CHUNK, tools.to_owned()));
        }
        std::io::Write::write_all(&mut staged.as_file(), &encoded(image, &chunks)?)?;
        let dest = dir.join(format!("{key}.png"));
        crate::file::take_place(staged, &dest)?;
        Ok(dest)
    }
}

/// `image` as the PNG the cache keeps, `chunks` its text: fdeflate's
/// (`Compression::Fast`), and zlib's second level's where it may be
/// smaller, the smaller of the two. fdeflate codes a photo as small as
/// zlib's default does, in a fiftieth of its time, but finds no repeat
/// farther than a run: a document's page, a drawing, a screenshot, a photo
/// in grey came out up to twice as large. zlib's second level finds those
/// in a millisecond or so, and only those repeat the differences between
/// their pixels ([`repeats`]): a photo's thumbnail is encoded once.
/// Thumbnails of 256 px, fdeflate's, zlib's second level's and zlib's
/// default, release: a photo's 88, 96 and 91 KB in 0.3, 2 and 11 ms; a
/// text page's 58, 30 and 28 KB in 0.3, 0.8 and 4 ms.
fn encoded(image: &Image, chunks: &[(&str, String)]) -> anyhow::Result<Vec<u8>> {
    let fast = encode(image, chunks, png::DeflateCompression::FdeflateUltraFast)?;
    if !repeats(&image.rgba) {
        return Ok(fast);
    }
    let matched = encode(image, chunks, png::DeflateCompression::Level(2))?;
    Ok(if matched.len() < fast.len() {
        matched
    } else {
        fast
    })
}

/// `image` as a PNG, `chunks` its text, its rows filtered as each fits
/// best, compressed by `deflate`.
fn encode(
    image: &Image,
    chunks: &[(&str, String)],
    deflate: png::DeflateCompression,
) -> anyhow::Result<Vec<u8>> {
    let mut png = Vec::new();
    let mut enc = png::Encoder::new(&mut png, image.width, image.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_filter(png::Filter::Adaptive);
    enc.set_deflate_compression(deflate);
    for (keyword, text) in chunks {
        enc.add_text_chunk((*keyword).to_owned(), text.clone())?;
    }
    let mut writer = enc.write_header()?;
    writer.write_image_data(&image.rgba)?;
    writer.finish()?;
    Ok(png)
}

/// Whether the differences between the picture's neighbouring pixels
/// repeat, so that zlib's matches may code it smaller than fdeflate's runs:
/// a PNG's filters code those differences. Told from 1024 pixels spread
/// over it and each one's left neighbour, at most half of them apart, in a
/// few hundredths of a millisecond. A photo's are its grain, mostly apart
/// (651 to 1020 of 1024 at 256 px), and zlib's second level writes it 9 to
/// 11 % larger in five times fdeflate's time; a page, a diagram, a
/// drawing, a gradient, a screenshot, an icon, a photo in grey or on a
/// plain ground repeat theirs (3 to 285 apart), and zlib writes all but
/// the last up to half as large.
fn repeats(rgba: &[u8]) -> bool {
    const SAMPLES: usize = 1024;
    // A prime step, so that the pixels taken do not line up in columns.
    const STEP: usize = 7919;
    let pixels = rgba.len() / 4;
    if pixels < 2 {
        return true;
    }
    let taken = SAMPLES.min(pixels - 1);
    let mut differences: Vec<[u8; 4]> = (0..taken)
        .map(|k| {
            let at = (1 + k * STEP % (pixels - 1)) * 4;
            std::array::from_fn(|c| rgba[at + c].wrapping_sub(rgba[at - 4 + c]))
        })
        .collect();
    differences.sort_unstable();
    differences.dedup();
    differences.len() * 2 <= taken
}

#[cfg(unix)]
fn set_mode(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_mode(_p: &Path, _mode: u32) {}

/// How many cached thumbnails were opened, for a test to count.
#[cfg(test)]
static OPENED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The PNG at `path` opened to be read through a buffer, if it is a file.
fn open_png(path: &Path) -> Option<BufReader<File>> {
    #[cfg(test)]
    OPENED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    crate::file::open_regular(path).ok().map(BufReader::new)
}

/// Read one `tEXt` chunk's value from a PNG without decoding the image
/// ([`png_text_chunks`]).
pub fn png_text_chunk(png: &Path, keyword: &str) -> Option<String> {
    png_text_chunks(&mut open_png(png)?, &[keyword])
        .pop()
        .flatten()
}

/// The values of the `tEXt` chunks named `keys`, in their order (`None`
/// for one the PNG holds none of), read from the PNG `reader` is at the
/// start of, without decoding the image: only the chunks before `IDAT`
/// are visited, through the reader's buffer, and the reading stops once
/// every key is found.
pub fn png_text_chunks<R: Read + Seek>(
    reader: &mut BufReader<R>,
    keys: &[&str],
) -> Vec<Option<String>> {
    let mut found: Vec<Option<String>> = vec![None; keys.len()];
    let _ = read_text_chunks(reader, keys, &mut found);
    found
}

/// [`png_text_chunks`] into `found`; `None` once the PNG ends or cannot
/// be read further.
fn read_text_chunks<R: Read + Seek>(
    reader: &mut BufReader<R>,
    keys: &[&str],
    found: &mut [Option<String>],
) -> Option<()> {
    let mut sig = [0u8; 8];
    reader.read_exact(&mut sig).ok()?;
    if sig != [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        return None;
    }
    let mut header = [0u8; 8];
    while found.iter().any(Option::is_none) {
        reader.read_exact(&mut header).ok()?;
        let len = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let kind = &header[4..8];
        if kind == b"IDAT" || kind == b"IEND" {
            return None;
        }
        if kind == b"tEXt" && len < 64 * 1024 {
            let mut data = vec![0u8; usize::try_from(len).ok()?];
            reader.read_exact(&mut data).ok()?;
            reader.seek_relative(4).ok()?; // CRC
            if let Some(nul) = data.iter().position(|b| *b == 0)
                && let Some(at) = keys.iter().position(|k| k.as_bytes() == &data[..nul])
                && found[at].is_none()
            {
                found[at] = Some(String::from_utf8_lossy(&data[nul + 1..]).into_owned());
            }
            continue;
        }
        reader.seek_relative(i64::from(len) + 4).ok()?;
    }
    Some(())
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

    /// A picture of repeats far apart (a page's lines of text) is kept
    /// far smaller than fdeflate alone writes it, and one of noise (a
    /// photo's grain) no larger.
    #[test]
    fn a_thumbnail_is_kept_in_the_smaller_of_two_encodings() {
        let cache = tempfile::tempdir().unwrap();
        let thumbs = ThumbCache::at(cache.path().join("thumbnails"));
        let src_dir = tempfile::tempdir().unwrap();
        let src = src_dir.path().join("page.pdf");
        std::fs::write(&src, b"only its mtime matters").unwrap();
        let mtime = std::fs::metadata(&src).unwrap().modified().unwrap();
        let (width, height) = (181u32, 256u32);
        // Words of a few glyphs, each a 4 x 6 cell of a dozen, in lines
        // 8 pixels apart; and noise, from a fixed seed.
        let mut seed = 0x9e37_79b9_u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let glyphs: Vec<u32> = (0..12).map(|_| next() & 0x00ff_ffff).collect();
        let cells = width.div_ceil(4);
        let words: Vec<usize> = (0..cells * height.div_ceil(8))
            .map(|_| next() as usize % 13)
            .collect();
        let page: Vec<u8> = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .flat_map(|(x, y)| {
                let (cell, row) = ((y / 8 * cells + x / 4) as usize, y % 8);
                // The thirteenth is a space between words.
                let ink = words[cell] < 12
                    && row < 6
                    && (glyphs[words[cell]] >> (row * 4 + x % 4)) & 1 == 1;
                if ink {
                    [40, 40, 40, 255]
                } else {
                    [255, 255, 255, 255]
                }
            })
            .collect();
        let noise: Vec<u8> = (0..width * height)
            .flat_map(|_| {
                let [r, g, b, _] = next().to_le_bytes();
                [r, g, b, 255]
            })
            .collect();
        let fast = |rgba: &[u8]| {
            let mut png = Vec::new();
            let mut enc = png::Encoder::new(&mut png, width, height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_compression(png::Compression::Fast);
            let mut writer = enc.write_header().unwrap();
            writer.write_image_data(rgba).unwrap();
            writer.finish().unwrap();
            png.len() as u64
        };
        let kept = |rgba: Vec<u8>| {
            let image = Image {
                width,
                height,
                rgba: rgba.into(),
            };
            let dest = thumbs.store(&src, ThumbSize::Large, &image, mtime).unwrap();
            std::fs::metadata(dest).unwrap().len()
        };
        let (page_fast, noise_fast) = (fast(&page), fast(&noise));
        let page_kept = kept(page);
        assert!(
            page_kept * 4 < page_fast * 3,
            "{page_kept} bytes kept, {page_fast} fast"
        );
        let noise_kept = kept(noise);
        // Its text chunks aside.
        assert!(
            noise_kept <= noise_fast + 512,
            "{noise_kept} bytes kept, {noise_fast} fast"
        );
    }

    /// The plasma of the pictures' tests (`picture.rs`), as a photo's
    /// thumbnail: 256 × 192.
    fn photo() -> Image {
        let plasma = include_bytes!("../testdata/synthetic-plasma-sequential.jpg");
        let rgba = image::load_from_memory(plasma)
            .unwrap()
            .thumbnail(256, 256)
            .into_rgba8();
        Image {
            width: rgba.width(),
            height: rgba.height(),
            rgba: rgba.into_raw().into(),
        }
    }

    /// A photo is encoded once, fdeflate's, which is smaller than zlib's
    /// second level's; in grey, the differences between its pixels repeat,
    /// and zlib's is kept, far smaller.
    #[test]
    fn a_photo_is_encoded_once_and_one_in_grey_twice() {
        let fast =
            |image: &Image| encode(image, &[], png::DeflateCompression::FdeflateUltraFast).unwrap();
        let matched =
            |image: &Image| encode(image, &[], png::DeflateCompression::Level(2)).unwrap();
        let photo = photo();
        assert_eq!(photo.width, 256);
        assert!(!repeats(&photo.rgba));
        let kept = encoded(&photo, &[]).unwrap();
        assert_eq!(kept, fast(&photo), "fdeflate's alone");
        assert!(
            kept.len() < matched(&photo).len(),
            "{} bytes kept, zlib's {}",
            kept.len(),
            matched(&photo).len()
        );
        let grey = Image {
            rgba: photo
                .rgba
                .chunks(4)
                .flat_map(|p| {
                    let [r, g, b] = [p[0], p[1], p[2]].map(u32::from);
                    let luma = u8::try_from((r * 299 + g * 587 + b * 114) / 1000).unwrap();
                    [luma, luma, luma, 255]
                })
                .collect(),
            ..photo
        };
        assert!(repeats(&grey.rgba));
        let kept = encoded(&grey, &[]).unwrap();
        assert_eq!(kept, matched(&grey), "zlib's");
        assert!(
            kept.len() * 3 < fast(&grey).len() * 2,
            "{} bytes kept, fdeflate's {}",
            kept.len(),
            fast(&grey).len()
        );
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

    /// A thumbnail loaded is opened once, its time and its pixels read
    /// through one buffer; a failure's two keys are read in one pass of
    /// one open, whatever their order in the file.
    #[test]
    fn a_thumbnail_is_opened_once_and_its_keys_read_in_one_pass() {
        use std::sync::atomic::Ordering;
        let cache = tempfile::tempdir().unwrap();
        let thumbs = ThumbCache::at(cache.path().join("thumbnails"));
        let src_dir = tempfile::tempdir().unwrap();
        let src = src_dir.path().join("it's a photo.jpg");
        std::fs::write(&src, b"only its mtime matters").unwrap();
        let mtime = std::fs::metadata(&src).unwrap().modified().unwrap();
        let img = Image {
            width: 4,
            height: 2,
            rgba: vec![9u8; 4 * 2 * 4].into(),
        };
        thumbs.store(&src, ThumbSize::Normal, &img, mtime).unwrap();
        let before = OPENED.load(Ordering::Relaxed);
        let back = thumbs.load(&src, ThumbSize::Normal).unwrap();
        assert_eq!((back.width, back.height), (4, 2));
        assert_eq!(OPENED.load(Ordering::Relaxed) - before, 1, "one open");

        thumbs.store_failed(&src, mtime, "tools-a").unwrap();
        let failure = cache
            .path()
            .join("thumbnails/fail/skwir")
            .join(format!("{}.png", key(&src)));
        let secs = mtime
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string();
        let mut png = open_png(&failure).unwrap();
        assert_eq!(
            png_text_chunks(&mut png, &[TOOLS_CHUNK, "Thumb::MTime", "Thumb::None"]),
            [Some("tools-a".to_owned()), Some(secs), None]
        );
        let before = OPENED.load(Ordering::Relaxed);
        assert!(thumbs.failed(&src, "tools-a"));
        assert_eq!(OPENED.load(Ordering::Relaxed) - before, 1, "one open");
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
