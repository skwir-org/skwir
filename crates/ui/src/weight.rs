//! What the files a pane shows weigh together, for its footer while
//! nothing is selected: counted as a selection's size is (the files' own
//! sizes; folders are not walked, that is the Usage view).
//!
//! The listing cannot say: a big folder learns the sizes of the rows it
//! draws, and a sum of those is a wrong number that grows as it scrolls.
//! So a worker reads the folder's files once its listing is complete, those
//! the pane shows (hidden files only when it shows them, the name filter
//! applied), again when the folder or what the pane shows of it changes.
//! A request supersedes the one before; both end with the location
//! (`Pane::location_lifetime`). The footer keeps the last answer for the
//! same folder until the next, so it does not blink.

use std::path::{Path, PathBuf};

use bibli_core::CancelToken;
use gpui::{Context, Window};

use crate::shell::Shell;

/// What a sum was asked for: the listing and its version, and what the
/// pane shows of it (hidden files, the filter's version). A listing is one
/// location's: navigating makes another.
type Key = (u64, u64, bool, u64);

/// A pane's weight: the last sum, the folder it is of, and what it was, or
/// is being, asked for.
#[derive(Default)]
pub struct Weight {
    key: Option<Key>,
    dir: Option<PathBuf>,
    bytes: Option<u64>,
    cancel: Option<CancelToken>,
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
            (
                p.listing.id(),
                p.listing.version(),
                p.sort.show_hidden,
                p.filter_version(),
            )
        };
        if self.ui().weight.key == Some(key) {
            return;
        }
        let asked = {
            let ws = self.app.workspace.read();
            ws.pane(pane).and_then(|p| {
                let dir = p.location()?.path()?;
                Some((dir, p.filter().map(str::to_owned), p.location_lifetime()))
            })
        };
        let weight = &mut self.ui_mut().weight;
        if let Some(cancel) = weight.cancel.take() {
            cancel.cancel();
        }
        weight.key = Some(key);
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
        let hidden = key.2;
        let (tx, rx) = flume::bounded(1);
        let token = cancel.clone();
        self.app.scheduler.spawn(
            bibli_core::scheduler::Priority::Background,
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

/// The files in `dir` a pane shows, added up: what is not a folder, names
/// starting with a dot only when `hidden`, those holding `filter`. Links
/// count as themselves, as the listing sizes them. `None` once cancelled,
/// or when the folder cannot be read.
fn weigh(dir: &Path, hidden: bool, filter: Option<&str>, cancel: &CancelToken) -> Option<u64> {
    let mut bytes = 0u64;
    for entry in std::fs::read_dir(dir).ok()? {
        if cancel.is_cancelled() {
            return None;
        }
        let Ok(entry) = entry else { continue };
        let name = entry.file_name();
        if !hidden && name.as_encoded_bytes().starts_with(b".") {
            continue;
        }
        if filter.is_some_and(|f| !bibli_core::workspace::name_holds(&name, f)) {
            continue;
        }
        // Not followed: a link weighs what the listing says it does.
        match entry.metadata() {
            Ok(meta) if !meta.is_dir() => bytes = bytes.saturating_add(meta.len()),
            _ => {}
        }
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::weigh;
    use bibli_core::CancelToken;

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
        token.cancel();
        assert_eq!(weigh(dir, false, None, &token), None);
        assert_eq!(
            weigh(&dir.join("missing"), false, None, &CancelToken::new()),
            None
        );
    }
}
