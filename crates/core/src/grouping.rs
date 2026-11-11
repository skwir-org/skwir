//! What a pane's entries are grouped by, and what a plugin knows of them
//! that a sort or a grouping needs and core cannot read (ADR-130).
//!
//! The sort and the group take their criteria from one list
//! ([`SortKey`]). Core reads a name, a size, the dates, a rating, an
//! extension and tags itself; a kind, a type's name, an owner's name and
//! the groups (their bounds, their words) are a plugin's, which answers
//! through [`Keys`] made for one sort ([`crate::registry::Sorter`], the
//! query plugin's). A listing sorts with [`Sort`]: the pane's spec and
//! the keys made for it.

use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::entry::{Entry, EntryId};
use crate::location::{SortKey, SortSpec};

/// What a pane's entries are grouped by (`SortSpec::group`).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum GroupBy {
    /// Not grouped.
    #[default]
    None,
    /// By the sort's criterion, in the sort's direction: headings over the
    /// order as it stands.
    SameAsSort,
    /// By a criterion of its own, in a direction of its own
    /// (`SortSpec::group_ascending`).
    #[serde(untagged)]
    Key(SortKey),
}

impl GroupBy {
    /// The criterion grouped by under a sort by `sort`.
    pub fn key(self, sort: SortKey) -> Option<SortKey> {
        match self {
            GroupBy::None => None,
            GroupBy::SameAsSort => Some(sort),
            GroupBy::Key(key) => Some(key),
        }
    }
}

/// A group, or a criterion's value in words (a kind, a type's name, an
/// owner's): where it stands among the others, and what it is called.
/// Made once per value by whoever makes keys, and shared by every entry
/// that has it, so an order compares pointers first.
#[derive(Debug, PartialEq, Eq)]
pub struct Group {
    /// Orders before `key`: a day's start, a size's bound, Other before
    /// 0–9 before the letters. What has none is the lowest (ADR-130, 6).
    pub rank: u64,
    /// What orders it among those of its rank:
    /// [`crate::collate::sort_key`] of its words, or empty when its rank
    /// alone does.
    pub key: Arc<[u8]>,
    /// What its heading says: `Today`, `Images`, `JPEG image`.
    pub label: Arc<str>,
    /// It holds only folders (Folders, by size or extension; the kind
    /// Folders): first while folders come first, whatever the direction.
    pub folders: bool,
    /// The query term that keeps its files (`modified:today`), for Show
    /// only these; none where no term says it.
    pub term: Option<Arc<str>>,
}

impl Group {
    /// A group ranked `rank` and called `label`, ordered among its rank
    /// by `label`'s collation.
    pub fn new(rank: u64, label: &str) -> Self {
        Group {
            rank,
            key: Arc::from(crate::collate::sort_key(std::ffi::OsStr::new(label))),
            label: Arc::from(label),
            folders: false,
            term: None,
        }
    }

    /// A group ordered by `rank` alone.
    pub fn ranked(rank: u64, label: &str) -> Self {
        Group {
            rank,
            key: Arc::from(&[][..]),
            label: Arc::from(label),
            folders: false,
            term: None,
        }
    }

    /// One that holds only folders.
    #[must_use]
    pub fn of_folders(mut self) -> Self {
        self.folders = true;
        self
    }

    /// The query term that keeps its files.
    #[must_use]
    pub fn with_term(mut self, term: impl Into<Arc<str>>) -> Self {
        self.term = Some(term.into());
        self
    }

    /// Where it stands against `other`, going up: by rank, then by key.
    pub fn standing(&self, other: &Group) -> Ordering {
        self.rank
            .cmp(&other.rank)
            .then_with(|| self.key.cmp(&other.key))
    }
}

/// What gives an entry a group, or a criterion's value in words: answered
/// from memory, never blocking (ADR-130, 8).
pub type Keyer = Arc<dyn Fn(&Entry) -> Arc<Group> + Send + Sync>;

/// What a plugin knows of entries for one sort: the value of a criterion
/// core cannot read (a kind, a type's name, an owner's), an entry's group,
/// and when they change (ADR-130, 8). Made by a [`crate::registry::Sorter`]
/// for a sort's criterion and its group's; made again when either
/// changes, when the plugin is turned on or off, and at `next_change`.
#[derive(Clone)]
pub struct Keys {
    /// Which these are: a new one each time keys are made, so that an
    /// order made with others is made again.
    id: u64,
    /// The criterion sorted by that `value` answers for.
    pub sort: SortKey,
    /// The criterion grouped by that `group` answers for.
    pub grouped: Option<SortKey>,
    /// The sort's value, where core cannot read it (`SortKey::keyed`):
    /// its rank, then its key.
    pub value: Option<Keyer>,
    /// An entry's group.
    pub group: Option<Keyer>,
    /// When what they answer next changes (midnight, for the dates): the
    /// keys are made again then.
    pub next_change: Option<SystemTime>,
    /// Moved when what they answered changed without their being made
    /// again (a type's name, read on a worker while its type stood for
    /// it): an order made before is made again.
    pub changed: Arc<AtomicU64>,
}

