//! MIME types: the system's own registry first, and the desktop's
//! database, shared-mime-info, for what it does not answer (ADR-037). On
//! Linux and the BSDs the database is the system's registry; on macOS the
//! system's is Uniform Type Identifiers (`uttype`), on Windows the
//! registry's classes (`registry`). The system names a file by its
//! extension and says what people call its type; the database says it
//! more precisely where it can (`refine`), reads the bytes, and holds the
//! hierarchy and the aliases, whose names are the ones bibli speaks.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt;
use std::path::Path;
use std::sync::{OnceLock, RwLock};

/// A MIME type such as `image/png`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Mime(pub String);

impl Mime {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `image` for `image/png`.
    pub fn top_level(&self) -> &str {
        self.0.split('/').next().unwrap_or(&self.0)
    }

    /// The icon name derived from the type: `image-png`.
    pub fn icon_name(&self) -> String {
        self.0.replace('/', "-")
    }

    /// Generic fallback icon: `image-x-generic`.
    pub fn generic_icon_name(&self) -> String {
        database::generic_icon_name(self)
    }

    /// What people call files of this type: the system's own words where
    /// it has a registry beside the database (macOS's, Windows', in the
    /// user's language), else the database's comment, if available. Reads
    /// files: call from a worker and cache the answer.
    pub fn description(&self) -> Option<String> {
        system::description(self).or_else(|| database::description(self))
    }
}

impl fmt::Display for Mime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Guess from the file name only. No I/O; cheap enough for every row
/// (the system's registry, where it is asked, once per extension).
///
/// `None` means the name says nothing (`settings.cfg`, `README`, `.env`):
/// not that the file is binary. Only `by_content` can say that.
///
/// Several types can claim a name equally (`*.ogg`: Ogg audio, Vorbis,
/// FLAC, Speex, Ogg video, Theora). The answer is then the most general of
/// them, the one the others descend from, the same in every process: the
/// database hands the ties back in no particular order. What such a file
/// really holds is for [`identify`] to say, where the bytes can be read.
pub fn by_name(name: &OsStr) -> Option<Mime> {
    database::representative(candidates(name))
}

/// The type an entry of a listing is shown as. No I/O. What it is whatever
/// its name comes first (`Entry::inode_type`: a folder, a pipe, a device,
/// a link that leads nowhere); a file, or a link to one, is typed by the
/// name that says its type (`Entry::type_name`). Type an entry through
/// this, never by `by_name` of a name of your choosing.
pub fn of_entry(entry: &bibli_core::Entry) -> Option<Mime> {
    match entry.inode_type() {
        Some(mime) => Some(Mime(mime.to_owned())),
        None => by_name(entry.type_name()),
    }
}

/// Whether the name leaves the type to the bytes: several types claim it
/// equally ([`by_name`]). No I/O.
pub fn ambiguous(name: &OsStr) -> bool {
    candidates(name).len() > 1
}

/// Selections no larger than this have each name asked as it is.
const EXACT: usize = 64;
/// Distinct questions one [`by_names`] asks at most.
const ASKED: usize = 256;

/// [`by_name`] for many names at once (a selection), in their order.
///
/// The database takes about 13 µs a name (a release build, 2026-10-06),
/// so a large selection cannot be asked name by name on the UI thread.
/// Up to `EXACT` names are each asked as they are; beyond, a name is asked
/// once per suffix from its first dot (`.jpg`, `.tar.gz`), as any file with
/// that suffix would be, so a name only a whole-name pattern knows
/// (`CMakeLists.txt`) takes its suffix's type there. A name without a
/// suffix is asked as it is. At most `ASKED` distinct questions: the names
/// past them are unknown (`None`), which bounds the call whatever the
/// selection. No I/O once the database is loaded.
pub fn by_names<'a, I>(names: I) -> Vec<Option<Mime>>
where
    I: IntoIterator<Item = &'a OsStr>,
    I::IntoIter: ExactSizeIterator,
{
    let names = names.into_iter();
    let exact = names.len() <= EXACT;
    let mut asked: std::collections::HashMap<String, Option<Mime>> =
        std::collections::HashMap::new();
    names
        .map(|name| {
            let name = name.to_str()?;
            let question = match suffix(name) {
                Some(suffix) if !exact => format!("file{suffix}"),
                _ => name.to_owned(),
            };
            if let Some(answer) = asked.get(&question) {
                return answer.clone();
            }
            if asked.len() >= ASKED {
                return None;
            }
            let answer = by_name(OsStr::new(&question));
            asked.insert(question, answer.clone());
            answer
        })
        .collect()
}

/// `.tar.gz` for `photos.tar.gz`: from the first dot that does not start
/// the name (a hidden file's own dot is not a suffix).
fn suffix(name: &str) -> Option<&str> {
    let start = name.len() - name.trim_start_matches('.').len();
    name[start..].find('.').map(|at| &name[start + at..])
}

/// What the file `name` at `path` is, from its name and, only where the
/// name is not enough, its bytes: the shared-mime-info order. A name one
/// type claims is that type. A name several claim is settled by the magic
/// among them (a Vorbis `.ogg` is Vorbis, a Theora one a film), and stays
/// the name's answer when the bytes do not choose. A name no type claims is
/// [`by_content`]'s. Reads the head of the file: call from a worker.
pub fn identify(name: &OsStr, path: &Path) -> Option<Mime> {
    let candidates = candidates(name);
    if candidates.len() < 2 {
        return match candidates.into_iter().next() {
            Some(only) => Some(only),
            None => by_content(path),
        };
    }
    // The bytes choose only among what the name allows, or something
    // more precise than one of those: magic that names another kind of
    // file altogether does not outvote an extension several types share.
    let sniffed = head(path)
        .and_then(|head| database::magic(&head))
        .filter(|m| candidates.iter().any(|c| database::is_a(m, c)));
    sniffed.or_else(|| database::representative(candidates))
}

