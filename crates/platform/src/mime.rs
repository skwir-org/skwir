//! MIME types via the shared-mime-info database.

use std::ffi::OsStr;
use std::fmt;
use std::path::Path;

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
        imp::generic_icon_name(self)
    }

    /// Human description from the database, if available.
    pub fn description(&self) -> Option<String> {
        imp::description(self)
    }
}

impl fmt::Display for Mime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Guess from the file name only. No I/O; cheap enough for every row.
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
    imp::by_name(name)
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
    imp::ambiguous(name)
}

/// Selections no larger than this have each name asked as it is.
const EXACT: usize = 64;
/// Distinct questions one [`by_names`] asks at most.
const ASKED: usize = 256;

/// [`by_name`] for many names at once (a selection), in their order.
///
/// The database takes tens of microseconds a name, so a large selection
/// cannot be asked name by name on the UI thread. Up to `EXACT` names are
/// each asked as they are; beyond, a name is asked once per suffix from its
/// first dot (`.jpg`, `.tar.gz`), as any file with that suffix would be,
/// so a name only a whole-name pattern knows (`CMakeLists.txt`) takes its
/// suffix's type there. A name without a suffix is asked as it is. At most
/// `ASKED` distinct questions: the names past them are unknown (`None`),
/// which bounds the call whatever the selection. No I/O once the database
/// is loaded.
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
    imp::identify(name, path)
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
    imp::by_content(path)
}

/// Bytes read to decide what a file without a known name holds. The
/// database's deepest magic rules look further, but at that distance the
/// name usually says it first; the text rule is git's (a NUL in the first
/// 8000 bytes).
#[cfg(target_os = "linux")]
const SNIFF: usize = 8 * 1024;

/// `text/plain` or `application/octet-stream` from a file's head: the one
/// text-or-binary rule, the same one the preview applies to a declared type
/// (`crate::inspection::is_text`).
#[cfg(any(target_os = "linux", test))]
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
#[cfg(any(target_os = "linux", test))]
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
    use std::collections::HashMap;
    use std::sync::{OnceLock, RwLock};
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
    let mime = imp::unalias(mime);
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
    by_icon.map_or(0, Kind::bit) | if text { Kind::Text.bit() } else { 0 }
}

pub fn is_directory(mime: &Mime) -> bool {
    mime.0 == "inode/directory"
}

