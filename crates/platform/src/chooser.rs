//! Other programs' open and save dialogs (ADR-141): skwir as the
//! xdg-desktop-portal's `FileChooser` back end. The portal hands a
//! request (`OpenFile`, `SaveFile`, `SaveFiles`) to the name skwir's
//! `.portal` file names, which the program that takes launches over holds
//! (`handover`); what is asked arrives as data ([`Asked`]), through the
//! gate launches pass, as a [`Choosing`] the window answers once the user
//! decides, or the asking program gives up (`Request.Close`).
//!
//! The request's options are read as the portal writes them
//! (`a{sv}`), each key optional; what skwir does not know is passed over.
//! Linux alone, by nature (ADR-141, 11): elsewhere nothing asks.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// The argument the bus starts skwir with for the portal: the program
/// starts without a window and opens one for the request (ADR-141, 3).
pub const STARTED_BY_PORTAL: &str = "--portal";

/// The bus name skwir's `.portal` file names.
pub const BACK_END: &str = "org.freedesktop.impl.portal.desktop.skwir";

/// What a request asks chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Files to open, or folders (`directory`); several with `multiple`.
    Open { multiple: bool, directory: bool },
    /// A file to save: the name suggested, and the file saved again
    /// (its folder and name), if any.
    Save {
        name: Option<String>,
        file: Option<PathBuf>,
    },
    /// A folder to save `files` in, each under its name.
    SaveFiles { files: Vec<OsString> },
}

/// One of the program's filters: its name, and what it admits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    pub name: String,
    pub patterns: Vec<Pattern>,
}

/// What a filter admits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pattern {
    /// A name's pattern (`*.png`), its case as written.
    Glob(String),
    /// A type (`image/png`), or a family of them (`image/*`).
    Mime(String),
}

impl Pattern {
    /// Whether it takes every file, as a program says All files: a
    /// name's pattern of stars alone (`*`, GTK's and Qt's), the type of
    /// every type (`*/*`), or the type every file's descends from
    /// (`application/octet-stream`, Qt's All files where a program gives
    /// its filters as types). What `Filter::admits`, `admits_all` and
    /// `naming` all take it for.
    fn takes_every(&self) -> bool {
        match self {
            Pattern::Glob(glob) => !glob.is_empty() && glob.chars().all(|c| c == '*'),
            Pattern::Mime(mime) => {
                mime == "*/*" || mime.eq_ignore_ascii_case("application/octet-stream")
            }
        }
    }
}

impl Filter {
    /// Whether it admits `entry`, made once for the entries it is asked
    /// about (every file of a listing): a name by the globs, case ignored
    /// (a camera's `.JPG` is the program's `*.jpg`), a type by the types,
    /// its kind (`image/*`) or what it descends from (ADR-141, 6), each
    /// suffix typed once and each type matched once. A pattern that
    /// takes every file (`*`, `*/*`, `application/octet-stream`:
    /// `Pattern::takes_every`) admits every file, a file of a type
    /// unknown too. A pattern that does not compile admits nothing.
    pub fn admits(&self) -> impl Fn(&skwir_core::Entry) -> bool + Send + Sync + 'static {
        let mut globs = globset::GlobSetBuilder::new();
        let mut types = Vec::new();
        let mut every = false;
        for pattern in &self.patterns {
            match pattern {
                pattern if pattern.takes_every() => every = true,
                Pattern::Glob(glob) => {
                    if let Ok(glob) = globset::GlobBuilder::new(glob)
                        .case_insensitive(true)
                        .literal_separator(true)
                        .build()
                    {
                        globs.add(glob);
                    }
                }
                Pattern::Mime(mime) => types.push(mime.to_ascii_lowercase()),
            }
        }
        let globs = globs.build().unwrap_or_else(|_| globset::GlobSet::empty());
        let by_suffix = crate::mime::BySuffix::default();
        let matched = parking_lot::Mutex::new(std::collections::HashMap::<String, bool>::new());
        move |entry| {
            if every {
                return true;
            }
            if !globs.is_empty() && globs.is_match(std::path::Path::new(&entry.name)) {
                return true;
            }
            if types.is_empty() {
                return false;
            }
            let mime = match entry.inode_type() {
                Some(mime) => Some(crate::mime::Mime(mime.to_owned())),
                None => by_suffix.of(entry.type_name()),
            };
            let Some(mime) = mime else {
                return false;
            };
            if let Some(known) = matched.lock().get(mime.as_str()) {
                return *known;
            }
            let lineage = crate::mime::lineage(&mime);
            let admitted = types.iter().any(|wanted| {
                if let Some(top) = wanted.strip_suffix("/*") {
                    return lineage.iter().any(|m| {
                        m.as_str() != "application/octet-stream"
                            && m.top_level().eq_ignore_ascii_case(top)
                    });
                }
                let wanted = crate::mime::unalias(&crate::mime::Mime(wanted.clone()));
                lineage
                    .iter()
                    .any(|m| m.as_str().eq_ignore_ascii_case(wanted.as_str()))
            });
            matched.lock().insert(mime.as_str().to_owned(), admitted);
            admitted
        }
    }
}

impl Filter {
    /// Whether it admits every file: one of its patterns one that takes
    /// every file (`*`, `*/*`, `application/octet-stream`:
    /// `Pattern::takes_every`). The program's own All files, which the
    /// window shows in place of its own, and which tests no file.
    pub fn admits_all(&self) -> bool {
        self.patterns.iter().any(Pattern::takes_every)
    }

    /// Whether it asks for pictures or films alone (ADR-144, 5): each of
    /// its patterns a type of either kind (`image/*`, `video/mp4`, what
    /// descends from one), or a name's suffix (`*.png`, GTK's
    /// `*.[pP][nN][gG]`) the type database gives either. A pattern of any
    /// other shape asks for what cannot be told. No I/O once the database
    /// is loaded.
    pub fn asks_pictures(&self) -> bool {
        use crate::mime::{Kind, Mime};
        let pictures = |mime: &Mime| {
            crate::mime::is_kind(mime, Kind::Image) || crate::mime::is_kind(mime, Kind::Video)
        };
        !self.patterns.is_empty()
            && self.patterns.iter().all(|pattern| match pattern {
                Pattern::Mime(mime) => {
                    let mime = mime.to_ascii_lowercase();
                    matches!(mime.as_str(), "image/*" | "video/*") || pictures(&Mime(mime))
                }
                Pattern::Glob(glob) => suffix_of(glob).is_some_and(|suffix| {
                    crate::mime::by_name(std::ffi::OsStr::new(&format!("a.{suffix}")))
                        .is_some_and(|mime| pictures(&mime))
                }),
            })
    }

    /// The extension its patterns give a name saved through it (ADR-144,
    /// 6): its first suffix pattern's (`*.png`, `*.[pP][nN][gG]`), else
    /// its first type's first extension in the type database; none for a
    /// filter of neither (`*`, `image/*`).
    pub fn extension(&self) -> Option<String> {
        self.patterns
            .iter()
            .find_map(|pattern| match pattern {
                Pattern::Glob(glob) => suffix_of(glob),
                Pattern::Mime(_) => None,
            })
            .or_else(|| {
                self.patterns.iter().find_map(|pattern| match pattern {
                    Pattern::Mime(mime) if !mime.ends_with("/*") => {
                        crate::mime::extensions(&crate::mime::Mime(mime.to_ascii_lowercase()))
                            .into_iter()
                            .next()
                    }
                    _ => None,
                })
            })
    }

