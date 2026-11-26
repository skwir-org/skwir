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
//!
//! An index may keep a journal (ADR-150, D5): what changed below a root
//! since a [`Mark`], and how far that is every change. Who keeps state of
//! its own from a walk (the tags index, a harvest) follows it instead of
//! walking again, where it is complete ([`Journal`]), on a session of what
//! already runs ([`NameIndex::follow`]). It may also sum what a folder
//! holds ([`Session::totals`], D3): what Usage measures, without reading
//! the disk.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use crate::cancel::CancelToken;
use crate::coverage::{Coverage, Hole};
use crate::entry::EntryKind;
use crate::name_match::NameMatch;
use crate::registry::{Contributions, OwnerGate};
use crate::words::Words;

/// A name every file must have, matched as every place Skwir matches a
/// name matches it ([`NameMatch::term`]): a glob where the text holds
/// `*`, `?` or `[` and compiles as one (globset, `literal_separator(false)`),
/// else a substring; case ignored unless it counts, both sides lowercased
/// (a glob of ASCII literals folding ASCII alone, on the name as it is);
/// in both its forms, composed and decomposed (NFC and NFD), lowercased
/// first, then normalized, a glob in its literal parts alone. skwird's
/// protocol 0 (its `Match::Skwir`) matches a term as written, one form: it
/// names what this finds only where [`NameTerm::as_written`] says so, and
/// an engine that matches both forms says it, on the connection it answers
/// on ([`Session::both_forms`], [`NameIndex::covers_both_forms`]). A change
/// here is told to skwird's session first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameTerm {
    pub text: String,
    pub case_sensitive: bool,
}

impl NameTerm {
    /// What finds the names it asks for.
    pub fn matcher(&self) -> NameMatch {
        NameMatch::term(&self.text, self.case_sensitive)
    }

    /// Whether an index that matches it as written, in one form, names
    /// every file it finds ([`NameMatch::as_written`]): every ASCII term,
    /// and one whose forms are one, but, where case is ignored, a glob
    /// holding more than ASCII and a word holding a sigma.
    pub fn as_written(&self) -> bool {
        self.text.is_ascii() || self.matcher().as_written()
    }
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
    /// Sizes from the first to the second, both included: what the file
    /// opens weighs, a link's target's, as the search tests it. An index
    /// that holds a link's own names its links whatever they weigh, and
    /// the search stats each (ADR-150, D2).
    pub size: Option<(u64, u64)>,
    /// Modification times, both ends included, a link's as for `size`.
    pub modified: Option<Times>,
    /// Whether these terms are the whole search, nothing else asked of a
    /// file. An index that proves nothing may answer only such a search
    /// (plocate, ADR-050).
    pub whole: bool,
}

/// Times from `from` to `to`, both included; `None`: no end there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Times {
    pub from: Option<SystemTime>,
    pub to: Option<SystemTime>,
}

impl Query {
    /// What an index that does not hold sizes and times live is asked
    /// (ADR-150, 5: a check proves names and kinds only): this without
    /// them, the search testing them on what it names, and so not the
    /// whole search. `None` where nothing else is asked.
    pub fn by_name(&self) -> Option<Query> {
        if self.size.is_none() && self.modified.is_none() {
            return Some(self.clone());
        }
        if self.names.is_empty() && self.kinds.is_none() {
            return None;
        }
        Some(Query {
            size: None,
            modified: None,
            whole: false,
            ..self.clone()
        })
    }

    /// Whether every name term is matched alike as written, in one form
    /// ([`NameTerm::as_written`]: every ASCII one), so that an index that
    /// matches so names every file the search finds by its names.
    pub fn as_written(&self) -> bool {
        self.names.iter().all(NameTerm::as_written)
    }

    /// Whether what `session` names may be taken as what this search finds
    /// by its names: where every term is matched alike as written, or where
    /// the engine it talks to said it matches both forms
    /// ([`Session::both_forms`]). Elsewhere it may name fewer than match:
    /// the index is not taken as covering, and the search walks.
    pub fn answered_by(&self, session: &dyn Session) -> bool {
        session.both_forms() || self.as_written()
    }

