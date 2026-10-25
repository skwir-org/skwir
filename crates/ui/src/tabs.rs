//! Tabs (ADR-018): the strip above the panes, and the shell following the
//! workspace's tabs.
//!
//! The core owns the tabs (`skwir_core::tabs`): which panes each groups,
//! which is shown, which of its panes is active. The shell holds a `PaneUi`
//! per pane and reconciles them with the workspace whenever its revision
//! moves (`Shell::sync_tabs`): the panes of the tab shown are
//! `Shell::panes`, in the tab's order; those of other tabs wait in
//! `Shell::hidden` with what they drew from let go; those gone are
//! released. Whoever changed the tabs (a key, a click, an agent), this is
//! the one place the UI follows it.
//!
//! Showing another tab keeps the tab left drawn until the tab shown is
//! finished, for at most `HOLD` (`Shell::present`): the window goes from
//! one finished frame to the next, with no blank or half-listed one.
//!
//! The strip stands over the panes, as they are what a tab changes, when
//! there are two tabs or more, or one and it is asked for
//! (`ui.single_tab`, drawn without a ×: the last tab stays). Without it a
//! window has no row above its panes' own: New tab is then More's, Ctrl+T's
//! and the middle button's, and a second tab brings the strip, which moves
//! the panes down (`STRIP_HEIGHT`). A tab is named like the
//! place it shows, is a drop target like a sidebar row, is dragged to
//! reorder, and closes with its ×, a middle click or Ctrl+W.

use std::collections::HashMap;

use gpui::{
    AnyElement, Context, IntoElement, MouseButton, SharedString, Window, div, prelude::*, px, svg,
};
use skwir_core::Location;
use skwir_core::command::Invocation;
use skwir_core::workspace::{PaneId, TabId};

use crate::shell::Shell;
use crate::theme::Theme;

/// How long the tab left stays drawn while the tab shown gets ready. Up to
/// 100 000 entries and a search of 10 000 results are ready within it
/// (ADR-018 addendum); past it the tab shown streams in as a navigation
/// does.
pub const HOLD: std::time::Duration = std::time::Duration::from_millis(150);

/// A tab being dragged along the strip, to put it elsewhere.
#[derive(Clone, Copy, Debug)]
pub struct TabDrag {
    pub tab: TabId,
    pub index: usize,
}

/// What the strip draws for one tab, read when the tabs or what they show
/// change, not per frame.
#[derive(Clone, Debug)]
pub struct Label {
    pub tab: TabId,
    pub title: SharedString,
    /// The full location, for the tooltip.
    pub tip: SharedString,
    /// The theme icon of the place it shows, if it shows one.
    pub icon: Option<String>,
    /// The bundled icon otherwise: a folder, a search, the shelf.
    pub glyph: &'static str,
    /// The folder a drop on the tab goes into.
    pub folder: Option<std::path::PathBuf>,
}

/// The strip's labels and what they were read from.
#[derive(Default)]
pub struct Labels {
    key: Option<(u64, u64, Vec<(TabId, Option<Location>)>)>,
    pub labels: Vec<Label>,
}

