//! The file list: a `uniform_list` over the active pane's listing, sorted
//! directories-first, with lazy hydration of visible rows, selection, cursor
//! movement, type-ahead, and the context menu trigger.

use std::collections::HashSet;
use std::ops::Range;
use std::time::{Duration, Instant};

use bibli_core::entry::keys;
use bibli_core::{Entry, EntryId, Listing};
use gpui::{
    AnyElement, Context, Div, KeyContext, KeyDownEvent, MouseButton, MouseDownEvent,
    ScrollStrategy, Stateful, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};

use crate::format;
use crate::keymap;
use crate::menu::SelectionCtx;
use crate::shell::Shell;
use crate::theme::Theme;

pub const ROW_HEIGHT: f32 = 26.0;

pub struct ListState {
    /// Entry ids in display order.
    pub sorted: Vec<EntryId>,
    sorted_events: u64,
    sorted_len: usize,
    sorted_hidden: bool,
    /// Hydration requested and not yet answered; cleared when the listing changes.
    pub pending_hydrate: HashSet<EntryId>,
    pub scroll: UniformListScrollHandle,
    pub show_hidden: bool,
    pub visible_rows: usize,
    typeahead: String,
    typeahead_at: Option<Instant>,
}

impl Default for ListState {
    fn default() -> Self {
        ListState {
            sorted: Vec::new(),
            sorted_events: u64::MAX,
            sorted_len: usize::MAX,
            sorted_hidden: false,
            pending_hydrate: HashSet::new(),
            scroll: UniformListScrollHandle::new(),
            show_hidden: false,
            visible_rows: 20,
            typeahead: String::new(),
            typeahead_at: None,
        }
    }
}

impl ListState {
    /// Recompute the sorted order when the listing or the hidden toggle changed.
    pub fn sync(&mut self, listing: &Listing, events_seen: u64) {
        if self.sorted_events == events_seen
            && self.sorted_len == listing.len()
            && self.sorted_hidden == self.show_hidden
        {
            return;
        }
        let show_hidden = self.show_hidden;
        let mut keyed: Vec<(bool, String, EntryId)> = listing
            .iter()
            .filter(|e| show_hidden || !e.name.to_string_lossy().starts_with('.'))
            .map(|e| (!e.is_dir(), e.name_str().to_lowercase(), e.id))
            .collect();
        keyed.sort_unstable();
        self.sorted = keyed.into_iter().map(|(_, _, id)| id).collect();
        if self.sorted_events != events_seen {
            self.pending_hydrate.clear();
        }
        self.sorted_events = events_seen;
        self.sorted_len = listing.len();
        self.sorted_hidden = show_hidden;
    }

    /// Forget everything: a new location was opened.
    pub fn reset(&mut self) {
        self.sorted.clear();
        self.sorted_events = u64::MAX;
        self.sorted_len = usize::MAX;
        self.pending_hydrate.clear();
        self.scroll.scroll_to_item_strict(0, ScrollStrategy::Top);
    }

    /// Changes every time the order is recomputed (instrumentation).
    pub fn sorted_version(&self) -> (u64, usize, bool) {
        (self.sorted_events, self.sorted_len, self.sorted_hidden)
    }

    pub fn index_of(&self, id: EntryId) -> Option<usize> {
        self.sorted.iter().position(|x| *x == id)
    }
}

impl Shell {
    /// Selection summary for `when` evaluation and the pane key context.
    pub fn selection_ctx(&self) -> SelectionCtx {
        let ws = self.app.workspace.read();
        let Some(pane) = ws.pane(self.pane) else {
            return SelectionCtx::default();
        };
        let mut ctx = SelectionCtx {
            count: pane.selection.len(),
            all_dirs: true,
            all_files: true,
        };
        for id in pane.selection.iter() {
            match pane.listing.get(id) {
                Some(e) if e.is_dir() => ctx.all_files = false,
                Some(_) => ctx.all_dirs = false,
                None => {}
            }
        }
        ctx
    }

    fn pane_key_context(&self) -> KeyContext {
        let mut kc = KeyContext::default();
        kc.add(keymap::PANE);
        for f in self.selection_ctx().flags() {
            kc.add(f);
        }
        kc
    }

    fn select_ids(
        &mut self,
        ids: Vec<EntryId>,
        mode: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run(
            "select",
            serde_json::json!({ "ids": ids, "mode": mode }),
            window,
            cx,
        );
    }

