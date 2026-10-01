//! The Tags view, a lens (ADR-007): the tags of the files under the folder
//! a pane shows, with how many files have each, for the shell's view to
//! list and act on (pin, rename, merge, color, remove).
//!
//! The counts are the index's (`hints::TagIndex::tally`): what it knows at
//! once, then what a count teaches it (`count.rs`). A folder counted before,
//! or one inside it, is covered: its counts are shown as of that count, even
//! after a restart, while a check reads what changed since. Without, the
//! count reads every file. Between, the index learns from every folder a
//! pane lists and from what bibli applies, so the view follows. Each pane
//! has its own, ended with the pane's location (`Pane::location_lifetime`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use bibli_core::app::App;
use bibli_core::cancel::CancelToken;
use bibli_core::location::Location;
use bibli_core::metadata::same_tag;
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;
use parking_lot::Mutex;

use crate::hints::TagIndex;
use crate::sidebar;
use crate::vocabulary::Vocabulary;

/// The view's id, as the shell resolves its renderer.
pub const VIEW: &str = "tags";

/// Where a lens's count stands.
#[derive(Clone, Debug, Default)]
struct Progress {
    /// Counting every file: the folder was not covered (or Count again).
    walking: bool,
    /// Reading what changed since the folder was covered.
    checking: bool,
    error: Option<String>,
}

/// One pane's lens.
struct Lens {
    root: PathBuf,
    progress: Progress,
    cancel: CancelToken,
    /// Which count this is: a count started for an earlier one stops.
    generation: u64,
    /// What it shows, made on a worker when something changed.
    status: Arc<Status>,
}

/// A tag as the view lists it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Row {
    pub tag: String,
    /// How many files under the root have it.
    pub files: usize,
    /// Its color, by name ([`bibli_core::metadata::TAG_COLORS`]).
    pub color: String,
    pub pinned: bool,
}

/// What a pane's lens shows now (`tags.lens_status`).
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Status {
    pub root: Option<PathBuf>,
    pub tags: Vec<Row>,
    /// How many files have a tag.
    pub files: usize,
    /// Counting every file: the counts are what is known so far.
    pub walking: bool,
    /// Checking what changed since `counted`.
    pub checking: bool,
    /// When the counts held whole, in seconds since the epoch: the count
    /// that covers the folder.
    pub counted: Option<u64>,
    /// More tagged files than the index holds: the counts leave some out.
    pub partial: bool,
    pub error: Option<String>,
}

/// Every pane's lens.
pub struct Lenses {
    /// The view's revision: raised, then the app woken, when one changes.
    pub revision: Arc<AtomicU64>,
    open: Mutex<HashMap<PaneId, Lens>>,
    index: Arc<TagIndex>,
    vocabulary: Arc<Vocabulary>,
    lifetime: CancelToken,
    generations: AtomicU64,
}

impl Lenses {
    pub fn new(
        index: Arc<TagIndex>,
        vocabulary: Arc<Vocabulary>,
        revision: Arc<AtomicU64>,
        lifetime: CancelToken,
    ) -> Arc<Self> {
        Arc::new(Self {
            revision,
            open: Mutex::new(HashMap::new()),
            index,
            vocabulary,
            lifetime,
            generations: AtomicU64::new(0),
        })
    }