impl Shell {
    /// Follow the workspace's tabs: see the module comment. Cheap when
    /// nothing moved: one number compared.
    ///
    /// When another tab is shown, the one left stays drawn (`presented`)
    /// until the tab shown is ready or `HOLD` has passed (`present`): its
    /// panes list, get their selection back, scroll where they were and
    /// have the metadata of what they show first, out of sight, so the
    /// window goes from one finished frame to the next.
    pub(crate) fn sync_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (revision, shown, wanted, active, all) = {
            let ws = self.app.workspace.read();
            if ws.revision() == self.tabs_revision {
                // Commands may still make another pane of this tab active.
                self.pane = ws.active_id();
                return;
            }
            self.tab_count = ws.tabs().len();
            let tab = ws.active_tab();
            (
                ws.revision(),
                tab.id,
                tab.panes.clone(),
                tab.active,
                ws.panes()
                    .iter()
                    .map(|p| p.id)
                    .collect::<std::collections::HashSet<_>>(),
            )
        };
        self.tabs_revision = revision;
        let current: Vec<PaneId> = self.panes.iter().map(|p| p.id).collect();
        // The tab shown was split or lost its split, by whatever route (F3,
        // a pane closed, a script): a new split starts even.
        if self.shown_tab == Some(shown) && current.len() != wanted.len() {
            self.split_share = crate::shell::SPLIT_SHARE;
        }
        // What is on screen: the tab left while it is held, else the panes.
        let on_screen: Vec<PaneId> = match &self.presented {
            Some(left) => left.iter().map(|p| p.id).collect(),
            None => current.clone(),
        };
        // A new pane opens in the view the active one shows.
        let view = if self.panes.is_empty() {
            crate::view::LIST
        } else {
            self.ui().view_id
        };
        let mut pool: HashMap<PaneId, crate::shell::PaneUi> = self
            .panes
            .drain(..)
            .chain(self.presented.take().into_iter().flatten())
            .chain(self.hidden.drain().map(|(_, ui)| ui))
            .map(|ui| (ui.id, ui))
            .collect();
        let mut panes = Vec::with_capacity(wanted.len());
        for id in &wanted {
            let ui = pool.remove(id).unwrap_or_else(|| {
                let mut ui = self.make_pane_ui(*id, window, cx);
                if ui.views.contains_key(view) {
                    ui.view_id = view;
                }
                ui
            });
            panes.push(ui);
        }
        self.panes = panes;
        // Hold the tab left on screen when there is one to hold: it still
        // exists, and it is not the tab shown (shown again before it went).
        let hold = self.presented_tab.is_some()
            && self.presented_tab != Some(shown)
            && !on_screen.is_empty()
            && on_screen
                .iter()
                .all(|id| all.contains(id) && pool.contains_key(id));
        if hold {
            self.presented = Some(on_screen.iter().filter_map(|id| pool.remove(id)).collect());
            // The wait counts from the last tab asked for, and ends by the
            // deadline whatever happens.
            self.held_since = Some(std::time::Instant::now());
            self.hold_timer = Some(cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(HOLD).await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.sync(window, cx);
                    cx.notify();
                });
            }));
        }
        for (id, mut ui) in pool {
            if all.contains(&id) {
                if current.contains(&id) || on_screen.contains(&id) {
                    ui.let_go();
                    self.release_usage(id);
                }
                self.hidden.insert(id, ui);
            } else {
                // Closed: with the split, with its tab, or by a script.
                self.release_usage(id);
            }
        }
        self.pane = active;
        if self.shown_tab != Some(shown) {
            let first = self.shown_tab.is_none();
            self.shown_tab = Some(shown);
            // Whether the window is split is the shown tab's.
            if !first {
                self.remember_session();
            }
        }
        if !hold {
            self.present(true, window, cx);
        }
        {
            let ws = self.app.workspace.read();
            self.tab_splits.retain(|tab, _| ws.tab(*tab).is_some());
        }
    }

    /// Draw the tab shown: the tab left, if it was still drawn, lets go of
    /// what it drew from and its panes are suspended; the tab's own split
    /// arrangement and the keyboard follow. `ready` says whether the tab
    /// got ready or the wait ended (deadline, or the user acting).
    pub(crate) fn present(&mut self, ready: bool, window: &mut Window, cx: &mut Context<Self>) {
        let left = self.presented.take();
        if let Some(since) = self.held_since.take() {
            tracing::debug!(
                ms = since.elapsed().as_secs_f64() * 1000.,
                ready,
                "tab drawn"
            );
        }
        self.hold_timer = None;
        let had_focus = left
            .iter()
            .flatten()
            .chain(self.panes.iter())
            .any(|p| p.focus.contains_focused(window, cx))
            || window.focused(cx).is_none();
        if let Some(left) = left {
            let all: std::collections::HashSet<PaneId> = self
                .app
                .workspace
                .read()
                .panes()
                .iter()
                .map(|p| p.id)
                .collect();
            for mut ui in left {
                ui.let_go();
                self.release_usage(ui.id);
                if all.contains(&ui.id) {
                    self.hidden.insert(ui.id, ui);
                }
            }
        }
        self.app.settle_tabs();
        if self.presented_tab != self.shown_tab {
            // A split's own arrangement and share belong to its tab.
            if let Some(left) = self.presented_tab {
                let split = crate::layout::TabSplit {
                    panes: self.layout.split.take(),
                    share: std::mem::replace(&mut self.split_share, crate::shell::SPLIT_SHARE),
                };
                if split == crate::layout::TabSplit::default() {
                    self.tab_splits.remove(&left);
                } else {
                    self.tab_splits.insert(left, split);
                }
            }
            if let Some(shown) = self.shown_tab {
                let split = self.tab_splits.remove(&shown).unwrap_or_default();
                self.layout.split = split.panes;
                self.split_share = split.share;
            }
            self.presented_tab = self.shown_tab;
            // The window's shape is now the tab shown's, split included.
            self.remember_session();
            if had_focus {
                window.focus(&self.ui().focus, cx);
            }
        }
        cx.notify();
    }

    /// The focus of the pane drawn as active: the one element sure to be
    /// in the window, whatever tab is held.
    pub(crate) fn drawn_focus(&self) -> Option<gpui::FocusHandle> {
        let id = self.drawn_pane();
        self.presented
            .iter()
            .flatten()
            .chain(&self.panes)
            .find(|p| p.id == id)
            .map(|p| p.focus.clone())
    }

    /// The pane drawn as active: the tab left's while it is held.
    pub(crate) fn drawn_pane(&self) -> PaneId {
        match &self.presented {
            Some(left) => self.drawn_active(left),
            None => self.pane,
        }
    }

    /// The active pane of `left`, the tab left, as it was.
    pub(crate) fn drawn_active(&self, left: &[crate::shell::PaneUi]) -> PaneId {
        let active = self
            .presented_tab
            .and_then(|tab| self.app.workspace.read().tab(tab).map(|t| t.active));
        active
            .filter(|id| left.iter().any(|p| p.id == *id))
            .or_else(|| left.first().map(|p| p.id))
            .unwrap_or(self.pane)
    }

    /// Put the pane `self.pane` where it was once its listing is complete
    /// and its selection is back (the first entry it showed at the top, or
    /// its cursor in view), and ask for the metadata of what it will show
    /// first. True once that has arrived: the pane can be drawn finished.
    pub(crate) fn place_pane(&mut self) -> bool {
        let app = std::sync::Arc::clone(&self.app);
        let settled = {
            let ws = app.workspace.read();
            let Some(p) = ws.pane(self.pane) else {
                return true;
            };
            // A pane that opened nothing has nothing to wait for.
            p.open.is_none()
                || ((p.listing.is_complete() || p.listing.error().is_some()) && !p.restoring())
        };
        if !settled {
            return false;
        }
        if std::mem::replace(&mut self.ui_mut().returning, false) {
            let ui = self.ui();
            // The entry that was at the top, where it is now; if it went,
            // the place it had.
            let len = ui.list.sorted.len();
            let anchor =
                ui.anchor
                    .filter(|_| len > 0)
                    .map(|(id, ix, within)| match ui.list.index_of(id) {
                        Some(now) => (now, within),
                        None => (ix.min(len - 1), 0.),
                    });
            let items = ui.list.visible_items.max(ui.list.visible_rows).max(1);
            let (lo, hi) = match (anchor, self.cursor_index()) {
                (Some((ix, within)), _) => {
                    self.with_view(|view, _| view.restore_anchor((ix, within)));
                    (ix, ix + items)
                }
                (None, Some(ix)) => {
                    self.with_view(|view, _| view.scroll_to(ix));
                    (ix.saturating_sub(items), ix + items)
                }
                (None, None) => (0, items),
            };
            let sorted = std::sync::Arc::clone(&self.ui().list.sorted);
            let first = &sorted[lo.min(sorted.len())..hi.min(sorted.len())];
            let warm: Vec<skwir_core::EntryId> = {
                let ws = app.workspace.read();
                ws.pane(self.pane).map_or_else(Vec::new, |p| {
                    first
                        .iter()
                        .copied()
                        .filter(|id| {
                            p.listing
                                .get(*id)
                                .is_some_and(|e| !e.metadata.is_hydrated())
                        })
                        .collect()
                })
            };
            self.request_hydrate(warm.iter().copied());
            self.ui_mut().warm = warm;
        }
        if self.ui().warm.is_empty() {
            return true;
        }
        let ws = app.workspace.read();
        let Some(p) = ws.pane(self.pane) else {
            return true;
        };
        let listing = &p.listing;
        self.ui_mut()
            .warm
            .retain(|id| listing.get(*id).is_some_and(|e| !e.metadata.is_hydrated()));
        self.ui().warm.is_empty()
    }

    fn release_usage(&self, pane: PaneId) {
        let _ = self.app.dispatch(&Invocation::with_args(
            "usage.release",
            &serde_json::json!({ "pane": pane }),
        ));
    }

    /// The strip's labels, read again when the tabs, what their active panes
    /// show, or the sidebar's places moved.
    fn sync_tab_labels(&mut self) {
        let shows: Vec<(TabId, Option<Location>)> = {
            let ws = self.app.workspace.read();
            ws.tabs()
                .iter()
                .map(|t| (t.id, ws.pane(t.active).and_then(|p| p.shown()).cloned()))
                .collect()
        };
        self.sync_roots();
        let key = (
            self.app.registry.places.revision(),
            self.roots.generation,
            shows,
        );
        if self.tab_labels.key.as_ref() == Some(&key) {
            return;
        }
        let places = self.app.registry.places.all();
        let labels = key
            .2
            .iter()
            .map(|(tab, location)| {
                let place = location
                    .as_ref()
                    .and_then(|l| places.iter().find(|p| p.location.same_place(l)));
                let glyph = location
                    .as_ref()
                    .map_or("folder", |l| crate::trail::glyph(&self.app, l));
                Label {
                    tab: *tab,
                    title: location
                        .as_ref()
                        .map(|l| self.app.location_title(l))
                        .unwrap_or_default()
                        .into(),
                    tip: location
                        .as_ref()
                        .map(|l| match l.path() {
                            Some(path) => path.display().to_string(),
                            None => l.to_string(),
                        })
                        .unwrap_or_default()
                        .into(),
                    icon: place.and_then(|p| p.icon.clone()),
                    glyph,
                    folder: location.as_ref().and_then(Location::path),
                }
            })
            .collect();
        self.tab_labels = Labels {
            key: Some(key),
            labels,
        };
    }
}

