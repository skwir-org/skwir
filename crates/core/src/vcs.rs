//! What a version control system says of a folder's files (ADR-134): in
//! conflict, changed, staged, new, ignored, and of a folder, the strongest
//! of what is below it. A plugin answers for a folder (`StatesProvider`),
//! on a worker, once a pane's listing is whole and again when it changes,
//! when the provider says its repository did (`generation`) or when the
//! providers change; the pane keeps each entry's state in its metadata
//! (`keys::VCS`), where every view marks it and an order sorts by it. The
//! state is the system's, never the file's: a listing keeps it across a
//! file's updates until the system says it again.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::CancelToken;
use crate::registry::{Contributions, OwnerGate};

/// What an entry is to its repository, the strongest first: a folder is
/// what the strongest below it is, a file staged and changed again is
/// changed. What is as the last commit has it has none.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Both sides of a merge changed it, and it is not settled.
    Conflict,
    /// It differs from what is staged.
    Changed,
    /// What is staged differs from the last commit.
    Staged,
    /// The system does not keep it (untracked).
    New,
    /// The system is told to leave it out. Never summed into a folder.
    Ignored,
}

impl State {
    pub const ALL: [State; 5] = [
        State::Conflict,
        State::Changed,
        State::Staged,
        State::New,
        State::Ignored,
    ];

    /// The word kept in an entry's metadata (`keys::VCS`).
    pub fn word(self) -> &'static str {
        match self {
            State::Conflict => "conflict",
            State::Changed => "changed",
            State::Staged => "staged",
            State::New => "new",
            State::Ignored => "ignored",
        }
    }

    pub fn of_word(word: &str) -> Option<State> {
        State::ALL.into_iter().find(|state| state.word() == word)
    }

    /// What a column, a caption or a group calls it.
    pub fn title(self) -> &'static str {
        match self {
            State::Conflict => "In conflict",
            State::Changed => "Changed",
            State::Staged => "Staged",
            State::New => "New",
            State::Ignored => "Ignored",
        }
    }

    /// What a folder holding it below is called, where the folder is not
    /// it itself: "Holds changes".
    pub fn below(self) -> &'static str {
        match self {
            State::Conflict => "Holds a conflict",
            State::Changed => "Holds changes",
            State::Staged => "Holds staged changes",
            State::New => "Holds new files",
            State::Ignored => "Ignored",
        }
    }

    /// An entry's state, as its listing keeps it; `None` for one as the
    /// last commit has it, or not known (`known` tells them apart): what
    /// is marked.
    pub fn of(entry: &crate::entry::Entry) -> Option<State> {
        match Self::known(entry) {
            Known::Is(state) => Some(state),
            Known::Committed | Known::Not => None,
        }
    }

    /// What the entry's listing knows of it (`keys::VCS`).
    pub fn known(entry: &crate::entry::Entry) -> Known {
        match entry.metadata.value(crate::entry::keys::VCS) {
            Some(crate::entry::Value::Bool(false)) => Known::Committed,
            Some(value) => value
                .as_str()
                .and_then(State::of_word)
                .map_or(Known::Not, Known::Is),
            None => Known::Not,
        }
    }

    /// Its place in an order by it (`known`): the most pressing first,
    /// then what is as committed, then what is not known, the ignored
    /// last.
    pub fn rank(known: Known) -> u8 {
        match known {
            Known::Is(State::Conflict) => 0,
            Known::Is(State::Changed) => 1,
            Known::Is(State::Staged) => 2,
            Known::Is(State::New) => 3,
            Known::Committed => 4,
            Known::Not => 5,
            Known::Is(State::Ignored) => 6,
        }
    }
}

/// What an entry's listing knows of it (`keys::VCS`, `State::known`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Known {
    /// Its state.
    Is(State),
    /// As the last commit has it, a whole answer says
    /// (`Value::Bool(false)`).
    Committed,
    /// Nothing is known: not asked yet, an answer that stopped short did
    /// not reach it, no repository holds it, or what was said went
    /// (`UNKNOWN`). Never said unchanged.
    Not,
}