/// What the bytes say, for a file whose name says nothing. Reads the head
/// of the file: call from a worker. The database's magic first (a PNG named
/// `picture.cfg` is a PNG); without a match, `text/plain` when the head is
/// text (`inspection::is_text`: UTF-8 without NUL, or UTF-16 or a legacy
/// code page without control characters), else `application/octet-stream`,
/// which from here, and
/// only from here, means the bytes were looked at and are binary. `None`
/// when the file cannot be read or is not a regular file (a FIFO would
/// otherwise block until something writes to it).
pub fn by_content(path: &Path) -> Option<Mime> {
    let head = head(path)?;
    // There is no magic for text/plain: what it does not know is text or
    // binary by the bytes.
    Some(database::magic(&head).unwrap_or_else(|| text_or_binary(&head)))
}

/// Bytes read to decide what a file without a known name holds. The
/// database's deepest magic rules look further, but at that distance the
/// name usually says it first; the text rule is git's (a NUL in the first
/// 8000 bytes).
const SNIFF: usize = 8 * 1024;

/// `text/plain` or `application/octet-stream` from a file's head: the one
/// text-or-binary rule, the same one the preview applies to a declared type
/// (`crate::inspection::is_text`).
fn text_or_binary(head: &[u8]) -> Mime {
    Mime(
        if crate::inspection::is_text(head) {
            "text/plain"
        } else {
            "application/octet-stream"
        }
        .to_owned(),
    )
}

/// The untranslated `<comment>` of a shared-mime-info type file, capitalized.
fn comment_of(xml: &str) -> Option<String> {
    let start = xml.find("<comment>")? + "<comment>".len();
    let end = start + xml[start..].find("</comment>")?;
    let text = xml[start..end]
        .trim()
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&apos;", "'")
        .replace("&quot;", "\"");
    let mut chars = text.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

/// What sort of file a type is, as the desktop groups them for people:
/// read from the database, not from a list of our own. A type's generic
/// icon says it (`x-office-document` for PDF, EPUB, Markdown and word
/// processor files; `package-x-generic` for archives), else its top-level
/// type; text is any type that descends from `text/plain`, code included.
/// A type can be of several kinds (Markdown is a document and text).
/// Where the system groups types itself, its groups count too
/// (`system::kinds`, ADR-037): macOS's conformance adds to the
/// database's; Windows' `PerceivedType`, coarser, groups only what the
/// database gives no kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Folder,
    Image,
    Video,
    Audio,
    Document,
    Spreadsheet,
    Presentation,
    Text,
    Archive,
    Font,
    Program,
}

impl Kind {
    pub const ALL: [Kind; 11] = [
        Kind::Folder,
        Kind::Image,
        Kind::Video,
        Kind::Audio,
        Kind::Document,
        Kind::Spreadsheet,
        Kind::Presentation,
        Kind::Text,
        Kind::Archive,
        Kind::Font,
        Kind::Program,
    ];

    /// The word a query uses: `image`.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Folder => "folder",
            Kind::Image => "image",
            Kind::Video => "video",
            Kind::Audio => "audio",
            Kind::Document => "document",
            Kind::Spreadsheet => "spreadsheet",
            Kind::Presentation => "presentation",
            Kind::Text => "text",
            Kind::Archive => "archive",
            Kind::Font => "font",
            Kind::Program => "program",
        }
    }

    /// What people call a group of them: `Images`.
    pub fn title(self) -> &'static str {
        match self {
            Kind::Folder => "Folders",
            Kind::Image => "Images",
            Kind::Video => "Videos",
            Kind::Audio => "Music and audio",
            Kind::Document => "Documents",
            Kind::Spreadsheet => "Spreadsheets",
            Kind::Presentation => "Presentations",
            Kind::Text => "Text and code",
            Kind::Archive => "Archives",
            Kind::Font => "Fonts",
            Kind::Program => "Programs",
        }
    }

    /// A kind by its word, its plural or a common synonym (`music`,
    /// `photos`), in any case.
    pub fn from_name(word: &str) -> Option<Kind> {
        Some(match word.to_ascii_lowercase().as_str() {
            "folder" | "folders" | "dir" | "dirs" | "directory" | "directories" => Kind::Folder,
            "image" | "images" | "picture" | "pictures" | "photo" | "photos" => Kind::Image,
            "video" | "videos" | "movie" | "movies" | "film" | "films" => Kind::Video,
            "audio" | "music" | "sound" | "sounds" | "song" | "songs" => Kind::Audio,
            "document" | "documents" | "doc" | "docs" => Kind::Document,
            "spreadsheet" | "spreadsheets" => Kind::Spreadsheet,
            "presentation" | "presentations" | "slides" => Kind::Presentation,
            "text" | "code" => Kind::Text,
            "archive" | "archives" => Kind::Archive,
            "font" | "fonts" => Kind::Font,
            "program" | "programs" | "executable" | "executables" | "app" | "apps" => Kind::Program,
            _ => return None,
        })
    }

    fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// The kinds of `mime`. Cached per type: a walk asks for every file.
pub fn kinds(mime: &Mime) -> Vec<Kind> {
    let bits = kind_bits(mime);
    Kind::ALL
        .into_iter()
        .filter(|k| bits & k.bit() != 0)
        .collect()
}

/// Whether `mime` is of `kind`. Cached per type: no I/O once the database
/// is loaded.
pub fn is_kind(mime: &Mime, kind: Kind) -> bool {
    kind_bits(mime) & kind.bit() != 0
}

