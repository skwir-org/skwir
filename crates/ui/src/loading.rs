//! One delayed wake per listing, including listings waiting for a drive to wake.
use crate::shell::Shell;
use gpui::{Context, Task};
use skwir_core::{location::OpenLocation, workspace::PaneId};
use std::{sync::Arc, time::Duration};

#[derive(Default)]
pub struct State {
    open: Option<Arc<dyn OpenLocation>>,
    pub visible: bool,
    timer: Option<Task<()>>,
    /// A look, a second on, at whether a remote still being listed stopped
    /// answering (`message`): asked again while it is listed, never once
    /// it is.
    remote: Option<Task<()>>,
}

/// How long a remote's lane runs a job before the banner says the remote
/// is not answering (ADR-077).
const NOT_ANSWERING: Duration = Duration::from_secs(3);
impl Shell {
    pub fn sync_loading(&mut self, cx: &mut Context<Self>) {
        let pending: Vec<_> = {
            let ws = self.app.workspace.read();
            ws.panes()
                .iter()
                .map(|pane| {
                    (
                        pane.id,
                        pane.open.clone(),
                        !pane.listing.is_complete() && pane.listing.error().is_none(),
                    )
                })
                .collect()
        };
        for (pane, open, pending) in pending {
            let Some(ui) = self.panes.iter_mut().find(|ui| ui.id == pane) else {
                continue;
            };
            let same = match (&ui.loading.open, &open) {
                (Some(old), Some(new)) => Arc::ptr_eq(old, new),
                (None, None) => true,
                _ => false,
            };
            if !same {
                ui.loading = State {
                    open: open.clone(),
                    ..State::default()
                };
            }
            if !pending || open.is_none() {
                ui.loading.visible = false;
                ui.loading.timer = None;
                ui.loading.remote = None;
                continue;
            }
            if ui.loading.visible {
                // Shown for a remote's folder: looked at again in a second,
                // to say when it stops answering.
                let remote = open
                    .as_ref()
                    .and_then(|o| o.location().path())
                    .is_some_and(|path| skwir_core::fs::reach::of(&path).is_some());
                if remote && ui.loading.remote.is_none() {
                    ui.loading.remote = Some(cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(Duration::from_secs(1)).await;
                        let _ = this.update(cx, |shell, cx| {
                            if let Some(ui) = shell.panes.iter_mut().find(|ui| ui.id == pane) {
                                ui.loading.remote = None;
                            }
                            cx.notify();
                        });
                    }));
                }
                continue;
            }
            if ui.loading.timer.is_some() {
                continue;
            }
            let open = open.expect("pending location");
            ui.loading.timer = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let _ = this.update(cx, |shell, cx| {
                    let still_pending = shell.app.workspace.read().pane(pane).is_some_and(|p| {
                        p.open
                            .as_ref()
                            .is_some_and(|current| Arc::ptr_eq(current, &open))
                            && !p.listing.is_complete()
                            && p.listing.error().is_none()
                    });
                    if still_pending
                        && let Some(ui) = shell.panes.iter_mut().find(|ui| ui.id == pane)
                    {
                        ui.loading.visible = true;
                        cx.notify();
                    }
                });
            }));
        }
    }
}

/// The one delayed "still loading" signal; the pane's footer and its
/// banner both derive from it so they never disagree.
pub fn pending(shell: &Shell, pane: PaneId) -> bool {
    shell
        .panes
        .iter()
        .find(|ui| ui.id == pane)
        .is_some_and(|ui| ui.loading.visible)
}

pub fn message(shell: &Shell, pane: PaneId) -> Option<String> {
    let ws = shell.app.workspace.read();
    let pane_state = ws.pane(pane)?;
    // With nothing listed the pane itself explains the failure
    // (`placeholder.rs`); the banner is for an error beside existing entries.
    if let Some(error) = pane_state.listing.error()
        && !pane_state.listing.is_empty()
    {
        return Some(format!("Some of this location could not be read: {error}"));
    }
    if !pending(shell, pane) {
        return None;
    }
    if shell.ui().place.search {
        return Some("Searching…".into());
    }
    let location = pane_state.location()?.clone();
    drop(ws);
    // A remote's folder whose server stopped answering says so (ADR-077):
    // its lane has run a job that long.
    if let Some(path) = location.path()
        && let Some((remote, known)) = shell.app.remote(&path)
        && shell
            .app
            .scheduler
            .stalled(remote.lane())
            .is_some_and(|ran| ran >= NOT_ANSWERING)
    {
        let name = known.map_or_else(|| remote.point.display().to_string(), |known| known.label);
        return Some(format!("{name} is not answering…"));
    }
    // What is no folder says what it opens (an address connecting).
    Some(if location.is_path() {
        "Loading location…".into()
    } else {
        format!("Opening {}…", shell.app.location_title(&location))
    })
}
