//! Locations and streaming listings.
//!
//! A location is a string: a path, or `scheme:rest` for anything a plugin
//! provides (queries, trash, recent, ...). Opening one yields an
//! [`OpenLocation`] that streams [`ListingEvent`]s over a channel. The UI
//! applies them to a [`Listing`] (or its own model) and never waits.

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::cancel::CancelToken;
use crate::entry::{Entry, EntryId};
use crate::scheduler::{Priority, Scheduler};

/// The scheme used for plain filesystem paths.
pub const PATH_SCHEME: &str = "path";

/// A parsed location string.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Location {
    /// `path` for filesystem paths, otherwise a plugin's scheme.
    pub scheme: Arc<str>,
    /// Everything after the scheme; the path itself for `path`.
    pub rest: String,
}

impl Location {
    /// Parse `s`. Absolute or relative paths, `~`, and `scheme:rest` are all accepted.
    /// A single letter before the colon is treated as a Windows drive, not a scheme.
    pub fn parse(s: &str) -> Self {
        if let Some((scheme, rest)) = s.split_once(':')
            && scheme.len() > 1
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Location {
                scheme: scheme.into(),
                rest: rest.to_owned(),
            };
        }
        Location {
            scheme: PATH_SCHEME.into(),
            rest: s.to_owned(),
        }
    }

    pub fn from_path(p: impl AsRef<Path>) -> Self {
        Location {
            scheme: PATH_SCHEME.into(),
            rest: p.as_ref().to_string_lossy().into_owned(),
        }
    }

    pub fn is_path(&self) -> bool {
        &*self.scheme == PATH_SCHEME
    }

    /// The filesystem path, with `~` expanded, for `path` locations.
    pub fn path(&self) -> Option<PathBuf> {
        if !self.is_path() {
            return None;
        }
        let r = &self.rest;
        if r == "~" || r.starts_with("~/") {
            let home = dirs::home_dir()?;
            return Some(if r == "~" { home } else { home.join(&r[2..]) });
        }
        Some(PathBuf::from(r))
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_path() {
            f.write_str(&self.rest)
        } else {
            write!(f, "{}:{}", self.scheme, self.rest)
        }
    }
}

/// What an open location streams. Every variant is a batch.
#[derive(Clone, Debug)]
pub enum ListingEvent {
    /// New entries (or, for an id already present, replacements).
    Added(Vec<Entry>),
    /// Entries whose name, kind, or metadata changed.
    Updated(Vec<Entry>),
    /// Entries that no longer exist here.
    Removed(Vec<EntryId>),
    /// The initial listing is complete; further events come from watching.
    Complete,
    /// The listing failed or was interrupted.
    Error(String),
}

/// Called after events are queued so a sleeping UI can wake and pump.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// What a provider gets when a location is opened.
#[derive(Clone)]
pub struct OpenContext {
    pub scheduler: Scheduler,
    pub cancel: CancelToken,
    /// Preferred batch size for `Added` events.
    pub batch_size: usize,
    /// Wake hook; providers should send through [`OpenContext::channel`] which calls it.
    pub waker: Option<Waker>,
}

impl OpenContext {
    /// An event channel whose sender wakes the UI after each send.
    pub fn channel(&self) -> (EventSink, Receiver<ListingEvent>) {
        let (tx, rx) = crossbeam_channel::unbounded();
        (
            EventSink {
                tx,
                waker: self.waker.clone(),
            },
            rx,
        )
    }
}

/// Sender half of a listing channel. Never blocks; wakes the UI.
#[derive(Clone)]
pub struct EventSink {
    tx: Sender<ListingEvent>,
    waker: Option<Waker>,
}

impl EventSink {
    /// Queue an event. Returns false if the receiver is gone.
    pub fn send(&self, event: ListingEvent) -> bool {
        let ok = self.tx.send(event).is_ok();
        if let Some(w) = &self.waker {
            w();
        }
        ok
    }
}

/// An opened location. Dropping it (or `close`) stops all work for it.
pub trait OpenLocation: Send + Sync {
    fn location(&self) -> &Location;

    /// The event stream. Never blocks the producer: the channel is unbounded.
    fn events(&self) -> &Receiver<ListingEvent>;

    /// Ask for metadata on these entries (lazy stat). Results arrive as `Updated`.
    fn hydrate(&self, ids: &[EntryId], priority: Priority);

    /// Where this entry lives on disk, if anywhere. Operations need this.
    fn path_of(&self, id: EntryId) -> Option<PathBuf>;

    /// Re-list from scratch (emits `Removed` for gone entries, `Added` for new).
    fn refresh(&self);

    /// Stop listing, stat, and watching.
    fn close(&self);
}

/// Something that can open locations of one scheme.
pub trait LocationProvider: Send + Sync {
    fn scheme(&self) -> &str;
    fn open(&self, location: &Location, ctx: OpenContext) -> anyhow::Result<Arc<dyn OpenLocation>>;
}

/// An in-memory view of a listing, kept up to date by applying events.
/// Insertion order is preserved so streaming order is stable; views sort on top.
#[derive(Debug, Default)]
pub struct Listing {
    entries: IndexMap<EntryId, Entry>,
    complete: bool,
    error: Option<String>,
}

impl Listing {
    pub fn apply(&mut self, event: ListingEvent) {
        match event {
            ListingEvent::Added(batch) | ListingEvent::Updated(batch) => {
                for e in batch {
                    self.entries.insert(e.id, e);
                }
            }
            ListingEvent::Removed(ids) => {
                for id in ids {
                    self.entries.shift_remove(&id);
                }
            }
            ListingEvent::Complete => self.complete = true,
            ListingEvent::Error(e) => self.error = Some(e),
        }
    }

    /// Drain everything currently available without blocking.
    pub fn drain(&mut self, rx: &Receiver<ListingEvent>) -> usize {
        let mut n = 0;
        while let Ok(ev) = rx.try_recv() {
            self.apply(ev);
            n += 1;
        }
        n
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn get(&self, id: EntryId) -> Option<&Entry> {
        self.entries.get(&id)
    }

    pub fn get_index(&self, i: usize) -> Option<&Entry> {
        self.entries.get_index(i).map(|(_, e)| e)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values()
    }

    pub fn ids(&self) -> impl Iterator<Item = EntryId> + '_ {
        self.entries.keys().copied()
    }

    pub fn find_by_name(&self, name: &OsStr) -> Option<&Entry> {
        self.entries.values().find(|e| e.name == name)
    }
}
