//! What skwir lets go where (ADR-044).
//!
//! - **What a plugin may do** ([`Capability`]): read what files hold,
//!   change files, use the network, start programs. A plugin built into
//!   skwir declares it and the Plugins page says it; a plugin from outside
//!   skwir (a program, a WebAssembly module) gets what it declares and
//!   nothing more, from its host.
//! - **Private folders** ([`Privacy`]): folders whose files never leave
//!   this machine. Nothing skwir hands an agent or a plugin from outside
//!   names, lists or reads them: the folders the Privacy page lists
//!   (`~/.ssh` and the like) and any folder holding a `.private` file,
//!   with everything under them.
//! - **What an agent may run**: a command's [`Effect`](crate::command::Effect),
//!   read by [`for_agent`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::command::{CommandSpec, Effect};
use crate::fs::volume::Here;
use crate::scheduler::{Lane, Priority, Scheduler};
use crate::settings::{RecordField, SettingSpec, Settings};

/// The settings' namespace: the Privacy page.
pub const SETTINGS: &str = "privacy";
/// The private folders, records of one `path` each.
pub const FOLDERS_KEY: &str = "privacy.folders";
/// Whether a folder holding a [`MARKER`] file is private.
pub const MARKER_KEY: &str = "privacy.marker";
/// The file that makes the folder holding it private.
pub const MARKER: &str = ".private";
/// What a private path is said as, where an answer would name it.
pub const WITHHELD: &str = "(private)";

/// A folder of this machine's, where a lookup is asked from
/// ([`Checker::reaches_elsewhere`]): a share named by its server's name is
/// on none.
#[cfg(windows)]
const LOCAL_ROOT: &str = "C:\\";
#[cfg(not(windows))]
const LOCAL_ROOT: &str = "/";

/// Private from the start: where keys, passwords and credentials live,
/// the desktop's keyrings among them on each system.
pub const DEFAULT_FOLDERS: &[&str] = &[
    "~/.ssh",
    "~/.gnupg",
    "~/.password-store",
    // GNOME's keyrings, KDE's wallets.
    #[cfg(not(any(target_os = "macos", windows)))]
    "~/.local/share/keyrings",
    #[cfg(not(any(target_os = "macos", windows)))]
    "~/.local/share/kwalletd",
    #[cfg(target_os = "macos")]
    "~/Library/Keychains",
    // The Credential Manager's vaults, and the keys that protect them.
    #[cfg(windows)]
    "~/AppData/Roaming/Microsoft/Credentials",
    #[cfg(windows)]
    "~/AppData/Local/Microsoft/Credentials",
    #[cfg(windows)]
    "~/AppData/Local/Microsoft/Vault",
    #[cfg(windows)]
    "~/AppData/Roaming/Microsoft/Protect",
    "~/.pki",
    "~/.aws",
    "~/.kube",
    "~/.docker",
    "~/.netrc",
];

/// Something a plugin may do beyond naming and listing files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Reads what files hold.
    Read,
    /// Changes files: through a plan its caller applies or proposes.
    Write,
    /// Uses the network.
    Network,
    /// Starts programs.
    Programs,
}

impl Capability {
    pub const ALL: [Capability; 4] = [
        Capability::Read,
        Capability::Write,
        Capability::Network,
        Capability::Programs,
    ];

    /// What it lets a plugin do, as the Plugins page says it.
    pub fn words(self) -> &'static str {
        match self {
            Capability::Read => "reads what files hold",
            Capability::Write => "changes files",
            Capability::Network => "uses the network",
            Capability::Programs => "starts programs",
        }
    }

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// A set of capabilities: what a plugin declares, or what it was granted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Capabilities(u8);

impl Capabilities {
    pub const NONE: Capabilities = Capabilities(0);

    pub const fn of(list: &[Capability]) -> Capabilities {
        let mut bits = 0;
        let mut i = 0;
        while i < list.len() {
            bits |= list[i].bit();
            i += 1;
        }
        Capabilities(bits)
    }

    pub fn has(self, capability: Capability) -> bool {
        self.0 & capability.bit() != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn iter(self) -> impl Iterator<Item = Capability> {
        Capability::ALL.into_iter().filter(move |c| self.has(*c))
    }

    /// The ones `self` has that `granted` lacks.
    #[must_use]
    pub fn beyond(self, granted: Capabilities) -> Capabilities {
        Capabilities(self.0 & !granted.0)
    }

    /// What they let a plugin do, in words: "reads what files hold,
    /// starts programs"; `None` for none.
    pub fn words(self) -> Option<String> {
        let words: Vec<&str> = self.iter().map(Capability::words).collect();
        (!words.is_empty()).then(|| words.join(", "))
    }
}

impl FromIterator<Capability> for Capabilities {
    fn from_iter<I: IntoIterator<Item = Capability>>(iter: I) -> Self {
        Capabilities(iter.into_iter().fold(0, |bits, c| bits | c.bit()))
    }
}

impl Serialize for Capabilities {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(self.iter())
    }
}

impl<'de> Deserialize<'de> for Capabilities {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Vec::<Capability>::deserialize(d)?.into_iter().collect())
    }
}

/// Declare the Privacy page's settings.
pub fn declare(settings: &Settings) {
    let mut folders = SettingSpec::records(
        FOLDERS_KEY,
        "Private folders",
        "Files that never leave this machine: an agent and a plugin from outside skwir never see their names nor what they hold, nor anything under them. A path, ~ for your home folder.",
        "private folder",
        vec![
            RecordField::text(
                "path",
                "Folder",
                "A folder or a file, with everything under it",
                "~/.ssh",
            )
            .required(),
        ],
    );
    folders.default = toml::Value::Array(
        DEFAULT_FOLDERS
            .iter()
            .enumerate()
            .map(|(n, path)| {
                // Each record has an id, which the dialog edits it by.
                let mut record = toml::Table::new();
                record.insert("id".into(), toml::Value::String(format!("default-{n}")));
                record.insert("path".into(), toml::Value::String((*path).into()));
                toml::Value::Table(record)
            })
            .collect(),
    );
    settings.declare(folders.essential());
    settings.declare(SettingSpec::toggle(
        MARKER_KEY,
        "Folders marked private",
        "A folder holding a file named .private is private too, with everything under it.",
        true,
    ));
}