fn kind_bits(mime: &Mime) -> u16 {
    static CACHE: OnceLock<RwLock<HashMap<String, u16>>> = OnceLock::new();
    let cache = CACHE.get_or_init(RwLock::default);
    if let Some(bits) = cache
        .read()
        .ok()
        .and_then(|c| c.get(mime.as_str()).copied())
    {
        return bits;
    }
    let bits = classify(mime);
    if let Ok(mut c) = cache.write() {
        c.insert(mime.as_str().to_owned(), bits);
    }
    bits
}

fn classify(mime: &Mime) -> u16 {
    if is_directory(mime) {
        return Kind::Folder.bit();
    }
    let mime = unalias(mime);
    let by_icon = match mime.generic_icon_name().as_str() {
        "image-x-generic" => Some(Kind::Image),
        "video-x-generic" => Some(Kind::Video),
        "audio-x-generic" => Some(Kind::Audio),
        "x-office-document" => Some(Kind::Document),
        "x-office-spreadsheet" => Some(Kind::Spreadsheet),
        "x-office-presentation" => Some(Kind::Presentation),
        "package-x-generic" => Some(Kind::Archive),
        "font-x-generic" => Some(Kind::Font),
        "application-x-executable" => Some(Kind::Program),
        _ => None,
    };
    let text = lineage(&mime).iter().any(|m| m.as_str() == "text/plain");
    let database = by_icon.map_or(0, Kind::bit) | if text { Kind::Text.bit() } else { 0 };
    system::kinds(&mime, database)
}

pub fn is_directory(mime: &Mime) -> bool {
    mime.0 == "inode/directory"
}

/// The type `mime` is another name for (`model/x.stl-binary` is
/// `model/stl`), or `mime` itself. No I/O once the database is loaded.
///
/// The database's names first, whatever the system: they are the words
/// the providers and queries speak (`Types`). A type it does not know is
/// named as the system's registry names it, where it has one beside the
/// database.
pub fn unalias(mime: &Mime) -> Mime {
    static CANONICAL: Memo = OnceLock::new();
    if !system::ASKED || database::knows(mime) {
        return database::unalias(mime);
    }
    remembered(&CANONICAL, mime.as_str(), || {
        system::canonical(mime).map(|c| database::unalias(&c))
    })
    .unwrap_or_else(|| mime.clone())
}

/// Types named by hand (what a decoder reads), matched as the database
/// knows them: a type is one of them when it or the type it is another
/// name for is, or is what one of them is another name for. One name per
/// type is enough, whichever a database's version calls canonical: list
/// the older one (`audio/x-matroska`), which a newer database aliases.
pub struct Types {
    names: &'static [&'static str],
    canonical: std::sync::OnceLock<std::collections::HashSet<String>>,
}

impl Types {
    pub const fn new(names: &'static [&'static str]) -> Self {
        Self {
            names,
            canonical: std::sync::OnceLock::new(),
        }
    }

    /// Compared without case, as types are (the database's
    /// `…macroEnabled.12` is read back as `…macroenabled.12`). No I/O
    /// once the database is loaded.
    pub fn contains(&self, mime: &str) -> bool {
        let both = |name: &str| {
            let name = name.to_ascii_lowercase();
            let canonical = unalias(&Mime(name.clone())).0.to_ascii_lowercase();
            [name, canonical]
        };
        let known = self
            .canonical
            .get_or_init(|| self.names.iter().flat_map(|&name| both(name)).collect());
        both(mime).iter().any(|name| known.contains(name))
    }

    /// Whether a file asked about is of one of these types (`of_request`).
    pub fn holds(&self, path: &Path, mime: Option<&str>) -> bool {
        of_request(path, mime).is_some_and(|mime| self.contains(mime.as_str()))
    }
}

/// The type of a file asked about (a preview's or a thumbnail's request):
/// the one it was asked with, read from its bytes where its name is
/// ambiguous (an `.otf` font or formula template), else its name's.
pub fn of_request(path: &Path, mime: Option<&str>) -> Option<Mime> {
    match mime {
        Some(mime) => Some(Mime(mime.to_owned())),
        None => by_name(path.file_name()?),
    }
}

/// The extensions files of type `mime` are named by, lowercase: the
/// system registry's first, where it has one beside the database, then
/// the database's plain `*.ext` globs in its order (heaviest first, the
/// most specific folder first). Aliases are resolved. No I/O once the
/// database is loaded; the system's registry is asked.
pub fn extensions(mime: &Mime) -> Vec<String> {
    let mime = unalias(mime);
    let mut all = system::extensions(&mime);
    for extension in database::extensions(&mime) {
        if !all.contains(&extension) {
            all.push(extension);
        }
    }
    all
}

/// The extension a `globs2` pattern names, if it is a plain `*.ext`.
fn plain_extension(glob: &str) -> Option<String> {
    let extension = glob.strip_prefix("*.")?;
    (!extension.is_empty() && !extension.contains(['*', '?', '[', '.']))
        .then(|| extension.to_lowercase())
}

/// `mime` and the types it descends from, nearest first, each once: its
/// declared parents and theirs, then the ones every type implies, as the
/// desktop reads them to find an application: `text/plain` under any
/// `text/*`, `application/octet-stream` under any file (not a folder).
/// Aliases are resolved first. No I/O once the database is loaded.
pub fn lineage(mime: &Mime) -> Vec<Mime> {
    let mut out = vec![unalias(mime)];
    let mut next = 0;
    while next < out.len() {
        for parent in database::parents(&out[next]) {
            if !out.contains(&parent) {
                out.push(parent);
            }
        }
        next += 1;
    }
    let implied = [
        (mime.top_level() == "text").then_some("text/plain"),
        (mime.top_level() != "inode").then_some("application/octet-stream"),
    ];
    for m in implied.into_iter().flatten() {
        let m = Mime(m.to_owned());
        if !out.contains(&m) {
            out.push(m);
        }
    }
    out
}