/// The word an entry's `keys::VCS` holds once what was said of it went
/// (its provider gone, its setting off): nothing known, as before it was
/// asked. A listing keeps a value across an entry's update, so going is a
/// value too; no state has this word.
pub const UNKNOWN: &str = "unknown";

/// What a provider says of a folder: the folder itself, new or ignored
/// whole, or each path below it not as the last commit has it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Answer {
    /// The folder its paths are from: the one asked, or one above it the
    /// pane also shows (`StatesProvider::states`'s `above`).
    pub root: PathBuf,
    /// What all `root` holds is, unless a path says otherwise: new or
    /// ignored as a whole.
    pub folder: Option<State>,
    /// Each path below `root`, `/`-separated from it, not as the last
    /// commit has it: a file (one no listing holds, gone from the disk,
    /// too), or a folder new or ignored whole.
    pub states: Vec<(String, State)>,
    /// Every path below `root` was looked at: what `states` does not name
    /// is as the last commit has it. Not whole (the provider stopped at
    /// its limits), it says what it found and nothing of the rest.
    pub whole: bool,
    /// What tells this answer changed: the provider's `generation(key)`.
    pub key: u64,
}

/// An answer, ready to say what any entry under its root is: its own
/// state, else for a folder the strongest below it (`Ignored` not summed),
/// else what a folder above it is whole, else what the root is. Made on a
/// worker, in the answer's paths times their depth, with the names said
/// in each folder (`named_at`), so a listing is told what changed in it
/// by its names, never walked entry by entry for what nobody said.
#[derive(Debug, Default)]
pub struct States {
    root: PathBuf,
    folder: Option<State>,
    own: HashMap<String, State>,
    below: HashMap<String, State>,
    /// The names said in each folder below the root (`""` for the root),
    /// own or held below.
    named: HashMap<String, Vec<String>>,
    whole: bool,
    key: u64,
}

impl States {
    pub fn new(answer: Answer) -> Self {
        let mut own = HashMap::with_capacity(answer.states.len());
        let mut below: HashMap<String, State> = HashMap::new();
        let mut named: HashMap<String, std::collections::HashSet<String>> = HashMap::new();
        let mut name = |path: &str| {
            let (level, name) = path.rsplit_once('/').unwrap_or(("", path));
            named
                .entry(level.to_owned())
                .or_default()
                .insert(name.to_owned());
        };
        for (path, state) in &answer.states {
            let path = path.trim_end_matches('/');
            if path.is_empty() {
                continue;
            }
            own.entry(path.to_owned())
                .and_modify(|known: &mut State| *known = (*known).min(*state))
                .or_insert(*state);
            name(path);
            if *state == State::Ignored {
                continue;
            }
            let mut end = path.len();
            while let Some(slash) = path[..end].rfind('/') {
                end = slash;
                let folder = &path[..end];
                if let Some(known) = below.get_mut(folder) {
                    *known = (*known).min(*state);
                } else {
                    below.insert(folder.to_owned(), *state);
                    name(folder);
                }
            }
        }
        States {
            root: answer.root,
            folder: answer.folder,
            own,
            below,
            named: named
                .into_iter()
                .map(|(level, names)| (level, names.into_iter().collect()))
                .collect(),
            whole: answer.whole,
            key: answer.key,
        }
    }

    /// The folder its paths are from (`Answer::root`).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// What tells it changed (`Answer::key`).
    pub fn key(&self) -> u64 {
        self.key
    }

    /// Whether every path was looked at (`Answer::whole`): an entry it
    /// says nothing of is as the last commit has it.
    pub fn is_whole(&self) -> bool {
        self.whole
    }