/// The private folders, as the settings say them when read.
#[derive(Clone, Default)]
pub struct Privacy {
    folders: Vec<PathBuf>,
    marker: bool,
    /// Where a path on a remote is looked at, in its lane ([`Checker`]):
    /// none, and it is looked at where it is asked.
    workers: Option<Scheduler>,
}

impl std::fmt::Debug for Privacy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Privacy")
            .field("folders", &self.folders)
            .field("marker", &self.marker)
            .finish_non_exhaustive()
    }
}

impl Privacy {
    /// What `settings` say now: each folder with `~` read as the home
    /// folder; one that is not absolute is left out. A folder may be said
    /// as its path alone (`privacy.folders = ["~/.ssh"]`, as a hand or Home
    /// Manager writes it); a value that is no list keeps the folders private
    /// from the start: what cannot be read never makes a folder public.
    pub fn of(settings: &Settings) -> Privacy {
        let home = crate::os_path::home();
        let value = settings.get::<toml::Value>(FOLDERS_KEY);
        let listed: Vec<String> = match value.as_ref().map(toml::Value::as_array) {
            Some(Some(records)) => records
                .iter()
                .filter_map(|record| {
                    let path = match record {
                        toml::Value::String(path) => Some(path.as_str()),
                        record => record.get("path").and_then(toml::Value::as_str),
                    };
                    if path.is_none() {
                        tracing::warn!("{FOLDERS_KEY}: a folder is a path or a record of one");
                    }
                    path.map(str::to_owned)
                })
                .collect(),
            Some(None) => {
                tracing::warn!("{FOLDERS_KEY} is no list: the folders private from the start are");
                DEFAULT_FOLDERS
                    .iter()
                    .map(|path| (*path).to_owned())
                    .collect()
            }
            None => DEFAULT_FOLDERS
                .iter()
                .map(|path| (*path).to_owned())
                .collect(),
        };
        let folders = listed
            .iter()
            .filter_map(|text| expand(text, home.as_deref()))
            .collect();
        Privacy {
            folders,
            marker: settings.get::<bool>(MARKER_KEY).unwrap_or(true),
            workers: None,
        }
    }

    /// Private to these folders, no marker read: for tests.
    pub fn folders(folders: impl IntoIterator<Item = PathBuf>) -> Privacy {
        Privacy {
            folders: folders.into_iter().collect(),
            marker: false,
            workers: None,
        }
    }

    /// These folders, a path on a remote looked at by `workers`, in its
    /// lane: a server that stops answering never holds the one asking
    /// longer than [`reach::NOT_ANSWERING`](crate::fs::reach::NOT_ANSWERING).
    #[must_use]
    pub fn looked_at_by(mut self, workers: Scheduler) -> Privacy {
        self.workers = Some(workers);
        self
    }

    /// The folders listed, as absolute paths.
    pub fn listed(&self) -> &[PathBuf] {
        &self.folders
    }

    /// A checker of many paths, which reads each folder's marker once.
    pub fn checker(&self) -> Checker<'_> {
        Checker {
            privacy: self,
            marked: Arc::default(),
            resolved_folders: None,
            unanswered: Vec::new(),
            here: None,
        }
    }

    /// Whether `path` is private. Resolves links and reads markers: call it
    /// where reading may wait (a worker, an agent's or a plugin's thread).
    pub fn holds(&self, path: &Path) -> bool {
        self.checker().holds(path)
    }
}

/// `text` as an absolute path, `~` read as `home`.
fn expand(text: &str, home: Option<&Path>) -> Option<PathBuf> {
    let text = text.trim();
    let path = match text.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            home?.join(rest.trim_start_matches(['/', '\\']))
        }
        _ => PathBuf::from(text),
    };
    path.is_absolute().then_some(path)
}

/// The path `text` is as an argument writes one: absolute, from the home
/// folder (`~/…`), or a folder's location (`path:/…`, `Location::parse`).
/// A relative path is read where it is resolved (`Checker::admits`).
fn path_named(text: &str) -> Option<PathBuf> {
    let text = text
        .strip_prefix(crate::location::PATH_SCHEME)
        .and_then(|rest| rest.strip_prefix(':'))
        .unwrap_or(text);
    let path = Path::new(text);
    if path.is_absolute() {
        return Some(path.to_owned());
    }
    if text.starts_with('~') {
        return expand(text, crate::os_path::home().as_deref());
    }
    None
}

/// What an agent is told of a path that would reach another machine.
fn reaches_another_machine(path: &Path) -> String {
    format!(
        "looking up {} would make the system reach another machine, which an agent may not do unasked: the user opens it in skwir first",
        path.display()
    )
}

/// What an agent is told of a private path it named or would open.
const NAMES_PRIVATE: &str =
    "this names a private folder, which skwir keeps from agents (Settings, Privacy)";

/// At most so many paths are looked for written in one text.
const WRITTEN_PATHS: usize = 64;