    /// What `coverage` lets an index be asked of this: sizes and times
    /// only where it says them live ([`by_name`](Self::by_name) elsewhere).
    pub fn as_covered(&self, coverage: &Coverage) -> Option<Query> {
        if matches!(coverage.attrs, crate::coverage::Proof::Live { .. }) {
            Some(self.clone())
        } else {
            self.by_name()
        }
    }
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

/// Where in an index's journal its changes are asked from next: kept by
/// who follows a root, with the index it is of.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Mark {
    /// The index's id ([`NameIndex::id`]).
    pub by: String,
    /// The index's own, opaque here.
    pub token: String,
}

/// An entry a journal names, as the index names it: its real path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Noted {
    pub path: PathBuf,
    /// Its own kind: a link is `Symlink`.
    pub kind: EntryKind,
    /// Its device and inode, as a stat says them, where the index has them.
    pub id: Option<(u64, u64)>,
}

/// One change a journal tells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Added(Noted),
    /// What it was: a folder removed takes what was below it.
    Removed(Noted),
    /// Its size, times or anything else a stat says (a tag set as an
    /// attribute moves its change time alone).
    Changed(Noted),
    /// A subtree not covered from now on: its changes are told no more.
    NotCovered(Hole),
}

/// What changed below a root since a mark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Journal {
    pub changes: Vec<Change>,
    /// Where to ask from next.
    pub mark: Mark,
    /// Whether `changes` hold every name made, removed or renamed since the
    /// mark asked from, outside `holes`.
    pub names: bool,
    /// Whether they hold every change of what a stat says, a change time's
    /// alone among them, outside `holes`: only where the root was live all
    /// along.
    pub attrs: bool,
    /// The folders below the root not covered: their changes are in none.
    pub holes: Vec<Hole>,
}

impl Journal {
    /// Whether it holds every change, of names and of what a stat says:
    /// what stands for a walk that compares both.
    pub fn complete(&self) -> bool {
        self.names && self.attrs
    }
}

/// What a subtree holds, as an index sums it (ADR-150, D3): its files'
/// bytes (every entry but folders; a folder's own bytes are not counted),
/// how many files and folders (the subtree's own not counted), when its
/// newest file changed, its files by extension, and what deleting it
/// would not free. A file alone is its own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub size: u64,
    /// The bytes the filesystem holds for them.
    pub alloc: u64,
    /// Every entry but folders, a file of several names once.
    pub files: u64,
    /// Every entry but folders, each name counted, as a listing shows
    /// them: a file and its hard link in one folder are two.
    pub names: u64,
    pub folders: u64,
    /// The newest modification of its files (every entry but folders):
    /// none where it holds none, a folder then dated by its own
    /// ([`Child::mtime`], [`Summed::mtime`]).
    pub newest_file: Option<SystemTime>,
    /// Its files by their names' extensions, the largest first.
    pub by_extension: Vec<Extension>,
    /// The bytes of its files with names outside it.
    pub shared: u64,
}

/// Files of one extension in a subtree: the name from its last dot on,
/// the dot left out, lowercased (`None`: no dot).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Extension {
    pub ext: Option<OsString>,
    pub files: u64,
    pub alloc: u64,
}

/// An entry of a folder an index summed, with its totals: a file's own, a
/// folder's subtree's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Child {
    /// Its real path.
    pub path: PathBuf,
    /// Its own kind: a link is `Symlink`.
    pub kind: EntryKind,
    /// Its device and inode, as a stat says them.
    pub id: Option<(u64, u64)>,
    /// Its own modification time, a folder's whatever it holds.
    pub mtime: Option<SystemTime>,
    pub totals: Totals,
}

/// The entries an answer sends of a large folder: the heaviest by bytes,
/// and those with the most files; the others summed ([`Summed::rest`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Largest {
    pub alloc: u64,
    pub files: u64,
}

