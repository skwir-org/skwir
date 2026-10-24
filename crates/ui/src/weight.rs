//! What the files a pane shows weigh together, for its footer while
//! nothing is selected: counted as a selection's size is (the files' own
//! sizes; folders are not walked, that is the Usage view).
//!
//! The listing cannot say: a big folder learns the sizes of the rows it
//! draws, and a sum of those is a wrong number that grows as it scrolls.
//! So a worker reads the folder's files once its listing is complete, those
//! the pane shows (hidden files only when it shows them, the name filter
//! applied), again when the names or what the pane shows of them change,
//! never for a batch of metadata: a scroll hydrates rows, and starting
//! over for each batch, a big folder's weight was never said. A request
//! supersedes the one before; both end with the location
//! (`Pane::location_lifetime`). Where the pane asks for every file's
//! metadata anyway (a narrowing, a sort by size or date), what it shows is
//! added up from the listing once each file has it, and again when a size
//! changes: nothing is read twice. Until then each batch of metadata asks
//! whether a file is still unknown, from where the last one was found
//! (`Pane::file_unknown_from`), so a big folder is not walked per batch. The footer keeps the last answer for
//! the same folder until the next, so it does not blink.

use std::path::{Path, PathBuf};

use gpui::{Context, Window};
use skwir_core::CancelToken;

use crate::shell::Shell;

/// What a sum was asked for: the listing, its names, its sizes when added
/// up from it (`from_listing`), and what the pane shows of it (hidden
/// files, the filter's version). A listing is one location's: navigating
/// makes another.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    listing: u64,
    names: u64,
    sizes: u64,
    hidden: bool,
    filter: u64,
    from_listing: bool,
}

/// A pane's weight: the last sum, the folder it is of, and what it was, or
/// is being, asked for.
#[derive(Default)]
pub struct Weight {
    key: Option<Key>,
    dir: Option<PathBuf>,
    bytes: Option<u64>,
    cancel: Option<CancelToken>,
    /// Where a file without its metadata was last found in the pane's
    /// order: the next look starts there.
    unknown_at: usize,
}