    /// Ids from the selection anchor to `target`, anchor first, target last,
    /// so the core keeps the anchor and moves the cursor to the target.
    fn range_to(&self, target: usize) -> Vec<EntryId> {
        let anchor = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane)
                .and_then(|p| p.selection.anchor)
                .and_then(|a| self.list.index_of(a))
        };
        let anchor = anchor.unwrap_or(target);
        if target >= anchor {
            self.list.sorted[anchor..=target].to_vec()
        } else {
            self.list.sorted[target..=anchor]
                .iter()
                .rev()
                .copied()
                .collect()
        }
    }

    fn cursor_index(&self) -> Option<usize> {
        let ws = self.app.workspace.read();
        ws.pane(self.pane)
            .and_then(|p| p.selection.cursor)
            .and_then(|c| self.list.index_of(c))
    }

    /// Move the cursor to `target` (clamped), extending the selection from the anchor if asked.
    pub fn cursor_to(
        &mut self,
        target: isize,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.list.sorted.is_empty() {
            return;
        }
        let target = target.clamp(0, self.list.sorted.len() as isize - 1) as usize;
        let ids = if extend {
            self.range_to(target)
        } else {
            vec![self.list.sorted[target]]
        };
        self.select_ids(ids, "replace", window, cx);
        self.list.scroll.scroll_to_item(target, ScrollStrategy::Top);
        cx.notify();
    }

    pub fn cursor_move(
        &mut self,
        delta: isize,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cur = self.cursor_index().map_or(-1, |i| i as isize);
        let target = if cur < 0 && delta < 0 { 0 } else { cur + delta };
        self.cursor_to(target, extend, window, cx);
    }

    pub fn on_row_mouse_down(
        &mut self,
        ix: usize,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.pane_focus);
        let Some(&id) = self.list.sorted.get(ix) else {
            return;
        };
        match ev.button {
            MouseButton::Left => {
                if ev.click_count >= 2 {
                    self.select_ids(vec![id], "replace", window, cx);
                    self.open_selection(window, cx);
                } else if ev.modifiers.control {
                    self.select_ids(vec![id], "toggle", window, cx);
                } else if ev.modifiers.shift {
                    let ids = self.range_to(ix);
                    self.select_ids(ids, "replace", window, cx);
                } else {
                    self.select_ids(vec![id], "replace", window, cx);
                }
            }
            MouseButton::Right => {
                let selected = {
                    let ws = self.app.workspace.read();
                    ws.pane(self.pane).is_some_and(|p| p.selection.contains(id))
                };
                if !selected {
                    self.select_ids(vec![id], "replace", window, cx);
                }
                self.open_context_menu(ev.position, cx);
            }
            _ => {}
        }
        cx.notify();
    }

    fn on_pane_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        if ks.modifiers.control
            || ks.modifiers.alt
            || ks.modifiers.platform
            || ks.modifiers.function
        {
            return;
        }
        let Some(ch) = ks.key_char.as_deref() else {
            return;
        };
        if ch.is_empty() || ch.chars().any(char::is_control) {
            return;
        }
        let now = Instant::now();
        if self
            .list
            .typeahead_at
            .is_none_or(|t| now.duration_since(t) > Duration::from_secs(2))
        {
            self.list.typeahead.clear();
        }
        self.list.typeahead_at = Some(now);
        self.list.typeahead.push_str(&ch.to_lowercase());
        let needle = self.list.typeahead.clone();
        let found = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane).and_then(|p| {
                self.list.sorted.iter().position(|id| {
                    p.listing
                        .get(*id)
                        .is_some_and(|e| e.name_str().to_lowercase().starts_with(&needle))
                })
            })
        };
        if let Some(ix) = found {
            self.cursor_to(ix as isize, false, window, cx);
        }
    }
}

fn row(
    theme: &Theme,
    ix: usize,
    e: &Entry,
    selected: bool,
    is_cursor: bool,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let size = e
        .metadata
        .value(keys::SIZE)
        .and_then(bibli_core::Value::as_u64)
        .map_or_else(
            || "—".to_owned(),
            |s| {
                if e.is_dir() {
                    "—".to_owned()
                } else {
                    format::size(s)
                }
            },
        );
    let modified = e
        .metadata
        .value(keys::MODIFIED)
        .and_then(bibli_core::Value::as_time)
        .map_or_else(|| "—".to_owned(), format::time);
    let marker = if e.is_dir() {
        theme.accent
    } else {
        theme.text_muted
    };
    div()
        .id(ix)
        .h(px(ROW_HEIGHT))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_2()
        .border_1()
        .border_color(if is_cursor {
            theme.accent
        } else {
            gpui::transparent_black()
        })
        .when(selected, |d| {
            d.bg(theme.selection_bg).text_color(theme.selection_text)
        })
        .when(!selected, |d| d.hover(|s| s.bg(theme.surface)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                this.on_row_mouse_down(ix, ev, window, cx);
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                this.on_row_mouse_down(ix, ev, window, cx);
            }),
        )
        .child(div().size(px(10.)).rounded_sm().bg(marker).flex_none())
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(e.name_str().into_owned()),
        )
        .child(
            div()
                .w(px(90.))
                .flex_none()
                .text_right()
                .text_color(if selected {
                    theme.selection_text
                } else {
                    theme.text_muted
                })
                .child(size),
        )
        .child(
            div()
                .w(px(130.))
                .flex_none()
                .text_color(if selected {
                    theme.selection_text
                } else {
                    theme.text_muted
                })
                .child(modified),
        )
}