/// The absolute paths written in `text` beyond the text itself (which
/// [`path_named`] reads): from where each starts (the text's start, or
/// after a space, a quote, a bracket or a punctuation mark: `in:/x`, “/x”;
/// never inside a word) to the text's end, and to each place a quote or a
/// punctuation mark ends it (`/x/diary: it is taken` names `/x/diary`), a
/// word too after the text's start. One the text starts with keeps its
/// spaces: a name holds them.
fn written_paths(text: &str) -> Vec<&Path> {
    const ENDS: &str = "\"”’»)]}>,;:";
    let mut found = Vec::new();
    let mut before: Option<char> = None;
    for (at, c) in text.char_indices() {
        let starts = before
            .replace(c)
            .is_none_or(|b| b.is_whitespace() || "\"'“”‘’«»([{<:=,;".contains(b));
        let rest = &text[at..];
        if !starts || !Path::new(rest).is_absolute() {
            continue;
        }
        if at > 0 {
            found.push(Path::new(rest));
        }
        for (end, e) in rest.char_indices().skip(1) {
            let ends = ENDS.contains(e) || (at > 0 && (e.is_whitespace() || e == '\''));
            let cut = Path::new(&rest[..end]);
            if ends && cut.is_absolute() {
                found.push(cut);
            }
            if found.len() >= WRITTEN_PATHS {
                return found;
            }
        }
    }
    found
}

/// While an agent's call runs on this thread ([`as_agent`]), refuse what
/// reading `path` would reach as [`Checker::admits`] does: where a location
/// or a text typed is resolved to what is read, whatever its argument said.
/// Anyone else's call reads what it reads.
///
/// # Errors
/// Saying why, as an agent is told.
pub fn agent_may_read(path: &Path) -> anyhow::Result<()> {
    match agent_call() {
        Some(privacy) => privacy.checker().admits(path),
        None => Ok(()),
    }
}

/// Checks paths against a [`Privacy`], remembering the markers it read.
///
/// What it reads of a path on a remote (its links resolved, its folders'
/// markers) is read in the remote's lane where the privacy has workers
/// ([`Privacy::looked_at_by`]), waited for [`reach::NOT_ANSWERING`] at
/// most: a server that does not answer by then keeps what it holds
/// private, and is not asked again by this checker. What cannot be
/// checked is never counted public.
///
/// [`reach::NOT_ANSWERING`]: crate::fs::reach::NOT_ANSWERING
pub struct Checker<'a> {
    privacy: &'a Privacy,
    marked: Arc<Mutex<HashMap<PathBuf, bool>>>,
    /// Resolve lazily: taking a Privacy snapshot itself never reads disk.
    resolved_folders: Option<Arc<Vec<PathBuf>>>,
    /// The lanes of the remotes that did not answer in time.
    unanswered: Vec<Lane>,
    /// The mounts, read once: what a lookup would reach another machine
    /// for ([`Checker::reaches_elsewhere`]).
    here: Option<Here>,
}