    /// What a lens on `root` shows. Reads the index: on a worker.
    fn make_status(&self, app: &App, root: &Path, progress: &Progress) -> Status {
        let (tags, files) = self.index.tally(root);
        let (colors, pins) = (sidebar::Colors::of(app), sidebar::pinned(app));
        let counted = self
            .index
            .covering(root)
            .and_then(|c| c.since().duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        Status {
            root: Some(root.to_path_buf()),
            tags: tags
                .into_iter()
                .map(|(tag, files)| Row {
                    color: colors.of_tag(&self.vocabulary, &tag),
                    pinned: pins.iter().any(|p| same_tag(p, &tag)),
                    tag,
                    files,
                })
                .collect(),
            files,
            walking: progress.walking,
            checking: progress.checking,
            counted,
            partial: self.index.is_full(),
            error: progress.error.clone(),
        }
    }

    /// Make every open lens's status again and tell the view: the index,
    /// the pins or the colors changed. Reads the index: on a worker.
    pub fn touch(&self, app: &App) {
        let lenses: Vec<(PaneId, u64, PathBuf, Progress)> = self
            .open
            .lock()
            .iter()
            .map(|(pane, l)| (*pane, l.generation, l.root.clone(), l.progress.clone()))
            .collect();
        if lenses.is_empty() {
            return;
        }
        let made: Vec<(PaneId, u64, Status)> = lenses
            .into_iter()
            .map(|(pane, generation, root, progress)| {
                (pane, generation, self.make_status(app, &root, &progress))
            })
            .collect();
        {
            let mut open = self.open.lock();
            for (pane, generation, status) in made {
                if let Some(lens) = open.get_mut(&pane)
                    && lens.generation == generation
                {
                    lens.status = Arc::new(status);
                }
            }
        }
        self.revision.fetch_add(1, Ordering::Release);
        app.wake();
    }

    /// `tags.lens`: count the tags under the folder `pane` shows, unless
    /// they are counted already; `again` counts every file anew.
    pub fn open(self: &Arc<Self>, app: &App, pane: PaneId, again: bool) -> anyhow::Result<()> {
        let (root, lifetime) = {
            let ws = app.workspace.read();
            let p = ws
                .pane(pane)
                .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
            let root = p.location().and_then(Location::path);
            (root, p.location_lifetime())
        };
        let Some(root) = root else {
            self.release(app, pane);
            return Ok(());
        };
        // Ends with the pane's location or with the plugin.
        let cancel = lifetime.linked(&self.lifetime);
        let generation = self.generations.fetch_add(1, Ordering::Relaxed);
        {
            let mut open = self.open.lock();
            if let Some(lens) = open.get(&pane)
                && lens.root == root
                && !lens.cancel.is_cancelled()
                && !again
            {
                return Ok(());
            }
            if let Some(old) = open.remove(&pane) {
                old.cancel.cancel();
            }
            // Whether it walks or checks is known once a worker asked the
            // index.
            let progress = Progress {
                checking: true,
                ..Progress::default()
            };
            open.insert(
                pane,
                Lens {
                    root: root.clone(),
                    status: Arc::new(Status {
                        root: Some(root.clone()),
                        checking: true,
                        ..Status::default()
                    }),
                    progress,
                    cancel: cancel.clone(),
                    generation,
                },
            );
        }
        let (this, weak) = (Arc::clone(self), app.weak());
        app.scheduler
            .spawn(Priority::Normal, "tags.lens", cancel.clone(), move || {
                let Some(app) = weak.upgrade() else { return };
                this.count(&app, pane, &root, again, &cancel, generation);
            });
        Ok(())
    }

    /// Show what the index knows, then count or check.
    fn count(
        &self,
        app: &App,
        pane: PaneId,
        root: &Path,
        again: bool,
        cancel: &CancelToken,
        generation: u64,
    ) {
        let set = |progress: Progress| {
            let mut open = self.open.lock();
            match open.get_mut(&pane) {
                Some(lens) if !cancel.is_cancelled() && lens.generation == generation => {
                    lens.progress = progress;
                    true
                }
                _ => false,
            }
        };
        let covered = if again {
            None
        } else {
            self.index.covering(root).map(|c| c.since())
        };
        if !set(Progress {
            walking: covered.is_none(),
            checking: covered.is_some(),
            error: None,
        }) {
            return;
        }
        self.touch(app);
        let result = crate::count::run(app, &self.index, root, covered, cancel);
        if cancel.is_cancelled() {
            return;
        }
        let error = match result {
            Ok(Some((since, dirs, counted))) => {
                tracing::debug!(
                    checked = covered.is_some(),
                    folders = counted.folders,
                    read = counted.read,
                    "tags: counted"
                );
                self.index.cover(root, since, &dirs);
                None
            }
            Ok(None) => return,
            Err(error) => Some(format!("{error:#}")),
        };
        set(Progress {
            walking: false,
            checking: false,
            error,
        });
        self.touch(app);
    }

    /// Let go of `pane`'s lens (`tags.lens_release`).
    pub fn release(&self, app: &App, pane: PaneId) {
        if let Some(lens) = self.open.lock().remove(&pane) {
            lens.cancel.cancel();
            self.revision.fetch_add(1, Ordering::Release);
            app.wake();
        }
    }

    /// What `pane`'s lens shows: made on a worker, so asking costs a clone.
    pub fn status(&self, pane: PaneId) -> Arc<Status> {
        self.open
            .lock()
            .get(&pane)
            .map_or_else(Arc::default, |lens| Arc::clone(&lens.status))
    }
}
