//! The Tags view, a lens (ADR-007): the tags of the files under the folder
//! a pane shows, with how many files have each, for the shell's view to
//! list and act on (pin, rename, merge, color, remove).
//!
//! What the index knows comes first, at once; then a search for any tag
//! (`tag:*`, the query plugin's walk: ADR-026, an index hints and the walk
//! answers) counts every file, and its counts replace the index's when it
//! ends. Afterwards what bibli applies is counted as it is applied
//! (`Lenses::observe`), never by walking again. Each pane has its own,
//! ended with the pane's location (`Pane::location_lifetime`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bibli_core::app::App;
use bibli_core::cancel::CancelToken;
use bibli_core::command::{Invocation, Outcome};
use bibli_core::entry::{Metadata, Value, keys};
use bibli_core::location::{ListingEvent, Location};
use bibli_core::metadata::same_tag;
use bibli_core::os_path::PathArg;
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;
use parking_lot::Mutex;

use crate::hints::TagIndex;

/// The view's id, as the shell resolves its renderer.
pub const VIEW: &str = "tags";

/// How often a walk's counts are published while it runs.
const PUBLISH_EVERY: Duration = Duration::from_millis(250);

/// One pane's lens.
struct Lens {
    root: PathBuf,
    /// The tagged files the walk found, with their tags.
    files: Files,
    /// What the index knew, until the walk ends.
    hinted: Vec<(String, usize)>,
    walking: bool,
    error: Option<String>,
    cancel: CancelToken,
    /// Which count this is: a walk started for an earlier one stops.
    generation: u64,
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
    /// Counting still: the counts are the index's, or the walk's so far.
    pub walking: bool,
    pub error: Option<String>,
}

/// The tags each spelled as most of its files spell it, with how many
/// files have it: the index's counts while `hinted` is given, raised to
/// what the walk found so far.
fn count(
    files: &HashMap<PathBuf, Vec<String>>,
    hinted: Option<&[(String, usize)]>,
) -> Vec<(String, usize)> {
    let mut counts: HashMap<String, (usize, HashMap<&str, usize>)> = HashMap::new();
    for tags in files.values() {
        for tag in tags {
            let (n, spellings) = counts.entry(tag.to_lowercase()).or_default();
            *n += 1;
            *spellings.entry(tag.as_str()).or_default() += 1;
        }
    }
    let mut out: HashMap<String, (String, usize)> = counts
        .into_iter()
        .filter_map(|(lower, (n, spellings))| {
            let spelled = spellings
                .into_iter()
                .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))?
                .0
                .to_owned();
            Some((lower, (spelled, n)))
        })
        .collect();
    for (tag, n) in hinted.unwrap_or_default() {
        let known = out
            .entry(tag.to_lowercase())
            .or_insert_with(|| (tag.clone(), 0));
        known.1 = known.1.max(*n);
    }
    let mut tags: Vec<(String, usize)> = out.into_values().collect();
    tags.sort_by_key(|a| a.0.to_lowercase());
    tags
}

/// How many completed counts are kept, the most recent first.
const KEEP_COUNTED: usize = 4;

/// The tagged files under a root and their tags.
type Files = HashMap<PathBuf, Vec<String>>;

/// `path` has `tags` now: whether that changed anything.
fn set(files: &mut Files, path: &Path, tags: &[String]) -> bool {
    match files.get(path) {
        Some(had) if had == tags => false,
        None if tags.is_empty() => false,
        _ if tags.is_empty() => files.remove(path).is_some(),
        _ => {
            files.insert(path.to_path_buf(), tags.to_vec());
            true
        }
    }
}

/// What a change does to the files' places: `from` (and what is under it)
/// is now at `to`, or gone when `None`.
fn moved(files: &mut Files, from: &Path, to: Option<&Path>) -> bool {
    let under: Vec<PathBuf> = files
        .keys()
        .filter(|p| p.starts_with(from))
        .cloned()
        .collect();
    for path in &under {
        let tags = files.remove(path).unwrap_or_default();
        if let Some(to) = to
            && let Ok(rest) = path.strip_prefix(from)
        {
            files.insert(
                if rest.as_os_str().is_empty() {
                    to.to_path_buf()
                } else {
                    to.join(rest)
                },
                tags,
            );
        }
    }
    !under.is_empty()
}

/// What happened to files, as an applied plan says.
#[derive(Clone, Debug)]
pub enum Seen {
    /// The file has these tags now.
    Tags(PathBuf, Vec<String>),
    /// The file, or a folder and what is in it, went from the first place
    /// to the second, or away.
    Moved(PathBuf, Option<PathBuf>),
}