/// Off Linux, the data folder whose `mime` holds the type database
/// (ADR-037), the first of: beside bibli (`share` next to the program or
/// above it, a `.app`'s `Resources/share`: what a package ships), where a
/// package manager installed it (Homebrew on a Mac), `$XDG_DATA_DIRS`.
/// Reads the disk once.
#[cfg(not(target_os = "linux"))]
fn found_database() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            let program = std::env::current_exe().ok();
            let beside = program
                .as_deref()
                .and_then(Path::parent)
                .into_iter()
                .flat_map(|dir| {
                    [
                        dir.join("share"),
                        dir.join("../share"),
                        dir.join("../Resources/share"),
                    ]
                });
            let managed: &[&str] = if cfg!(target_os = "macos") {
                &["/opt/homebrew/share", "/usr/local/share"]
            } else {
                &[]
            };
            let named = std::env::var_os("XDG_DATA_DIRS")
                .map(|dirs| std::env::split_paths(&dirs).collect::<Vec<_>>())
                .unwrap_or_default();
            beside
                .chain(managed.iter().map(PathBuf::from))
                .chain(named)
                .find(|dir| dir.join("mime").join("globs2").is_file())
        })
        .clone()
}

/// The desktop's type database, shared-mime-info (ADR-037): Linux's own
/// registry, read where the desktop installed it; elsewhere the fallback
/// of the system's, read from the one found (`found_database`), which a
/// package carries beside bibli. Where there is none it knows nothing.
mod database {
    use std::collections::{HashMap, HashSet};
    use std::ffi::OsStr;
    use std::path::PathBuf;
    use std::str::FromStr;
    use std::sync::OnceLock;

    use super::Mime;

    fn db() -> Option<&'static xdg_mime::SharedMimeInfo> {
        static DB: OnceLock<Option<xdg_mime::SharedMimeInfo>> = OnceLock::new();
        DB.get_or_init(|| {
            #[cfg(target_os = "linux")]
            {
                Some(xdg_mime::SharedMimeInfo::new())
            }
            #[cfg(not(target_os = "linux"))]
            {
                super::found_database().map(xdg_mime::SharedMimeInfo::new_for_directory)
            }
        })
        .as_ref()
    }

    fn to_mime(m: &mime::Mime) -> Mime {
        Mime(m.essence_str().to_owned())
    }

    fn parse(m: &Mime) -> Option<mime::Mime> {
        mime::Mime::from_str(&m.0).ok()
    }

    /// The types the name's best globs give, all of equal weight. The
    /// database answers application/octet-stream for a name that matches
    /// no glob: its "no idea", never a type the name said.
    pub fn candidates(name: &OsStr) -> Vec<Mime> {
        let (Some(db), Some(name)) = (db(), name.to_str()) else {
            return Vec::new();
        };
        db.get_mime_types_from_file_name(name)
            .iter()
            .filter(|m| **m != mime::APPLICATION_OCTET_STREAM)
            .map(to_mime)
            .collect()
    }

    /// Whether `m` is `base` or descends from it, by any of their names.
    pub fn is_a(m: &Mime, base: &Mime) -> bool {
        if m == base {
            return true;
        }
        let (Some(db), Some(m), Some(base)) = (db(), parse(m), parse(base)) else {
            return false;
        };
        db.mime_type_subclass(&m, &base)
    }

    /// The most general of equally likely types: the one most of the others
    /// descend from (`audio/ogg` over Vorbis, FLAC and Speex), the name as
    /// the last word, so the choice never depends on the order they came in.
    pub fn representative(mut candidates: Vec<Mime>) -> Option<Mime> {
        if candidates.len() < 2 {
            return candidates.pop();
        }
        let descendants = |base: &Mime| {
            candidates
                .iter()
                .filter(|m| *m != base && is_a(m, base))
                .count()
        };
        candidates
            .iter()
            .max_by(|a, b| {
                descendants(a)
                    .cmp(&descendants(b))
                    .then_with(|| b.as_str().cmp(a.as_str()))
            })
            .cloned()
    }

    /// What the database's magic says of a file's first bytes, if it
    /// knows them (it also names an empty file, application/x-zerosize).
    pub fn magic(head: &[u8]) -> Option<Mime> {
        db()?.get_mime_type_for_data(head).map(|(m, _)| to_mime(&m))
    }

    pub fn unalias(m: &Mime) -> Mime {
        db().zip(parse(m))
            .and_then(|(db, p)| db.unalias_mime_type(&p))
            .map_or_else(|| m.clone(), |p| to_mime(&p))
    }

    /// Whether the database knows the type `m` by this name: one its
    /// files name (a glob, a parent, an alias, an icon), without case.
    /// Read once.
    pub fn knows(m: &Mime) -> bool {
        static NAMED: OnceLock<HashSet<String>> = OnceLock::new();
        NAMED
            .get_or_init(|| {
                let mut named = HashSet::new();
                for dir in data_dirs() {
                    for (file, separator) in [
                        ("globs2", ':'),
                        ("subclasses", ' '),
                        ("aliases", ' '),
                        ("generic-icons", ':'),
                    ] {
                        let Ok(text) = std::fs::read_to_string(dir.join("mime").join(file)) else {
                            continue;
                        };
                        for line in text.lines().filter(|l| !l.starts_with('#')) {
                            let mut fields = line.split(separator);
                            // globs2 is `weight:type:glob`; an alias line
                            // names two types; the others start with one.
                            let types: Vec<&str> = match file {
                                "globs2" => fields.nth(1).into_iter().collect(),
                                "aliases" => fields.collect(),
                                _ => fields.next().into_iter().collect(),
                            };
                            named.extend(types.into_iter().map(str::to_ascii_lowercase));
                        }
                    }
                }
                named
            })
            .contains(&m.as_str().to_ascii_lowercase())
    }

    /// The declared parents of `m`, not `m` itself. xdg-mime 0.4 keeps its
    /// subclass table private and its `get_parents` answers only for
    /// aliases, so the database's `subclasses` files are read here, once.
    pub fn parents(m: &Mime) -> Vec<Mime> {
        static PARENTS: OnceLock<HashMap<String, Vec<String>>> = OnceLock::new();
        PARENTS
            .get_or_init(|| {
                let mut map = HashMap::<String, Vec<String>>::new();
                // Most specific directory first: its lines come first.
                for dir in data_dirs() {
                    let Ok(text) = std::fs::read_to_string(dir.join("mime/subclasses")) else {
                        continue;
                    };
                    for (child, parent) in text.lines().filter_map(|l| l.split_once(' ')) {
                        let parents = map.entry(child.to_owned()).or_default();
                        if !parents.iter().any(|p| p == parent) {
                            parents.push(parent.to_owned());
                        }
                    }
                }
                map
            })
            .get(m.as_str())
            .map(|all| all.iter().map(|p| Mime(p.clone())).collect())
            .unwrap_or_default()
    }

    /// The extensions of the type `m` in the database, from every folder's
    /// `globs2` (lines of `weight:type:glob[:flags]`), read once.
    pub fn extensions(m: &Mime) -> Vec<String> {
        static EXTENSIONS: OnceLock<HashMap<String, Vec<String>>> = OnceLock::new();
        EXTENSIONS
            .get_or_init(|| {
                let mut map = HashMap::<String, Vec<String>>::new();
                for dir in data_dirs() {
                    let Ok(text) = std::fs::read_to_string(dir.join("mime/globs2")) else {
                        continue;
                    };
                    for line in text.lines().filter(|l| !l.starts_with('#')) {
                        let mut fields = line.split(':');
                        let (Some(_weight), Some(mime), Some(glob)) =
                            (fields.next(), fields.next(), fields.next())
                        else {
                            continue;
                        };
                        let Some(extension) = super::plain_extension(glob) else {
                            continue;
                        };
                        let known = map.entry(mime.to_owned()).or_default();
                        if !known.contains(&extension) {
                            known.push(extension);
                        }
                    }
                }
                map
            })
            .get(m.as_str())
            .cloned()
            .unwrap_or_default()
    }

    /// Where the database lives: on Linux `$XDG_DATA_HOME`, then
    /// `$XDG_DATA_DIRS`; elsewhere the one found (`super::found_database`).
    fn data_dirs() -> Vec<PathBuf> {
        #[cfg(target_os = "linux")]
        {
            crate::xdg::data_search()
        }
        #[cfg(not(target_os = "linux"))]
        {
            super::found_database().into_iter().collect()
        }
    }

    pub fn generic_icon_name(m: &Mime) -> String {
        db().zip(parse(m))
            .and_then(|(db, p)| db.lookup_generic_icon_name(&p))
            .unwrap_or_else(|| format!("{}-x-generic", m.top_level()))
    }

    /// xdg-mime 0.4 does not expose the `<comment>` element, so read it from
    /// the per-type XML the database installs (a package leaves it out:
    /// the system names its types). Reads a file: call from a worker and
    /// cache the answer.
    pub fn description(m: &Mime) -> Option<String> {
        data_dirs().into_iter().find_map(|dir| {
            let xml =
                std::fs::read_to_string(dir.join("mime").join(format!("{}.xml", m.0))).ok()?;
            super::comment_of(&xml)
        })
    }
}

