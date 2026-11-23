//! One delayed wake per listing, including listings waiting for a drive to wake,
//! and the banner that says what a pane waits for, with the ways out of it.
use crate::shell::Shell;
use crate::sizes::TextSizes;
use crate::sizes::sized;
use crate::theme::Theme;
use crate::toolbar::Button;
use gpui::{AnyElement, Context, Task, div, prelude::*};
use skwir_core::command::Invocation;
use skwir_core::location::{Location, OpenLocation};
use skwir_core::words;
use skwir_core::words::Words;
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
    /// The order waits for what it reads of the entries (ADR-121,
    /// amended): said by the footer once it has for 200 ms, as "Loading…".
    pub reading: bool,
    reading_timer: Option<Task<()>>,
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
                        pane.listing.reading().is_some(),
                    )
                })
                .collect()
        };
        for (pane, open, pending, reading) in pending {
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
            if !reading {
                ui.loading.reading = false;
                ui.loading.reading_timer = None;
            } else if !ui.loading.reading && ui.loading.reading_timer.is_none() {
                ui.loading.reading_timer = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(200))
                        .await;
                    let _ = this.update(cx, |shell, cx| {
                        let still = shell
                            .app
                            .workspace
                            .read()
                            .pane(pane)
                            .is_some_and(|p| p.listing.reading().is_some());
                        if let Some(ui) = shell.panes.iter_mut().find(|ui| ui.id == pane) {
                            ui.loading.reading_timer = None;
                            ui.loading.reading = still;
                        }
                        cx.notify();
                    });
                }));
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

/// Whether the footer says how far the reading the order waits for went:
/// once it has for 200 ms (`State::reading`).
pub fn reading(shell: &Shell, pane: PaneId) -> bool {
    shell
        .panes
        .iter()
        .find(|ui| ui.id == pane)
        .is_some_and(|ui| ui.loading.reading)
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
    label: Words,
    /// Its tooltip, after which its keys are said.
    title: Words,
    command: Invocation,
}

impl Banner {
    fn said(text: &Words) -> Self {
        Banner {
            text: text.say(),
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
        // Its server gone, while its files show as they were: said so,
        // with the way to connect again (ADR-149, amended).
        let reopen = pane_state
            .location()
            .and_then(Location::path)
            .and_then(|path| crate::placeholder::reopening(shell, pane, &path));
        if let Some(reopen) = reopen {
            return Some(Banner {
                text: reopen.title.say(),
                ways: reopen
                    .busy
                    .is_none()
                    .then(|| Way {
                        id: "reopen",
                        icon: "refresh",
                        label: reopen.label.clone(),
                        title: reopen.label,
                        command: reopen.command,
                    })
                    .into_iter()
                    .collect(),
            });
        }
        return Some(Banner::said(&words!(
            loading_partly_failed = "Some of this location could not be read: { $error }",
            error = error.clone(),
            note = "A banner above a folder listed in part: what stopped the rest. error: why, as the system said it."
        )));
    }
    if !pending(shell, pane) {
        return None;
    }
    if shell.ui().place.search {
        return Some(Banner::said(&words!(
            loading_searching = "Searching…",
            note = "A banner above a search while it is still looking."
        )));
    }
    let location = pane_state.location()?.clone();
    let back = pane_state.history_pos > 0;
    // A folder filled elsewhere says how far it is (ADR-120).
    let read = pane_state.listing.filling_count().filter(|&n| n > 0);
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
        let name = known.map_or_else(
            || Words::as_written(remote.point.display().to_string()),
            |known| known.label,
        );
        let mut ways: Vec<Way> = leave(back).into_iter().collect();
        ways.extend(disconnect(shell, &remote.point));
        return Some(Banner {
            text: words!(
                loading_not_answering = "{ $name } is not answering…",
                name = name,
                note = "A banner above a remote folder whose server stopped answering. name: the server's or the place's name."
            )
            .say(),
            ways,
        });
    }
    // What is no folder says what it opens (an address connecting), and
    // may be left: what opens it is the pane's, and stops.
    Some(if location.is_path() {
        Banner::said(&match read {
            Some(count) => words!(
                loading_location_read = "Loading location… { $count ->
                    [one] { $count } item
                   *[other] { $count } items
                }",
                count = count,
                note =
                    "A banner above a folder being read: how many of its items were read so far."
            ),
            None => words!(
                loading_location = "Loading location…",
                note = "A banner above a folder being read."
            ),
        })
    } else {
        Banner {
            text: words!(
                loading_opening = "Opening { $place }…",
                place = shell.app.location_title(&location),
                note = "A banner above a location being opened that is no folder (an address connecting). place: its title."
            )
            .say(),
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
            label: words!(
                loading_go_back = "Go Back",
                note = "A button of the banner above a location that takes long: leave it for the one before."
            ),
            title: words!(
                loading_go_back_tip = "Back",
                note = "Pointing at the Go Back button of the banner above a location that takes long."
            ),
            command: Invocation::new("back"),
        });
    }
    Some(Way {
        id: "home",
        icon: "home",
        label: words!(
            loading_go_home = "Go Home",
            note = "A button of the banner above a location that takes long: leave it for the home folder."
        ),
        title: words!(
            loading_go_home_tip = "Go to the home folder",
            note = "Pointing at the Go Home button of the banner above a location that takes long."
        ),
        command: home()?,
    })
}

/// Stop opening the location: leave it as [`leave`] does.
fn cancel(back: bool) -> Option<Way> {
    Some(if back {
        Way {
            id: "cancel",
            icon: "close",
            label: cancel_label(),
            title: words!(
                loading_cancel_back = "Stop opening it and go back",
                note = "Pointing at the Cancel button of the banner above a location being opened."
            ),
            command: Invocation::new("back"),
        }
    } else {
        Way {
            id: "cancel",
            icon: "close",
            label: cancel_label(),
            title: words!(
                loading_cancel_home = "Stop opening it and go to the home folder",
                note = "Pointing at the Cancel button of the banner above a location being opened."
            ),
            command: home()?,
        }
    })
}

fn cancel_label() -> Words {
    words!(
        loading_cancel = "Cancel",
        note = "A button of the banner above a location being opened: stop opening it."
    )
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
        label: words!(
            loading_disconnect = "Disconnect",
            note = "A button of the banner above a remote folder whose server stopped answering."
        ),
        title: words!(
            loading_disconnect_tip = "Let go of the server, as the sidebar's Eject does",
            note =
                "Pointing at the Disconnect button. Eject is the button of a place in the sidebar."
        ),
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
        .min_h(sized(38.))
        .px_3()
        .py_1()
        .flex()
        .items_center()
        .gap_2()
        .text_body()
        .text_color(theme.text_muted)
        .bg(theme.bar_ground(shell.lowered()))
        .child(div().flex_1().min_w_0().child(banner.text));
    for way in banner.ways {
        row = row.child(
            Button::new(
                format!("banner-{}-{}", way.id, pane.0),
                way.icon,
                way.title.clone(),
                way.command.name.to_string(),
            )
            .args(way.command.args)
            .label(way.label.clone())
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
            (way.label.english(), way.command.name.to_string())
        };
        assert_eq!(
            named(leave(true)),
            ("Go Back".to_owned(), "back".to_owned())
        );
        assert_eq!(
            named(leave(false)),
            ("Go Home".to_owned(), "navigate".to_owned())
        );
        assert_eq!(
            named(cancel(true)),
            ("Cancel".to_owned(), "back".to_owned())
        );
        assert_eq!(
            named(cancel(false)),
            ("Cancel".to_owned(), "navigate".to_owned())
        );
    }
}
