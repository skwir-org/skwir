//! The free desktops' icon themes, indexed as the icon theme
//! specification looks them up: each theme's `index.theme` read once, its
//! parents with it, and the names in each of its folders known without a
//! `stat` per folder. A theme folder with GTK's `icon-theme.cache`, kept
//! current, is asked through it (a hash of every name: a few reads of a
//! file the system keeps in memory); one without is listed once, folder
//! by folder, into a map of its names. Asking a theme folder by folder
//! (what the `freedesktop-icons` crate did) cost tens of thousands of
//! `stat` calls and dozens of parses of the theme's index for one name no
//! theme had.
//!
//! It reads the disk: a worker's, never a render's. Linux's alone: the
//! other systems draw their own icons (`system_icons`).

use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The extensions an icon can have, in the order the specification tries
/// them, as bits.
const PNG: u8 = 1;
const SVG: u8 = 2;
const XPM: u8 = 4;
const EXTENSIONS: [(u8, &str); 3] = [(PNG, "png"), (SVG, "svg"), (XPM, "xpm")];

/// The installed themes, each read when a lookup first reaches it.
pub struct Index {
    /// Where themes are, in the specification's order: `~/.icons`, then
    /// `icons` in each data folder, those that exist, each once.
    bases: Vec<PathBuf>,
    /// Where icons outside any theme are: the bases, then `pixmaps`.
    loose: Vec<PathBuf>,
    themes: Mutex<HashMap<String, Option<Arc<Theme>>>>,
}

impl Index {
    pub fn new(bases: Vec<PathBuf>, pixmaps: Vec<PathBuf>) -> Self {
        let mut seen = std::collections::HashSet::new();
        let bases: Vec<PathBuf> = bases
            .into_iter()
            .filter(|base| seen.insert(base.clone()) && base.is_dir())
            .collect();
        let loose = bases
            .iter()
            .cloned()
            .chain(
                pixmaps
                    .into_iter()
                    .filter(|dir| seen.insert(dir.clone()) && dir.is_dir()),
            )
            .collect();
        Index {
            bases,
            loose,
            themes: Mutex::new(HashMap::new()),
        }
    }

    /// The file of the icon `name` at `size` pixels: from the first of
    /// `themes` that has it, each before its parents (and theirs), then
    /// hicolor after the first of them, as the specification has it; then
    /// an icon outside the themes. A name that is an absolute path is that
    /// file.
    pub fn find(&self, themes: &[&str], name: &str, size: u16) -> Option<PathBuf> {
        if name.is_empty() {
            return None;
        }
        let as_path = Path::new(name);
        if as_path.is_absolute() {
            return in_folder(as_path.parent()?, &as_path.file_stem()?.to_string_lossy());
        }
        if name.contains('/') {
            return None;
        }
        let size = i32::from(size);
        let mut visited = Vec::new();
        for (i, theme) in themes.iter().enumerate() {
            if let Some(found) = self.find_in(theme, name, size, &mut visited) {
                return Some(found);
            }
            // Hicolor, every theme's last parent, comes after the chosen
            // one: before the themes bibli falls back to.
            if i == 0
                && let Some(found) = self.find_in("hicolor", name, size, &mut visited)
            {
                return Some(found);
            }
        }
        self.loose.iter().find_map(|dir| in_folder(dir, name))
    }

    /// `name` in `theme`, then in its parents, each once.
    fn find_in(
        &self,
        theme: &str,
        name: &str,
        size: i32,
        visited: &mut Vec<String>,
    ) -> Option<PathBuf> {
        if visited.iter().any(|t| t == theme) {
            return None;
        }
        visited.push(theme.to_owned());
        let theme = self.theme(theme)?;
        if let Some(found) = theme.lookup(name, size) {
            return Some(found);
        }
        theme
            .parents
            .iter()
            .find_map(|parent| self.find_in(parent, name, size, visited))
    }

