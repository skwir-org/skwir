//! Names indexes (ADR-150, 2): what answers, faster than a walk, which
//! files under a root may match a search's names and kinds, and how far
//! that answer is proven (`crate::coverage`). skwird's is its plugin's;
//! plocate's is the query plugin's. Registered by owner, withdrawn with it.
//!
//! An index may name more files than match, and fewer only where it says
//! it does not cover (a hole, or the root itself): the search checks every
//! file it names as any other (ADR-050, 4), walks the holes, and walks
//! everything where no index answers. An index changes what is fast, never
//! what is found (`docs/invariants/indexes.md`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::cancel::CancelToken;
use crate::coverage::Coverage;
use crate::entry::EntryKind;
use crate::registry::{Contributions, OwnerGate};
use crate::words::Words;

/// A name every file must have, matched as the query plugin matches a
/// name term (`NamePat`): a glob where the text holds `*`, `?` or `[` and
/// compiles as one (globset, `literal_separator(false)`), else a
/// substring; both sides lowercased unless case counts. skwird matches so
/// (its `Match::Skwir`); a change here is told to its session first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameTerm {
    pub text: String,
    pub case_sensitive: bool,
}

/// What a search asks an index: the terms of its AND an index can answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    /// The folder searched, as the index knows it: its real path
    /// (`os_path::canonical`). What it names is below it, real too.
    pub root: PathBuf,
    /// Names a file must all have, its own name's (a link's, not its
    /// target's).
    pub names: Vec<NameTerm>,
    /// Kinds of which a file's own must be one (`None`: any). A link is
    /// `Symlink` whatever it leads to, so a search for folders asks for
    /// links too, and checks what they lead to itself.
    pub kinds: Option<Vec<EntryKind>>,
    /// Whether these terms are the whole search, nothing else asked of a
    /// file. An index that proves nothing may answer only such a search
    /// (plocate, ADR-050).
    pub whole: bool,
}

/// How an index's answer ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    /// The coverage that held while it answered.
    pub coverage: Coverage,
    /// Whether it named fewer than it found (a limit of its own): what it
    /// left unnamed is walked.
    pub truncated: bool,
}

/// One index's answers to one search, on a connection of the search's own
/// where it has one, given up on with the search's token: dropped, it is
/// let go of.
pub trait Session: Send {
    /// Prove the root current now: a barrier where it is live, a check
    /// elsewhere. `None` where it failed, or cannot ([`NameIndex::syncs`]).
    fn sync(&mut self) -> Option<Coverage>;

    /// Name the files below `q.root` that may match, a batch at a time,
    /// until `each` says to stop. `None` where it cannot answer this
    /// query, or failed: the search then walks what it has not
    /// considered.
    fn query(&mut self, q: &Query, each: &mut dyn FnMut(Vec<PathBuf>) -> bool) -> Option<Answer>;
}

/// An index a search was opened on, as it covers the root.
pub struct Opened {
    pub coverage: Coverage,
    pub session: Box<dyn Session>,
}

/// A names index.
pub trait NameIndex: Send + Sync {
    fn id(&self) -> &'static str;

    /// What answered, in the reader's words: said where the answer is
    /// (`location::RestsOn::by`), "plocate's database".
    fn by(&self) -> Words;

    /// Whether a sync can prove its answer. One that cannot (plocate) is
    /// taken as of when it is; one that can and fails is walked.
    fn syncs(&self) -> bool;

    /// Ready a search below `root` (a real path): how far it covers it now,
    /// and the session that answers. `None` where it has nothing to say
    /// (no engine, another root). Blocking, on the search's worker; given
    /// up on with `cancel`.
    fn open(&self, root: &Path, cancel: &CancelToken) -> Option<Opened>;

    /// How far it would answer `q` now, from what already runs: never
    /// starting anything, nor telling it anything, and answering within a
    /// socket's round trip or not at all (ADR-150, D1). What the address
    /// field asks as each key is typed, on a worker, to show a search of
    /// the whole disk at once where an index would answer it. `None`
    /// (the default): it cannot tell without opening, or would not answer
    /// `q`.
    fn covers(&self, _q: &Query, _cancel: &CancelToken) -> Option<Coverage> {
        None
    }
}

/// An index opened for a search: which, how far it covers, its session.
pub struct Asked {
    pub index: Arc<dyn NameIndex>,
    pub coverage: Coverage,
    pub session: Box<dyn Session>,
}

/// The registered names indexes, in registration order.
#[derive(Default)]
pub struct NameIndexes {
    list: Contributions<Arc<dyn NameIndex>>,
}

