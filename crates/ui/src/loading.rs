//! One delayed wake per listing, including listings waiting for a drive to wake,
//! and the banner that says what a pane waits for, with the ways out of it.
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toolbar::Button;
use gpui::{AnyElement, Context, Task, div, prelude::*, px};
use skwir_core::command::Invocation;
use skwir_core::location::{Location, OpenLocation};
use skwir_core::workspace::PaneId;
use std::path::Path;
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

/// What the banner above a pane's view says, and the ways out it offers:
/// leaving a location still being opened, or one on a server that does
/// not answer, and letting go of that server.
pub struct Banner {
    pub text: String,
    pub ways: Vec<Way>,
}

/// One of the banner's buttons: what it shows, and what it runs on the
/// banner's pane.
pub struct Way {
    /// What it is known by within its pane (`elements::known_by`).
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    /// Its tooltip, after which its keys are said.
    title: &'static str,
    command: Invocation,
}

impl Banner {
    fn said(text: impl Into<String>) -> Self {
        Banner {
            text: text.into(),
            ways: Vec::new(),
        }
    }
}

pub fn message(shell: &Shell, pane: PaneId) -> Option<Banner> {
    let ws = shell.app.workspace.read();
    let pane_state = ws.pane(pane)?;
    // With nothing listed the pane itself explains the failure
    // (`placeholder.rs`); the banner is for an error beside existing entries.
    if let Some(error) = pane_state.listing.error()
        && !pane_state.listing.is_empty()
    {
        return Some(Banner::said(format!(
            "Some of this location could not be read: {error}"
        )));
    }
    if !pending(shell, pane) {
        return None;
    }
    if shell.ui().place.search {
        return Some(Banner::said("Searching…"));
    }
    let location = pane_state.location()?.clone();
    let back = pane_state.history_pos > 0;
    drop(ws);
    // A remote's folder whose server stopped answering says so (ADR-077):
    // its lane has run a job that long. The pane may leave it, or let go
    // of the server where its place can (its Eject).
    if let Some(path) = location.path()
        && let Some((remote, known)) = shell.app.remote(&path)
        && shell
            .app
            .scheduler
            .stalled(remote.lane())
            .is_some_and(|ran| ran >= skwir_core::fs::reach::NOT_ANSWERING)
    {
        let name = known.map_or_else(|| remote.point.display().to_string(), |known| known.label);
        let mut ways: Vec<Way> = leave(back).into_iter().collect();
        ways.extend(disconnect(shell, &remote.point));
        return Some(Banner {
            text: format!("{name} is not answering…"),
            ways,
        });
    }
    // What is no folder says what it opens (an address connecting), and
    // may be left: what opens it is the pane's, and stops.
    Some(if location.is_path() {
        Banner::said("Loading location…")
    } else {
        Banner {
            text: format!("Opening {}…", shell.app.location_title(&location)),
            ways: cancel(back).into_iter().collect(),
        }
    })
}

/// Leave the location shown: back where the pane was, else home (a tab
/// opened there, or kept there from the last run).
fn leave(back: bool) -> Option<Way> {
    if back {
        return Some(Way {
            id: "back",
            icon: "back",
            label: "Go Back",
            title: "Back",
            command: Invocation::new("back"),
        });
    }
    Some(Way {
        id: "home",
        icon: "home",
        label: "Go Home",
        title: "Go to the home folder",
        command: home()?,
    })
}

/// Stop opening the location: leave it as [`leave`] does.
fn cancel(back: bool) -> Option<Way> {
    Some(if back {
        Way {
            id: "cancel",
            icon: "close",
            label: "Cancel",
            title: "Stop opening it and go back",
            command: Invocation::new("back"),
        }
    } else {
        Way {
            id: "cancel",
            icon: "close",
            label: "Cancel",
            title: "Stop opening it and go to the home folder",
            command: home()?,
        }
    })
}

fn home() -> Option<Invocation> {
    skwir_core::os_path::home().map(|home| Invocation::navigate(&Location::from_path(home)))
}

/// Let go of the server mounted at `point`, as the sidebar's place for it
/// does (its Eject), where it offers that and is not at it already.
fn disconnect(shell: &Shell, point: &Path) -> Option<Way> {
    let places = shell.arranged_places();
    let eject = places
        .iter()
        .map(|arranged| &arranged.place)
        .find(|place| place.busy.is_none() && place.location.path().as_deref() == Some(point))?
        .eject
        .clone()?;
    Some(Way {
        id: "disconnect",
        icon: "eject",
        label: "Disconnect",
        title: "Let go of the server, as the sidebar's Eject does",
        command: eject,
    })
}

/// The banner of the pane being drawn (`Shell::pane`), its buttons the
/// toolbar's (their keys in their tooltips, Tab reaching them), known by
/// the pane and what they do.
pub fn render(banner: Banner, shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let pane = shell.pane;
    let mut row = div()
        .flex_none()
        .min_h(px(38.))
        .px_3()
        .py_1()
        .flex()
        .items_center()
        .gap_2()
        .text_sm()
        .text_color(theme.text_muted)
        .bg(theme.surface)
        .child(div().flex_1().min_w_0().child(banner.text));
    for way in banner.ways {
        row = row.child(
            Button::new(
                format!("banner-{}-{}", way.id, pane.0),
                way.icon,
                way.title,
                way.command.name.to_string(),
            )
            .args(way.command.args)
            .label(way.label)
            .render(shell, cx),
        );
    }
    row.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{cancel, leave};

    /// A pane leaves a wait back where it was, else home; Cancel likewise.
    #[test]
    fn a_wait_is_left_back_where_the_pane_was_else_home() {
        let named = |way: Option<super::Way>| {
            let way = way.expect("a way out");
            (way.label, way.command.name.to_string())
        };
        assert_eq!(named(leave(true)), ("Go Back", "back".to_owned()));
        assert_eq!(named(leave(false)), ("Go Home", "navigate".to_owned()));
        assert_eq!(named(cancel(true)), ("Cancel", "back".to_owned()));
        assert_eq!(named(cancel(false)), ("Cancel", "navigate".to_owned()));
    }
}