    /// The theme `name`, read the first time it is asked for.
    fn theme(&self, name: &str) -> Option<Arc<Theme>> {
        if let Some(known) = self.themes.lock().ok()?.get(name) {
            return known.clone();
        }
        let read = Theme::read(&self.bases, name).map(Arc::new);
        let mut themes = self.themes.lock().ok()?;
        themes.entry(name.to_owned()).or_insert(read).clone()
    }
}

/// `dir/name.png`, `.svg` or `.xpm`, the first there is.
fn in_folder(dir: &Path, name: &str) -> Option<PathBuf> {
    EXTENSIONS
        .iter()
        .map(|(_, ext)| dir.join(format!("{name}.{ext}")))
        .find(|path| path.is_file())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Fixed,
    Scalable,
    Threshold,
}

/// A folder of a theme, as its `index.theme` describes it.
#[derive(Debug)]
struct Folder {
    path: Box<str>,
    size: i32,
    scale: i32,
    kind: Kind,
    min: i32,
    max: i32,
    threshold: i32,
}

impl Folder {
    /// The specification's `DirectoryMatchesSize`.
    fn matches(&self, size: i32, scale: i32) -> bool {
        self.scale == scale
            && match self.kind {
                Kind::Fixed => self.size == size,
                Kind::Scalable => self.min <= size && size <= self.max,
                Kind::Threshold => {
                    self.size - self.threshold <= size && size <= self.size + self.threshold
                }
            }
    }

    /// The specification's `DirectorySizeDistance`.
    fn distance(&self, size: i32, scale: i32) -> i32 {
        let wanted = size * scale;
        match self.kind {
            Kind::Fixed => (self.size * self.scale - wanted).abs(),
            Kind::Scalable | Kind::Threshold => {
                let (low, high) = if self.kind == Kind::Scalable {
                    (self.min * self.scale, self.max * self.scale)
                } else {
                    (
                        (self.size - self.threshold) * self.scale,
                        (self.size + self.threshold) * self.scale,
                    )
                };
                if wanted < low {
                    self.min * self.scale - wanted
                } else if wanted > high {
                    wanted - self.max * self.scale
                } else {
                    0
                }
            }
        }
    }
}

/// One theme: its folders, its parents, and what each of its copies (the
/// folders of its name in each base) holds.
struct Theme {
    folders: Vec<Folder>,
    parents: Vec<String>,
    copies: Vec<(PathBuf, Names)>,
}

impl Theme {
    /// The theme `name` where themes are: every folder of that name is a
    /// copy of it, the first `index.theme` describing them all.
    fn read(bases: &[PathBuf], name: &str) -> Option<Theme> {
        if name.is_empty() || name.contains(['/', '\\']) || name == ".." || name == "." {
            return None;
        }
        let copies: Vec<PathBuf> = bases
            .iter()
            .map(|base| base.join(name))
            .filter(|copy| copy.is_dir())
            .collect();
        let text = copies.iter().find_map(|copy| {
            crate::file::read_regular_text(&copy.join("index.theme"), crate::file::DESKTOP_FILE)
                .ok()
        })?;
        let (folders, parents) = parse_index(&text);
        let at: HashMap<&str, u16> = folders
            .iter()
            .enumerate()
            .filter_map(|(i, f)| Some((&*f.path, u16::try_from(i).ok()?)))
            .collect();
        let copies = copies
            .into_iter()
            .map(|copy| {
                let names = Names::read(&copy, &folders, &at);
                (copy, names)
            })
            .collect();
        Some(Theme {
            folders,
            parents,
            copies,
        })
    }