/// How a folder's totals ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summed {
    /// The folder's own.
    pub totals: Totals,
    /// The folder's own modification time.
    pub mtime: Option<SystemTime>,
    /// Where the journal stood as they were summed: its changes since say
    /// what moved them.
    pub mark: Mark,
    /// Its `attrs` proof is the totals'; its holes, what is not counted.
    pub coverage: Coverage,
    /// Whether every file's count of names was known: else `shared` is at
    /// least what it says.
    pub links_known: bool,
    /// The entries sent.
    pub count: u64,
    /// The entries not sent, how many and their totals summed as the
    /// folder's sum its entries (a folder among them counted).
    pub rest: Option<(u64, Totals)>,
}

/// A journal asked from a mark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Since {
    Journal(Journal),
    /// The mark is older than what the index keeps (or of an index rebuilt
    /// since): walk again, from a mark taken now.
    TooOld,
}

/// One index's answers to one search, on a connection of the search's own
/// where it has one, given up on with the search's token: dropped, it is
/// let go of.
pub trait Session: Send {
    /// Whether the engine this session talks to said it matches a name term
    /// in both its forms, composed and decomposed, as [`NameMatch::term`]
    /// matches it (lowercased first, then normalized; a glob in its literal
    /// parts alone, and, where those hold more than ASCII and case is
    /// ignored, lowercased and matched on the name lowercased): skwird in
    /// its hello's features, `names-both-forms` (its 0.2); plocate, asked
    /// each form. Read from what the connection said, never kept from an
    /// earlier one: an engine replaced by an older one says less. One that
    /// does not (the default: it matches a term as written, one form, as
    /// skwird's protocol 0) is taken as covering a search only where every
    /// name term is matched alike either way ([`Query::answered_by`]).
    fn both_forms(&self) -> bool {
        false
    }

    /// Prove the root current now: a barrier where it is live, a check
    /// elsewhere. `None` where it failed, or cannot ([`NameIndex::syncs`]).
    fn sync(&mut self) -> Option<Coverage>;

    /// Name the files below `q.root` that may match, a batch at a time,
    /// until `each` says to stop. `None` where it cannot answer this
    /// query, or failed: the search then walks what it has not
    /// considered.
    fn query(&mut self, q: &Query, each: &mut dyn FnMut(Vec<PathBuf>) -> bool) -> Option<Answer>;

    /// What changed below the root since `since`, its own mark, and how far
    /// that is every change; `None` for `since`: from now, no change, the
    /// mark to go on from (taken before a walk, nothing is lost between).
    /// `None` (the default) where it keeps no journal, or failed.
    fn changes(&mut self, _since: Option<&Mark>) -> Option<Since> {
        None
    }

    /// What `folder` (a real path below the root) holds, each entry with
    /// its totals, a batch at a time until `each` says to stop: all of
    /// them, or only `largest`'s, the rest summed. `None` (the default)
    /// where it sums none, or failed.
    fn totals(
        &mut self,
        _folder: &Path,
        _largest: Option<Largest>,
        _each: &mut dyn FnMut(Vec<Child>) -> bool,
    ) -> Option<Summed> {
        None
    }
}

/// An index a search was opened on, as it covers the root.
pub struct Opened {
    pub coverage: Coverage,
    pub session: Box<dyn Session>,
}

