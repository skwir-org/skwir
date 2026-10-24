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

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::command::{CommandSpec, Effect};
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
#[derive(Clone, Debug, Default)]
pub struct Privacy {
    folders: Vec<PathBuf>,
    marker: bool,
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
        }
    }

    /// Private to these folders, no marker read: for tests.
    pub fn folders(folders: impl IntoIterator<Item = PathBuf>) -> Privacy {
        Privacy {
            folders: folders.into_iter().collect(),
            marker: false,
        }
    }

    /// The folders listed, as absolute paths.
    pub fn listed(&self) -> &[PathBuf] {
        &self.folders
    }

    /// A checker of many paths, which reads each folder's marker once.
    pub fn checker(&self) -> Checker<'_> {
        Checker {
            privacy: self,
            marked: HashMap::new(),
            resolved_folders: None,
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

/// Checks paths against a [`Privacy`], remembering the markers it read.
pub struct Checker<'a> {
    privacy: &'a Privacy,
    marked: HashMap<PathBuf, bool>,
    /// Resolve lazily: taking a Privacy snapshot itself never reads disk.
    resolved_folders: Option<Vec<PathBuf>>,
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
        if self.marked(path) {
            return true;
        }
        if self.privacy.folders.is_empty() && !self.privacy.marker {
            return false;
        }
        // A missing final name is normal for a proposed write. Resolve its
        // existing parent too; other failures cannot establish a public path.
        let Ok(resolved) = resolved_name(path) else {
            return true;
        };
        // A folder that cannot be resolved (a link to a drive not
        // mounted, a parent that cannot be read) holds what is under it as
        // it is written, checked above: nothing reached through it could
        // be resolved either. It never makes every path private.
        let folders = self.resolved_folders.get_or_insert_with(|| {
            self.privacy
                .folders
                .iter()
                .filter_map(|folder| match resolved_name(folder) {
                    Ok(resolved) => Some(resolved),
                    Err(error) => {
                        tracing::debug!("a private folder cannot be resolved: {error}");
                        None
                    }
                })
                .collect()
        });
        if folders.iter().any(|folder| resolved.starts_with(folder)) {
            return true;
        }
        self.marked(&resolved)
    }

    fn marked(&mut self, path: &Path) -> bool {
        if !self.privacy.marker {
            return false;
        }
        path.ancestors().any(|folder| {
            if let Some(known) = self.marked.get(folder) {
                return *known;
            }
            let marked = match std::fs::symlink_metadata(folder.join(MARKER)) {
                Ok(_) => true,
                Err(error) => !matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ),
            };
            self.marked.insert(folder.to_owned(), marked);
            marked
        })
    }

    /// The first private path `value` names: a string that is a path, or
    /// that holds a private folder listed (a location's text, a title), or
    /// a path written as its bytes.
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
        let path = Path::new(text);
        if path.is_absolute() && self.holds(path) {
            return Some(path.to_owned());
        }
        // A path from the home folder, as a location may be written.
        if text.starts_with('~')
            && let Some(path) = expand(text, crate::os_path::home().as_deref())
            && self.holds(&path)
        {
            return Some(path);
        }
        self.privacy
            .folders
            .iter()
            .find(|folder| holds_folder(text, folder))
            .cloned()
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
            Value::String(text) => {
                let path = Path::new(text);
                path.is_absolute() && self.holds(path)
            }
            Value::Object(object) => bytes_path(object).is_some_and(|path| self.holds(&path)),
            _ => false,
        }
    }
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