impl Shell {
    /// Ask for the weight of what the pane being rendered shows, when that
    /// changed. Every frame costs one key of numbers compared; the location
    /// is read only when it moved.
    pub(crate) fn sync_weight(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.pane;
        let key = {
            let ws = self.app.workspace.read();
            let Some(p) = ws.pane(pane) else { return };
            // A folder still listing keeps what it had until it is complete.
            if !p.listing.is_complete() {
                return;
            }
            // A folder sorted by size or date (another location is not
            // weighed), or any narrowed.
            let folder = p.location().is_some_and(|l| l.path().is_some());
            let from_listing = p.narrowed().is_some() || folder && p.sort.key.needs_metadata();
            Key {
                listing: p.listing.id(),
                names: p.listing.names_version(),
                sizes: if from_listing {
                    p.listing.size_version()
                } else {
                    0
                },
                hidden: p.sort.show_hidden,
                filter: p.filter_version(),
                from_listing,
            }
        };
        if self.ui().weight.key == Some(key) {
            return;
        }
        let unknown_at = self.ui().weight.unknown_at;
        let (asked, listed) = {
            let ws = self.app.workspace.read();
            let Some(p) = ws.pane(pane) else { return };
            (
                p.location()
                    .and_then(skwir_core::Location::path)
                    .map(|dir| (dir, p.filter().map(str::to_owned), p.location_lifetime())),
                key.from_listing.then(|| listed_bytes(p, unknown_at)),
            )
        };
        let weight = &mut self.ui_mut().weight;
        if let Some(cancel) = weight.cancel.take() {
            cancel.cancel();
        }
        weight.key = Some(key);
        if let Some(listed) = listed {
            let dir = asked.map(|(dir, _, _)| dir);
            if weight.dir != dir {
                weight.dir = dir;
                weight.bytes = None;
            }
            // Until every file has its size, the last answer stays.
            match listed {
                Listed::Unknown(at) => weight.unknown_at = at,
                Listed::Bytes(bytes) => weight.bytes = Some(bytes),
            }
            return;
        }
        let Some((dir, filter, lifetime)) = asked else {
            // Not a folder: nothing to weigh.
            weight.dir = None;
            weight.bytes = None;
            return;
        };
        // Another folder: what was said of the one before is not its own.
        if weight.dir.as_ref() != Some(&dir) {
            weight.bytes = None;
            weight.dir = Some(dir.clone());
        }
        let cancel = lifetime.child();
        weight.cancel = Some(cancel.clone());
        let hidden = key.hidden;
        let (tx, rx) = flume::bounded(1);
        let token = cancel.clone();
        // A stat for each file: in the folder's lane, so a server that
        // stopped answering holds its own.
        self.app.scheduler.spawn_in(
            skwir_core::fs::reach::lane(&dir),
            skwir_core::scheduler::Priority::Background,
            "folder-weight",
            cancel,
            move || {
                if let Some(bytes) = weigh(&dir, hidden, filter.as_deref(), &token) {
                    let _ = tx.send(bytes);
                }
            },
        );
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(bytes) = rx.recv_async().await {
                this.update_in(cx, |shell, _, cx| {
                    if let Some(ui) = shell.pane_ui_mut(pane)
                        && ui.weight.key == Some(key)
                    {
                        ui.weight.bytes = Some(bytes);
                        ui.weight.cancel = None;
                        cx.notify();
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    /// What the files the pane being rendered shows weigh, once known.
    pub(crate) fn listed_bytes(&self) -> Option<u64> {
        self.ui().weight.bytes
    }
}

/// What the listing says of what the files a pane shows weigh.
enum Listed {
    /// A file has no metadata yet; the first found is there in the order.
    Unknown(usize),
    Bytes(u64),
}

/// What the files the pane shows weigh, once each has its metadata,
/// looking for one without from `unknown_at` on: each file once, its
/// names (hard links) one file (`entry::weight`).
fn listed_bytes(p: &skwir_core::workspace::Pane, unknown_at: usize) -> Listed {
    if let Some(at) = p.file_unknown_from(unknown_at) {
        return Listed::Unknown(at);
    }
    Listed::Bytes(skwir_core::entry::weight(
        p.order().iter().filter_map(|id| p.entry(*id)),
    ))
}

/// The files in `dir` a pane shows, added up: what is not a folder, names
/// hidden only when `hidden`, those holding `filter`. Each is stat-ed as
/// the listing stats it (`fs::stat_entry`) and weighs what it does there
/// (`Entry::weighed`): a link its target's size, a link to a folder
/// nothing. A file counts once however many names lead to it, as
/// `entry::weight` counts (ADR-049). `None` once cancelled, or when the
/// folder cannot be read.
fn weigh(dir: &Path, hidden: bool, filter: Option<&str>, cancel: &CancelToken) -> Option<u64> {
    use skwir_core::{Entry, EntryId, EntryKind};
    let folder = skwir_core::fs::Dir::open(dir).ok()?;
    let filter = filter.map(skwir_core::workspace::NameFilter::new);
    let mut bytes = 0u64;
    let mut seen = std::collections::HashSet::new();
    for listed in folder.read().ok()? {
        if cancel.is_cancelled() {
            return None;
        }
        let Ok(listed) = listed else { continue };
        // A folder weighs nothing here: it needs no stat.
        if listed.kind == EntryKind::Directory
            || filter.as_ref().is_some_and(|f| !f.holds(&listed.name))
        {
            continue;
        }
        let mut entry = Entry::new(EntryId::UNKNOWN, listed.name, listed.kind);
        entry.hidden = listed.hidden;
        if !hidden && entry.is_hidden() {
            continue;
        }
        if skwir_core::fs::stat_entry(&folder, &mut entry)
            && let Some((file, size)) = entry.weighed()
            && seen.insert(file)
        {
            bytes = bytes.saturating_add(size);
        }
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::weigh;
    use skwir_core::CancelToken;

    #[test]
    fn a_folder_weighs_the_files_it_shows() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("it's a report.txt"), [0u8; 300]).unwrap();
        std::fs::write(dir.join("notes.md"), [0u8; 20]).unwrap();
        std::fs::write(dir.join(".hidden"), [0u8; 5]).unwrap();
        std::fs::create_dir(dir.join("folder")).unwrap();
        std::fs::write(dir.join("folder").join("inside"), [0u8; 1000]).unwrap();
        let token = CancelToken::new();
        // Folders are not walked; hidden files only when shown.
        assert_eq!(weigh(dir, false, None, &token), Some(320));
        assert_eq!(weigh(dir, true, None, &token), Some(325));
        // The name filter, as the pane applies it.
        assert_eq!(weigh(dir, false, Some("rep"), &token), Some(300));
        // Two names of one file (hard links) weigh the file once (ADR-049).
        #[cfg(unix)]
        {
            std::fs::hard_link(dir.join("notes.md"), dir.join("notes again.md")).unwrap();
            assert_eq!(weigh(dir, false, None, &token), Some(320));
        }
        // A link weighs what it leads to, as the listing sizes it: a file
        // from elsewhere its size, one counted here already nothing more,
        // a folder nothing.
        #[cfg(unix)]
        {
            let away = tempfile::tempdir().unwrap();
            std::fs::write(away.path().join("big «iso»"), [0u8; 4000]).unwrap();
            let link = |to: &std::path::Path, name: &str| {
                std::os::unix::fs::symlink(to, dir.join(name)).unwrap();
            };
            link(&away.path().join("big «iso»"), "iso ’link’");
            link(&dir.join("it's a report.txt"), "report again");
            link(away.path(), "away");
            assert_eq!(weigh(dir, false, None, &token), Some(4320));
        }
        token.cancel();
        assert_eq!(weigh(dir, false, None, &token), None);
        assert_eq!(
            weigh(&dir.join("missing"), false, None, &CancelToken::new()),
            None
        );
    }
}