    /// What the entry at `relative` (from the root, `/`-separated) is;
    /// `is_dir` for a folder, which holds what is below it.
    pub fn of(&self, relative: &str, is_dir: bool) -> Option<State> {
        if let Some(state) = self.own.get(relative) {
            return Some(*state);
        }
        if is_dir && let Some(state) = self.below.get(relative) {
            return Some(*state);
        }
        match relative.rsplit_once('/') {
            Some((up, _)) => self.default_at(up),
            None => self.folder,
        }
    }

    /// The names said in the folder at `level` (from the root, `""` for
    /// the root itself): the entries of its listing an answer names.
    pub fn named_at(&self, level: &str) -> &[String] {
        self.named.get(level).map_or(&[], Vec::as_slice)
    }

    /// What every entry of the folder at `level` is that no name says
    /// otherwise: the folder, or one above it, new or ignored as a whole,
    /// else what the root is (`Answer::folder`).
    pub fn default_at(&self, level: &str) -> Option<State> {
        let mut at = Some(level);
        while let Some(path) = at.filter(|path| !path.is_empty()) {
            if let Some(state @ (State::New | State::Ignored)) = self.own.get(path).copied() {
                return Some(state);
            }
            at = path.rsplit_once('/').map(|(up, _)| up);
        }
        self.folder
    }
}

/// What a version control system says of the files of a folder.
pub trait StatesProvider: Send + Sync {
    fn id(&self) -> &'static str;
    /// What the entries of `folder` and below are, read now, on a worker in
    /// the folder's lane; `Ok(None)` when it is in no repository the
    /// provider knows, or the provider shows nothing now (its setting).
    /// `above` are the folders above it the pane shows too (a columns
    /// view's): the answer may be from the highest of them in the same
    /// repository (`Answer::root`), so one answer says what all of them
    /// hold. `opened` are the folders below it opened in place (a tree's):
    /// what a folder said new whole holds that is not new (ignored) is
    /// said for them too, as for the folder asked.
    fn states(
        &self,
        folder: &Path,
        above: &[PathBuf],
        opened: &[PathBuf],
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Answer>>;
    /// Moves when what an answer of `key` (`Answer::key`) said may have
    /// changed: its repository's index or `HEAD` did, a window came to the
    /// front (what is below a folder shown has no watch). Asked by the
    /// pump: from memory, never the disk.
    fn generation(&self, key: u64) -> u64;
}

/// The registered providers; the first that answers says.
#[derive(Default)]
pub struct StatesProviders {
    list: Contributions<Arc<dyn StatesProvider>>,
}

impl StatesProviders {
    pub fn gated(gate: Arc<OwnerGate>) -> Self {
        StatesProviders {
            list: Contributions::gated(gate),
        }
    }

    pub fn register(&self, owner: &str, provider: Arc<dyn StatesProvider>) {
        self.list.add(owner, provider);
    }

    pub fn unregister_owner(&self, owner: &str) {
        self.list.remove_owner(owner);
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.list.all().iter().map(|p| p.id()).collect()
    }