    /// It with only the patterns that name what they take: none that
    /// takes every file (the program's All files, `*`, `*/*` or
    /// `application/octet-stream`: `Pattern::takes_every`) nor one of
    /// stars and dots alone (`*.*`, every name with an extension). What
    /// takes every extension makes none the program's (ADR-144, 6).
    fn naming(&self) -> Filter {
        Filter {
            name: self.name.clone(),
            patterns: self
                .patterns
                .iter()
                .filter(|pattern| {
                    let dotted = matches!(pattern, Pattern::Glob(glob)
                        if glob.chars().all(|c| c == '*' || c == '.'));
                    !pattern.takes_every() && !dotted
                })
                .cloned()
                .collect(),
        }
    }

    /// The extensions its suffix patterns name, lowercase: what a name
    /// saved through it ends with.
    pub fn suffixes(&self) -> Vec<String> {
        self.patterns
            .iter()
            .filter_map(|pattern| match pattern {
                Pattern::Glob(glob) => suffix_of(glob),
                Pattern::Mime(_) => None,
            })
            .collect()
    }
}

/// The name a file saved as `name` takes as the program's filter `to` is
/// chosen (ADR-144, 6): its extension swapped for the one `to` gives
/// (`Filter::extension`) where one of the program's `filters` takes a file
/// of that name and `to` does not, as each admits a file; a compound
/// extension the type database knows, or a filter names (`tar.gz`),
/// swapped whole. `None` where it stays: `to` takes it already or gives no
/// extension (`*`, `image/*`), none of the program's takes it (an
/// extension of the user's own: the program's All files, `*`, `*/*` or
/// `application/octet-stream`, and a `*.*` take every extension and name
/// none, `Filter::naming`), it has
/// none. Its bytes are kept, but the extension's. No I/O once the type
/// database is loaded.
pub fn swapped_name(name: &std::ffi::OsStr, filters: &[Filter], to: &Filter) -> Option<OsString> {
    let new = to.extension()?;
    let file = skwir_core::Entry::new(
        skwir_core::EntryId::UNKNOWN,
        name,
        skwir_core::entry::EntryKind::File,
    );
    if to.admits()(&file) || !filters.iter().any(|filter| filter.naming().admits()(&file)) {
        return None;
    }
    let bytes = name.as_encoded_bytes();
    let cut = extension_at(bytes, filters)?;
    let mut swapped = bytes[..cut].to_vec();
    swapped.push(b'.');
    swapped.extend_from_slice(new.as_bytes());
    Some(skwir_core::os_path::from_bytes(swapped))
}

/// Where `name`'s extension starts (its dot): the longest compound one
/// the type database knows for its last (`tar.gz` for `gz`) or one of
/// `filters` names, case ignored, else its last. None for a name of no
/// extension, or nothing but one (`.bashrc`).
fn extension_at(name: &[u8], filters: &[Filter]) -> Option<usize> {
    let dot = name.iter().rposition(|b| *b == b'.')?;
    if dot == 0 || dot + 1 == name.len() {
        return None;
    }
    let mut compounds: Vec<String> = std::str::from_utf8(&name[dot + 1..])
        .map(crate::mime::compound_suffixes)
        .unwrap_or_default();
    compounds.extend(
        filters
            .iter()
            .flat_map(Filter::suffixes)
            .filter(|suffix| suffix.contains('.')),
    );
    compounds.sort_by_key(|suffix| std::cmp::Reverse(suffix.len()));
    let compound = compounds.iter().find_map(|suffix| {
        let cut = name.len().checked_sub(suffix.len() + 1)?;
        (cut > 0 && name[cut] == b'.' && name[cut + 1..].eq_ignore_ascii_case(suffix.as_bytes()))
            .then_some(cut)
    });
    Some(compound.unwrap_or(dot))
}

/// The suffix a pattern names, lowercase, if it is one name's suffix and
/// nothing else: `*.png`, `*.tar.gz`, GTK's `*.[pP][nN][gG]` (a letter's
/// two cases in brackets, as GTK writes a suffix matched in either case).
fn suffix_of(glob: &str) -> Option<String> {
    let rest = glob.strip_prefix("*.")?;
    let mut suffix = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '[' => {
                let (a, b, close) = (chars.next()?, chars.next()?, chars.next()?);
                if close != ']' || !a.is_alphabetic() || a.to_lowercase().ne(b.to_lowercase()) {
                    return None;
                }
                suffix.extend(a.to_lowercase());
            }
            '*' | '?' | ']' | '/' | '\\' | '{' | '}' => return None,
            c => suffix.extend(c.to_lowercase()),
        }
    }
    (!suffix.is_empty() && !suffix.starts_with('.') && !suffix.ends_with('.')).then_some(suffix)
}

/// One of the program's choices: a dropdown of `options` (id, label), or
/// a switch where there are none (`true`, `false`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub label: String,
    pub options: Vec<(String, String)>,
    /// What is chosen at first; empty to let skwir choose.
    pub initial: String,
}

impl Choice {
    /// A switch rather than a dropdown.
    pub fn is_switch(&self) -> bool {
        self.options.is_empty()
    }
}

/// What a request asks, as data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asked {
    /// The asking program's id (`org.mozilla.firefox`); empty for a
    /// program of the host's.
    pub app_id: String,
    /// The asking window (`x11:<XID>`, `wayland:<handle>`), or empty.
    pub parent_window: String,
    pub title: String,
    pub accept_label: Option<String>,
    pub kind: Kind,
    pub filters: Vec<Filter>,
    pub current_filter: Option<Filter>,
    pub choices: Vec<Choice>,
    /// Where to start: the folder suggested, or the saved file's.
    pub folder: Option<PathBuf>,
    /// Opening files: whether the desktop's recently used files had
    /// something to give as it asked (`recent::offered`, read then, on a
    /// thread of its own): Recent comes first among the places and is
    /// where a request naming no folder may start (ADR-144, 7). Not what
    /// the program asks: what the desktop said as it asked.
    pub recent: bool,
}

impl Asked {
    /// The name a save starts with: the file saved again's, else the one
    /// suggested (ADR-141, 5); as bytes, as a folder holds it (a name not
    /// UTF-8 is that file's, never another's said alike).
    pub fn save_name(&self) -> Option<OsString> {
        match &self.kind {
            Kind::Save { name, file } => file
                .as_deref()
                .and_then(std::path::Path::file_name)
                .map(std::ffi::OsStr::to_os_string)
                .or_else(|| name.clone().map(OsString::from)),
            _ => None,
        }
    }

    /// The program's own All files (`Filter::admits_all`), its first if it
    /// sent several: what stands for every file in its list, where the
    /// window adds none of its own, so that the answer names the filter
    /// the program gave.
    pub fn all_files(&self) -> Option<usize> {
        self.filters.iter().position(Filter::admits_all)
    }

    /// The filter shown at first: the one in use, else the first.
    pub fn first_filter(&self) -> Option<usize> {
        if self.filters.is_empty() {
            return None;
        }
        let current = self.current_filter.as_ref();
        Some(
            current
                .and_then(|current| self.filters.iter().position(|f| f == current))
                .unwrap_or(0),
        )
    }
}

/// What the user decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// The paths chosen (local files), the choices as set (id, value), and
    /// the filter in use.
    Chosen {
        paths: Vec<PathBuf>,
        choices: Vec<(String, String)>,
        filter: Option<Filter>,
    },
    Cancelled,
    /// Not answered: what took it went before deciding.
    Failed,
}

/// A request under way: what it asks, and how the answer goes back. Its
/// first answer is the one sent; a request every copy of which is let go
/// unanswered answers [`Answer::Failed`].
#[derive(Clone)]
pub struct Choosing {
    asked: Arc<Asked>,
    holder: Arc<Holder>,
    id: u64,
}

impl std::fmt::Debug for Choosing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Choosing")
            .field("id", &self.id)
            .field("asked", &self.asked)
            .finish_non_exhaustive()
    }
}