/// The strip's height, above the panes when it shows: a row's, so its
/// edge meets the sidebar header's; the tabs stand on its foot.
pub const STRIP_HEIGHT: f32 = crate::address::PANE_ROW_HEIGHT;

/// The widest a tab grows.
const TAB_MAX: f32 = 220.;

/// The tab strip, when it shows (see the module comment), and the button
/// that opens another tab.
pub fn render(shell: &mut Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    shell.sync_tab_labels();
    let shown = shell.shown_tab;
    let labels = shell.tab_labels.labels.clone();
    if !shell.strip_shown() {
        return None;
    }
    // Split and Preview say their names only while every tab can be as
    // wide as a tab gets: the labeled row holds its tabs at `TAB_MAX`
    // (`AdaptiveRow` measures it so), so the labels never take a tab's room.
    if shell.toggles_at() != crate::shell::TogglesAt::Strip {
        return Some(row(shell, &labels, shown, None, cx));
    }
    let variants = [true, false]
        .into_iter()
        .map(|labeled| row(shell, &labels, shown, Some(labeled), cx))
        .collect();
    let theme = cx.global::<Theme>().clone();
    Some(
        div()
            .flex_none()
            .h(px(STRIP_HEIGHT))
            .w_full()
            .bg(theme.surface)
            .border_b_1()
            .border_color(theme.border)
            .child(crate::elements::adaptive::AdaptiveRow {
                variants,
                height: STRIP_HEIGHT - 1.,
                chosen: None,
            })
            .into_any_element(),
    )
}