// The system's own registry, asked before the database (ADR-037).
#[cfg(target_os = "macos")]
mod uttype;
#[cfg(target_os = "macos")]
use uttype as system;
#[cfg(windows)]
mod registry;
#[cfg(windows)]
use registry as system;

/// On Linux and the BSDs the desktop's database is the system's own
/// registry: nothing is asked beside it.
#[cfg(not(any(target_os = "macos", windows)))]
mod system {
    use super::Mime;

    pub const ASKED: bool = false;

    pub fn by_extension(_extension: &str) -> Option<Mime> {
        None
    }
    pub fn description(_m: &Mime) -> Option<String> {
        None
    }
    pub fn kinds(_m: &Mime, database: u16) -> u16 {
        database
    }
    pub fn extensions(_m: &Mime) -> Vec<String> {
        Vec::new()
    }
    pub fn canonical(_m: &Mime) -> Option<Mime> {
        None
    }
}

/// Answers of the system's registry by question: it is asked once per
/// extension or type, not once per row. Forgotten all at once past `MEMO`
/// questions, which bounds it whatever names a folder holds.
type Memo = OnceLock<RwLock<HashMap<String, Option<Mime>>>>;
const MEMO: usize = 4096;

fn remembered(
    memo: &'static Memo,
    question: &str,
    ask: impl FnOnce() -> Option<Mime>,
) -> Option<Mime> {
    let memo = memo.get_or_init(RwLock::default);
    if let Some(answer) = memo.read().ok().and_then(|m| m.get(question).cloned()) {
        return answer;
    }
    let answer = ask();
    if let Ok(mut m) = memo.write() {
        if m.len() >= MEMO {
            m.clear();
        }
        m.insert(question.to_owned(), answer.clone());
    }
    answer
}

/// What the system's registry calls files named `name`, by their
/// extension, in the database's words where it knows the type by another
/// name (Windows' `application/x-zip-compressed` is `application/zip`).
fn system_type(name: &OsStr) -> Option<Mime> {
    static SAID: Memo = OnceLock::new();
    if !system::ASKED {
        return None;
    }
    let extension = Path::new(name).extension()?.to_str()?.to_lowercase();
    if extension.is_empty() {
        return None;
    }
    remembered(&SAID, &extension, || {
        system::by_extension(&extension).map(|m| database::unalias(&m))
    })
}