/// Why an index answers no search here ([`NameIndex::absent`]): its
/// program is not installed. Said where a search walked for want of it
/// (`location::ListingEvent::WalkedWithout`: its footer's tooltip) and on
/// the settings' Index page, beside a button opening the page it is got
/// from: the index's words, the shell naming no index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Absent {
    /// Why, in whole sentences: what is missing, what searches do without
    /// it, what it would do, and where it is got.
    pub why: Words,
    /// The words of the Index page's button ("Get skwird").
    pub label: Words,
    /// The web page the button opens, where it is got
    /// (`https://skwir.org/download#skwird`): opened by the desktop as the
    /// user asks, never fetched.
    pub url: String,
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

    /// [`covers`](Self::covers) for a query some of whose name terms are
    /// not matched alike as written ([`Query::as_written`]): how far it
    /// would answer `q` now only where the engine, asked now, says it
    /// matches both forms ([`Session::both_forms`]: skwird's hello,
    /// `names-both-forms`). `None` (the default) where it cannot tell: the
    /// address field then takes no index as answering at once.
    fn covers_both_forms(&self, _q: &Query, _cancel: &CancelToken) -> Option<Coverage> {
        None
    }

    /// A session on `root` from what already runs, nothing started: for
    /// who follows a root's journal (a tag place counted, a harvest), which
    /// is no search, where [`open`](Self::open) may start an engine.
    /// `None` (the default) where it has none.
    fn follow(&self, _root: &Path, _cancel: &CancelToken) -> Option<Opened> {
        None
    }

    /// Let go of everything its engine holds at or below `point`, a volume
    /// about to be unmounted (ADR-149, ADR-150 18): returned once nothing
    /// of it is held, its engine reading nothing there until the volume is
    /// mounted again or a while has passed, should the unmount be refused.
    /// Asked of what already runs, never starting anything (no engine, no
    /// hold); blocking, on a worker, given up on with `cancel` and within a
    /// few seconds, all of it (skwird's: 8 s, its connect and hello
    /// included), since an Eject waits for it before its last let-go of the
    /// volume. The default holds nothing.
    fn release(&self, _point: &Path, _cancel: &CancelToken) {}

    /// Why it answers no search here, where that is what it last found
    /// (its program not installed), with what the Index page offers for
    /// it ([`Absent`]): only where it would have answered a search below
    /// `root` (a real path; a local folder, not a server's), or anywhere
    /// for `None` (the Index page). Asked by a search that walked whole,
    /// and as the window draws: from memory, never looked up where it is
    /// asked; the index looks again off the window's thread and wakes it
    /// when the answer changes. `None` (the default) where it may answer.
    fn absent(&self, _root: Option<&Path>) -> Option<Absent> {
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
    /// them; none that cannot tell or does not cover it. A query with a
    /// name term not matched alike as written ([`Query::as_written`]) is
    /// asked of each only as one of both forms
    /// ([`NameIndex::covers_both_forms`]).
    pub fn covers(&self, q: &Query, cancel: &CancelToken) -> Vec<(Arc<dyn NameIndex>, Coverage)> {
        let as_written = q.as_written();
        let mut covered: Vec<(Arc<dyn NameIndex>, Coverage)> = Vec::new();
        for index in self.list.all() {
            if cancel.is_cancelled() {
                return Vec::new();
            }
            let coverage = if as_written {
                index.covers(q, cancel)
            } else {
                index.covers_both_forms(q, cancel)
            };
            if let Some(coverage) = coverage
                && coverage.names.answers()
            {
                covered.push((index, coverage));
            }
        }
        covered.sort_by_key(|(_, c)| std::cmp::Reverse(c.names.strength()));
        covered
    }

    /// Why each index that answers no search here does not, of those
    /// that would have answered one below `root` (`None`: anywhere)
    /// ([`NameIndex::absent`]), in registration order: from memory.
    pub fn absent(&self, root: Option<&Path>) -> Vec<Absent> {
        self.list
            .all()
            .iter()
            .filter_map(|i| i.absent(root))
            .collect()
    }

    /// Every index let go of `point` before its volume is unmounted
    /// ([`NameIndex::release`]): returned once none holds anything there.
    /// Blocking, on a worker.
    pub fn release(&self, point: &Path, cancel: &CancelToken) {
        for index in self.list.all() {
            if cancel.is_cancelled() {
                return;
            }
            index.release(point, cancel);
        }
    }

    /// A session to follow `root`'s journal on ([`NameIndex::follow`]):
    /// that of the index `by` names where it answers (a mark kept is its
    /// own), else the strongest that does.
    pub fn follow(
        &self,
        root: &Path,
        by: Option<&str>,
        cancel: &CancelToken,
    ) -> Option<(Arc<dyn NameIndex>, Opened)> {
        let all = self.list.all();
        // Asked once: the one the mark names is not asked again below.
        let mut asked: Option<&'static str> = None;
        if let Some(by) = by
            && let Some(index) = all.iter().find(|i| i.id() == by)
        {
            asked = Some(index.id());
            if let Some(opened) = index.follow(root, cancel)
                && opened.coverage.names.answers()
            {
                return Some((Arc::clone(index), opened));
            }
        }
        // The strongest, the first registered among equals.
        let mut best: Option<(Arc<dyn NameIndex>, Opened)> = None;
        for index in all {
            if cancel.is_cancelled() {
                return None;
            }
            if asked == Some(index.id()) {
                continue;
            }
            if let Some(opened) = index.follow(root, cancel)
                && opened.coverage.names.answers()
                && best.as_ref().is_none_or(|(_, b)| {
                    opened.coverage.names.strength() > b.coverage.names.strength()
                })
            {
                best = Some((index, opened));
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coverage::{Proof, Why};
    use std::time::{Duration, SystemTime};

    /// An index that notes the volumes it was asked to let go of.
    struct Releasing(
        &'static str,
        Arc<parking_lot::Mutex<Vec<(&'static str, PathBuf)>>>,
    );

    impl NameIndex for Releasing {
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
            None
        }
        fn release(&self, point: &Path, _: &CancelToken) {
            self.1.lock().push((self.0, point.to_path_buf()));
        }
    }

    /// An index whose program is missing, saying so of what is below
    /// `/local`.
    struct Missing(&'static str);

    impl NameIndex for Missing {
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
            None
        }
        fn absent(&self, root: Option<&Path>) -> Option<Absent> {
            root.is_none_or(|root| root.starts_with("/local"))
                .then(|| Absent {
                    why: Words::as_written(format!("{} is not installed", self.0)),
                    label: Words::as_written(format!("Get {}", self.0)),
                    url: "https://example.org/download".to_owned(),
                })
        }
    }

    /// An index says why it answers nowhere only where it says so: by
    /// default none does. What those that do say is listed in their
    /// order, of those that would have answered below the root asked (all
    /// for none), and withdrawn with their owner.
    #[test]
    fn only_an_index_that_says_it_is_absent_is() {
        let indexes = NameIndexes::default();
        assert!(indexes.absent(None).is_empty(), "no index");
        indexes.register("a", Arc::new(Fixed("answers", Coverage::as_of(None))));
        assert!(
            indexes.absent(None).is_empty(),
            "an index of none's default is not absent"
        );
        indexes.register("b", Arc::new(Missing("first")));
        indexes.register("c", Arc::new(Missing("second")));
        let said = |indexes: &NameIndexes, root: Option<&Path>| -> Vec<String> {
            indexes
                .absent(root)
                .iter()
                .map(|absent| absent.why.english())
                .collect()
        };
        assert_eq!(
            said(&indexes, None),
            ["first is not installed", "second is not installed"]
        );
        assert_eq!(
            said(&indexes, Some(Path::new("/local/docs"))),
            ["first is not installed", "second is not installed"]
        );
        assert!(
            said(&indexes, Some(Path::new("/server/docs"))).is_empty(),
            "below a root they would not have answered"
        );
        let absent = indexes.absent(None).remove(0);
        assert_eq!(absent.label.english(), "Get first");
        assert_eq!(absent.url, "https://example.org/download");
        indexes.unregister_owner("b");
        assert_eq!(said(&indexes, None), ["second is not installed"]);
    }

    /// Before a volume is unmounted, every index is asked to let go of it,
    /// in their order, an index of none its default holding nothing; none
    /// once the unmount is given up on.
    #[test]
    fn every_index_lets_go_of_a_volume_before_its_unmount() {
        let asked = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let indexes = NameIndexes::default();
        indexes.register("a", Arc::new(Releasing("first", Arc::clone(&asked))));
        indexes.register("b", Arc::new(Fixed("holds nothing", Coverage::as_of(None))));
        indexes.register("c", Arc::new(Releasing("second", Arc::clone(&asked))));
        let point = PathBuf::from("/media/someone/it's USB");
        indexes.release(&point, &CancelToken::new());
        assert_eq!(
            *asked.lock(),
            [("first", point.clone()), ("second", point.clone())]
        );
        let cancelled = CancelToken::new();
        cancelled.cancel();
        indexes.release(&point, &cancelled);
        assert_eq!(asked.lock().len(), 2, "given up on: none asked");
    }

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

    /// An index that counts how often it is asked to follow, and answers
    /// with its coverage.
    struct Counted(&'static str, Coverage, std::sync::atomic::AtomicUsize);

    impl NameIndex for Counted {
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
            None
        }
        fn follow(&self, _: &Path, _: &CancelToken) -> Option<Opened> {
            self.2.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some(Opened {
                coverage: self.1.clone(),
                session: Box::new(Nothing),
            })
        }
    }

    /// The index a mark names is followed where it answers; where it does
    /// not, the strongest other is, each asked once.
    #[test]
    fn the_index_a_mark_names_is_asked_once() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_007);
        let counted = |id, coverage| Arc::new(Counted(id, coverage, 0.into()));
        let asked = |index: &Counted| index.2.load(std::sync::atomic::Ordering::Relaxed);
        let indexes = NameIndexes::default();
        let kept = counted("kept", Coverage::not_covered(Why::Denied));
        let other = counted("other", Coverage::as_of(Some(t)));
        indexes.register("a", Arc::clone(&kept) as Arc<dyn NameIndex>);
        indexes.register("b", Arc::clone(&other) as Arc<dyn NameIndex>);
        let followed = indexes.follow(Path::new("/"), Some("kept"), &CancelToken::new());
        assert_eq!(followed.map(|(index, _)| index.id()), Some("other"));
        assert_eq!((asked(&kept), asked(&other)), (1, 1));
        let followed = indexes.follow(Path::new("/"), Some("other"), &CancelToken::new());
        assert_eq!(followed.map(|(index, _)| index.id()), Some("other"));
        assert_eq!((asked(&kept), asked(&other)), (1, 2), "the mark's answers");
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
            size: None,
            modified: None,
            whole: true,
        };
        let covered: Vec<_> = indexes
            .covers(&q, &CancelToken::new())
            .into_iter()
            .map(|(index, _)| index.id())
            .collect();
        assert_eq!(covered, asked, "told as they are opened");
        // Sizes and times only of one that holds them live.
        let dated = Query {
            modified: Some(Times {
                from: Some(t),
                to: None,
            }),
            ..q.clone()
        };
        let live = Coverage {
            names: Proof::Live { since: t },
            attrs: Proof::Live { since: t },
            holes: Vec::new(),
        };
        let checked = Coverage {
            attrs: Proof::Checked { at: t },
            ..live.clone()
        };
        assert_eq!(dated.as_covered(&live), Some(dated.clone()));
        assert_eq!(dated.as_covered(&checked), None, "nothing else asked");
        let named = Query {
            names: vec![NameTerm {
                text: "report".into(),
                case_sensitive: false,
            }],
            ..dated
        };
        let by_name = named.as_covered(&checked).expect("by its name");
        assert_eq!((by_name.modified, by_name.whole), (None, false));
        indexes.unregister_owner("d");
        assert_eq!(indexes.ids(), ["as of", "denied", "checked", "checked too"]);
    }

    /// A session whose engine says whether it matches both forms.
    struct Said(bool);

    impl Session for Said {
        fn both_forms(&self) -> bool {
            self.0
        }
        fn sync(&mut self) -> Option<Coverage> {
            None
        }
        fn query(&mut self, _: &Query, _: &mut dyn FnMut(Vec<PathBuf>) -> bool) -> Option<Answer> {
            None
        }
    }

    /// An index whose engine says, on each connection, whether it matches a
    /// term in both its forms.
    struct Matching(&'static str, bool);

    impl NameIndex for Matching {
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
                coverage: Coverage::as_of(None),
                session: Box::new(Said(self.1)),
            })
        }
        fn covers(&self, _: &Query, _: &CancelToken) -> Option<Coverage> {
            Some(Coverage::as_of(None))
        }
        /// Asked now, its engine says whether it matches both forms.
        fn covers_both_forms(&self, q: &Query, cancel: &CancelToken) -> Option<Coverage> {
            self.covers(q, cancel).filter(|_| self.1)
        }
    }

    /// An index that keeps every default but `covers`: protocol 0's.
    struct Silent;

    impl NameIndex for Silent {
        fn id(&self) -> &'static str {
            "silent"
        }
        fn by(&self) -> Words {
            Words::as_written("silent")
        }
        fn syncs(&self) -> bool {
            true
        }
        fn open(&self, _: &Path, _: &CancelToken) -> Option<Opened> {
            Some(Opened {
                coverage: Coverage::as_of(None),
                session: Box::new(Nothing),
            })
        }
        fn covers(&self, _: &Query, _: &CancelToken) -> Option<Coverage> {
            Some(Coverage::as_of(None))
        }
    }

    /// Coverage is never overstated: what a connection that did not say it
    /// matches both forms names is not taken for a search with a term whose
    /// forms differ (`caf\u{e9}`, composed or decomposed), nor a glob it
    /// would match otherwise (Cyrillic letters, case ignored), and the
    /// probe says none covers such a search but one whose engine said so;
    /// for an ASCII term and one of one form, every index is taken.
    #[test]
    fn only_an_engine_that_says_both_forms_covers_a_term_whose_forms_differ() {
        let indexes = NameIndexes::default();
        indexes.register("a", Arc::new(Matching("one form", false)));
        indexes.register("b", Arc::new(Matching("both forms", true)));
        indexes.register("c", Arc::new(Silent));
        let query = |text: &str, case_sensitive: bool| Query {
            root: PathBuf::from("/"),
            names: vec![
                NameTerm {
                    text: "report".into(),
                    case_sensitive: false,
                },
                NameTerm {
                    text: text.into(),
                    case_sensitive,
                },
            ],
            kinds: None,
            size: None,
            modified: None,
            whole: true,
        };
        let never = CancelToken::new();
        let taken = |q: &Query| -> (Vec<&str>, Vec<&str>) {
            (
                indexes
                    .covers(q, &never)
                    .into_iter()
                    .map(|(index, _)| index.id())
                    .collect(),
                indexes
                    .open(&q.root, &never)
                    .into_iter()
                    .filter(|asked| q.answered_by(&*asked.session))
                    .map(|asked| asked.index.id())
                    .collect(),
            )
        };
        for (text, case_sensitive) in [
            ("caf\u{e9}", false),
            ("cafe\u{301}", false),
            ("CAF\u{c9}", true),
            ("caf\u{e9}*", false),
            ("\u{434}\u{43e}\u{43c}*", false),
        ] {
            let q = query(text, case_sensitive);
            assert!(!q.as_written(), "{text:?}");
            assert_eq!(
                taken(&q),
                (vec!["both forms"], vec!["both forms"]),
                "{text:?}"
            );
        }
        for (text, case_sensitive) in [
            ("cafe", false),
            ("Caf*.TXT", false),
            ("\u{434}\u{43e}\u{43c}", false),
            ("\u{434}\u{43e}\u{43c}*", true),
            ("\u{65e5}\u{672c}", false),
        ] {
            let q = query(text, case_sensitive);
            assert!(q.as_written(), "{text:?}");
            let all = vec!["one form", "both forms", "silent"];
            assert_eq!(taken(&q), (all.clone(), all), "{text:?}");
        }
    }
}