    /// The specification's `LookupIcon`: a folder of the size asked for,
    /// in the order the index lists them, each copy in turn; else the
    /// closest size.
    fn lookup(&self, name: &str, size: i32) -> Option<PathBuf> {
        // (folder, copy, extensions), in the order tried.
        let mut found: Vec<(u16, usize, u8)> = Vec::new();
        for (copy, (_, names)) in self.copies.iter().enumerate() {
            names.of(name, &mut |folder, exts| found.push((folder, copy, exts)));
        }
        if found.is_empty() {
            return None;
        }
        found.sort_by_key(|&(folder, copy, _)| (folder, copy));
        let file = |&(folder, copy, exts): &(u16, usize, u8)| {
            let dir = self.copies[copy]
                .0
                .join(&*self.folders[usize::from(folder)].path);
            EXTENSIONS
                .iter()
                .filter(|(bit, _)| exts & bit != 0)
                .map(|(_, ext)| dir.join(format!("{name}.{ext}")))
                // What the index says is there, checked: a link that leads
                // nowhere, a cache that missed a removal.
                .find(|path| path.is_file())
        };
        found
            .iter()
            .filter(|(folder, _, _)| self.folders[usize::from(*folder)].matches(size, 1))
            .find_map(file)
            .or_else(|| {
                found.sort_by_key(|&(folder, copy, _)| {
                    (
                        self.folders[usize::from(folder)].distance(size, 1),
                        folder,
                        copy,
                    )
                });
                found.iter().find_map(file)
            })
    }
}