    /// Advanced when providers come or go.
    pub fn revision(&self) -> u64 {
        self.list.revision()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// What the first provider that knows `folder` says of it (and of the
    /// folders `above` it shown too, and those `opened` in place below),
    /// and its id.
    pub fn states(
        &self,
        folder: &Path,
        above: &[PathBuf],
        opened: &[PathBuf],
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<(&'static str, Answer)>> {
        for provider in self.list.all() {
            if cancel.is_cancelled() {
                return Ok(None);
            }
            if let Some(answer) = provider.states(folder, above, opened, cancel)? {
                return Ok(Some((provider.id(), answer)));
            }
        }
        Ok(None)
    }

    /// The generation of the answer `key` provider `id` gave; 0 where it is
    /// gone (the revision moved with it).
    pub fn generation(&self, id: &str, key: u64) -> u64 {
        self.list
            .find(|provider| provider.id() == id)
            .map_or(0, |provider| provider.generation(key))
    }
}

#[cfg(test)]
mod tests {
    use super::{Answer, Known, State, States};

    /// A file is its own state; a folder the strongest below it, what is
    /// ignored not summed; what is inside a folder new or ignored whole is
    /// it; a root new whole makes all it holds new but what a path says.
    #[test]
    fn an_entry_is_its_own_state_or_the_strongest_below_it() {
        let answer = Answer {
            folder: None,
            states: vec![
                ("src/a.rs".into(), State::Changed),
                ("src/deep/b.rs".into(), State::Staged),
                ("src/deep/c.rs".into(), State::Conflict),
                ("docs/new".into(), State::New),
                ("target".into(), State::Ignored),
                ("logs/x.log".into(), State::Ignored),
            ],
            key: 1,
            ..Answer::default()
        };
        let states = States::new(answer);
        assert_eq!(states.of("src/a.rs", false), Some(State::Changed));
        assert_eq!(
            states.of("src", true),
            Some(State::Conflict),
            "the strongest"
        );
        assert_eq!(states.of("src/deep", true), Some(State::Conflict));
        assert_eq!(states.of("docs", true), Some(State::New));
        assert_eq!(states.of("docs/new/inside.md", false), Some(State::New));
        assert_eq!(states.of("target", true), Some(State::Ignored));
        assert_eq!(states.of("target/debug", true), Some(State::Ignored));
        assert_eq!(states.of("logs", true), None, "ignored is not summed");
        assert_eq!(states.of("README.md", false), None);
        assert_eq!(states.of("src", false), None, "a file of a folder's name");
        let whole = States::new(Answer {
            folder: Some(State::New),
            states: vec![("a.log".into(), State::Ignored)],
            ..Answer::default()
        });
        assert_eq!(whole.of("anything", false), Some(State::New));
        assert_eq!(whole.of("a.log", false), Some(State::Ignored), "said apart");
    }

    /// Each folder's listing is told the names said in it, and what the
    /// rest of it is: a folder new or ignored whole, else nothing.
    #[test]
    fn each_folder_is_told_the_names_said_in_it() {
        let states = States::new(Answer {
            states: vec![
                ("src/a.rs".into(), State::Changed),
                ("src/deep/b.rs".into(), State::Staged),
                ("notes/".into(), State::New),
                ("top.txt".into(), State::Staged),
            ],
            whole: true,
            ..Answer::default()
        });
        let sorted = |level: &str| {
            let mut names = states.named_at(level).to_vec();
            names.sort();
            names
        };
        assert_eq!(sorted(""), ["notes", "src", "top.txt"]);
        assert_eq!(sorted("src"), ["a.rs", "deep"]);
        assert_eq!(sorted("src/deep"), ["b.rs"]);
        assert!(sorted("docs").is_empty());
        assert_eq!(states.default_at(""), None);
        assert_eq!(states.default_at("src/deep"), None);
        assert_eq!(states.default_at("notes"), Some(State::New));
        assert_eq!(states.default_at("notes/inside"), Some(State::New));
        assert!(states.is_whole());
    }

    #[test]
    fn a_state_is_kept_as_its_word_and_ranked_most_pressing_first() {
        for state in State::ALL {
            assert_eq!(State::of_word(state.word()), Some(state));
        }
        assert_eq!(State::of_word(super::UNKNOWN), None);
        let mut ranked: Vec<Known> = State::ALL.into_iter().map(Known::Is).collect();
        ranked.extend([Known::Not, Known::Committed]);
        ranked.sort_by_key(|known| State::rank(*known));
        assert_eq!(
            ranked,
            [
                Known::Is(State::Conflict),
                Known::Is(State::Changed),
                Known::Is(State::Staged),
                Known::Is(State::New),
                Known::Committed,
                Known::Not,
                Known::Is(State::Ignored)
            ]
        );
    }
}