impl Checker<'_> {
    /// Whether `path` is private: under a folder listed, or under one
    /// holding a `.private` file, by its given name or its resolved target.
    pub fn holds(&mut self, path: &Path) -> bool {
        if self
            .privacy
            .folders
            .iter()
            .any(|folder| path.starts_with(folder))
        {
            return true;
        }
        if self.privacy.folders.is_empty() && !self.privacy.marker {
            return false;
        }
        // Looked up, it would reach another machine: it is never read, and
        // what cannot be checked is not public.
        if self.reaches_elsewhere(path) {
            return true;
        }
        let folders = self.resolved_folders();
        let (privacy, marked, asked, here) = (
            self.privacy.clone(),
            Arc::clone(&self.marked),
            path.to_owned(),
            self.here().clone(),
        );
        self.answered(path, move || {
            looks_private(&privacy, &folders, &marked, &here, &asked)
        })
        .unwrap_or(true)
    }

    /// The private folders resolved, each in its lane: one that cannot be
    /// resolved (a link to a drive not mounted, a parent that cannot be
    /// read, a server that does not answer) holds what is under it as it
    /// is written, checked first: nothing reached through it could be
    /// resolved either. It never makes every path private.
    fn resolved_folders(&mut self) -> Arc<Vec<PathBuf>> {
        if let Some(folders) = &self.resolved_folders {
            return Arc::clone(folders);
        }
        let mut folders = Vec::new();
        for folder in self.privacy.folders.clone() {
            if self.reaches_elsewhere(&folder) {
                continue;
            }
            let asked = folder.clone();
            match self.answered(&folder, move || resolved_name(&asked)) {
                Some(Ok(resolved)) => folders.push(resolved),
                Some(Err(error)) => {
                    tracing::debug!("a private folder cannot be resolved: {error}");
                }
                None => {}
            }
        }
        Arc::clone(self.resolved_folders.insert(Arc::new(folders)))
    }

    /// What `look` says of `path`: here, where it is on no remote, the
    /// privacy has no workers or the caller already runs in its remote's
    /// lane (a field read there, which a look queued in the lane would wait
    /// behind); else in that lane, `None` when the server did not answer in
    /// time.
    fn answered<T: Send + 'static>(
        &mut self,
        path: &Path,
        look: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        let Some(workers) = self.privacy.workers.clone() else {
            return Some(look());
        };
        let lane = lane_reached(path);
        if lane == Lane::SHARED || crate::scheduler::running_lane() == Some(lane) {
            return Some(look());
        }
        if self.unanswered.contains(&lane) {
            return None;
        }
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let cancel = crate::cancel::CancelToken::new();
        workers.spawn_in(
            lane,
            Priority::Normal,
            "privacy",
            cancel.clone(),
            move || {
                let _ = tx.send(look());
            },
        );
        let answer = rx.recv_timeout(crate::fs::reach::NOT_ANSWERING).ok();
        if answer.is_none() {
            cancel.cancel();
            tracing::debug!("a remote did not answer the privacy check: what it holds is private");
            self.unanswered.push(lane);
        }
        answer
    }

    /// Whether looking `path` up would make the system reach another
    /// machine the user is not on yet (`fs::volume::Here`, ADR-029): below
    /// an automount point nothing is mounted on (`/net/host`); on Windows
    /// a share by its server's name (`\\host\share`), whether connected
    /// or not being nothing the system says unasked (a drive letter is
    /// this machine's). A server mounted already is no other machine: its
    /// mount stands where the automount point was. Its `..` are taken as
    /// written (`/tmp/../net/host` is below `/net`): it is never read.
    pub fn reaches_elsewhere(&mut self, path: &Path) -> bool {
        let path = lexical(path);
        !self.here().holds(&path, Path::new(LOCAL_ROOT))
    }

    fn here(&mut self) -> &Here {
        self.here.get_or_insert_with(Here::now)
    }

    /// Refuse what an agent's call would open or read at `path` (ADR-044):
    /// a path looking up would reach another machine for, or a private one,
    /// by its name or what it resolves to. Where a relative path or a
    /// location's text is resolved (`App::open_location`, the address
    /// field's names), what is checked is what would be read, never the
    /// argument's text.
    ///
    /// # Errors
    /// Saying why, as an agent is told.
    pub fn admits(&mut self, path: &Path) -> anyhow::Result<()> {
        if self.reaches_elsewhere(path) {
            anyhow::bail!(reaches_another_machine(path));
        }
        anyhow::ensure!(!self.holds(path), NAMES_PRIVATE);
        Ok(())
    }

    /// The first path `value` names (a string that is one, a folder's
    /// location, a path written as its bytes) that looking up would reach
    /// another machine for ([`Checker::reaches_elsewhere`]): an agent naming
    /// it is refused before anything reads it.
    pub fn elsewhere_in(&mut self, value: &Value) -> Option<PathBuf> {
        match value {
            Value::String(text) => path_named(text)
                .into_iter()
                .chain(written_paths(text).into_iter().map(Path::to_path_buf))
                .find(|path| self.reaches_elsewhere(path)),
            Value::Array(items) => items.iter().find_map(|item| self.elsewhere_in(item)),
            Value::Object(object) => {
                if let Some(path) = bytes_path(object)
                    && self.reaches_elsewhere(&path)
                {
                    return Some(path);
                }
                object.values().find_map(|item| self.elsewhere_in(item))
            }
            _ => None,
        }
    }

    /// The first private path `value` names: a string that is a path or a
    /// folder's location, or that holds a private folder listed or a
    /// private path written in it (a location's text, a title, an error),
    /// or a path written as its bytes.
    pub fn named_in(&mut self, value: &Value) -> Option<PathBuf> {
        match value {
            Value::String(text) => self.in_text(text),
            Value::Array(items) => items.iter().find_map(|item| self.named_in(item)),
            Value::Object(object) => {
                if let Some(path) = bytes_path(object)
                    && self.holds(&path)
                {
                    return Some(path);
                }
                object.values().find_map(|item| self.named_in(item))
            }
            _ => None,
        }
    }

    fn in_text(&mut self, text: &str) -> Option<PathBuf> {
        if let Some(path) = path_named(text)
            && self.holds(&path)
        {
            return Some(path);
        }
        if let Some(folder) = self
            .privacy
            .folders
            .iter()
            .find(|folder| holds_folder(text, folder))
        {
            return Some(folder.clone());
        }
        // A path written in the text (an error, a plan's title, a search's
        // `in:`), a marked folder's too; resolving one reads only as far as
        // what exists.
        written_paths(text)
            .into_iter()
            .find(|path| self.holds(path))
            .map(Path::to_path_buf)
    }

    /// `value` without what names a private path: an object whose `path`
    /// is private leaves the list it is in; any other string that names
    /// one says [`WITHHELD`].
    pub fn redact(&mut self, value: Value) -> Value {
        match value {
            Value::String(text) if self.in_text(&text).is_some() => Value::String(WITHHELD.into()),
            Value::Array(items) => {
                let mut kept = Vec::with_capacity(items.len());
                for item in items {
                    if !self.is_private_record(&item) {
                        kept.push(self.redact(item));
                    }
                }
                Value::Array(kept)
            }
            Value::Object(object) => {
                if let Some(path) = bytes_path(&object)
                    && self.holds(&path)
                {
                    return Value::String(WITHHELD.into());
                }
                Value::Object(
                    object
                        .into_iter()
                        .map(|(key, item)| (key, self.redact(item)))
                        .collect(),
                )
            }
            other => other,
        }
    }

    /// An object that stands for a private file: its `path` is private.
    fn is_private_record(&mut self, value: &Value) -> bool {
        let Some(path) = value.get("path") else {
            return false;
        };
        match path {
            Value::String(text) => path_named(text).is_some_and(|path| self.holds(&path)),
            Value::Object(object) => bytes_path(object).is_some_and(|path| self.holds(&path)),
            _ => false,
        }
    }
}

/// Whether `path` is private, as the disk says: under a folder holding a
/// [`MARKER`] by its given name, or, resolved, under a private folder
/// (`folders`, resolved) or a marked one.
fn looks_private(
    privacy: &Privacy,
    folders: &[PathBuf],
    marks: &Mutex<HashMap<PathBuf, bool>>,
    here: &Here,
    path: &Path,
) -> bool {
    if marked(privacy, marks, here, path) {
        return true;
    }
    // A missing final name is normal for a proposed write. Resolve its
    // existing parent too; other failures cannot establish a public path.
    let Ok(resolved) = resolved_name(path) else {
        return true;
    };
    if folders.iter().any(|folder| resolved.starts_with(folder)) {
        return true;
    }
    marked(privacy, marks, here, &resolved)
}

/// Whether a folder holding `path` holds a [`MARKER`], each folder read
/// once (`marks`, never held while reading). An automount point holds
/// only what it mounts: its marker is never looked up, which would ask
/// for a machine of that name (`/net/.private`).
fn marked(
    privacy: &Privacy,
    marks: &Mutex<HashMap<PathBuf, bool>>,
    here: &Here,
    path: &Path,
) -> bool {
    if !privacy.marker {
        return false;
    }
    path.ancestors().any(|folder| {
        if let Some(known) = marks.lock().get(folder).copied() {
            return known;
        }
        let file = folder.join(MARKER);
        if !here.holds(&file, Path::new(LOCAL_ROOT)) {
            return false;
        }
        let marked = match std::fs::symlink_metadata(file) {
            Ok(_) => true,
            Err(error) => !matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ),
        };
        marks.lock().insert(folder.to_owned(), marked);
        marked
    })
}

