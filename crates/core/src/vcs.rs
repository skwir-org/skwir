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
use std::path::Path;
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
    /// last commit has it, or not known.
    pub fn of(entry: &crate::entry::Entry) -> Option<State> {
        entry
            .metadata
            .value(crate::entry::keys::VCS)
            .and_then(crate::entry::Value::as_str)
            .and_then(State::of_word)
    }

    /// Its place in an order by it: the most pressing first, what has none
    /// after every state but `Ignored`, which is last.
    pub fn rank(state: Option<State>) -> u8 {
        match state {
            Some(State::Conflict) => 0,
            Some(State::Changed) => 1,
            Some(State::Staged) => 2,
            Some(State::New) => 3,
            None => 4,
            Some(State::Ignored) => 5,
        }
    }
}

/// What a provider says of a folder: the folder itself, new or ignored
/// whole, or each path below it not as the last commit has it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Answer {
    /// The folder asked is new or ignored as a whole: all it holds is too.
    pub folder: Option<State>,
    /// Each path below the folder, `/`-separated from it, not as the last
    /// commit has it: a file (one no listing holds, gone from the disk,
    /// too), or a folder new or ignored whole.
    pub states: Vec<(String, State)>,
    /// What tells this answer changed: the provider's `generation(key)`.
    pub key: u64,
}

/// An answer, ready to say what any entry under the folder asked is: its
/// own state, else for a folder the strongest below it (`Ignored` not
/// summed), else what a folder above it is whole. Made in the answer's
/// paths times their depth; each entry then said in its own depth.
#[derive(Debug, Default)]
pub struct States {
    folder: Option<State>,
    own: HashMap<String, State>,
    below: HashMap<String, State>,
}

impl States {
    pub fn new(answer: &Answer) -> Self {
        let mut own = HashMap::with_capacity(answer.states.len());
        let mut below: HashMap<String, State> = HashMap::new();
        for (path, state) in &answer.states {
            let path = path.trim_end_matches('/');
            own.entry(path.to_owned())
                .and_modify(|known: &mut State| *known = (*known).min(*state))
                .or_insert(*state);
            if *state == State::Ignored {
                continue;
            }
            let mut end = path.len();
            while let Some(slash) = path[..end].rfind('/') {
                end = slash;
                let known = below.entry(path[..end].to_owned()).or_insert(*state);
                *known = (*known).min(*state);
            }
        }
        States {
            folder: answer.folder,
            own,
            below,
        }
    }

    /// What the entry at `relative` (from the folder asked, `/`-separated)
    /// is; `is_dir` for a folder, which holds what is below it.
    pub fn of(&self, relative: &str, is_dir: bool) -> Option<State> {
        if self.folder.is_some() {
            return self.folder;
        }
        if let Some(state) = self.own.get(relative) {
            return Some(*state);
        }
        if is_dir && let Some(state) = self.below.get(relative) {
            return Some(*state);
        }
        // Inside a folder new or ignored as a whole.
        let mut end = relative.len();
        while let Some(slash) = relative[..end].rfind('/') {
            end = slash;
            if let Some(state @ (State::New | State::Ignored)) = self.own.get(&relative[..end]) {
                return Some(*state);
            }
        }
        None
    }
}

/// What a version control system says of the files of a folder.
pub trait StatesProvider: Send + Sync {
    fn id(&self) -> &'static str;
    /// What the entries of `folder` and below are, read now, on a worker in
    /// the folder's lane; `Ok(None)` when it is in no repository the
    /// provider knows, or the provider shows nothing now (its setting).
    fn states(&self, folder: &Path, cancel: &CancelToken) -> anyhow::Result<Option<Answer>>;
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

    /// What the first provider that knows `folder` says of it, and its id.
    pub fn states(
        &self,
        folder: &Path,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<(&'static str, Answer)>> {
        for provider in self.list.all() {
            if cancel.is_cancelled() {
                return Ok(None);
            }
            if let Some(answer) = provider.states(folder, cancel)? {
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
    use super::{Answer, State, States};

    /// A file is its own state; a folder the strongest below it, what is
    /// ignored not summed; what is inside a folder new or ignored whole is
    /// it; a folder asked that is new whole makes all it holds new.
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
        };
        let states = States::new(&answer);
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
        let whole = States::new(&Answer {
            folder: Some(State::New),
            ..Answer::default()
        });
        assert_eq!(whole.of("anything", false), Some(State::New));
    }

    #[test]
    fn a_state_is_kept_as_its_word_and_ranked_most_pressing_first() {
        for state in State::ALL {
            assert_eq!(State::of_word(state.word()), Some(state));
        }
        let mut ranked: Vec<Option<State>> = State::ALL.into_iter().map(Some).collect();
        ranked.push(None);
        ranked.sort_by_key(|state| State::rank(*state));
        assert_eq!(
            ranked,
            [
                Some(State::Conflict),
                Some(State::Changed),
                Some(State::Staged),
                Some(State::New),
                None,
                Some(State::Ignored)
            ]
        );
    }
}