/// The types a name makes equally likely: the system's answer, or what
/// the database's own answers make of it more precisely (`refine`).
fn candidates(name: &OsStr) -> Vec<Mime> {
    refine(system_type(name), database::candidates(name))
}

/// The system's answer `said` and the database's `known` for one name.
/// The system says what the file is; the database may only say it more
/// precisely: its types that descend from the system's (a `.tar.gz` the
/// system calls gzip is a compressed tar; a `.js` an editor registered as
/// plain text is JavaScript). Where none does, the system's answer
/// stands alone; where the system says nothing, the database's.
fn refine(said: Option<Mime>, known: Vec<Mime>) -> Vec<Mime> {
    let Some(said) = said else {
        return known;
    };
    let refined: Vec<Mime> = known
        .into_iter()
        .filter(|m| database::is_a(m, &said))
        .collect();
    if refined.is_empty() {
        vec![said]
    } else {
        refined
    }
}

/// The first bytes of a regular file, as much as sniffing reads.
fn head(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut head = Vec::with_capacity(SNIFF);
    crate::file::open_regular(path)
        .ok()?
        .take(SNIFF as u64)
        .read_to_end(&mut head)
        .ok()?;
    Some(head)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_suffix_starts_at_the_first_dot_after_a_hidden_files_own() {
        assert_eq!(suffix("a.tar.gz"), Some(".tar.gz"));
        assert_eq!(suffix("photo.JPG"), Some(".JPG"));
        assert_eq!(suffix("Makefile"), None);
        assert_eq!(suffix(".bashrc"), None);
        assert_eq!(suffix(".config.toml"), Some(".toml"));
        assert_eq!(suffix(""), None);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn many_names_are_typed_by_their_suffix_and_a_few_as_they_are() {
        let names = ["a.png", "archive.tar.gz", "Makefile", "notes.md"];
        let one_by_one: Vec<Option<Mime>> = names.iter().map(|n| by_name(OsStr::new(n))).collect();
        let few = by_names(names.iter().map(OsStr::new));
        assert_eq!(few, one_by_one);
        // Past `EXACT`, the same answers, asked once per suffix.
        let many: Vec<String> = (0..EXACT * 2)
            .map(|i| format!("{i}-{}", names[i % names.len()]))
            .collect();
        let typed = by_names(many.iter().map(OsStr::new));
        for (i, answer) in typed.iter().enumerate() {
            let expected = if names[i % names.len()] == "Makefile" {
                // A whole-name pattern: `0-Makefile` is its own question.
                by_name(OsStr::new(&many[i]))
            } else {
                one_by_one[i % names.len()].clone()
            };
            assert_eq!(*answer, expected, "{}", many[i]);
        }
        assert_eq!(typed[0].as_ref().map(Mime::as_str), Some("image/png"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn names_past_the_questions_asked_are_unknown() {
        let many: Vec<String> = (0..ASKED + 10).map(|i| format!("f.x{i}q")).collect();
        let mut names: Vec<&OsStr> = many.iter().map(OsStr::new).collect();
        names.push(OsStr::new("late.png"));
        let typed = by_names(names);
        assert_eq!(typed.last(), Some(&None), "asked too many questions");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn lineage_goes_from_the_type_to_what_every_type_implies() {
        let svg = lineage(&Mime("image/svg+xml".into()));
        let at = |m: &str| svg.iter().position(|x| x.as_str() == m);
        assert_eq!(at("image/svg+xml"), Some(0));
        let xml = at("application/xml").expect("svg is xml");
        let text = at("text/plain").expect("xml is text");
        assert!(xml < text);
        assert_eq!(svg.last().unwrap().as_str(), "application/octet-stream");
        assert_eq!(
            lineage(&Mime("inode/directory".into())),
            [Mime("inode/directory".into())]
        );
        let c = lineage(&Mime("text/x-csrc".into()));
        assert!(c.contains(&Mime("text/plain".into())));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn png_by_name() {
        let m = by_name(OsStr::new("foo.png")).expect("shared-mime-info present");
        assert_eq!(m.as_str(), "image/png");
        assert_eq!(m.top_level(), "image");
        assert_eq!(m.icon_name(), "image-png");
        assert_eq!(m.generic_icon_name(), "image-x-generic");
    }

    /// An Ogg page whose first packet starts with `codec`, as far as the
    /// database's magic looks.
    #[cfg(target_os = "linux")]
    fn ogg(codec: &[u8]) -> Vec<u8> {
        let mut bytes = b"OggS".to_vec();
        bytes.resize(28, 0);
        bytes.extend_from_slice(codec);
        bytes.resize(256, 0);
        bytes
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_name_several_types_claim_has_one_answer_until_the_bytes_speak() {
        let name = OsStr::new("a tone.ogg");
        assert!(ambiguous(name));
        assert!(!ambiguous(OsStr::new("a.png")));
        assert_eq!(by_name(name).map(|m| m.0), Some("audio/ogg".into()));
        // The database hands the ties back in no particular order: every
        // order gives the same answer.
        let ties: Vec<Mime> = [
            "video/x-theora+ogg",
            "audio/x-flac+ogg",
            "audio/ogg",
            "video/ogg",
            "audio/x-vorbis+ogg",
            "audio/x-speex+ogg",
        ]
        .iter()
        .map(|m| Mime((*m).to_owned()))
        .collect();
        for rotation in 0..ties.len() {
            let mut order = ties.clone();
            order.rotate_left(rotation);
            let reversed: Vec<_> = order.iter().rev().cloned().collect();
            for order in [order, reversed] {
                assert_eq!(
                    database::representative(order).map(|m| m.0),
                    Some("audio/ogg".into())
                );
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let file = |bytes: &[u8]| {
            let path = dir.path().join("a tone.ogg");
            std::fs::write(&path, bytes).unwrap();
            identify(name, &path).map(|m| m.0)
        };
        assert_eq!(file(&ogg(b"\x01vorbis")), Some("audio/x-vorbis+ogg".into()));
        assert_eq!(file(&ogg(b"\x80theora")), Some("video/x-theora+ogg".into()));
        // Bytes that do not choose among them, or name another kind of
        // file, leave the name's answer.
        assert_eq!(file(&ogg(b"\x00nothing")), Some("audio/ogg".into()));
        assert_eq!(
            file(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"),
            Some("audio/ogg".into())
        );
        // A name one type claims is that type without a read; a name none
        // claims is the bytes'.
        let png = dir.path().join("p.png");
        assert_eq!(
            identify(OsStr::new("p.png"), &png).map(|m| m.0),
            Some("image/png".into())
        );
        let cfg = dir.path().join("s.cfg");
        std::fs::write(&cfg, "key = value\n").unwrap();
        assert_eq!(
            identify(OsStr::new("s.cfg"), &cfg).map(|m| m.0),
            Some("text/plain".into())
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_name_the_database_does_not_know_says_nothing() {
        for name in ["settings.cfg", "app.ini", "README", ".env", "Cargo.lock"] {
            assert_eq!(by_name(OsStr::new(name)), None, "{name}");
        }
        assert_eq!(
            by_name(OsStr::new("notes.txt")).map(|m| m.0),
            Some("text/plain".into())
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn the_bytes_decide_for_a_name_that_says_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = |name: &str, bytes: &[u8]| {
            let path = dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            by_content(&path).map(|m| m.0)
        };
        assert_eq!(
            file("settings.cfg", "[section]\nkey = valeur é\n".as_bytes()),
            Some("text/plain".into())
        );
        assert_eq!(
            file("blob.cfg", b"\x00\x01\x02text after a NUL"),
            Some("application/octet-stream".into())
        );
        assert_eq!(
            file("picture.cfg", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"),
            Some("image/png".into())
        );
        // A shell script, by the name the installed database gives it:
        // `application/x-shellscript` until shared-mime-info renamed it
        // `text/x-shellscript` (2.5.1 has the new name, the old one an alias).
        assert_eq!(
            file("script.cfg", b"#!/bin/sh\necho hi\n"),
            Some(unalias(&Mime("application/x-shellscript".into())).0)
        );
        assert_eq!(
            file("empty.cfg", b""),
            Some("application/x-zerosize".into())
        );
        assert_eq!(by_content(&dir.path().join("missing.cfg")), None);
        assert_eq!(by_content(dir.path()), None, "a directory has no content");
        let fifo = dir.path().join("pipe");
        rustix::fs::mknodat(
            rustix::fs::CWD,
            &fifo,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(0o600),
            0,
        )
        .unwrap();
        assert_eq!(by_content(&fifo), None, "a FIFO is refused, not waited on");
    }

    #[test]
    fn a_head_cut_through_a_character_is_still_text() {
        let text = "caf\u{e9}".repeat(3);
        let cut = &text.as_bytes()[..text.len() - 1];
        assert_eq!(text_or_binary(cut).0, "text/plain");
        assert_eq!(
            text_or_binary(&[0xff, 0xfe, 0x00, 0x00]).0,
            "application/octet-stream"
        );
    }

    #[test]
    fn the_untranslated_comment_is_the_description() {
        let xml = r#"<mime-type type="text/x-sample"><comment>plain &amp; simple document</comment>
            <comment xml:lang="fr">document</comment></mime-type>"#;
        assert_eq!(comment_of(xml).as_deref(), Some("Plain & simple document"));
        assert_eq!(comment_of("<mime-type/>"), None);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn types_named_by_hand_match_through_aliases() {
        let types = super::Types::new(&["image/x-icon", "audio/mp4"]);
        // What a listed alias stands for, and another alias of it.
        assert!(types.contains("image/vnd.microsoft.icon"));
        assert!(types.contains("image/x-ico"));
        // Another name of a listed type.
        assert!(types.contains("audio/x-m4a"));
        assert!(!types.contains("audio/mpeg"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn kinds_come_from_the_database() {
        use super::{Kind, kinds};
        let of = |name: &str| {
            by_name(OsStr::new(name))
                .map(|m| kinds(&m))
                .unwrap_or_default()
        };
        assert_eq!(of("a.png"), [Kind::Image]);
        assert_eq!(of("a.mp4"), [Kind::Video]);
        assert_eq!(of("a.flac"), [Kind::Audio]);
        assert_eq!(of("a.pdf"), [Kind::Document]);
        assert_eq!(of("a.epub"), [Kind::Document]);
        assert_eq!(of("a.odt"), [Kind::Document]);
        assert_eq!(of("a.ods"), [Kind::Spreadsheet]);
        assert_eq!(of("a.odp"), [Kind::Presentation]);
        assert_eq!(of("a.zip"), [Kind::Archive]);
        assert_eq!(of("a.tar.gz"), [Kind::Archive]);
        assert_eq!(of("a.ttf"), [Kind::Font]);
        assert_eq!(of("a.rs"), [Kind::Text], "code is text");
        assert_eq!(of("a.txt"), [Kind::Text]);
        assert_eq!(
            of("a.md"),
            [Kind::Document, Kind::Text],
            "one type, two kinds"
        );
        assert_eq!(kinds(&Mime("inode/directory".into())), [Kind::Folder]);
        assert!(of("a.unknown-extension").is_empty());
        for kind in Kind::ALL {
            assert_eq!(Kind::from_name(kind.name()), Some(kind));
        }
        assert_eq!(Kind::from_name("Photos"), Some(Kind::Image));
        assert_eq!(Kind::from_name("music"), Some(Kind::Audio));
        assert_eq!(Kind::from_name("pdf"), None);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn the_system_names_a_file_and_the_database_only_says_it_more_precisely() {
        let m = |name: &str| Mime(name.to_owned());
        let known = |name: &str| database::candidates(OsStr::new(name));
        // A compressed tar the system calls gzip; JavaScript an editor
        // registered as plain text.
        assert_eq!(
            refine(Some(m("application/gzip")), known("a.tar.gz")),
            [m("application/x-compressed-tar")]
        );
        assert_eq!(refine(Some(m("text/plain")), known("a.js")), known("a.js"));
        assert!(!known("a.js").contains(&m("text/plain")));
        // The database's answer says something else: the system's stands.
        assert_eq!(
            refine(Some(m("image/png")), known("a.txt")),
            [m("image/png")]
        );
        // The database's several answers within the system's stay, for the
        // bytes to choose among; those outside it go.
        let ogg = refine(Some(m("audio/ogg")), known("a.ogg"));
        assert!(ogg.contains(&m("audio/x-vorbis+ogg")), "{ogg:?}");
        assert!(!ogg.contains(&m("video/x-theora+ogg")), "{ogg:?}");
        // One silent: the other's.
        assert_eq!(refine(None, known("a.png")), [m("image/png")]);
        assert_eq!(
            refine(Some(m("application/x-bibli-test")), Vec::new()),
            [m("application/x-bibli-test")]
        );
        assert!(database::knows(&m("image/png")));
        assert!(
            database::knows(&m("application/x-zip-compressed")),
            "an alias"
        );
        assert!(!database::knows(&m("application/x-bibli-test")));
    }

    /// The database read on Windows, where `just test-windows` puts one
    /// beside the tests, as a package does; a machine without one says it
    /// skips.
    #[test]
    #[cfg(windows)]
    fn on_windows_the_database_answers_what_the_registry_does_not() {
        if found_database().is_none() {
            assert!(
                std::env::var_os("BIBLI_TEST_UNDER_WINE").is_none(),
                "test-windows.sh puts the dev shell's database beside the tests"
            );
            eprintln!("skipped: no type database beside the tests");
            return;
        }
        let name = |n: &str| by_name(OsStr::new(n)).map(|m| m.0);
        assert_eq!(name("a.png").as_deref(), Some("image/png"));
        assert_eq!(
            name("archive.tar.gz").as_deref(),
            Some("application/x-compressed-tar")
        );
        assert_eq!(name("a tone.ogg").as_deref(), Some("audio/ogg"));
        let svg = lineage(&Mime("image/svg+xml".into()));
        assert!(svg.iter().any(|m| m.as_str() == "text/plain"), "{svg:?}");
        assert_eq!(
            kinds(&Mime("text/markdown".into())),
            [Kind::Document, Kind::Text]
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("picture.cfg");
        std::fs::write(&path, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").unwrap();
        assert_eq!(by_content(&path).map(|m| m.0).as_deref(), Some("image/png"));
    }

    /// Writes the registry: in Wine's scratch prefix alone
    /// (`just test-windows`), never a user's.
    #[test]
    #[cfg(windows)]
    fn on_windows_the_registry_names_a_type_first() {
        use crate::windows::classes;
        /// Takes the test's classes out, whatever the test did.
        struct Classes;
        impl Drop for Classes {
            fn drop(&mut self) {
                classes::remove(".bibliqz");
                classes::remove("bibli.test.qz");
                classes::remove(r"MIME\Database\Content Type\application/x-bibli-qz");
            }
        }
        if std::env::var_os("BIBLI_TEST_UNDER_WINE").is_none() {
            eprintln!("skipped: writes the registry; only in Wine's scratch prefix");
            return;
        }
        let _classes = Classes;
        classes::set(".bibliqz", "Content Type", "Application/X-Bibli-QZ");
        classes::set(".bibliqz", "PerceivedType", "image");
        classes::set(".bibliqz", "", "bibli.test.qz");
        classes::set("bibli.test.qz", "", "Bibli test picture");
        classes::set(
            r"MIME\Database\Content Type\application/x-bibli-qz",
            "Extension",
            ".bibliqz",
        );
        let qz = by_name(OsStr::new("Some Picture.BIBLIQZ")).expect("the registry's type");
        assert_eq!(qz.as_str(), "application/x-bibli-qz", "lowercase");
        assert!(!ambiguous(OsStr::new("a.bibliqz")));
        assert_eq!(kinds(&qz), [Kind::Image], "as the registry perceives it");
        assert_eq!(extensions(&qz), ["bibliqz"]);
        assert_eq!(unalias(&qz), qz);
        assert_eq!(by_name(OsStr::new("a.bibli-nothing")), None);
        // What Explorer calls them: the name of the class they open as.
        assert_eq!(qz.description().as_deref(), Some("Bibli test picture"));
    }

    #[test]
    fn directory_check() {
        assert!(is_directory(&Mime("inode/directory".into())));
        assert!(!is_directory(&Mime("text/plain".into())));
    }

    #[test]
    fn a_type_is_named_by_its_plain_globs() {
        assert_eq!(plain_extension("*.CR2").as_deref(), Some("cr2"));
        assert_eq!(plain_extension("*.tar.gz"), None, "not one extension");
        assert_eq!(plain_extension("*.[ch]"), None);
        assert_eq!(plain_extension("README"), None);
        #[cfg(target_os = "linux")]
        {
            let heif = extensions(&Mime("image/heif".into()));
            assert!(heif.iter().any(|e| e == "heic"), "{heif:?}");
            assert!(extensions(&Mime("image/x-unknown-to-anyone".into())).is_empty());
        }
    }
}