/// The tabs and the button that opens another, after the sidebar toggle
/// and More when the strip holds them (`Shell::corner_at`), then Split and
/// Preview when it holds them (`toggles`: whether they have their labels,
/// its tabs then held at their widest). Alone, the row draws its own border;
/// under `AdaptiveRow`, its parent's.
fn row(
    shell: &mut Shell,
    labels: &[Label],
    shown: Option<TabId>,
    toggles: Option<bool>,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let mut strip = div()
        .id("tab-strip")
        .role(gpui::Role::TabList)
        .aria_label("Tabs")
        .when(shell.a11y, |d| d.aria_size_of_set(labels.len()))
        .flex_1()
        .h_full()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .items_end()
        .gap(px(2.))
        .px(px(6.))
        // A double click beside the tabs opens one, as in a browser.
        .on_click(cx.listener(|this, event: &gpui::ClickEvent, window, cx| {
            if event.click_count() == 2 {
                this.run("tab.new", serde_json::json!({}), window, cx);
            }
        }))
        // The wheel over the tabs goes through them, a notch a tab, as in
        // KDE and GTK's notebooks.
        .on_scroll_wheel(
            cx.listener(|this, event: &gpui::ScrollWheelEvent, window, cx| {
                let delta = event.delta.pixel_delta(window.line_height());
                // A sideways swipe counts as much as a turn of the wheel.
                let along = if delta.y.abs() >= delta.x.abs() {
                    delta.y
                } else {
                    delta.x
                };
                let lines = matches!(event.delta, gpui::ScrollDelta::Lines(_));
                match this.tab_wheel.turn(f32::from(along), lines, false) {
                    0 => {}
                    // Down (or right) is the next tab.
                    turn => this.run(
                        "tab.select",
                        serde_json::json!({ "delta": turn }),
                        window,
                        cx,
                    ),
                }
                cx.stop_propagation();
            }),
        );
    let count = labels.len();
    for (index, label) in labels.iter().enumerate() {
        strip = strip.child(tab(
            shell,
            label,
            index,
            count,
            shown == Some(label.tab),
            toggles == Some(true),
            cx,
        ));
    }
    let new = crate::toolbar::Button::new("tab-strip-new", "plus", "New tab", "tab.new")
        .render(shell, cx);
    strip = strip.child(div().flex_none().h_full().flex().items_center().child(new));
    // Split and Preview end the strip while the preview is not open at the
    // right (`Shell::toggles_at`): beside the tabs, not in them, so a
    // double click on one opens no tab.
    let toggles = toggles.map(|labeled| {
        let [split, preview] = crate::address::panel_toggles(shell, labeled, cx);
        div()
            .flex_none()
            .h_full()
            .flex()
            .items_center()
            .gap(px(2.))
            .px(px(6.))
            .child(split)
            .child(preview)
    });
    // The sidebar's toggle and More start the strip while the sidebar is
    // not drawn, on the row its header would share; beside the tabs too.
    let corner = (shell.corner_at() == crate::shell::CornerAt::Strip).then(|| {
        let [toggle, more] = crate::address::corner(shell, cx);
        div()
            .flex_none()
            .h_full()
            .flex()
            .items_center()
            .gap(px(2.))
            .px(px(6.))
            .child(toggle)
            .child(more)
    });
    let line = div().flex().children(corner).child(strip);
    match toggles {
        Some(toggles) => line.size_full().child(toggles),
        None => line
            .flex_none()
            .h(px(STRIP_HEIGHT))
            .w_full()
            .bg(theme.surface)
            .border_b_1()
            .border_color(theme.border),
    }
    .into_any_element()
}

