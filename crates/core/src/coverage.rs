//! How far an index's answer can be trusted (ADR-150, 1; ADR-050, 3): live
//! since a time, checked at a time, as of a time, or not covered and why.
//! The four words are skwird's; a names index says them (`crate::names`),
//! a listing keeps what its answer rests on (`location::RestsOn`), and the
//! footer, the status line and `entries` say it.
//!
//! Coverage is never overstated (`docs/invariants/indexes.md`): what an
//! index cannot prove it says it does not cover, and the search walks it.

use std::path::PathBuf;
use std::time::SystemTime;

/// How far a kind of fact below a root can be trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proof {
    /// Every change applied, none dropped, since a full read at `since`,
    /// up to a barrier the search set (a sync).
    Live { since: SystemTime },
    /// Every folder compared with the index at `at`, or read after. Proves
    /// names and kinds, never sizes or times (ADR-026's amendment of
    /// 2026-10-10): a write in place changes no folder.
    Checked { at: SystemTime },
    /// A snapshot as of `at`, not proven since (an index read back,
    /// plocate's database); `None` when when cannot be told.
    AsOf { at: Option<SystemTime> },
    /// Nothing is known there that a search may trust.
    NotCovered(Why),
}

impl Proof {
    /// Whether what the index names is all there is, as of now: live or
    /// checked. A search under such a proof walks only the holes.
    pub fn proves(&self) -> bool {
        matches!(self, Proof::Live { .. } | Proof::Checked { .. })
    }

    /// Whether an answer may rest on it at all: proven, or as of a time.
    pub fn answers(&self) -> bool {
        !matches!(self, Proof::NotCovered(_))
    }

    /// Live, then checked, then as of, then nothing: of two indexes
    /// covering a root, the search asks the stronger first.
    pub fn strength(&self) -> u8 {
        match self {
            Proof::Live { .. } => 3,
            Proof::Checked { .. } => 2,
            Proof::AsOf { .. } => 1,
            Proof::NotCovered(_) => 0,
        }
    }

    /// The moment it speaks of: since when it is live, when it was
    /// checked, what it is as of.
    pub fn at(&self) -> Option<SystemTime> {
        match *self {
            Proof::Live { since } => Some(since),
            Proof::Checked { at } => Some(at),
            Proof::AsOf { at } => at,
            Proof::NotCovered(_) => None,
        }
    }
}

/// Why a subtree is not covered: skwird's reasons.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Why {
    /// Outside every root the index covers.
    Outside,
    /// A rule keeps it out (its own defaults, the user's, a client's).
    Excluded,
    /// It holds a `.private` file (ADR-044).
    Private,
    Denied,
    /// A read failed, with this error number.
    Failed(i32),
    /// A listing failed part-way, with this error number.
    CutShort(i32),
    Gone,
    /// Its path now goes through a link, or to another folder.
    Replaced,
    /// Another filesystem is mounted there.
    Mount,
    /// A network mount starts there: never crawled.
    NetworkMount,
    /// A btrfs snapshot below a root, a copy of what it was taken of: left
    /// out as too costly, not as private.
    Snapshot,
    /// Found, not read yet.
    Unread,
    /// The watches it needs passed the system's limit.
    WatchLimit,
}

impl Why {
    /// Whether a search passes it over below the folder searched rather
    /// than walking it (ADR-150, 5): a snapshot, a copy of what it was
    /// taken of, as ADR-125's `searches` role passes over a tool store.
    /// Walked when searched in, or with `excluded:yes`.
    pub fn passed_over(self) -> bool {
        matches!(self, Why::Snapshot)
    }
}

/// A subtree not covered, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hole {
    pub path: PathBuf,
    pub why: Why,
}

/// How far an answer below a root can be trusted, for each kind of fact,
/// and the subtrees below it that are not covered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coverage {
    /// Names and kinds.
    pub names: Proof,
    /// Sizes and times.
    pub attrs: Proof,
    pub holes: Vec<Hole>,
}

impl Coverage {
    /// A snapshot as of `at`, names and attributes alike, with no hole
    /// known: plocate's database.
    pub fn as_of(at: Option<SystemTime>) -> Coverage {
        Coverage {
            names: Proof::AsOf { at },
            attrs: Proof::AsOf { at },
            holes: Vec::new(),
        }
    }

    /// Nothing below the root covered, for this reason.
    pub fn not_covered(why: Why) -> Coverage {
        Coverage {
            names: Proof::NotCovered(why),
            attrs: Proof::NotCovered(why),
            holes: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn live_then_checked_then_as_of() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_007);
        let live = Proof::Live { since: t };
        let checked = Proof::Checked { at: t };
        let as_of = Proof::AsOf { at: Some(t) };
        let none = Proof::NotCovered(Why::Denied);
        assert!(live.strength() > checked.strength());
        assert!(checked.strength() > as_of.strength());
        assert!(as_of.strength() > none.strength());
        assert!(live.proves() && checked.proves());
        assert!(!as_of.proves() && !none.proves());
        assert!(as_of.answers() && !none.answers());
        assert_eq!(as_of.at(), Some(t));
        assert_eq!(Proof::AsOf { at: None }.at(), None);
    }

    #[test]
    fn only_a_snapshot_is_passed_over() {
        assert!(Why::Snapshot.passed_over());
        for why in [
            Why::Outside,
            Why::Excluded,
            Why::Private,
            Why::Denied,
            Why::Failed(5),
            Why::CutShort(5),
            Why::Gone,
            Why::Replaced,
            Why::Mount,
            Why::NetworkMount,
            Why::Unread,
            Why::WatchLimit,
        ] {
            assert!(!why.passed_over(), "{why:?} is walked");
        }
    }
}