/// A file's tags as its metadata holds them.
fn tags_of(metadata: &Metadata) -> Vec<String> {
    match metadata.value(keys::TAGS) {
        Some(Value::List(tags)) => tags
            .iter()
            .filter_map(|t| t.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// Every pane's lens.
pub struct Lenses {
    /// The view's revision: raised, then the app woken, when one changes.
    pub revision: Arc<AtomicU64>,
    open: Mutex<HashMap<PaneId, Lens>>,
    /// Completed counts, by root, the most recent first: a folder counted
    /// is not walked again for a lens on it or under it, and what bibli
    /// applies keeps them current. "Count again" walks anew.
    counted: Mutex<Vec<(PathBuf, Files)>>,
    index: Arc<TagIndex>,
    lifetime: CancelToken,
    generations: AtomicU64,
}

impl Lenses {
    pub fn new(index: Arc<TagIndex>, revision: Arc<AtomicU64>, lifetime: CancelToken) -> Arc<Self> {
        Arc::new(Self {
            revision,
            open: Mutex::new(HashMap::new()),
            counted: Mutex::new(Vec::new()),
            index,
            lifetime,
            generations: AtomicU64::new(0),
        })
    }

    fn changed(&self, app: &App) {
        self.revision.fetch_add(1, Ordering::Release);
        app.wake();
    }

    /// The pins or the colors changed: the views read them again, if any
    /// is open.
    pub fn touch(&self, app: &App) {
        if !self.open.lock().is_empty() {
            self.changed(app);
        }
    }

    /// `tags.lens`: count the tags under the folder `pane` shows, unless
    /// they are counted already (`again` counts them anew).
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
            // Counted already, there or above: no walk.
            let known = if again {
                self.counted.lock().retain(|(r, _)| !root.starts_with(r));
                None
            } else {
                self.counted
                    .lock()
                    .iter()
                    .find(|(r, _)| root.starts_with(r))
                    .map(|(_, files)| {
                        files
                            .iter()
                            .filter(|(p, _)| p.starts_with(&root))
                            .map(|(p, t)| (p.clone(), t.clone()))
                            .collect::<Files>()
                    })
            };
            let walking = known.is_none();
            open.insert(
                pane,
                Lens {
                    root: root.clone(),
                    files: known.unwrap_or_default(),
                    hinted: Vec::new(),
                    walking,
                    error: None,
                    cancel: cancel.clone(),
                    generation,
                },
            );
            if !walking {
                drop(open);
                self.changed(app);
                return Ok(());
            }
        }
        let (this, weak) = (Arc::clone(self), app.weak());
        app.scheduler
            .spawn(Priority::Normal, "tags.lens", cancel.clone(), move || {
                let Some(app) = weak.upgrade() else { return };
                this.walk(&app, pane, &root, &cancel, generation);
            });
        Ok(())
    }

    /// Count: the index's first, then the walk's.
    fn walk(&self, app: &App, pane: PaneId, root: &Path, cancel: &CancelToken, generation: u64) {
        let hinted = self.index.top(usize::MAX, root);
        let live = |f: &mut dyn FnMut(&mut Lens)| {
            let mut open = self.open.lock();
            match open.get_mut(&pane) {
                Some(lens) if !cancel.is_cancelled() && lens.generation == generation => {
                    f(lens);
                    true
                }
                _ => false,
            }
        };
        if !live(&mut |lens| lens.hinted.clone_from(&hinted)) {
            return;
        }
        self.changed(app);
        let result = self.search(app, root, cancel, &mut |found| {
            live(&mut |lens| lens.files.extend(found.drain()))
        });
        if cancel.is_cancelled() {
            return;
        }
        let mut done = None;
        live(&mut |lens| {
            lens.walking = false;
            lens.error = result.as_ref().err().map(|e| format!("{e:#}"));
            if lens.error.is_none() {
                lens.hinted.clear();
                done = Some(lens.files.clone());
            }
        });
        if let Some(files) = done {
            let mut counted = self.counted.lock();
            counted.retain(|(r, _)| !r.starts_with(root));
            counted.insert(0, (root.to_path_buf(), files));
            counted.truncate(KEEP_COUNTED);
        }
        self.changed(app);
    }

    /// Run `tag:*` under `root`, handing what it finds to `found` every
    /// `PUBLISH_EVERY` (false: the lens is gone, stop).
    fn search(
        &self,
        app: &App,
        root: &Path,
        cancel: &CancelToken,
        found: &mut dyn FnMut(&mut HashMap<PathBuf, Vec<String>>) -> bool,
    ) -> anyhow::Result<()> {
        let Outcome::Value(v) = app.dispatch(&Invocation::with_args(
            "query.location",
            &serde_json::json!({ "facet": "tag", "root": PathArg::from(root) }),
        ))?
        else {
            anyhow::bail!("the search gave no location");
        };
        let location = v["location"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("the search gave no location"))?;
        let open = app.open_location(&Location::parse(location))?;
        let mut batch: HashMap<PathBuf, Vec<String>> = HashMap::new();
        let mut last = Instant::now();
        let outcome = loop {
            if cancel.is_cancelled() {
                break Ok(());
            }
            match open.events().recv_timeout(Duration::from_millis(100)) {
                Ok(ListingEvent::Added(entries) | ListingEvent::Updated(entries)) => {
                    for e in entries {
                        if let Some(path) = open.path_of(e.id) {
                            batch.insert(path, tags_of(&e.metadata));
                        }
                    }
                }
                Ok(ListingEvent::Complete) => break Ok(()),
                Ok(ListingEvent::Error(e)) => break Err(anyhow::anyhow!("{e}")),
                Ok(_) | Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    break Err(anyhow::anyhow!("the search stopped"));
                }
            }
            if last.elapsed() >= PUBLISH_EVERY && !batch.is_empty() {
                last = Instant::now();
                if !found(&mut batch) {
                    break Ok(());
                }
                self.changed(app);
            }
        };
        open.close();
        found(&mut batch);
        outcome
    }

    /// Let go of `pane`'s lens (`tags.lens_release`).
    pub fn release(&self, app: &App, pane: PaneId) {
        if let Some(lens) = self.open.lock().remove(&pane) {
            lens.cancel.cancel();
            self.changed(app);
        }
    }

    /// What `pane`'s lens shows, each tag as `row` makes it (color, pin).
    pub fn status(&self, pane: PaneId, row: impl Fn(String, usize) -> Row) -> Status {
        let open = self.open.lock();
        let Some(lens) = open.get(&pane) else {
            return Status::default();
        };
        let hinted = (lens.walking || lens.error.is_some()).then_some(lens.hinted.as_slice());
        let counted = count(&lens.files, hinted);
        let files = lens.files.values().filter(|t| !t.is_empty()).count();
        let root = lens.root.clone();
        let (walking, error) = (lens.walking, lens.error.clone());
        drop(open);
        Status {
            root: Some(root),
            tags: counted.into_iter().map(|(t, n)| row(t, n)).collect(),
            files,
            walking,
            error,
        }
    }

    /// What bibli applied: each lens, and each count kept, under the
    /// files it names counts them again.
    pub fn observe(&self, app: &App, seen: &[Seen]) {
        let apply = |root: &Path, files: &mut Files, hinted: Option<&mut Vec<(String, usize)>>| {
            let mut changed = false;
            let mut retagged: Vec<&String> = Vec::new();
            for s in seen {
                match s {
                    Seen::Tags(path, tags) if path.starts_with(root) => {
                        if set(files, path, tags) {
                            changed = true;
                            retagged.extend(tags);
                        }
                    }
                    Seen::Moved(from, to) => {
                        let to = to.as_deref().filter(|t| t.starts_with(root));
                        changed |= moved(files, from, to);
                    }
                    Seen::Tags(..) => {}
                }
            }
            // The index's counts no longer hold for a tag counted here.
            if let Some(hinted) = hinted {
                hinted.retain(|(h, _)| !retagged.iter().any(|t| same_tag(h, t)));
            }
            changed
        };
        let mut changed = false;
        for lens in self.open.lock().values_mut() {
            changed |= apply(&lens.root.clone(), &mut lens.files, Some(&mut lens.hinted));
        }
        for (root, files) in self.counted.lock().iter_mut() {
            apply(&root.clone(), files, None);
        }
        if changed {
            self.changed(app);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{count, moved, set};
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn tags_are_counted_by_file_and_spelled_as_most_files_spell_them() {
        let files: HashMap<PathBuf, Vec<String>> = [
            ("a", vec!["Work", "x"]),
            ("b", vec!["work"]),
            ("c", vec!["work", "Été"]),
        ]
        .into_iter()
        .map(|(p, t)| (PathBuf::from(p), t.into_iter().map(str::to_owned).collect()))
        .collect();
        let owned = |v: &[(&str, usize)]| {
            v.iter()
                .map(|(t, n)| ((*t).to_owned(), *n))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            count(&files, None),
            owned(&[("work", 3), ("x", 1), ("Été", 1)])
        );
        // While walking, the index's counts hold where the walk has not
        // caught up yet.
        let hinted = owned(&[("WORK", 9), ("old", 2)]);
        assert_eq!(
            count(&files, Some(&hinted)),
            owned(&[("old", 2), ("work", 9), ("x", 1), ("Été", 1)])
        );
    }

    #[test]
    fn files_moved_or_gone_take_their_tags_along() {
        let mut files: HashMap<PathBuf, Vec<String>> = HashMap::new();
        let t = |v: &[&str]| v.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
        assert!(set(
            &mut files,
            PathBuf::from("/r/d/a").as_path(),
            &t(&["x"])
        ));
        assert!(set(&mut files, PathBuf::from("/r/b").as_path(), &t(&["y"])));
        assert!(!set(
            &mut files,
            PathBuf::from("/r/b").as_path(),
            &t(&["y"])
        ));
        assert!(!set(&mut files, PathBuf::from("/r/c").as_path(), &[]));
        // A folder renamed carries what is in it.
        assert!(moved(
            &mut files,
            PathBuf::from("/r/d").as_path(),
            Some(PathBuf::from("/r/e").as_path())
        ));
        assert_eq!(files.get(&PathBuf::from("/r/e/a")), Some(&t(&["x"])));
        // A file trashed is gone.
        assert!(moved(&mut files, PathBuf::from("/r/b").as_path(), None));
        assert_eq!(files.len(), 1);
        assert!(set(&mut files, PathBuf::from("/r/e/a").as_path(), &[]));
        assert!(files.is_empty());
    }
}