fn tab(
    shell: &mut Shell,
    label: &Label,
    index: usize,
    count: usize,
    shown: bool,
    widest: bool,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let id = label.tab;
    let color = if shown { theme.text } else { theme.text_muted };
    let icon: AnyElement = match label
        .icon
        .as_deref()
        .and_then(|name| shell.icons.named(name, 16))
    {
        Some(path) => crate::icons::themed(&path, 14., color),
        None => svg()
            .path(format!("toolbar/{}.svg", label.glyph))
            .size(px(14.))
            .flex_none()
            .text_color(color)
            .into_any_element(),
    };
    let tip = label.tip.clone();
    let hover = theme.hover;
    let a11y = shell.a11y;
    let close = div()
        .id(("tab-close", id.0 as usize))
        .role(gpui::Role::Button)
        .aria_label("Close tab")
        .flex_none()
        .size(px(18.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .hover(move |s| s.bg(hover))
        // Goes with what it closes: no focus of its own, the keys have Ctrl+W.
        .on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            this.run("tab.close", serde_json::json!({ "tab": id }), window, cx);
        }))
        .child(crate::controls::glyph("close", theme.text_muted, 12.));
    let drag = TabDrag { tab: id, index };
    let title = label.title.clone();
    // The tab shown is the selected one; its full location describes it.
    let mut el = div()
        .id(("tab", id.0 as usize))
        .role(gpui::Role::Tab)
        .when(a11y, |d| {
            d.aria_label(label.title.clone())
                .aria_description(label.tip.clone())
                .aria_selected(shown)
                .aria_position_in_set(index)
        })
        .relative()
        .flex_1()
        .min_w(px(if widest { TAB_MAX } else { 56. }))
        .max_w(px(TAB_MAX))
        .h(px(30.))
        .flex()
        .items_center()
        .gap(px(6.))
        .pl(px(8.))
        .pr(px(4.))
        .rounded_t_md()
        .border_1()
        .border_b_0()
        .text_xs()
        .text_color(color)
        .cursor_pointer()
        .when(shown, |d| d.bg(theme.bg).border_color(theme.border))
        .when(!shown, |d| {
            d.border_color(gpui::transparent_black())
                .hover(move |s| s.bg(hover))
        })
        .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), cx))
        .on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            this.run("tab.select", serde_json::json!({ "tab": id }), window, cx);
        }))
        .on_mouse_down(
            MouseButton::Middle,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.run("tab.close", serde_json::json!({ "tab": id }), window, cx);
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.open_tab_menu(id, event.position, window, cx);
            }),
        )
        .on_drag(drag, move |_, position, _, cx| {
            cx.new(|_| crate::dnd::DragPreview::named(title.to_string(), position))
        })
        .child(icon)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis_middle()
                .child(label.title.clone()),
        )
        .when(count > 1, |d| d.child(close))
        .child(gap(index, count, true, cx))
        .child(gap(index + 1, count, false, cx));
    if let Some(folder) = label.folder.clone() {
        el = crate::dnd::droppable_dir(el, folder, cx);
    }
    el.into_any_element()
}