impl Keys {
    /// Keys for a sort by `sort`, grouped by `grouped`, answering nothing
    /// yet.
    pub fn new(sort: SortKey, grouped: Option<SortKey>) -> Self {
        static MADE: AtomicU64 = AtomicU64::new(1);
        Keys {
            id: MADE.fetch_add(1, AtomicOrdering::Relaxed),
            sort,
            grouped,
            value: None,
            group: None,
            next_change: None,
            changed: Arc::default(),
        }
    }

    /// Which keys these are, and how often what they answered changed:
    /// what an order made with them is kept against.
    pub fn stamp(&self) -> (u64, u64) {
        (self.id, self.changed.load(AtomicOrdering::Acquire))
    }
}

impl fmt::Debug for Keys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keys")
            .field("id", &self.id)
            .field("sort", &self.sort)
            .field("grouped", &self.grouped)
            .field("value", &self.value.is_some())
            .field("group", &self.group.is_some())
            .field("next_change", &self.next_change)
            .finish_non_exhaustive()
    }
}

/// A sort as a listing applies it: the pane's spec, and the keys made for
/// it. A spec alone sorts as core reads its entries: no groups, and a
/// criterion only a plugin reads leaves the order to the names.
#[derive(Clone, Debug, Default)]
pub struct Sort {
    pub spec: SortSpec,
    pub keys: Option<Keys>,
}

impl From<SortSpec> for Sort {
    fn from(spec: SortSpec) -> Self {
        Sort { spec, keys: None }
    }
}

impl From<&Sort> for Sort {
    fn from(sort: &Sort) -> Self {
        sort.clone()
    }
}

impl Sort {
    /// What an order made with it is kept against: the spec, and the keys'
    /// stamp.
    pub fn stamp(&self) -> SortStamp {
        SortStamp {
            spec: self.spec,
            keys: self.keys.as_ref().map(Keys::stamp),
        }
    }

    /// What gives the sort's value, where the keys answer for its
    /// criterion.
    pub fn value(&self) -> Option<&Keyer> {
        let keys = self.keys.as_ref()?;
        (keys.sort == self.spec.key)
            .then_some(keys.value.as_ref())
            .flatten()
    }

    /// The criterion grouped by and what gives each entry its group, where
    /// the keys answer for it: none, the entries are not grouped.
    pub fn grouping(&self) -> Option<(SortKey, &Keyer)> {
        let key = self.spec.group_key()?;
        let keys = self.keys.as_ref()?;
        (keys.grouped == Some(key))
            .then_some(keys.group.as_ref())
            .flatten()
            .map(|keyer| (key, keyer))
    }
}

/// What an order is kept against (`Sort::stamp`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SortStamp {
    pub spec: SortSpec,
    pub keys: Option<(u64, u64)>,
}

/// Where a group starts in an order, how many entries it holds, and the
/// group; none for the entries whose group is not read yet, after the
/// rest.
#[derive(Clone, Debug)]
pub struct Span {
    pub start: usize,
    pub len: usize,
    pub group: Option<Arc<Group>>,
}

/// The spans of `order`, whose entries' groups `group_of` says, in
/// order: a new one wherever the group changes.
pub fn spans_of(
    order: &[EntryId],
    mut group_of: impl FnMut(usize, EntryId) -> Option<Option<Arc<Group>>>,
) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for (at, id) in order.iter().enumerate() {
        // An entry the order holds that has no group of its own (one
        // opened in place) stays in the group of the entry before it.
        let Some(group) = group_of(at, *id) else {
            if let Some(last) = spans.last_mut() {
                last.len += 1;
            } else {
                spans.push(Span {
                    start: at,
                    len: 1,
                    group: None,
                });
            }
            continue;
        };
        match spans.last_mut() {
            Some(last) if same(last.group.as_ref(), group.as_ref()) => last.len += 1,
            _ => spans.push(Span {
                start: at,
                len: 1,
                group,
            }),
        }
    }
    spans
}

/// Whether two groups are one: the same, or ordered alike.
pub fn same(a: Option<&Arc<Group>>, b: Option<&Arc<Group>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a.standing(b) == Ordering::Equal,
        (None, None) => true,
        _ => false,
    }
}