/// The lane of what reading `path` reaches: its remote's, by its text or
/// through a link on the way into one (a folder of the home linked to a
/// mount); the shared one elsewhere. What it reads is links on this
/// machine, and none while no remote is mounted.
fn lane_reached(path: &Path) -> Lane {
    if let Some(remote) = crate::fs::reach::of(path) {
        return remote.lane();
    }
    if crate::fs::reach::all().is_empty() {
        return Lane::SHARED;
    }
    path.ancestors()
        .find_map(|ancestor| {
            let target = std::fs::read_link(ancestor).ok()?;
            let target = ancestor
                .parent()
                .map_or_else(|| target.clone(), |parent| parent.join(&target));
            crate::fs::reach::of(&lexical(&target)).map(|remote| remote.lane())
        })
        .unwrap_or(Lane::SHARED)
}

/// `path` with its `.` and `..` taken as written: to tell a link's target
/// by its text, never to read it.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            part => out.push(part),
        }
    }
    out
}

/// Resolve what exists without losing the name of a prospective file.
/// Do not normalize `..` before resolving links: the kernel follows the
/// link first. A dangling link or inaccessible ancestor is not proof that
/// a path is public, so leave its error for the caller to withhold it.
fn resolved_name(path: &Path) -> std::io::Result<PathBuf> {
    let mut ancestor = path;
    let mut missing = Vec::new();
    loop {
        match crate::os_path::canonical(ancestor) {
            Ok(mut resolved) => {
                for name in missing.iter().rev() {
                    resolved.push(name);
                }
                return Ok(resolved);
            }
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound
                    || std::fs::symlink_metadata(ancestor).is_ok()
                {
                    return Err(error);
                }
                let Some(name) = ancestor.file_name() else {
                    return Err(error);
                };
                missing.push(name);
                let Some(parent) = ancestor.parent() else {
                    return Err(error);
                };
                ancestor = parent;
            }
        }
    }
}

/// A path written as its bytes (`{"bytes": [..]}`, `os_path::PathArg`).
fn bytes_path(object: &serde_json::Map<String, Value>) -> Option<PathBuf> {
    if object.len() != 1 {
        return None;
    }
    let bytes: Vec<u8> = serde_json::from_value(object.get("bytes")?.clone()).ok()?;
    let path = PathBuf::from(crate::os_path::from_bytes(bytes));
    path.is_absolute().then_some(path)
}

/// Whether `text` names `folder` or something under it: its path, where
/// it ends or a separator follows.
fn holds_folder(text: &str, folder: &Path) -> bool {
    let Some(folder) = folder.to_str() else {
        return false;
    };
    text.match_indices(folder).any(|(at, _)| {
        text[at + folder.len()..]
            .chars()
            .next()
            .is_none_or(|next| next == '/' || next == '\\' || !is_path_char(next))
    })
}

fn is_path_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | '~')
}

/// What happens to an agent's call of a command, before it runs, by what
/// the command says it does (ADR-044).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentGate {
    /// It reads, changes what the window shows, or plans (a plan is
    /// proposed for review): it runs.
    Runs,
    /// It acts without a plan: the user is asked first, in the window.
    Asks,
    /// It answers what the window asked the user: never an agent's.
    Refused,
}

/// What an agent's call of `spec` does before it runs.
pub fn for_agent(spec: &CommandSpec) -> AgentGate {
    match spec.effect {
        Effect::Reads | Effect::Shows | Effect::Plans => AgentGate::Runs,
        Effect::Acts => AgentGate::Asks,
        Effect::UserOnly => AgentGate::Refused,
    }
}

thread_local! {
    /// The private folders while an agent's call runs on this thread.
    static AGENT: std::cell::RefCell<Option<Privacy>> = const { std::cell::RefCell::new(None) };
}

/// Run `f` as an agent's call: what a command counts of a pane's entries
/// for it leaves the private ones out (`App::withheld`), so no number it
/// answers tells of them, where redacting the answer could only take out
/// what it names. The control plugin runs every agent's call so; a
/// command that hands its work to another thread counts there as anyone's.
pub fn as_agent<T>(privacy: Privacy, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<Privacy>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            AGENT.with(|agent| *agent.borrow_mut() = previous);
        }
    }
    let _restore = Restore(AGENT.with(|agent| agent.borrow_mut().replace(privacy)));
    f()
}