impl Choosing {
    pub fn asked(&self) -> &Asked {
        &self.asked
    }

    /// The request's own number, for this program's life.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Send `answer`, unless one was sent or the request closed.
    pub fn answer(&self, answer: Answer) {
        self.holder.reply.settle(answer);
    }

    /// The asking program gave up (`Request.Close`): the dialog goes.
    pub fn is_closed(&self) -> bool {
        self.holder.reply.state.lock().closed
    }

    /// Whether it was answered, or closed.
    pub fn is_settled(&self) -> bool {
        self.holder.reply.state.lock().answer.is_some()
    }

    /// Call `hook` once the asking program gives up, at once if it has;
    /// on the thread that is told, so it must return at once (wake).
    pub fn when_closed(&self, hook: impl Fn() + Send + Sync + 'static) {
        let mut state = self.holder.reply.state.lock();
        if state.closed {
            drop(state);
            hook();
            return;
        }
        state.on_closed.push(Box::new(hook));
    }

    /// What was answered, if it was: for who asked in a test.
    pub fn answered(&self) -> Option<Answer> {
        self.holder.reply.state.lock().answer.clone()
    }

    /// A request asking `asked`, answered to nobody: for tests. The
    /// portal makes the others.
    #[must_use]
    pub fn new(asked: Asked) -> Choosing {
        Choosing::made(asked).0
    }

    fn made(asked: Asked) -> (Choosing, Arc<Reply>) {
        let reply = Arc::new(Reply::default());
        (Choosing::around(asked, Arc::clone(&reply)), reply)
    }

    /// The request asking `asked`, answered at `reply`.
    fn around(asked: Asked, reply: Arc<Reply>) -> Choosing {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Choosing {
            asked: Arc::new(asked),
            holder: Arc::new(Holder { reply }),
            id: NEXT.fetch_add(1, Ordering::Relaxed),
        }
    }
}

/// What every copy of a request holds: let go of by all unanswered, it
/// answers that it failed.
struct Holder {
    reply: Arc<Reply>,
}

impl Drop for Holder {
    fn drop(&mut self) {
        self.reply.settle(Answer::Failed);
    }
}

/// Where a request's answer waits.
#[derive(Default)]
pub(crate) struct Reply {
    state: parking_lot::Mutex<State>,
    #[cfg(target_os = "linux")]
    told: event_listener::Event,
}

#[derive(Default)]
struct State {
    answer: Option<Answer>,
    closed: bool,
    on_closed: Vec<Box<dyn Fn() + Send + Sync>>,
}

impl Reply {
    fn settle(&self, answer: Answer) {
        let mut state = self.state.lock();
        if state.answer.is_none() {
            state.answer = Some(answer);
            drop(state);
            #[cfg(target_os = "linux")]
            self.told.notify(usize::MAX);
        }
    }

    /// The asking program gave up: answered cancelled, and told to those
    /// who wait to close the dialog. Only the portal closes one (Linux).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn close(&self) {
        let hooks = {
            let mut state = self.state.lock();
            if state.closed {
                return;
            }
            state.closed = true;
            if state.answer.is_none() {
                state.answer = Some(Answer::Cancelled);
            }
            std::mem::take(&mut state.on_closed)
        };
        #[cfg(target_os = "linux")]
        self.told.notify(usize::MAX);
        for hook in hooks {
            hook();
        }
    }

    /// The answer, once there is one.
    #[cfg(target_os = "linux")]
    pub(crate) async fn wait(&self) -> Answer {
        loop {
            let listener = self.told.listen();
            if let Some(answer) = self.state.lock().answer.clone() {
                return answer;
            }
            listener.await;
        }
    }
}

#[cfg(target_os = "linux")]
pub(crate) mod linux {
    use std::path::PathBuf;
    use std::sync::Arc;

    use zbus::zvariant::{DeserializeDict, OwnedObjectPath, SerializeDict, Type};

    use super::{Answer, Asked, Choice, Choosing, Filter, Kind, Pattern, Reply};

    /// Where the portal calls the back end.
    pub(crate) const PATH: &str = "/org/freedesktop/portal/desktop";

