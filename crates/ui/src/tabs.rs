//! Tabs (ADR-018): the strip above the panes, and the shell following the
//! workspace's tabs.
//!
//! The core owns the tabs (`bibli_core::tabs`): which panes each groups,
//! which is shown, which of its panes is active. The shell holds a `PaneUi`
//! per pane and reconciles them with the workspace whenever its revision
//! moves (`Shell::sync_tabs`): the panes of the tab shown are
//! `Shell::panes`, in the tab's order; those of other tabs wait in
//! `Shell::hidden` with what they drew from let go; those gone are
//! released. Whoever changed the tabs (a key, a click, an agent), this is
//! the one place the UI follows it.
//!
//! The strip shows only with two tabs or more. A tab is named like the place
//! it shows, is a drop target like a sidebar row, is dragged to reorder,
//! and closes with its ×, a middle click or Ctrl+W.

use std::collections::HashMap;

use bibli_core::Location;
use bibli_core::command::Invocation;
use bibli_core::workspace::{PaneId, TabId};
use gpui::{
    AnyElement, Context, IntoElement, MouseButton, SharedString, Window, div, prelude::*, px, svg,
};

use crate::shell::Shell;
use crate::theme::Theme;

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
    key: Option<(u64, Vec<(TabId, Option<Location>)>)>,
    pub labels: Vec<Label>,
}

impl Shell {
    /// Follow the workspace's tabs: see the module comment. Cheap when
    /// nothing moved: one number compared.
    pub(crate) fn sync_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (revision, shown, wanted, active, all) = {
            let ws = self.app.workspace.read();
            if ws.revision() == self.tabs_revision {
                // Commands may still make another pane of this tab active.
                self.pane = ws.active_id();
                return;
            }
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
        let before: Vec<PaneId> = self.panes.iter().map(|p| p.id).collect();
        let had_focus = self
            .panes
            .iter()
            .any(|p| p.focus.contains_focused(window, cx))
            || window.focused(cx).is_none();
        // A new pane opens in the view the active one shows.
        let view = if self.panes.is_empty() {
            crate::view::LIST
        } else {
            self.ui().view_id
        };
        let mut pool: HashMap<PaneId, crate::shell::PaneUi> = self
            .panes
            .drain(..)
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
        for (id, mut ui) in pool {
            if all.contains(&id) {
                if before.contains(&id) {
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
            // A split's own arrangement belongs to its tab.
            if let Some(left) = self.shown_tab {
                match self.layout.split.take() {
                    Some(split) => self.tab_splits.insert(left, split),
                    None => self.tab_splits.remove(&left),
                };
            }
            self.layout.split = self.tab_splits.remove(&shown);
            let first = self.shown_tab.is_none();
            self.shown_tab = Some(shown);
            // Whether the window is split is the shown tab's.
            if !first {
                self.remember_session();
            }
            if had_focus {
                window.focus(&self.ui().focus);
            }
        }
        {
            let ws = self.app.workspace.read();
            self.tab_splits.retain(|tab, _| ws.tab(*tab).is_some());
        }
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
            if ws.tabs().len() < 2 {
                self.tab_labels = Labels::default();
                return;
            }
            ws.tabs()
                .iter()
                .map(|t| (t.id, ws.pane(t.active).and_then(|p| p.shown()).cloned()))
                .collect()
        };
        let key = (self.app.registry.places.revision(), shows);
        if self.tab_labels.key.as_ref() == Some(&key) {
            return;
        }
        let places = self.app.registry.places.all();
        let labels = key
            .1
            .iter()
            .map(|(tab, location)| {
                let place = location
                    .as_ref()
                    .and_then(|l| places.iter().find(|p| p.location.same_place(l)));
                let glyph = match location.as_ref().map(|l| &*l.scheme) {
                    Some("query") => "search",
                    Some("shelf") => "shelf",
                    _ => "folder",
                };
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

/// The tab strip, when there are two tabs or more.
pub fn render(shell: &mut Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    shell.sync_tab_labels();
    if shell.tab_labels.labels.len() < 2 {
        return None;
    }
    let theme = cx.global::<Theme>().clone();
    let shown = shell.shown_tab;
    let labels = shell.tab_labels.labels.clone();
    let mut strip = div()
        .id("tab-strip")
        .flex_none()
        .h(px(36.))
        .w_full()
        .min_w_0()
        .flex()
        .items_end()
        .gap(px(2.))
        .px(px(6.))
        .bg(theme.surface)
        .border_b_1()
        .border_color(theme.border)
        // A double click beside the tabs opens one, as in a browser.
        .on_click(cx.listener(|this, event: &gpui::ClickEvent, window, cx| {
            if event.click_count() == 2 {
                this.run("tab.new", serde_json::json!({}), window, cx);
            }
        }));
    let count = labels.len();
    for (index, label) in labels.into_iter().enumerate() {
        strip = strip.child(tab(
            shell,
            &label,
            index,
            count,
            shown == Some(label.tab),
            cx,
        ));
    }
    let new = crate::toolbar::Button::new("tab-strip-new", "plus", "New tab", "tab.new")
        .render(shell, cx);
    strip = strip.child(div().flex_none().pb(px(2.)).child(new));
    Some(strip.into_any_element())
}

fn tab(
    shell: &mut Shell,
    label: &Label,
    index: usize,
    count: usize,
    shown: bool,
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
    let font = shell.font.clone();
    let tip = label.tip.clone();
    let hover = theme.hover;
    let close = div()
        .id(("tab-close", id.0 as usize))
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
    let mut el = div()
        .id(("tab", id.0 as usize))
        .relative()
        .flex_1()
        .min_w(px(56.))
        .max_w(px(220.))
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
        .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), font.clone(), cx))
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
        .child(close)
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