/// The private folders, while an agent's call runs on this thread.
pub fn agent_call() -> Option<Privacy> {
    AGENT.with(|agent| agent.borrow().clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    impl Privacy {
        /// A checker that takes the mounts as `here` says them.
        fn checker_in(&self, here: Here) -> Checker<'_> {
            Checker {
                here: Some(here),
                ..self.checker()
            }
        }
    }

    /// A path a lookup would reach another machine for is never read: below
    /// an automount point nothing is mounted on, it is private (unchecked)
    /// and said to reach elsewhere wherever an agent names it; the point
    /// itself, and this machine's paths, are checked as ever.
    #[test]
    fn a_path_that_would_reach_another_machine_is_not_read() {
        let tmp = tempfile::tempdir().unwrap();
        let net = tmp.path().join("net");
        let file = net.join("host").join("export").join("it's a note");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "public, were it read").unwrap();
        let local = tmp.path().join("local note");
        std::fs::write(&local, "public").unwrap();
        let privacy = Privacy {
            folders: vec![tmp.path().join("keys")],
            marker: true,
            workers: None,
        };
        let here = || Here::with_automounts([net.clone()]);
        let mut check = privacy.checker_in(here());
        assert!(check.reaches_elsewhere(&file));
        assert!(check.holds(&file), "never read, so private");
        assert!(!check.reaches_elsewhere(&net) && !check.holds(&net));
        // Its marker would be a machine's name: never looked up.
        std::fs::write(net.join(MARKER), "").unwrap();
        assert!(!privacy.checker_in(here()).holds(&net));
        assert!(!check.holds(&local));
        let args = json!({ "expr": "ext:txt", "root": file, "pane": 1 });
        assert_eq!(check.elsewhere_in(&args), Some(file.clone()));
        let bytes = json!({ "path": { "bytes": file.as_os_str().as_encoded_bytes() } });
        assert_eq!(check.elsewhere_in(&bytes), Some(file.clone()));
        assert_eq!(check.elsewhere_in(&json!({ "root": local })), None);
        assert_eq!(
            privacy
                .checker_in(here())
                .redact(json!([file.to_str().unwrap()])),
            json!([WITHHELD])
        );
    }

    /// A path is found however an argument writes it: a folder's location
    /// (`path:`), within a search's text (`in:`), quoted in an error; a
    /// marked folder's as a listed one's. `..` never hides an automount.
    #[test]
    fn a_path_is_found_as_a_location_or_written_in_text() {
        let tmp = tempfile::tempdir().unwrap();
        let diary = tmp.path().join("it's a diary");
        std::fs::create_dir_all(diary.join("2026")).unwrap();
        std::fs::write(diary.join(MARKER), "").unwrap();
        let open = tmp.path().join("open notes");
        std::fs::create_dir(&open).unwrap();
        let net = tmp.path().join("net");
        let privacy = Privacy {
            folders: vec![],
            marker: true,
            workers: None,
        };
        let here = || Here::with_automounts([net.clone()]);
        let mut check = privacy.checker_in(here());
        let note = diary.join("2026").join("note.txt");
        let as_location = format!("path:{}", note.display());
        assert_eq!(
            check.named_in(&json!({ "location": as_location })),
            Some(note.clone())
        );
        let search = format!("in:{} report", diary.display());
        assert!(check.named_in(&json!({ "expr": search })).is_some());
        let error = format!("cannot copy “{}”: it is taken", note.display());
        assert_eq!(check.redact(json!(error)), json!(WITHHELD));
        let public = format!("cannot copy “{}”: it is taken", open.join("a").display());
        assert_eq!(check.redact(json!(public.clone())), json!(public));
        assert!(check.admits(&note).is_err() && check.admits(&open).is_ok());
        // Below an automount point, however it is written.
        let around = tmp.path().join("open notes/../net/host/share");
        assert!(check.reaches_elsewhere(&around));
        assert!(check.admits(&around).is_err());
        let location = format!("path:{}", around.display());
        assert!(
            check
                .elsewhere_in(&json!({ "location": location }))
                .is_some()
        );
        let search = format!("in:{} x", net.join("host").display());
        assert!(check.elsewhere_in(&json!({ "expr": search })).is_some());
        assert_eq!(check.elsewhere_in(&json!({ "expr": "in:recent: x" })), None);
    }

    /// On Windows a share named by its server's name connects to that
    /// server when looked up: never read for an agent.
    #[cfg(windows)]
    #[test]
    fn a_share_by_its_server_s_name_is_not_read() {
        let privacy = Privacy::folders([]);
        let mut check = privacy.checker();
        let share = Path::new(r"\\host.example\share\note.txt");
        assert!(check.reaches_elsewhere(share));
        assert_eq!(
            check.elsewhere_in(&json!({ "root": share })),
            Some(share.to_owned())
        );
        assert!(!check.reaches_elsewhere(Path::new(r"C:\Users\note.txt")));
    }

    #[test]
    fn capabilities_are_a_set_said_in_words() {
        let declared = Capabilities::of(&[Capability::Read, Capability::Programs]);
        assert!(declared.has(Capability::Read) && !declared.has(Capability::Network));
        assert_eq!(
            declared.words().as_deref(),
            Some("reads what files hold, starts programs")
        );
        assert_eq!(Capabilities::NONE.words(), None);
        assert_eq!(
            serde_json::to_value(declared).unwrap(),
            json!(["read", "programs"])
        );
        let back: Capabilities = serde_json::from_value(json!(["programs", "read"])).unwrap();
        assert_eq!(back, declared);
        assert_eq!(
            declared.beyond(Capabilities::of(&[Capability::Read])),
            Capabilities::of(&[Capability::Programs])
        );
    }

    #[test]
    fn a_folder_listed_or_marked_is_private_with_what_it_holds() {
        let tmp = tempfile::tempdir().unwrap();
        let keys = tmp.path().join("keys");
        let marked = tmp.path().join("diary");
        std::fs::create_dir_all(marked.join("2026")).unwrap();
        std::fs::write(marked.join(MARKER), "").unwrap();
        let privacy = Privacy {
            folders: vec![keys.clone()],
            marker: true,
            workers: None,
        };
        let mut check = privacy.checker();
        assert!(check.holds(&keys));
        assert!(check.holds(&keys.join("id_ed25519")));
        assert!(
            !check.holds(&tmp.path().join("keys-not")),
            "a name, not a prefix"
        );
        assert!(check.holds(&marked.join("2026").join("notes.txt")));
        assert!(!check.holds(&tmp.path().join("open.txt")));
        let unmarked = Privacy {
            marker: false,
            ..privacy.clone()
        };
        assert!(!unmarked.holds(&marked.join("2026")));
    }

    #[cfg(unix)]
    #[test]
    fn aliases_keep_the_privacy_of_both_names_and_targets() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let keys = tmp.path().join("private keys");
        let public = tmp.path().join("public");
        std::fs::create_dir(&keys).unwrap();
        std::fs::create_dir(&public).unwrap();
        std::fs::write(keys.join("id"), "synthetic secret").unwrap();
        std::fs::write(public.join("note"), "public").unwrap();
        let alias = public.join("it's an alias");
        symlink(&keys, &alias).unwrap();
        let file_alias = public.join("note-link");
        symlink(keys.join("id"), &file_alias).unwrap();
        let outward = keys.join("public-link");
        symlink(public.join("note"), &outward).unwrap();
        let privacy = Privacy::folders([keys.clone()]);
        let mut check = privacy.checker();
        assert!(check.holds(&file_alias));
        assert!(check.holds(&alias.join("id")));
        assert!(check.holds(&alias.join("new folder/new file")));
        assert!(check.holds(&outward), "a private name stays private");
        assert!(!check.holds(&public.join("note")));
        assert!(!check.holds(&public.join("new file")));
        assert_eq!(
            check.named_in(&json!({ "entries": [file_alias] })),
            Some(file_alias.clone())
        );
        assert_eq!(
            check.redact(json!([
                {"path": file_alias, "name": "note-link"},
                {"path": public.join("note"), "name": "note"}
            ])),
            json!([{"path": public.join("note"), "name": "note"}])
        );
        // The configured folder can itself be an alias too.
        let by_alias = Privacy::folders([alias.clone()]);
        assert!(by_alias.holds(&keys.join("id")));
        assert!(by_alias.holds(&file_alias));
        assert!(by_alias.holds(&keys.join("new")));
        // Dot components are resolved after links, as the filesystem does.
        assert!(privacy.holds(&alias.join("../private keys/id")));
        assert!(!privacy.holds(&alias.join("../public/note")));
    }

    #[cfg(unix)]
    #[test]
    fn marked_targets_and_unresolvable_links_are_withheld() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let diary = tmp.path().join("diary");
        std::fs::create_dir(&diary).unwrap();
        std::fs::write(diary.join(MARKER), "").unwrap();
        std::fs::write(diary.join("note"), "synthetic").unwrap();
        let alias = tmp.path().join("note");
        symlink(diary.join("note"), &alias).unwrap();
        let privacy = Privacy {
            folders: vec![],
            marker: true,
            workers: None,
        };
        assert!(privacy.holds(&alias));
        let looped = tmp.path().join("loop");
        symlink(&looped, &looped).unwrap();
        assert!(privacy.holds(&looped));
        let dangling = tmp.path().join("dangling");
        symlink(diary.join("missing"), &dangling).unwrap();
        assert!(privacy.holds(&dangling));
        assert!(!privacy.holds(&tmp.path().join("public future file")));
    }

    /// A private folder that cannot be resolved (a link to a drive not
    /// mounted) keeps what is under its name private, and leaves the other
    /// files as they are.
    #[cfg(unix)]
    #[test]
    fn a_private_folder_that_cannot_be_resolved_keeps_only_itself() {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("it's a store");
        std::os::unix::fs::symlink(tmp.path().join("unmounted/drive"), &store).unwrap();
        let keys = tmp.path().join("keys");
        std::fs::create_dir(&keys).unwrap();
        let open = tmp.path().join("open.txt");
        std::fs::write(&open, "public").unwrap();
        let privacy = Privacy::folders([store.clone(), keys.clone()]);
        let mut check = privacy.checker();
        assert!(check.holds(&store.join("bank.gpg")));
        assert!(check.holds(&keys.join("id")));
        assert!(!check.holds(&open));
    }

    /// A server that stops answering never holds the one asking: a path
    /// on it, by its name or through a link into it, is private once
    /// `NOT_ANSWERING` passed (what cannot be checked is never public),
    /// and its server is not asked again by that checker. Paths on this
    /// machine are checked as ever, and the server answering again is
    /// asked again.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_server_that_does_not_answer_keeps_its_paths_private_without_holding_the_asker() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::time::{Duration, Instant};

        use crate::fs::reach::NOT_ANSWERING;

        let tmp = tempfile::tempdir().unwrap();
        let point = tmp.path().join("server's share");
        std::fs::create_dir(&point).unwrap();
        std::fs::write(point.join("note"), "public").unwrap();
        let link = tmp.path().join("linked share");
        std::os::unix::fs::symlink(&point, &link).unwrap();
        let local = tmp.path().join("local note");
        std::fs::write(&local, "public").unwrap();
        crate::fs::reach::pretend_remote(&point);

        // The server holds every job its lane runs at once.
        let workers = Scheduler::new(4);
        let lane = crate::fs::reach::lane(&point);
        let (began, released) = (
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicBool::new(false)),
        );
        for _ in 0..crate::scheduler::LANE_RUNS {
            let (began, released) = (Arc::clone(&began), Arc::clone(&released));
            workers.spawn_in(
                lane,
                Priority::Normal,
                "held",
                crate::cancel::CancelToken::new(),
                move || {
                    began.fetch_add(1, Ordering::SeqCst);
                    while !released.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                },
            );
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while began.load(Ordering::SeqCst) < crate::scheduler::LANE_RUNS {
            assert!(Instant::now() < deadline, "the held jobs began");
            std::thread::sleep(Duration::from_millis(10));
        }

        let privacy = Privacy {
            folders: vec![tmp.path().join("keys")],
            marker: true,
            workers: None,
        }
        .looked_at_by(workers.clone());
        let mut check = privacy.checker();
        let asked = Instant::now();
        assert!(check.holds(&point.join("note")), "not checked, so private");
        let waited = asked.elapsed();
        assert!(
            waited >= NOT_ANSWERING && waited < NOT_ANSWERING + Duration::from_secs(2),
            "waited {waited:?}"
        );
        let again = Instant::now();
        assert!(check.holds(&point.join("other note")));
        assert!(
            again.elapsed() < Duration::from_millis(500),
            "not asked again"
        );
        assert!(!check.holds(&local), "this machine's paths are checked");

        let mut through_link = privacy.checker();
        let asked = Instant::now();
        assert!(
            through_link.holds(&link.join("note")),
            "a link into it is its"
        );
        assert!(asked.elapsed() < NOT_ANSWERING + Duration::from_secs(2));

        released.store(true, Ordering::SeqCst);
        assert!(
            !privacy.holds(&point.join("note")),
            "answering, it is checked"
        );
        assert!(!privacy.holds(&link.join("note")));
    }

    /// A check asked by a job already in a remote's lane (a field read
    /// there) looks in place: queued in that lane, it would wait behind
    /// the job asking it and the lane's other jobs.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_check_asked_in_its_remote_s_lane_looks_in_place() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::{Duration, Instant};

        let tmp = tempfile::tempdir().unwrap();
        let point = tmp.path().join("server's share");
        std::fs::create_dir(&point).unwrap();
        std::fs::write(point.join("note"), "public").unwrap();
        crate::fs::reach::pretend_remote(&point);
        let workers = Scheduler::new(4);
        let lane = crate::fs::reach::lane(&point);
        let released = Arc::new(AtomicBool::new(false));
        // The lane's other turns are taken.
        for _ in 1..crate::scheduler::LANE_RUNS {
            let released = Arc::clone(&released);
            workers.spawn_in(
                lane,
                Priority::Normal,
                "held",
                crate::cancel::CancelToken::new(),
                move || {
                    while !released.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                },
            );
        }
        let privacy = Privacy {
            folders: vec![tmp.path().join("keys")],
            marker: true,
            workers: None,
        }
        .looked_at_by(workers.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        let asked = Instant::now();
        workers.spawn_in(
            lane,
            Priority::Normal,
            "field",
            crate::cancel::CancelToken::new(),
            move || {
                let _ = tx.send(privacy.holds(&point.join("note")));
            },
        );
        let private = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        released.store(true, Ordering::SeqCst);
        assert!(!private, "checked, and public");
        assert!(asked.elapsed() < crate::fs::reach::NOT_ANSWERING);
    }

    /// The private folders as the settings say them: records, or paths
    /// alone as a hand writes them; a value that is no list keeps the
    /// folders private from the start.
    #[cfg(unix)]
    #[test]
    fn private_folders_are_read_however_they_are_written() {
        let settings = Settings::in_memory();
        declare(&settings);
        let home = crate::os_path::home().unwrap();
        let listed = |value: toml::Value| {
            settings.set(FOLDERS_KEY, value);
            Privacy::of(&settings).listed().to_vec()
        };
        let paths: toml::Value = toml::Value::Array(vec![
            toml::Value::String("~/it's mine".into()),
            toml::Value::String("/srv/l'été".into()),
        ]);
        assert_eq!(
            listed(paths),
            [home.join("it's mine"), PathBuf::from("/srv/l'été")]
        );
        let mut record = toml::Table::new();
        record.insert("path".into(), toml::Value::String("~/.ssh".into()));
        assert_eq!(
            listed(toml::Value::Array(vec![toml::Value::Table(record)])),
            [home.join(".ssh")]
        );
        let defaults = listed(toml::Value::String("~/.ssh".into()));
        assert!(defaults.contains(&home.join(".ssh")) && defaults.contains(&home.join(".gnupg")));
    }

    // Its paths are Unix's, which Windows reads as relative: Windows' own
    // in the next test.
    #[cfg(unix)]
    #[test]
    fn the_home_folder_is_read_for_a_tilde() {
        let home = Path::new("/home/someone");
        assert_eq!(
            expand("~/.ssh", Some(home)),
            Some(PathBuf::from("/home/someone/.ssh"))
        );
        assert_eq!(
            expand("~", Some(home)),
            Some(PathBuf::from("/home/someone"))
        );
        assert_eq!(expand("relative/dir", Some(home)), None);
        assert_eq!(expand("~other/x", Some(home)), None);
    }

    /// A home with its drive, read for a tilde either way it is written.
    #[cfg(windows)]
    #[test]
    fn the_home_folder_is_read_for_a_tilde_on_windows() {
        let home = Path::new(r"C:\Users\someone");
        assert_eq!(expand(r"~\.ssh", Some(home)), Some(home.join(".ssh")));
        assert_eq!(expand("~/.ssh", Some(home)), Some(home.join(".ssh")));
        assert_eq!(expand(r"relative\dir", Some(home)), None);
        assert_eq!(expand("~other/x", Some(home)), None);
    }

    // Its paths are Unix's, in requests' and answers' text alike.
    #[cfg(unix)]
    #[test]
    fn what_names_a_private_path_is_found_and_withheld() {
        let privacy = Privacy::folders([PathBuf::from("/home/someone/.ssh")]);
        let mut check = privacy.checker();
        let asked = json!({ "entries": ["/home/someone/.ssh/id_rsa"] });
        assert_eq!(
            check.named_in(&asked),
            Some(PathBuf::from("/home/someone/.ssh/id_rsa"))
        );
        if let Some(home) = crate::os_path::home() {
            let privacy = Privacy::folders([home.join(".ssh")]);
            let named = privacy
                .checker()
                .named_in(&json!({ "location": "~/.ssh/config" }));
            assert_eq!(
                named,
                Some(home.join(".ssh/config")),
                "from the home folder"
            );
        }
        let location = json!({ "location": "tags:/work?in=/home/someone/.ssh" });
        assert!(
            check.named_in(&location).is_some(),
            "held in a location's text"
        );
        assert!(
            check
                .named_in(&json!({ "entries": ["/home/someone/.sshkeys"] }))
                .is_none()
        );
        let bytes: Vec<u8> = b"/home/someone/.ssh/config".to_vec();
        assert!(
            check
                .named_in(&json!({ "dest": { "bytes": bytes } }))
                .is_some()
        );

        let answer = json!({
            "location": { "scheme": "path", "rest": "/home/someone/.ssh" },
            "entries": [
                { "name": "id_rsa", "path": "/home/someone/.ssh/id_rsa" },
                { "name": "notes.txt", "path": "/home/someone/notes.txt" },
            ],
            "plan": "move to /home/someone/.ssh/old",
        });
        assert_eq!(
            check.redact(answer),
            json!({
                "location": { "scheme": "path", "rest": WITHHELD },
                "entries": [{ "name": "notes.txt", "path": "/home/someone/notes.txt" }],
                "plan": WITHHELD,
            })
        );
    }
}