    /// Which of the portal's methods asked; as a number between two
    /// skwirs (`Finishing.Choose`).
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) enum Method {
        #[default]
        Open,
        Save,
        SaveFiles,
    }

    impl Method {
        pub(crate) fn number(self) -> u32 {
            match self {
                Method::Open => 0,
                Method::Save => 1,
                Method::SaveFiles => 2,
            }
        }

        pub(crate) fn of(number: u32) -> Method {
            match number {
                1 => Method::Save,
                2 => Method::SaveFiles,
                _ => Method::Open,
            }
        }
    }

    /// A filter as the portal writes it: a name, and (0 for a pattern, 1
    /// for a type, the text).
    pub(crate) type RawFilter = (String, Vec<(u32, String)>);

    /// The options the portal passes, each key optional; others are
    /// passed over.
    #[derive(Clone, Debug, Default, DeserializeDict, SerializeDict, Type)]
    #[zvariant(signature = "dict")]
    pub(crate) struct Options {
        pub(crate) accept_label: Option<String>,
        /// Read to be passed on whole (`Finishing.Choose`), not followed:
        /// the window is the asking window's child, kept over it, never
        /// holding it (ADR-141, 9).
        pub(crate) modal: Option<bool>,
        pub(crate) multiple: Option<bool>,
        pub(crate) directory: Option<bool>,
        pub(crate) filters: Option<Vec<RawFilter>>,
        pub(crate) current_filter: Option<RawFilter>,
        pub(crate) choices: Option<Vec<(String, String, Vec<(String, String)>, String)>>,
        pub(crate) current_name: Option<String>,
        pub(crate) current_folder: Option<Vec<u8>>,
        pub(crate) current_file: Option<Vec<u8>>,
        pub(crate) files: Option<Vec<Vec<u8>>>,
    }

    /// What the back end answers, each key given only where it says
    /// something.
    #[derive(Clone, Debug, Default, DeserializeDict, SerializeDict, Type)]
    #[zvariant(signature = "dict")]
    pub(crate) struct Results {
        pub(crate) uris: Option<Vec<String>>,
        pub(crate) choices: Option<Vec<(String, String)>>,
        pub(crate) current_filter: Option<RawFilter>,
        /// Whether the program may write what was chosen: always, said
        /// where something is chosen. The portal's interface says a
        /// back end that leaves it out gives read access alone (its
        /// documented default), which a portal reading it so would give
        /// a sandboxed editor; 1.22 reads its absence as yes.
        pub(crate) writable: Option<bool>,
    }

    /// The request as the portal gave it.
    #[derive(Clone, Debug, Default)]
    pub(crate) struct Raw {
        pub(crate) method: Method,
        pub(crate) app_id: String,
        pub(crate) parent_window: String,
        pub(crate) title: String,
        pub(crate) options: Options,
    }

    /// What `raw` asks, as data. None when a name the program gives
    /// (`current_name`, each of `files`) is not one name (`one_name`): it
    /// would put the file outside the folder the user chooses, and only
    /// what the user types may be a path (ADR-141, 5).
    pub(crate) fn asked(raw: &Raw) -> Option<Asked> {
        let options = &raw.options;
        let file = options.current_file.as_deref().and_then(path);
        let kind = match raw.method {
            Method::Open => Kind::Open {
                multiple: options.multiple.unwrap_or(false),
                directory: options.directory.unwrap_or(false),
            },
            Method::Save => {
                let name = options.current_name.clone().filter(|name| !name.is_empty());
                if name
                    .as_deref()
                    .is_some_and(|name| !one_name(std::ffi::OsStr::new(name)))
                {
                    return None;
                }
                Kind::Save {
                    name,
                    file: file.clone(),
                }
            }
            Method::SaveFiles => Kind::SaveFiles {
                files: options
                    .files
                    .iter()
                    .flatten()
                    .map(|name| {
                        path(name)
                            .map(PathBuf::into_os_string)
                            .filter(|name| one_name(name))
                    })
                    .collect::<Option<Vec<_>>>()?,
            },
        };
        // The file saved again first: its folder and its name (ADR-141, 5).
        let folder = file
            .as_deref()
            .and_then(|f| f.parent())
            .map(std::path::Path::to_path_buf)
            .or_else(|| options.current_folder.as_deref().and_then(path));
        Some(Asked {
            app_id: raw.app_id.clone(),
            parent_window: raw.parent_window.clone(),
            title: raw.title.clone(),
            accept_label: options
                .accept_label
                .clone()
                .filter(|label| !label.is_empty()),
            kind,
            // A filter in use with no list is applied all the same: the
            // list is that one (the portal's documentation).
            filters: match (&options.filters, &options.current_filter) {
                (Some(filters), _) if !filters.is_empty() => filters.iter().map(filter).collect(),
                (_, Some(current)) => vec![filter(current)],
                _ => Vec::new(),
            },
            current_filter: options.current_filter.as_ref().map(filter),
            choices: options
                .choices
                .iter()
                .flatten()
                .map(|(id, label, options, initial)| Choice {
                    id: id.clone(),
                    label: label.clone(),
                    options: options.clone(),
                    initial: initial.clone(),
                })
                .collect(),
            folder,
            recent: false,
        })
    }

    /// Whether `name` is one name, as a folder holds it: not empty, no
    /// `/`, neither `.` nor `..`.
    fn one_name(name: &std::ffi::OsStr) -> bool {
        let mut parts = std::path::Path::new(name).components();
        matches!(parts.next(), Some(std::path::Component::Normal(part)) if part == name)
            && parts.next().is_none()
    }

    fn filter((name, patterns): &RawFilter) -> Filter {
        Filter {
            name: name.clone(),
            patterns: patterns
                .iter()
                .map(|(kind, text)| match kind {
                    1 => Pattern::Mime(text.clone()),
                    _ => Pattern::Glob(text.clone()),
                })
                .collect(),
        }
    }

    fn raw_filter(filter: &Filter) -> RawFilter {
        (
            filter.name.clone(),
            filter
                .patterns
                .iter()
                .map(|pattern| match pattern {
                    Pattern::Glob(glob) => (0, glob.clone()),
                    Pattern::Mime(mime) => (1, mime.clone()),
                })
                .collect(),
        )
    }

    /// A path the portal wrote as bytes, a nul at its end; none when
    /// empty.
    fn path(bytes: &[u8]) -> Option<PathBuf> {
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        let bytes = &bytes[..end];
        (!bytes.is_empty()).then(|| PathBuf::from(skwir_core::os_path::from_bytes(bytes.to_vec())))
    }

    /// What the portal is answered for `answer`: 0 and the URIs chosen,
    /// 1 cancelled, 2 anything else.
    pub(crate) fn results(answer: Answer) -> (u32, Results) {
        match answer {
            Answer::Chosen {
                paths,
                choices,
                filter,
            } => (
                0,
                Results {
                    uris: Some(
                        paths
                            .iter()
                            .map(|path| crate::uri::file_uri(path))
                            .collect(),
                    ),
                    choices: (!choices.is_empty()).then_some(choices),
                    current_filter: filter.as_ref().map(raw_filter),
                    // As KDE's back end answers: what the user chose is
                    // the program's to write, saved over or opened.
                    writable: Some(true),
                },
            ),
            Answer::Cancelled => (1, Results::default()),
            Answer::Failed => (2, Results::default()),
        }
    }

    /// The answer `results` say, as another skwir gave them.
    #[cfg(test)]
    pub(crate) fn answer_of(response: u32, results: Results) -> Answer {
        match response {
            0 => Answer::Chosen {
                paths: results
                    .uris
                    .iter()
                    .flatten()
                    .filter_map(|uri| crate::uri::local_path_of(uri))
                    .collect(),
                choices: results.choices.unwrap_or_default(),
                filter: results.current_filter.as_ref().map(filter),
            },
            1 => Answer::Cancelled,
            _ => Answer::Failed,
        }
    }

    /// What the portal calls to close a request's dialog, at its handle.
    pub(crate) struct Request(pub(crate) Arc<Reply>);

    #[zbus::interface(name = "org.freedesktop.impl.portal.Request")]
    impl Request {
        fn close(&self) {
            self.0.close();
        }
    }

    /// Ask what `raw` asks of `take`, the request's dialog closable at
    /// `handle` on `server` meanwhile, and wait for its answer.
    pub(crate) async fn ask(
        server: &zbus::ObjectServer,
        handle: OwnedObjectPath,
        raw: Raw,
        take: impl FnOnce(Choosing) -> bool,
    ) -> (u32, Results) {
        let Some(mut asked) = asked(&raw) else {
            // Its window never opens: the name it gives is not one name.
            tracing::info!("another program's dialog gave a name that is a path: refused");
            return results(Answer::Failed);
        };
        let reply = Arc::new(Reply::default());
        let exported = server.at(&handle, Request(Arc::clone(&reply))).await;
        if let Err(error) = &exported {
            tracing::debug!("a request's dialog cannot be closed by its program: {error}");
        }
        if matches!(
            asked.kind,
            Kind::Open {
                directory: false,
                ..
            }
        ) {
            asked.recent = recent_offered().await;
        }
        let choosing = Choosing::around(asked, Arc::clone(&reply));
        // Given up on meanwhile: no window opens for it, and it is answered
        // as it was closed.
        let closed = choosing.is_settled();
        if closed || !take(choosing) {
            let _ = server.remove::<Request, _>(&handle).await;
            return results(if closed {
                reply.wait().await
            } else {
                Answer::Failed
            });
        }
        let answer = reply.wait().await;
        if matches!(exported, Ok(true)) {
            let _ = server.remove::<Request, _>(&handle).await;
        }
        results(answer)
    }

    /// How long a request waits to know whether Recent has something to
    /// give (`recent_offered`) before its window opens without it: a
    /// desktop's setting read in a moment, or not at all.
    const RECENT_PATIENCE: std::time::Duration = std::time::Duration::from_millis(500);

    /// Whether an Open dialog's Recent has something to give
    /// (`recent::offered`), read on a thread of its own: the bus's
    /// connection is never held while the desktop's settings and its list
    /// are read. Not known within `RECENT_PATIENCE`, none.
    async fn recent_offered() -> bool {
        let told = Arc::new((
            parking_lot::Mutex::new(None::<bool>),
            event_listener::Event::new(),
        ));
        let telling = Arc::clone(&told);
        let started = std::thread::Builder::new()
            .name("skwir-chooser-recent".into())
            .spawn(move || {
                let offered = crate::recent::offered();
                *telling.0.lock() = Some(offered);
                telling.1.notify(usize::MAX);
            });
        if let Err(error) = started {
            tracing::debug!("whether Recent has files is not read: {error}");
            return false;
        }
        let answered = async {
            loop {
                let listener = told.1.listen();
                if let Some(offered) = *told.0.lock() {
                    return offered;
                }
                listener.await;
            }
        };
        let waited = async {
            async_io::Timer::after(RECENT_PATIENCE).await;
            false
        };
        futures_lite::future::or(answered, waited).await
    }

    /// What takes a request: whether it took it.
    pub(crate) type Take = Arc<dyn Fn(Choosing) -> bool + Send + Sync>;

    /// The back end's object, its requests handed to what takes them.
    pub(crate) struct Portal(pub(crate) Take);

    #[zbus::interface(name = "org.freedesktop.impl.portal.FileChooser")]
    impl Portal {
        async fn open_file(
            &self,
            #[zbus(object_server)] server: &zbus::ObjectServer,
            handle: OwnedObjectPath,
            app_id: String,
            parent_window: String,
            title: String,
            options: Options,
        ) -> (u32, Results) {
            self.asked(
                server,
                handle,
                Method::Open,
                (app_id, parent_window, title),
                options,
            )
            .await
        }

        async fn save_file(
            &self,
            #[zbus(object_server)] server: &zbus::ObjectServer,
            handle: OwnedObjectPath,
            app_id: String,
            parent_window: String,
            title: String,
            options: Options,
        ) -> (u32, Results) {
            self.asked(
                server,
                handle,
                Method::Save,
                (app_id, parent_window, title),
                options,
            )
            .await
        }

        async fn save_files(
            &self,
            #[zbus(object_server)] server: &zbus::ObjectServer,
            handle: OwnedObjectPath,
            app_id: String,
            parent_window: String,
            title: String,
            options: Options,
        ) -> (u32, Results) {
            self.asked(
                server,
                handle,
                Method::SaveFiles,
                (app_id, parent_window, title),
                options,
            )
            .await
        }
    }

    impl Portal {
        async fn asked(
            &self,
            server: &zbus::ObjectServer,
            handle: OwnedObjectPath,
            method: Method,
            (app_id, parent_window, title): (String, String, String),
            options: Options,
        ) -> (u32, Results) {
            let raw = Raw {
                method,
                app_id,
                parent_window,
                title,
                options,
            };
            let take = Arc::clone(&self.0);
            ask(server, handle, raw, move |choosing| take(choosing)).await
        }
    }

    /// Serve the back end on `bus`, its requests handed to `take`, and ask
    /// its name, waiting in the queue behind a holder (a skwir the bus
    /// started): whether it is served.
    pub(crate) fn serve(bus: &zbus::blocking::Connection, take: Take) -> bool {
        let objects = bus.object_server();
        if let Err(error) = objects.at(PATH, Portal(take)) {
            tracing::debug!("not answering other programs' dialogs: {error}");
            return false;
        }
        // Queued behind a skwir the bus started, which lets it go once
        // done, never taking it nor letting it be taken: no flag (zbus's
        // `request_name` would ask all three).
        // An empty set, made from two flags: zbus names no type for it.
        let mut queued =
            zbus::fdo::RequestNameFlags::AllowReplacement | zbus::fdo::RequestNameFlags::DoNotQueue;
        queued.remove(queued);
        match bus.request_name_with_flags(super::BACK_END, queued) {
            Ok(reply) => {
                tracing::debug!(?reply, "answering other programs' dialogs");
                true
            }
            Err(error) => {
                tracing::debug!("not answering other programs' dialogs: {error}");
                let _ = objects.remove::<Portal, _>(PATH);
                false
            }
        }
    }

    /// The name let go and the object taken down, on `bus`.
    pub(crate) fn let_go(bus: &zbus::blocking::Connection) {
        release(bus);
        let _ = bus.object_server().remove::<Portal, _>(PATH);
    }

    /// Let go of the back end's name, asked of the bus itself
    /// (`handover::linux::release_name`).
    fn release(bus: &zbus::blocking::Connection) {
        crate::handover::linux::release_name(bus, super::BACK_END);
    }

    /// What a skwir the bus started answers while another of its instance
    /// takes launches over: each request asked of that one
    /// (`Finishing.Choose` on `holder`), its dialog's Close passed on.
    struct Forward {
        holder: String,
        under_way: Arc<Ways>,
    }

    /// How many requests are under way, and whether one came.
    #[derive(Default)]
    struct Ways {
        count: parking_lot::Mutex<(usize, bool)>,
        changed: parking_lot::Condvar,
    }

    impl Ways {
        fn begin(&self) {
            let mut count = self.count.lock();
            count.0 += 1;
            count.1 = true;
            self.changed.notify_all();
        }

        fn end(&self) {
            let mut count = self.count.lock();
            count.0 = count.0.saturating_sub(1);
            self.changed.notify_all();
        }
    }

    // The portal's request, whole, beside the bus and what serves it.
    #[allow(clippy::too_many_arguments)]
    #[zbus::interface(name = "org.freedesktop.impl.portal.FileChooser")]
    impl Forward {
        async fn open_file(
            &self,
            #[zbus(object_server)] server: &zbus::ObjectServer,
            #[zbus(connection)] bus: &zbus::Connection,
            handle: OwnedObjectPath,
            app_id: String,
            parent_window: String,
            title: String,
            options: Options,
        ) -> (u32, Results) {
            let raw = Raw {
                method: Method::Open,
                app_id,
                parent_window,
                title,
                options,
            };
            self.ask_holder(server, bus, handle, raw).await
        }

        async fn save_file(
            &self,
            #[zbus(object_server)] server: &zbus::ObjectServer,
            #[zbus(connection)] bus: &zbus::Connection,
            handle: OwnedObjectPath,
            app_id: String,
            parent_window: String,
            title: String,
            options: Options,
        ) -> (u32, Results) {
            let raw = Raw {
                method: Method::Save,
                app_id,
                parent_window,
                title,
                options,
            };
            self.ask_holder(server, bus, handle, raw).await
        }

        async fn save_files(
            &self,
            #[zbus(object_server)] server: &zbus::ObjectServer,
            #[zbus(connection)] bus: &zbus::Connection,
            handle: OwnedObjectPath,
            app_id: String,
            parent_window: String,
            title: String,
            options: Options,
        ) -> (u32, Results) {
            let raw = Raw {
                method: Method::SaveFiles,
                app_id,
                parent_window,
                title,
                options,
            };
            self.ask_holder(server, bus, handle, raw).await
        }
    }

    impl Forward {
        async fn ask_holder(
            &self,
            server: &zbus::ObjectServer,
            bus: &zbus::Connection,
            handle: OwnedObjectPath,
            raw: Raw,
        ) -> (u32, Results) {
            self.under_way.begin();
            let closer = PassClose {
                holder: self.holder.clone(),
                handle: handle.clone(),
                bus: bus.clone(),
            };
            let exported = server.at(&handle, closer).await;
            let asked = bus
                .call_method(
                    Some(self.holder.as_str()),
                    crate::handover::linux::PATH,
                    Some(crate::handover::linux::INTERFACE),
                    "Choose",
                    &(
                        raw.method.number(),
                        &handle,
                        &raw.app_id,
                        &raw.parent_window,
                        &raw.title,
                        &raw.options,
                    ),
                )
                .await;
            if matches!(exported, Ok(true)) {
                let _ = server.remove::<PassClose, _>(&handle).await;
            }
            let answered = asked.and_then(|reply| reply.body().deserialize::<(u32, Results)>());
            self.under_way.end();
            match answered {
                Ok(answered) => answered,
                Err(error) => {
                    tracing::debug!("the skwir that runs did not answer a dialog: {error}");
                    results(Answer::Failed)
                }
            }
        }
    }

    /// A request's Close, passed on to the skwir that answers it.
    struct PassClose {
        holder: String,
        handle: OwnedObjectPath,
        bus: zbus::Connection,
    }

    #[zbus::interface(name = "org.freedesktop.impl.portal.Request")]
    impl PassClose {
        async fn close(&self) {
            let closed = self
                .bus
                .call_method(
                    Some(self.holder.as_str()),
                    &self.handle,
                    Some("org.freedesktop.impl.portal.Request"),
                    "Close",
                    &(),
                )
                .await;
            if let Err(error) = closed {
                tracing::debug!("a dialog's close was not passed on: {error}");
            }
        }
    }

    /// [`crate::handover::answer_chooser_for_holder`] on `bus`: what is
    /// asked within `wait` is asked of `holder`, and answered as it
    /// answers; the name is let go once nothing is under way. Whether a
    /// request came.
    pub(crate) fn answer_for_holder_on(
        bus: &zbus::blocking::Connection,
        wait: std::time::Duration,
        holder: &str,
    ) -> bool {
        let under_way = Arc::new(Ways::default());
        let forward = Forward {
            holder: holder.to_owned(),
            under_way: Arc::clone(&under_way),
        };
        let objects = bus.object_server();
        if let Err(error) = objects.at(PATH, forward) {
            tracing::debug!("not answering other programs' dialogs: {error}");
            return false;
        }
        // Taken only where no one holds it (the bus started this program
        // for it), letting no one take it until all is answered.
        let taken = bus.request_name_with_flags(
            super::BACK_END,
            zbus::fdo::RequestNameFlags::DoNotQueue.into(),
        );
        if let Err(error) = taken {
            tracing::debug!("not answering other programs' dialogs: {error}");
            let _ = objects.remove::<Forward, _>(PATH);
            return false;
        }
        let came = {
            let mut count = under_way.count.lock();
            let deadline = std::time::Instant::now() + wait;
            while !count.1 {
                if under_way
                    .changed
                    .wait_until(&mut count, deadline)
                    .timed_out()
                {
                    break;
                }
            }
            count.1
        };
        // The name first: what the portal asks from now on goes to the
        // skwir queued behind (or starts one); then what reached this one
        // before, the last of it too, is answered whole; only then the
        // object goes and the program with it.
        release(bus);
        dispatched(bus);
        {
            let mut count = under_way.count.lock();
            while count.0 > 0 {
                under_way.changed.wait(&mut count);
            }
        }
        let _ = objects.remove::<Forward, _>(PATH);
        came
    }

    /// Once this returns, every call that reached `bus` before it has
    /// begun: a ping of its own, answered after them in the order they
    /// came (zbus spawns each call's task as it is read, and its executor,
    /// one thread, begins them in that order).
    fn dispatched(bus: &zbus::blocking::Connection) {
        let Some(own) = bus.unique_name().map(ToString::to_string) else {
            return;
        };
        let pinged = bus.call_method(
            Some(own.as_str()),
            PATH,
            Some("org.freedesktop.DBus.Peer"),
            "Ping",
            &(),
        );
        if let Err(error) = pinged {
            tracing::debug!("the calls under way were not waited for: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked() -> Asked {
        Asked {
            app_id: "org.example.Editor".into(),
            parent_window: String::new(),
            title: "Open".into(),
            accept_label: None,
            kind: Kind::Open {
                multiple: false,
                directory: false,
            },
            filters: Vec::new(),
            current_filter: None,
            choices: Vec::new(),
            folder: None,
            recent: false,
        }
    }

    fn named(ino: u64, name: &str, kind: skwir_core::entry::EntryKind) -> skwir_core::Entry {
        skwir_core::Entry::new(
            skwir_core::EntryId {
                file: skwir_core::FileId::new(1, ino),
                at: 0,
            },
            name,
            kind,
        )
    }

    /// A filter admits by its globs, case ignored, and by its types, a
    /// kind (`image/*`) and what a type descends from included; a glob
    /// that does not compile admits nothing.
    #[test]
    fn a_filter_admits_by_name_and_type() {
        use skwir_core::entry::EntryKind::File;
        let filter = Filter {
            name: "Pictures and text".into(),
            patterns: vec![
                Pattern::Glob("*.png".into()),
                Pattern::Glob("[".into()),
                Pattern::Mime("image/*".into()),
                Pattern::Mime("text/plain".into()),
            ],
        };
        let admits = filter.admits();
        assert!(
            admits(&named(1, "Été 2024.PNG", File)),
            "a glob, case ignored"
        );
        assert!(
            admits(&named(2, "dessin.svg", File)),
            "image/svg+xml is image/*"
        );
        assert!(admits(&named(3, "notes.txt", File)));
        assert!(
            admits(&named(4, "script.sh", File)),
            "a shell script is text/plain"
        );
        assert!(!admits(&named(5, "archive.zip", File)));
        assert!(!admits(&named(6, "sans suffixe", File)));
        let none = Filter {
            name: "None".into(),
            patterns: Vec::new(),
        };
        assert!(!none.admits()(&named(1, "a.png", File)));
        let every = Filter {
            name: "Every type".into(),
            patterns: vec![Pattern::Mime("*/*".into())],
        };
        assert!(
            every.admits()(&named(7, "sans suffixe", File)),
            "*/* admits every file, as * does"
        );
        let qt = Filter {
            name: "All files".into(),
            patterns: vec![Pattern::Mime("Application/Octet-Stream".into())],
        };
        assert!(
            qt.admits()(&named(8, "sans suffixe", File)),
            "Qt's All files admits a file of a type unknown too"
        );
        assert!(qt.admits()(&named(9, "notes.txt", File)));
    }

    /// A program's All files is the filter that admits every file, found
    /// among its filters whatever it calls it: the window adds none of
    /// its own beside it.
    #[test]
    fn a_program_s_all_files_is_found_by_what_it_admits() {
        let filter = |name: &str, patterns: &[Pattern]| Filter {
            name: name.into(),
            patterns: patterns.to_vec(),
        };
        let glob = |g: &str| Pattern::Glob(g.into());
        let mime = |m: &str| Pattern::Mime(m.into());
        assert!(filter("Tous les fichiers", &[glob("*")]).admits_all());
        assert!(filter("Everything", &[mime("*/*")]).admits_all());
        assert!(filter("All files (*)", &[mime("application/octet-stream")]).admits_all());
        assert!(filter("Any", &[glob("*.png"), glob("**")]).admits_all());
        assert!(!filter("Dotted", &[glob("*.*")]).admits_all());
        assert!(!filter("Pictures", &[mime("image/*")]).admits_all());
        assert!(!filter("Empty glob", &[glob("")]).admits_all());
        assert!(!filter("None", &[]).admits_all());

        let mut asked = Asked {
            app_id: String::new(),
            parent_window: String::new(),
            title: String::new(),
            accept_label: None,
            kind: Kind::Open {
                multiple: false,
                directory: false,
            },
            filters: vec![
                filter("Fichiers image", &[mime("image/*")]),
                filter("Tous les fichiers", &[glob("*")]),
            ],
            current_filter: None,
            choices: Vec::new(),
            folder: None,
            recent: false,
        };
        assert_eq!(asked.all_files(), Some(1));
        asked.filters.pop();
        assert_eq!(asked.all_files(), None, "the window's own then");
    }

    /// A filter asks for pictures or films alone when each of its patterns
    /// is a type or a suffix of either (ADR-144, 5), GTK's bracketed
    /// suffixes read as theirs; and it gives a saved name its first
    /// suffix, else its first type's (ADR-144, 6).
    #[test]
    fn a_filter_says_whether_it_asks_for_pictures_and_what_it_names() {
        let filter = |patterns: &[Pattern]| Filter {
            name: "f".into(),
            patterns: patterns.to_vec(),
        };
        let glob = |g: &str| Pattern::Glob(g.into());
        let mime = |m: &str| Pattern::Mime(m.into());
        assert!(filter(&[glob("*.png"), glob("*.[jJ][pP][gG]")]).asks_pictures());
        assert!(filter(&[mime("image/*")]).asks_pictures());
        assert!(filter(&[mime("video/mp4"), glob("*.webm")]).asks_pictures());
        assert!(filter(&[mime("image/svg+xml")]).asks_pictures());
        assert!(!filter(&[glob("*.png"), glob("*.txt")]).asks_pictures());
        assert!(!filter(&[glob("*")]).asks_pictures(), "anything");
        assert!(!filter(&[glob("photo*")]).asks_pictures(), "no suffix");
        assert!(!filter(&[mime("audio/*")]).asks_pictures());
        assert!(!filter(&[]).asks_pictures());

        assert_eq!(suffix_of("*.[pP][nN][gG]").as_deref(), Some("png"));
        assert_eq!(suffix_of("*.tar.gz").as_deref(), Some("tar.gz"));
        assert_eq!(suffix_of("*.JPG").as_deref(), Some("jpg"));
        assert_eq!(suffix_of("*.[ab]"), None, "a choice, not a case");
        assert_eq!(suffix_of("*.*"), None);
        assert_eq!(suffix_of("*."), None);
        assert_eq!(suffix_of("photo.png"), None);

        assert_eq!(
            filter(&[mime("image/png"), glob("*.[wW][eE][bB][pP]")])
                .extension()
                .as_deref(),
            Some("webp"),
            "a suffix first"
        );
        assert_eq!(
            filter(&[mime("image/png")]).extension().as_deref(),
            Some("png"),
            "else the type's"
        );
        assert_eq!(filter(&[mime("image/*"), glob("*")]).extension(), None);
        assert_eq!(
            filter(&[glob("*.odt"), glob("*.[dD][oO][cC][xX]")]).suffixes(),
            ["odt", "docx"]
        );
    }

    /// Another filter gives the name its extension in place of one the
    /// program's filters take (a type's, by the type database, a compound
    /// one whole), case ignored, its other bytes kept; a name the new
    /// filter takes already, an extension none takes, a name that is
    /// nothing but one, or a filter that gives none, stays (ADR-144, 6).
    #[test]
    fn a_filter_swaps_the_extension_the_program_took() {
        use std::ffi::OsStr;
        let filter = |patterns: &[Pattern]| Filter {
            name: "f".into(),
            patterns: patterns.to_vec(),
        };
        let glob = |g: &str| Pattern::Glob(g.into());
        let mime = |m: &str| Pattern::Mime(m.into());
        let swapped = |name: &str, filters: &[Filter], to: &Filter| {
            swapped_name(OsStr::new(name), filters, to).map(|n| n.to_string_lossy().into_owned())
        };
        let (jpeg, png) = (
            filter(&[glob("*.jpg"), glob("*.jpeg")]),
            filter(&[glob("*.png")]),
        );
        let both = [jpeg.clone(), png.clone()];
        assert_eq!(
            swapped("Été 2024.JPG", &both, &png).as_deref(),
            Some("Été 2024.png")
        );
        assert_eq!(swapped("notes.txt", &both, &png), None, "the user's own");
        // The program's All files, in any form (Qt's for a program giving
        // types included), and a `*.*`, take every extension and make
        // none the program's.
        for every in [
            glob("*"),
            mime("*/*"),
            mime("application/octet-stream"),
            glob("*.*"),
        ] {
            let with_all = [
                jpeg.clone(),
                png.clone(),
                filter(std::slice::from_ref(&every)),
            ];
            assert_eq!(
                swapped("notes.md", &with_all, &png),
                None,
                "the user's own beside {every:?}"
            );
            assert_eq!(
                swapped("photo.jpg", &with_all, &png).as_deref(),
                Some("photo.png"),
                "a real filter's beside {every:?}"
            );
        }
        let text = filter(&[glob("*.txt"), glob("*")]);
        assert_eq!(
            swapped("notes.txt", &[text, png.clone()], &png).as_deref(),
            Some("notes.png"),
            "a pattern that names it, beside an All files of its own filter"
        );
        assert_eq!(swapped("photo", &both, &png), None);
        assert_eq!(swapped(".jpg", &both, &png), None, "only an extension");
        let pictures = filter(&[glob("*.png"), glob("*.jpg")]);
        assert_eq!(
            swapped("x.jpg", &[jpeg.clone(), pictures.clone()], &pictures),
            None,
            "the new filter takes it"
        );
        assert_eq!(
            swapped("x.jpg", &both, &filter(&[glob("*")])),
            None,
            "all files"
        );
        // A program giving its filters as types, Qt's All files among them.
        let (text, png_type) = (filter(&[mime("text/plain")]), filter(&[mime("image/png")]));
        let types = [
            text.clone(),
            png_type.clone(),
            filter(&[mime("application/octet-stream")]),
        ];
        assert_eq!(
            swapped("scan.tiff", &types, &png_type),
            None,
            "the user's own"
        );
        assert_eq!(
            swapped("notes.txt", &types, &png_type).as_deref(),
            Some("notes.png"),
            "a type's extension"
        );
        assert_eq!(
            swapped("notes.txt", &types, &types[2]),
            None,
            "Qt's all files"
        );
        // A compound extension, whole, never doubled.
        let (gzip, tarball) = (filter(&[glob("*.gz")]), filter(&[glob("*.tar.gz")]));
        let zip = filter(&[glob("*.zip")]);
        let archives = [gzip.clone(), tarball.clone(), zip.clone()];
        assert_eq!(swapped("backup.tar.gz", &archives, &tarball), None);
        assert_eq!(
            swapped("backup.tar.gz", &archives, &zip).as_deref(),
            Some("backup.zip")
        );
        assert_eq!(
            swapped("backup.tar.xz", &archives, &zip),
            None,
            "none takes it"
        );
        assert_eq!(
            swapped("dump.tar.xz", &[filter(&[glob("*.xz")]), zip.clone()], &zip).as_deref(),
            Some("dump.zip"),
            "a compound the type database knows"
        );
        // Types: every extension of the type, and of what descends from it.
        let (jpeg_type, png_type) = (filter(&[mime("image/jpeg")]), filter(&[mime("image/png")]));
        let types = [jpeg_type.clone(), png_type.clone()];
        assert_eq!(
            swapped("photo.jpeg", &types, &png_type).as_deref(),
            Some("photo.png")
        );
        assert_eq!(
            swapped("photo.jpg", &types, &png_type).as_deref(),
            Some("photo.png")
        );
        let any_picture = filter(&[mime("image/*")]);
        assert_eq!(
            swapped("x.png", &[any_picture.clone(), jpeg.clone()], &jpeg).as_deref(),
            Some("x.jpg")
        );
        assert_eq!(
            swapped("x.png", &[jpeg.clone(), any_picture.clone()], &any_picture),
            None
        );
        // A name's bytes are kept, but its extension's.
        #[cfg(unix)]
        {
            use std::ffi::OsString;
            use std::os::unix::ffi::OsStringExt;
            let latin = OsString::from_vec(b"caf\xe9.jpg".to_vec());
            assert_eq!(
                swapped_name(&latin, &both, &png).map(OsString::into_vec),
                Some(b"caf\xe9.png".to_vec())
            );
        }
    }

    /// The name a save starts with is the saved file's, else the one
    /// suggested; the filter shown is the one in use, else the first.
    #[test]
    fn a_request_starts_where_it_asks() {
        let mut save = asked();
        save.kind = Kind::Save {
            name: Some("Untitled.txt".into()),
            file: Some(PathBuf::from("/srv/example/Lettre d'été.odt")),
        };
        assert_eq!(
            save.save_name().as_deref(),
            Some(std::ffi::OsStr::new("Lettre d'été.odt"))
        );
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            // A name not UTF-8 is its bytes, never its lossy form.
            let latin = OsString::from_vec(b"r\xe9sum\xe9.txt".to_vec());
            save.kind = Kind::Save {
                name: None,
                file: Some(PathBuf::from("/srv/example").join(&latin)),
            };
            assert_eq!(save.save_name(), Some(latin));
        }
        save.kind = Kind::Save {
            name: Some("Untitled.txt".into()),
            file: None,
        };
        assert_eq!(
            save.save_name().as_deref(),
            Some(std::ffi::OsStr::new("Untitled.txt"))
        );
        let filter = |name: &str| Filter {
            name: name.into(),
            patterns: vec![Pattern::Glob(format!("*.{name}"))],
        };
        save.filters = vec![filter("odt"), filter("txt")];
        assert_eq!(save.first_filter(), Some(0));
        save.current_filter = Some(filter("txt"));
        assert_eq!(save.first_filter(), Some(1));
        save.current_filter = Some(filter("pdf"));
        assert_eq!(save.first_filter(), Some(0));
        save.filters.clear();
        assert_eq!(save.first_filter(), None);
    }

    /// The first answer is the one sent; one closed is cancelled and told
    /// to those who wait; one let go of unanswered failed.
    #[test]
    fn a_request_is_answered_once() {
        let (choosing, reply) = Choosing::made(asked());
        choosing.answer(Answer::Cancelled);
        choosing.answer(Answer::Failed);
        assert_eq!(reply.state.lock().answer, Some(Answer::Cancelled));

        let (choosing, reply) = Choosing::made(asked());
        let told = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let hook = Arc::clone(&told);
        choosing.when_closed(move || hook.store(true, Ordering::SeqCst));
        reply.close();
        assert!(choosing.is_closed() && told.load(Ordering::SeqCst));
        assert_eq!(reply.state.lock().answer, Some(Answer::Cancelled));
        let late = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let hook = Arc::clone(&late);
        choosing.when_closed(move || hook.store(true, Ordering::SeqCst));
        assert!(late.load(Ordering::SeqCst), "told at once once closed");

        let (choosing, reply) = Choosing::made(asked());
        let copy = choosing.clone();
        drop(choosing);
        assert_eq!(reply.state.lock().answer, None, "a copy still holds it");
        drop(copy);
        assert_eq!(reply.state.lock().answer, Some(Answer::Failed));
    }

    /// What the portal writes is read as data: the kind, the folder (the
    /// saved file's where none is suggested), filters by pattern and type,
    /// choices, names not UTF-8; and answered back as URIs.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_request_reads_as_the_portal_writes_it() {
        use linux::{Method, Options, Raw, answer_of, asked, results};
        use std::os::unix::ffi::OsStringExt;
        let raw = Raw {
            method: Method::Save,
            app_id: "org.example.Editor".into(),
            parent_window: "x11:1a2b".into(),
            title: "Save As".into(),
            options: Options {
                current_name: Some("it's a draft.txt".into()),
                current_file: Some(b"/srv/Field Guide/r\xe9sum\xe9.txt\0".to_vec()),
                filters: Some(vec![
                    (
                        "Text".into(),
                        vec![(0, "*.txt".into()), (1, "text/*".into())],
                    ),
                    ("All".into(), vec![(0, "*".into())]),
                ]),
                choices: Some(vec![
                    (
                        "encoding".into(),
                        "Encoding".into(),
                        vec![("utf8".into(), "UTF-8".into())],
                        "utf8".into(),
                    ),
                    (
                        "bom".into(),
                        "Byte order mark".into(),
                        Vec::new(),
                        "false".into(),
                    ),
                ]),
                ..Options::default()
            },
        };
        let got = asked(&raw).expect("names that are one name each");
        assert_eq!(
            got.kind,
            Kind::Save {
                name: Some("it's a draft.txt".into()),
                file: Some(PathBuf::from(OsString::from_vec(
                    b"/srv/Field Guide/r\xe9sum\xe9.txt".to_vec()
                ))),
            }
        );
        assert_eq!(got.folder, Some(PathBuf::from("/srv/Field Guide")));
        assert_eq!(
            got.filters[0].patterns,
            [
                Pattern::Glob("*.txt".into()),
                Pattern::Mime("text/*".into())
            ]
        );
        assert!(got.choices[1].is_switch() && !got.choices[0].is_switch());

        let (code, said) = results(Answer::Chosen {
            paths: vec![PathBuf::from("/srv/Field Guide/it's a draft.txt")],
            choices: vec![("encoding".into(), "utf8".into())],
            filter: Some(got.filters[0].clone()),
        });
        assert_eq!(code, 0);
        assert_eq!(
            said.uris.as_deref(),
            Some(&["file:///srv/Field%20Guide/it's%20a%20draft.txt".to_owned()][..])
        );
        // The dict as the portal reads it: what was chosen is writable,
        // said; nothing is said of what was not chosen.
        let dict = |results: &linux::Results| {
            let ctxt = zbus::zvariant::serialized::Context::new_dbus(zbus::zvariant::LE, 0);
            let bytes = zbus::zvariant::to_bytes(ctxt, results).unwrap();
            let (dict, _): (
                std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
                _,
            ) = bytes.deserialize().unwrap();
            dict
        };
        let written = dict(&said);
        assert_eq!(
            written.get("writable").map(|v| bool::try_from(v).unwrap()),
            Some(true)
        );
        assert!(written.contains_key("uris") && written.contains_key("choices"));
        for unchosen in [Answer::Cancelled, Answer::Failed] {
            assert!(dict(&results(unchosen).1).is_empty());
        }
        assert_eq!(
            answer_of(code, said),
            Answer::Chosen {
                paths: vec![PathBuf::from("/srv/Field Guide/it's a draft.txt")],
                choices: vec![("encoding".into(), "utf8".into())],
                filter: Some(got.filters[0].clone()),
            }
        );
        assert_eq!(results(Answer::Cancelled).0, 1);
        assert_eq!(results(Answer::Failed).0, 2);
    }

    /// A name the program gives is one name, or the request is refused:
    /// a path, `.`, `..` or nothing would put the file elsewhere than in
    /// the folder the user chooses (ADR-141, 5).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_name_given_is_one_name() {
        use linux::{Method, Options, Raw, asked};
        let saving_files = |files: &[&[u8]]| Raw {
            method: Method::SaveFiles,
            options: Options {
                files: Some(files.iter().map(|name| name.to_vec()).collect()),
                ..Options::default()
            },
            ..Raw::default()
        };
        for refused in [
            &b"/etc/xdg/autostart/a.desktop\0"[..],
            b"../a.desktop\0",
            b"sub/a.txt\0",
            b"a.txt/\0",
            b".\0",
            b"..\0",
            b"\0",
            b"",
        ] {
            assert!(
                asked(&saving_files(&[b"fine.txt\0", refused])).is_none(),
                "{refused:?}"
            );
        }
        let taken = asked(&saving_files(&[
            b"r\xe9sum\xe9.txt\0",
            b" it's #1 %20.txt\0",
        ]))
        .expect("one name each");
        assert!(matches!(&taken.kind, Kind::SaveFiles { files } if files.len() == 2));

        let saving = |name: &str| Raw {
            method: Method::Save,
            options: Options {
                current_name: Some(name.to_owned()),
                ..Options::default()
            },
            ..Raw::default()
        };
        for refused in [
            "../../.bashrc",
            "/etc/x.conf",
            "notes/x.txt",
            "x.txt/",
            "..",
            ".",
        ] {
            assert!(asked(&saving(refused)).is_none(), "{refused}");
        }
        assert!(matches!(
            asked(&saving(" Lettre d'été .odt")).map(|a| a.kind),
            Some(Kind::Save { name: Some(name), .. }) if name == " Lettre d'été .odt"
        ));
        assert!(matches!(
            asked(&saving("")).map(|a| a.kind),
            Some(Kind::Save { name: None, .. })
        ));
    }
}