impl NameIndexes {
    pub fn gated(gate: Arc<OwnerGate>) -> Self {
        NameIndexes {
            list: Contributions::gated(gate),
        }
    }

    pub fn register(&self, owner: &str, index: Arc<dyn NameIndex>) {
        self.list.add(owner, index);
    }

    pub fn unregister_owner(&self, owner: &str) {
        self.list.remove_owner(owner);
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.list.all().iter().map(|i| i.id()).collect()
    }

    /// Every index that answers below `root` (a real path), the strongest
    /// proof of names first (live, checked, as of), registration order
    /// among equals; none that does not cover it.
    pub fn open(&self, root: &Path, cancel: &CancelToken) -> Vec<Asked> {
        let mut asked: Vec<Asked> = Vec::new();
        for index in self.list.all() {
            if cancel.is_cancelled() {
                return Vec::new();
            }
            let Some(Opened { coverage, session }) = index.open(root, cancel) else {
                continue;
            };
            if coverage.names.answers() {
                asked.push(Asked {
                    index,
                    coverage,
                    session,
                });
            }
        }
        asked.sort_by_key(|a| std::cmp::Reverse(a.coverage.names.strength()));
        asked
    }

    /// What each index would answer of `q` now ([`NameIndex::covers`]),
    /// the strongest proof of names first, as [`open`](Self::open) sorts
    /// them; none that cannot tell or does not cover it.
    pub fn covers(&self, q: &Query, cancel: &CancelToken) -> Vec<(Arc<dyn NameIndex>, Coverage)> {
        let mut covered: Vec<(Arc<dyn NameIndex>, Coverage)> = Vec::new();
        for index in self.list.all() {
            if cancel.is_cancelled() {
                return Vec::new();
            }
            if let Some(coverage) = index.covers(q, cancel)
                && coverage.names.answers()
            {
                covered.push((index, coverage));
            }
        }
        covered.sort_by_key(|(_, c)| std::cmp::Reverse(c.names.strength()));
        covered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coverage::{Proof, Why};
    use std::time::{Duration, SystemTime};

    struct Fixed(&'static str, Coverage);

    struct Nothing;

    impl Session for Nothing {
        fn sync(&mut self) -> Option<Coverage> {
            None
        }
        fn query(&mut self, _: &Query, _: &mut dyn FnMut(Vec<PathBuf>) -> bool) -> Option<Answer> {
            None
        }
    }

    impl NameIndex for Fixed {
        fn id(&self) -> &'static str {
            self.0
        }
        fn by(&self) -> Words {
            Words::as_written(self.0)
        }
        fn syncs(&self) -> bool {
            true
        }
        fn open(&self, _: &Path, _: &CancelToken) -> Option<Opened> {
            Some(Opened {
                coverage: self.1.clone(),
                session: Box::new(Nothing),
            })
        }
        fn covers(&self, _: &Query, _: &CancelToken) -> Option<Coverage> {
            Some(self.1.clone())
        }
    }

    #[test]
    fn the_strongest_proof_is_asked_first_and_none_that_does_not_cover() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_007);
        let indexes = NameIndexes::default();
        let proof = |names| Coverage {
            names,
            attrs: Proof::AsOf { at: Some(t) },
            holes: Vec::new(),
        };
        indexes.register("a", Arc::new(Fixed("as of", Coverage::as_of(Some(t)))));
        indexes.register(
            "b",
            Arc::new(Fixed("denied", Coverage::not_covered(Why::Denied))),
        );
        indexes.register(
            "c",
            Arc::new(Fixed("checked", proof(Proof::Checked { at: t }))),
        );
        indexes.register(
            "d",
            Arc::new(Fixed("live", proof(Proof::Live { since: t }))),
        );
        indexes.register(
            "e",
            Arc::new(Fixed("checked too", proof(Proof::Checked { at: t }))),
        );
        let asked: Vec<_> = indexes
            .open(Path::new("/"), &CancelToken::new())
            .into_iter()
            .map(|a| a.index.id())
            .collect();
        assert_eq!(asked, ["live", "checked", "checked too", "as of"]);
        let q = Query {
            root: PathBuf::from("/"),
            names: Vec::new(),
            kinds: None,
            whole: true,
        };
        let covered: Vec<_> = indexes
            .covers(&q, &CancelToken::new())
            .into_iter()
            .map(|(index, _)| index.id())
            .collect();
        assert_eq!(covered, asked, "told as they are opened");
        indexes.unregister_owner("d");
        assert_eq!(indexes.ids(), ["as of", "denied", "checked", "checked too"]);
    }
}