fn header(theme: &Theme) -> Div {
    div()
        .h(px(24.))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_2()
        .text_xs()
        .text_color(theme.text_muted)
        .border_b_1()
        .border_color(theme.border)
        .child(div().size(px(10.)).flex_none())
        .child(div().flex_1().child("Name"))
        .child(div().w(px(90.)).flex_none().text_right().child("Size"))
        .child(div().w(px(130.)).flex_none().child("Modified"))
}

pub fn render(shell: &mut Shell, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let count = shell.list.sorted.len();
    let key_context = shell.pane_key_context();
    let scroll = shell.list.scroll.clone();

    let list = uniform_list(
        "rows",
        count,
        cx.processor(move |this: &mut Shell, range: Range<usize>, _window, cx| {
            let theme = cx.global::<Theme>().clone();
            let mut rows = Vec::with_capacity(range.len());
            let mut need: Vec<EntryId> = Vec::new();
            this.list.visible_rows = range.len().max(1);
            {
                let ws = this.app.workspace.read();
                let Some(pane) = ws.pane(this.pane) else {
                    return rows;
                };
                let cursor = pane.selection.cursor;
                for ix in range {
                    let Some(&id) = this.list.sorted.get(ix) else {
                        break;
                    };
                    let Some(e) = pane.listing.get(id) else {
                        continue;
                    };
                    if !e.metadata.is_hydrated() && !this.list.pending_hydrate.contains(&id) {
                        need.push(id);
                    }
                    rows.push(row(
                        &theme,
                        ix,
                        e,
                        pane.selection.contains(id),
                        cursor == Some(id),
                        cx,
                    ));
                }
            }
            if !need.is_empty() {
                tracing::debug!(n = need.len(), "hydrate request");
                this.list.pending_hydrate.extend(need.iter().copied());
                if let Err(e) = this
                    .app
                    .dispatch(&bibli_core::command::Invocation::with_args(
                        "hydrate",
                        &serde_json::json!({ "ids": need }),
                    ))
                {
                    tracing::warn!("hydrate: {e:#}");
                }
            }
            rows
        }),
    )
    .track_scroll(scroll)
    .size_full();

    div()
        .id("pane")
        .flex_1()
        .h_full()
        .min_w_0()
        .flex()
        .flex_col()
        .track_focus(&shell.pane_focus)
        .key_context(key_context)
        .on_key_down(cx.listener(Shell::on_pane_key_down))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                // Empty area: menu for the directory itself.
                window.focus(&this.pane_focus);
                this.open_context_menu(ev.position, cx);
            }),
        )
        .child(header(&theme))
        .child(div().flex_1().min_h_0().w_full().child(list))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use bibli_core::{Entry, EntryId, EntryKind, Listing, ListingEvent};

    use super::ListState;

    fn id(n: u64) -> EntryId {
        EntryId { dev: 1, ino: n }
    }

    fn listing() -> Listing {
        let mut l = Listing::default();
        l.apply(ListingEvent::Added(vec![
            Entry::new(id(1), "zeta.txt", EntryKind::File),
            Entry::new(id(2), "Alpha", EntryKind::Directory),
            Entry::new(id(3), ".hidden", EntryKind::File),
            Entry::new(id(4), "beta.txt", EntryKind::File),
            Entry::new(id(5), "gamma", EntryKind::Directory),
        ]));
        l
    }

    #[test]
    fn sorts_dirs_first_then_case_insensitive_and_filters_hidden() {
        let l = listing();
        let mut s = ListState::default();
        s.sync(&l, 1);
        assert_eq!(s.sorted, [id(2), id(5), id(4), id(1)]);
        let version = s.sorted_version();
        s.sync(&l, 1);
        assert_eq!(s.sorted_version(), version, "no resort without change");
        s.show_hidden = true;
        s.sync(&l, 1);
        assert_eq!(s.sorted, [id(2), id(5), id(3), id(4), id(1)]);
        assert_eq!(s.index_of(id(4)), Some(3));
    }

    #[test]
    fn pending_hydration_clears_on_new_events_only() {
        let l = listing();
        let mut s = ListState::default();
        s.sync(&l, 1);
        s.pending_hydrate.insert(id(1));
        s.show_hidden = true;
        s.sync(&l, 1);
        assert!(s.pending_hydrate.contains(&id(1)), "toggle keeps pending");
        s.sync(&l, 2);
        assert!(s.pending_hydrate.is_empty(), "events clear pending");
    }
}