/// Half of a tab, over it: where a tab dragged along the strip goes, before
/// the tab at `before`, drawn as a line on that edge.
fn gap(before: usize, count: usize, left: bool, cx: &mut Context<Shell>) -> gpui::Div {
    let edge = cx.global::<Theme>().accent;
    let zone = div().absolute().top_0().bottom_0().w(gpui::relative(0.5));
    let zone = if left { zone.left_0() } else { zone.right_0() };
    zone.drag_over::<TabDrag>(move |s, drag, _, _| {
        if drag.index == before || drag.index + 1 == before {
            s
        } else if left {
            s.border_l_2().border_color(edge)
        } else {
            s.border_r_2().border_color(edge)
        }
    })
    .on_drop(cx.listener(move |this, drag: &TabDrag, window, cx| {
        cx.stop_propagation();
        if let Some(to) = move_target(drag.index, before, count) {
            this.run(
                "tab.move",
                serde_json::json!({ "tab": drag.tab, "to": to }),
                window,
                cx,
            );
        }
    }))
}

/// Where a tab at `from` goes when dropped in the gap before `before`
/// (`count` meaning after the last): its new index, or `None` when that
/// is where it is.
fn move_target(from: usize, before: usize, count: usize) -> Option<usize> {
    let before = before.min(count);
    if before == from || before == from + 1 {
        return None;
    }
    Some(if before > from { before - 1 } else { before })
}

#[cfg(test)]
mod tests {
    use super::move_target;

    #[test]
    fn a_tab_dropped_in_a_gap_takes_that_place() {
        // Tabs a b c d; a dropped before d goes between c and d.
        assert_eq!(move_target(0, 3, 4), Some(2));
        // d dropped before a goes first.
        assert_eq!(move_target(3, 0, 4), Some(0));
        // After the last.
        assert_eq!(move_target(0, 4, 4), Some(3));
        // On either side of itself: nowhere.
        assert_eq!(move_target(1, 1, 4), None);
        assert_eq!(move_target(1, 2, 4), None);
    }
}