/// A theme's folders (`Directories`, then `ScaledDirectories`) and its
/// parents (`Inherits`), hicolor left out: it comes last whatever the
/// theme says.
fn parse_index(text: &str) -> (Vec<Folder>, Vec<String>) {
    let mut groups: HashMap<&str, HashMap<&str, &str>> = HashMap::new();
    for (group, key, value) in crate::keyfile::entries(text) {
        groups
            .entry(group)
            .or_default()
            .entry(key)
            .or_insert(value.trim());
    }
    let main = groups.get("Icon Theme");
    let list = |key: &str| -> Vec<&str> {
        main.and_then(|m| m.get(key))
            .map(|v| {
                v.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };
    let parents = list("Inherits")
        .into_iter()
        .filter(|p| *p != "hicolor")
        .map(str::to_owned)
        .collect();
    let mut seen = std::collections::HashSet::new();
    let folders = list("Directories")
        .into_iter()
        .chain(list("ScaledDirectories"))
        .filter(|path| seen.insert(*path))
        .filter_map(|path| {
            let group = groups.get(path)?;
            let int = |key: &str| group.get(key).and_then(|v| v.parse::<i32>().ok());
            let size = int("Size")?;
            Some(Folder {
                path: path.into(),
                size,
                scale: int("Scale").unwrap_or(1),
                kind: match group.get("Type").copied() {
                    Some("Fixed") => Kind::Fixed,
                    Some("Scalable") => Kind::Scalable,
                    _ => Kind::Threshold,
                },
                min: int("MinSize").unwrap_or(size),
                max: int("MaxSize").unwrap_or(size),
                threshold: int("Threshold").unwrap_or(2),
            })
        })
        .collect();
    (folders, parents)
}

/// The names one copy of a theme holds, by folder.
enum Names {
    /// GTK's cache of the copy, current: asked name by name.
    Cache(GtkCache),
    /// The copy listed: each name once, the folders and extensions it has
    /// there a range of `places`.
    Listed {
        names: HashMap<Box<str>, (u32, u32)>,
        places: Vec<(u16, u8)>,
    },
}

impl Names {
    fn read(copy: &Path, folders: &[Folder], at: &HashMap<&str, u16>) -> Names {
        if let Some(cache) = GtkCache::open(copy, at) {
            return Names::Cache(cache);
        }
        let mut all: Vec<(Box<str>, u16, u8)> = Vec::new();
        for (i, folder) in folders.iter().enumerate() {
            let Ok(i) = u16::try_from(i) else { break };
            let Ok(read) = std::fs::read_dir(copy.join(&*folder.path)) else {
                continue;
            };
            for entry in read.flatten() {
                let file = entry.file_name();
                let Some(file) = file.to_str() else { continue };
                let Some((stem, ext)) = file.rsplit_once('.') else {
                    continue;
                };
                if let Some((bit, _)) = EXTENSIONS.iter().find(|(_, e)| *e == ext) {
                    all.push((stem.into(), i, *bit));
                }
            }
        }
        all.sort_unstable();
        let mut names: HashMap<Box<str>, (u32, u32)> = HashMap::new();
        let mut places: Vec<(u16, u8)> = Vec::with_capacity(all.len());
        let mut all = all.into_iter().peekable();
        while let Some((stem, folder, bit)) = all.next() {
            let start = u32::try_from(places.len()).unwrap_or(u32::MAX);
            places.push((folder, bit));
            // The same name in other folders, or with other extensions:
            // sorted, they follow.
            while let Some((_, folder, bit)) = all.next_if(|(next, _, _)| *next == stem) {
                match places.last_mut() {
                    Some((last, exts)) if *last == folder => *exts |= bit,
                    _ => places.push((folder, bit)),
                }
            }
            let end = u32::try_from(places.len()).unwrap_or(u32::MAX);
            names.insert(stem, (start, end));
        }
        names.shrink_to_fit();
        places.shrink_to_fit();
        Names::Listed { names, places }
    }

    /// Each folder holding `name`, with the extensions it has there.
    fn of(&self, name: &str, found: &mut dyn FnMut(u16, u8)) {
        match self {
            Names::Cache(cache) => cache.of(name, found),
            Names::Listed { names, places } => {
                if let Some(&(start, end)) = names.get(name) {
                    for &(folder, exts) in &places[start as usize..end as usize] {
                        found(folder, exts);
                    }
                }
            }
        }
    }
}

/// GTK's `icon-theme.cache` (written by `gtk-update-icon-cache`): a hash
/// of every icon name of a theme's copy, read where it is asked, never
/// whole. Its integers are big-endian.
struct GtkCache {
    file: File,
    len: u64,
    hash: u32,
    buckets: u32,
    /// The theme's folder for each of the cache's folders.
    folders: Vec<Option<u16>>,
}

/// GTK's bits for the extensions an icon has.
const GTK_XPM: u16 = 1;
const GTK_SVG: u16 = 2;
const GTK_PNG: u16 = 4;
/// No more: a chain longer is a broken file.
const CHAIN_MAX: usize = 4096;

impl GtkCache {
    /// The cache of `copy`, if it has one, of the version GTK writes, at
    /// least as new as the copy itself: GTK ignores one older (an icon
    /// installed since is missing from it), and so does this.
    fn open(copy: &Path, at: &HashMap<&str, u16>) -> Option<GtkCache> {
        let path = copy.join("icon-theme.cache");
        let file = crate::file::open_regular(&path).ok()?;
        let meta = file.metadata().ok()?;
        let dir_changed = std::fs::metadata(copy).ok()?.modified().ok()?;
        if meta.modified().ok()? < dir_changed {
            return None;
        }
        let mut cache = GtkCache {
            file,
            len: meta.len(),
            hash: 0,
            buckets: 0,
            folders: Vec::new(),
        };
        if cache.u16_at(0)? != 1 || cache.u16_at(2)? != 0 {
            return None;
        }
        cache.hash = cache.u32_at(4)?;
        let list = cache.u32_at(8)?;
        cache.buckets = cache.u32_at(cache.hash)?;
        if cache.buckets == 0 {
            return None;
        }
        let count = cache.u32_at(list)?;
        if u64::from(count) * 4 > cache.len {
            return None;
        }
        cache.folders = (0..count)
            .map(|i| {
                let offset = cache.u32_at(list.checked_add(4 + 4 * i)?)?;
                let name = cache.string_at(offset)?;
                at.get(name.as_str()).copied()
            })
            .collect();
        Some(cache)
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Option<()> {
        if offset.checked_add(buf.len() as u64)? > self.len {
            return None;
        }
        std::os::unix::fs::FileExt::read_exact_at(&self.file, buf, offset).ok()
    }

    fn u16_at(&self, offset: u32) -> Option<u16> {
        let mut buf = [0; 2];
        self.read_at(u64::from(offset), &mut buf)?;
        Some(u16::from_be_bytes(buf))
    }

    fn u32_at(&self, offset: u32) -> Option<u32> {
        let mut buf = [0; 4];
        self.read_at(u64::from(offset), &mut buf)?;
        Some(u32::from_be_bytes(buf))
    }

    /// The NUL-terminated string at `offset`, up to 4 KiB.
    fn string_at(&self, offset: u32) -> Option<String> {
        let offset = u64::from(offset);
        let mut buf = vec![0; usize::try_from((self.len.checked_sub(offset)?).min(4096)).ok()?];
        self.read_at(offset, &mut buf)?;
        let end = buf.iter().position(|&b| b == 0)?;
        buf.truncate(end);
        String::from_utf8(buf).ok()
    }

    /// Whether the string at `offset` is `name`.
    fn is_at(&self, offset: u32, name: &str) -> bool {
        let mut buf = vec![0; name.len() + 1];
        self.read_at(u64::from(offset), &mut buf).is_some()
            && buf[..name.len()] == *name.as_bytes()
            && buf[name.len()] == 0
    }

    fn of(&self, name: &str, found: &mut dyn FnMut(u16, u8)) {
        let bucket = gtk_hash(name) % self.buckets;
        let Some(mut chain) = self
            .hash
            .checked_add(4 + 4 * bucket)
            .and_then(|at| self.u32_at(at))
        else {
            return;
        };
        for _ in 0..CHAIN_MAX {
            if chain == u32::MAX {
                return;
            }
            let (Some(next), Some(name_at), Some(images)) = (
                self.u32_at(chain),
                chain.checked_add(4).and_then(|at| self.u32_at(at)),
                chain.checked_add(8).and_then(|at| self.u32_at(at)),
            ) else {
                return;
            };
            if self.is_at(name_at, name) {
                self.images(images, found);
                return;
            }
            chain = next;
        }
    }

    fn images(&self, list: u32, found: &mut dyn FnMut(u16, u8)) {
        let Some(count) = self.u32_at(list) else {
            return;
        };
        let Some(bytes) = usize::try_from(count).ok().and_then(|n| n.checked_mul(8)) else {
            return;
        };
        if bytes as u64 > self.len {
            return;
        }
        let mut buf = vec![0; bytes];
        if self.read_at(u64::from(list) + 4, &mut buf).is_none() {
            return;
        }
        for image in buf.as_chunks::<8>().0 {
            let dir = u16::from_be_bytes([image[0], image[1]]);
            let flags = u16::from_be_bytes([image[2], image[3]]);
            let Some(Some(folder)) = self.folders.get(usize::from(dir)) else {
                continue;
            };
            let exts = (if flags & GTK_PNG != 0 { PNG } else { 0 })
                | (if flags & GTK_SVG != 0 { SVG } else { 0 })
                | (if flags & GTK_XPM != 0 { XPM } else { 0 });
            if exts != 0 {
                found(*folder, exts);
            }
        }
    }
}

/// GTK's hash of an icon name (`icon_name_hash`): over signed bytes.
fn gtk_hash(name: &str) -> u32 {
    let mut bytes = name
        .bytes()
        .map(|b| i32::from(b.cast_signed()).cast_unsigned());
    let Some(first) = bytes.next() else {
        return 0;
    };
    bytes.fold(first, |h, b| (h << 5).wrapping_sub(h).wrapping_add(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme(root: &Path, name: &str, index: &str, files: &[&str]) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.theme"), index).unwrap();
        for file in files {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"").unwrap();
        }
    }

    const SIZES: &str = "[Icon Theme]\nName=T\nInherits=Parent,hicolor\nDirectories=16/places,48/places,scalable/places\n\n[16/places]\nSize=16\nType=Fixed\n\n[48/places]\nSize=48\nType=Fixed\n\n[scalable/places]\nSize=64\nMinSize=8\nMaxSize=512\nType=Scalable\n";

    #[test]
    fn a_size_asked_is_found_else_the_closest() {
        let root = tempfile::tempdir().unwrap();
        theme(
            root.path(),
            "T",
            SIZES,
            &[
                "16/places/folder.png",
                "48/places/folder.png",
                "48/places/big.svg",
            ],
        );
        let index = Index::new(vec![root.path().to_owned()], Vec::new());
        let t = root.path().join("T");
        assert_eq!(
            index.find(&["T"], "folder", 16),
            Some(t.join("16/places/folder.png"))
        );
        assert_eq!(
            index.find(&["T"], "folder", 48),
            Some(t.join("48/places/folder.png"))
        );
        // 24 is 8 from 16 and 24 from 48.
        assert_eq!(
            index.find(&["T"], "folder", 24),
            Some(t.join("16/places/folder.png"))
        );
        assert_eq!(
            index.find(&["T"], "big", 16),
            Some(t.join("48/places/big.svg"))
        );
        assert_eq!(index.find(&["T"], "absent", 16), None);
    }

    #[test]
    fn parents_then_hicolor_then_the_fallbacks_then_loose_icons() {
        let root = tempfile::tempdir().unwrap();
        let pixmaps = tempfile::tempdir().unwrap();
        theme(root.path(), "T", SIZES, &["16/places/own.png"]);
        theme(
            root.path(),
            "Parent",
            "[Icon Theme]\nInherits=Grand\nDirectories=16\n[16]\nSize=16\n",
            &["16/inherited.svg", "16/both.svg"],
        );
        theme(
            root.path(),
            "Grand",
            "[Icon Theme]\nInherits=T\nDirectories=16\n[16]\nSize=16\n",
            &["16/deep.png"],
        );
        theme(
            root.path(),
            "hicolor",
            "[Icon Theme]\nDirectories=16\n[16]\nSize=16\n",
            &["16/app.png", "16/both.png"],
        );
        theme(
            root.path(),
            "Fallback",
            "[Icon Theme]\nDirectories=16\n[16]\nSize=16\n",
            &["16/app.png", "16/spare.png"],
        );
        std::fs::write(pixmaps.path().join("old.xpm"), b"").unwrap();
        let index = Index::new(
            vec![root.path().to_owned()],
            vec![pixmaps.path().to_owned()],
        );
        let found = |name| index.find(&["T", "Fallback"], name, 16);
        let r = root.path();
        assert_eq!(found("own"), Some(r.join("T/16/places/own.png")));
        assert_eq!(found("inherited"), Some(r.join("Parent/16/inherited.svg")));
        // A parent's parent, a cycle back to the theme ending there.
        assert_eq!(found("deep"), Some(r.join("Grand/16/deep.png")));
        // The theme's parents before hicolor; hicolor before the fallback.
        assert_eq!(found("both"), Some(r.join("Parent/16/both.svg")));
        assert_eq!(found("app"), Some(r.join("hicolor/16/app.png")));
        assert_eq!(found("spare"), Some(r.join("Fallback/16/spare.png")));
        assert_eq!(found("old"), Some(pixmaps.path().join("old.xpm")));
        assert_eq!(found("nowhere"), None);
        assert_eq!(found("../T/16/places/own"), None);
    }

    #[test]
    fn copies_of_a_theme_are_one_theme_described_by_the_first_index() {
        let user = tempfile::tempdir().unwrap();
        let system = tempfile::tempdir().unwrap();
        // The user's copy has no index of its own.
        std::fs::create_dir_all(user.path().join("T/16/places")).unwrap();
        std::fs::write(user.path().join("T/16/places/mine.png"), b"").unwrap();
        theme(
            system.path(),
            "T",
            SIZES,
            &["16/places/mine.png", "16/places/system.png"],
        );
        let index = Index::new(
            vec![user.path().to_owned(), system.path().to_owned()],
            Vec::new(),
        );
        assert_eq!(
            index.find(&["T"], "mine", 16),
            Some(user.path().join("T/16/places/mine.png"))
        );
        assert_eq!(
            index.find(&["T"], "system", 16),
            Some(system.path().join("T/16/places/system.png"))
        );
    }

    #[test]
    fn png_before_svg_and_a_link_to_nothing_is_not_an_icon() {
        let root = tempfile::tempdir().unwrap();
        theme(
            root.path(),
            "T",
            SIZES,
            &["16/places/a.svg", "16/places/a.png", "48/places/b.png"],
        );
        #[cfg(unix)]
        std::os::unix::fs::symlink("nowhere.png", root.path().join("T/16/places/b.png")).unwrap();
        let index = Index::new(vec![root.path().to_owned()], Vec::new());
        let t = root.path().join("T");
        assert_eq!(index.find(&["T"], "a", 16), Some(t.join("16/places/a.png")));
        assert_eq!(index.find(&["T"], "b", 16), Some(t.join("48/places/b.png")));
    }

    /// A cache as `gtk-update-icon-cache` writes one: what it holds is
    /// what the theme has, even a name its folders do not list.
    #[test]
    fn a_current_cache_is_read_and_an_older_one_ignored() {
        let root = tempfile::tempdir().unwrap();
        theme(root.path(), "T", SIZES, &["16/places/listed.png"]);
        let t = root.path().join("T");
        std::fs::write(t.join("16/places/cached.svg"), b"").unwrap();
        // One bucket: "cached" in 16/places (folder 0), as an SVG.
        let mut cache = Vec::new();
        let mut put = |at: usize, bytes: &[u8]| {
            if cache.len() < at + bytes.len() {
                cache.resize(at + bytes.len(), 0);
            }
            cache[at..at + bytes.len()].copy_from_slice(bytes);
        };
        put(0, &1u16.to_be_bytes());
        put(2, &0u16.to_be_bytes());
        put(4, &12u32.to_be_bytes()); // the hash
        put(8, &20u32.to_be_bytes()); // the folders
        put(12, &1u32.to_be_bytes()); // one bucket
        put(16, &40u32.to_be_bytes()); // its chain
        put(20, &1u32.to_be_bytes()); // one folder
        put(24, &28u32.to_be_bytes()); // its name
        put(28, b"16/places\0");
        put(40, &u32::MAX.to_be_bytes()); // the chain's end
        put(44, &52u32.to_be_bytes()); // the icon's name
        put(48, &60u32.to_be_bytes()); // its images
        put(52, b"cached\0");
        put(60, &1u32.to_be_bytes()); // one image
        put(64, &0u16.to_be_bytes()); // in folder 0
        put(66, &GTK_SVG.to_be_bytes());
        put(68, &0u32.to_be_bytes());
        std::fs::write(t.join("icon-theme.cache"), &cache).unwrap();
        let index = Index::new(vec![root.path().to_owned()], Vec::new());
        assert_eq!(
            index.find(&["T"], "cached", 16),
            Some(t.join("16/places/cached.svg"))
        );
        assert_eq!(
            index.find(&["T"], "listed", 16),
            None,
            "read from the cache alone"
        );

        // Older than its folder: the folder is listed.
        let older =
            std::fs::metadata(&t).unwrap().modified().unwrap() - std::time::Duration::from_secs(60);
        File::options()
            .write(true)
            .open(t.join("icon-theme.cache"))
            .unwrap()
            .set_modified(older)
            .unwrap();
        let index = Index::new(vec![root.path().to_owned()], Vec::new());
        assert_eq!(
            index.find(&["T"], "listed", 16),
            Some(t.join("16/places/listed.png"))
        );
    }

    #[test]
    fn the_hash_is_gtks() {
        // icon_name_hash("folder") as GTK computes it.
        let mut h: u32 = u32::from(b'f');
        for b in b"older" {
            h = (h << 5).wrapping_sub(h).wrapping_add(u32::from(*b));
        }
        assert_eq!(gtk_hash("folder"), h);
        assert_eq!(gtk_hash(""), 0);
        // A byte over 127 counts as negative, as a signed char.
        assert_eq!(gtk_hash("é"), {
            let [a, b] = [0xc3u8, 0xa9u8].map(|b| i32::from(b.cast_signed()).cast_unsigned());
            (a << 5).wrapping_sub(a).wrapping_add(b)
        });
    }
}