/// The type `mime` is another name for (`model/x.stl-binary` is
/// `model/stl`), or `mime` itself. No I/O once the database is loaded.
pub fn unalias(mime: &Mime) -> Mime {
    imp::unalias(mime)
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

/// The extensions the MIME database names files of type `mime` by, from
/// its plain `*.ext` globs, lowercase, in the database's order (heaviest
/// first, the most specific folder first). Aliases are resolved. No I/O
/// once the database is loaded.
pub fn extensions(mime: &Mime) -> Vec<String> {
    imp::extensions(&imp::unalias(mime))
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
    let mut out = vec![imp::unalias(mime)];
    let mut next = 0;
    while next < out.len() {
        for parent in imp::parents(&out[next]) {
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

#[cfg(target_os = "linux")]
mod imp {
    use std::ffi::OsStr;
    use std::path::Path;
    use std::str::FromStr;
    use std::sync::OnceLock;

    use super::Mime;

    fn db() -> &'static xdg_mime::SharedMimeInfo {
        static DB: OnceLock<xdg_mime::SharedMimeInfo> = OnceLock::new();
        DB.get_or_init(xdg_mime::SharedMimeInfo::new)
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
    fn candidates(name: &OsStr) -> Vec<mime::Mime> {
        let Some(name) = name.to_str() else {
            return Vec::new();
        };
        let mut all = db().get_mime_types_from_file_name(name);
        all.retain(|m| *m != mime::APPLICATION_OCTET_STREAM);
        all
    }

    /// The most general of equally likely types: the one most of the others
    /// descend from (`audio/ogg` over Vorbis, FLAC and Speex), the name as
    /// the last word, so the choice never depends on the order they came in.
    pub(super) fn representative(mut candidates: Vec<mime::Mime>) -> Option<mime::Mime> {
        if candidates.len() < 2 {
            return candidates.pop();
        }
        let descendants = |base: &mime::Mime| {
            candidates
                .iter()
                .filter(|m| *m != base && db().mime_type_subclass(m, base))
                .count()
        };
        candidates
            .iter()
            .max_by(|a, b| {
                descendants(a)
                    .cmp(&descendants(b))
                    .then_with(|| b.essence_str().cmp(a.essence_str()))
            })
            .cloned()
    }

    pub fn by_name(name: &OsStr) -> Option<Mime> {
        representative(candidates(name)).as_ref().map(to_mime)
    }

    pub fn ambiguous(name: &OsStr) -> bool {
        candidates(name).len() > 1
    }

    pub fn identify(name: &OsStr, path: &Path) -> Option<Mime> {
        let candidates = candidates(name);
        if candidates.len() < 2 {
            return match candidates.first() {
                Some(only) => Some(to_mime(only)),
                None => by_content(path),
            };
        }
        // The bytes choose only among what the name allows, or something
        // more precise than one of those: magic that names another kind of
        // file altogether does not outvote an extension several types share.
        let sniffed = head(path)
            .and_then(|head| db().get_mime_type_for_data(&head))
            .map(|(m, _)| m)
            .filter(|m| candidates.iter().any(|c| db().mime_type_subclass(m, c)));
        sniffed
            .or_else(|| representative(candidates))
            .as_ref()
            .map(to_mime)
    }

    /// The first bytes of a regular file, as much as sniffing reads.
    fn head(path: &Path) -> Option<Vec<u8>> {
        use std::io::Read;
        let mut head = Vec::with_capacity(super::SNIFF);
        crate::file::open_regular(path)
            .ok()?
            .take(super::SNIFF as u64)
            .read_to_end(&mut head)
            .ok()?;
        Some(head)
    }

    pub fn by_content(path: &Path) -> Option<Mime> {
        let head = head(path)?;
        // Magic, which also names an empty file (application/x-zerosize).
        // There is no magic for text/plain: what it does not know is text
        // or binary by the bytes.
        Some(
            db().get_mime_type_for_data(&head)
                .map_or_else(|| super::text_or_binary(&head), |(m, _)| to_mime(&m)),
        )
    }

    pub fn unalias(m: &Mime) -> Mime {
        parse(m)
            .and_then(|p| db().unalias_mime_type(&p))
            .map_or_else(|| m.clone(), |p| to_mime(&p))
    }

    /// The declared parents of `m`, not `m` itself. xdg-mime 0.4 keeps its
    /// subclass table private and its `get_parents` answers only for
    /// aliases, so the database's `subclasses` files are read here, once.
    pub fn parents(m: &Mime) -> Vec<Mime> {
        static PARENTS: OnceLock<std::collections::HashMap<String, Vec<String>>> = OnceLock::new();
        PARENTS
            .get_or_init(|| {
                let mut map = std::collections::HashMap::<String, Vec<String>>::new();
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

    /// What `extensions` answers, from every folder's `globs2` (lines of
    /// `weight:type:glob[:flags]`), read once.
    pub fn extensions(m: &Mime) -> Vec<String> {
        static EXTENSIONS: OnceLock<std::collections::HashMap<String, Vec<String>>> =
            OnceLock::new();
        EXTENSIONS
            .get_or_init(|| {
                let mut map = std::collections::HashMap::<String, Vec<String>>::new();
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

    /// `$XDG_DATA_HOME`, then `$XDG_DATA_DIRS`: where the database lives.
    fn data_dirs() -> Vec<std::path::PathBuf> {
        crate::xdg::data_search()
    }

    pub fn generic_icon_name(m: &Mime) -> String {
        parse(m)
            .and_then(|p| db().lookup_generic_icon_name(&p))
            .unwrap_or_else(|| format!("{}-x-generic", m.top_level()))
    }

    /// xdg-mime 0.4 does not expose the `<comment>` element, so read it from
    /// the per-type XML the database installs. Reads a file: call from a
    /// worker and cache the answer.
    pub fn description(m: &Mime) -> Option<String> {
        data_dirs().into_iter().find_map(|dir| {
            let xml =
                std::fs::read_to_string(dir.join("mime").join(format!("{}.xml", m.0))).ok()?;
            super::comment_of(&xml)
        })
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::ffi::OsStr;
    use std::path::Path;

    use super::Mime;

    pub fn by_name(_name: &OsStr) -> Option<Mime> {
        None
    }
    pub fn by_content(_path: &Path) -> Option<Mime> {
        None
    }
    pub fn ambiguous(_name: &OsStr) -> bool {
        false
    }
    pub fn identify(_name: &OsStr, _path: &Path) -> Option<Mime> {
        None
    }
    pub fn unalias(m: &Mime) -> Mime {
        m.clone()
    }
    pub fn parents(_m: &Mime) -> Vec<Mime> {
        Vec::new()
    }
    pub fn extensions(_m: &Mime) -> Vec<String> {
        Vec::new()
    }
    pub fn generic_icon_name(m: &Mime) -> String {
        format!("{}-x-generic", m.top_level())
    }
    pub fn description(_m: &Mime) -> Option<String> {
        None
    }
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
        let ties: Vec<mime::Mime> = [
            "video/x-theora+ogg",
            "audio/x-flac+ogg",
            "audio/ogg",
            "video/ogg",
            "audio/x-vorbis+ogg",
            "audio/x-speex+ogg",
        ]
        .iter()
        .map(|m| m.parse().unwrap())
        .collect();
        for rotation in 0..ties.len() {
            let mut order = ties.clone();
            order.rotate_left(rotation);
            let reversed: Vec<_> = order.iter().rev().cloned().collect();
            for order in [order, reversed] {
                assert_eq!(
                    imp::representative(order).map(|m| m.essence_str().to_owned()),
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
