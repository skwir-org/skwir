//! The list view: one row per entry with icon, name, size, modified;
//! clickable, sortable headers.

use std::ops::Range;
use std::path::PathBuf;

use bibli_core::entry::keys;
use bibli_core::{EntryId, SortKey};
use gpui::{
    AnyElement, Context, Div, MouseButton, MouseDownEvent, ScrollStrategy, Stateful,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};

use crate::dnd;
use crate::format;
use crate::icons;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::view::{Nav, PaneView};

pub const ROW_HEIGHT: f32 = 26.0;
const ICON_PX: f32 = 18.0;

#[derive(Default)]
pub struct ListView {
    scroll: UniformListScrollHandle,
}

/// Everything a row needs, collected under the workspace lock.
#[allow(clippy::struct_excessive_bools)]
pub struct RowData {
    pub ix: usize,
    pub id: EntryId,
    pub name: String,
    pub is_dir: bool,
    pub path: Option<PathBuf>,
    pub size: String,
    pub modified: String,
    pub selected: bool,
    pub is_cursor: bool,
    pub icon: Option<std::sync::Arc<std::path::Path>>,
    pub hydrated: bool,
    pub tags: Vec<String>,
}

/// Tags from the entry's `tags` metadata field (a list of text values).
pub fn tags_of(e: &bibli_core::Entry) -> Vec<String> {
    match e.metadata.value("tags") {
        Some(bibli_core::Value::List(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        Some(bibli_core::Value::Text(t)) => vec![t.to_string()],
        _ => Vec::new(),
    }
}

impl Shell {
    /// Collect row data for `range` of the sorted order.
    pub fn rows(&mut self, range: Range<usize>, icon_px: u16) -> Vec<RowData> {
        let mut out = Vec::with_capacity(range.len());
        let ws = self.app.workspace.read();
        let Some(pane) = ws.pane(self.pane) else {
            return out;
        };
        let cursor = pane.selection.cursor;
        for ix in range {
            let Some(&id) = self.ui().list.sorted.get(ix) else {
                break;
            };
            let Some(e) = pane.listing.get(id) else {
                continue;
            };
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
            out.push(RowData {
                ix,
                id,
                name: e.name_str().into_owned(),
                is_dir: e.is_dir(),
                path: if e.is_dir() {
                    pane.open.as_ref().and_then(|o| o.path_of(id))
                } else {
                    None
                },
                size,
                modified,
                selected: pane.selection.contains(id),
                is_cursor: cursor == Some(id),
                icon: self.icons.for_entry(e, icon_px),
                hydrated: e.metadata.is_hydrated(),
                tags: tags_of(e),
            });
        }
        out
    }
}

/// A full-width row for the list view and the last column of the columns view.
pub fn row(
    shell: &Shell,
    theme: &Theme,
    r: &RowData,
    compact: bool,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let ix = r.ix;
    let pane = shell.pane;
    let muted = if r.selected {
        theme.selection_text
    } else {
        theme.text_muted
    };
    let icon: AnyElement = match &r.icon {
        Some(p) => icons::element(p, ICON_PX),
        None => div()
            .size(px(ICON_PX))
            .rounded_sm()
            .bg(if r.is_dir {
                theme.accent
            } else {
                theme.text_muted
            })
            .flex_none()
            .into_any_element(),
    };
    let mut el = div()
        .id(ix)
        .h(px(ROW_HEIGHT))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_2()
        .border_1()
        .border_color(if r.is_cursor {
            theme.accent
        } else {
            gpui::transparent_black()
        })
        .when(r.selected, |d| {
            d.bg(theme.selection_bg).text_color(theme.selection_text)
        })
        .when(!r.selected, |d| d.hover(|s| s.bg(theme.surface)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                this.on_entry_mouse_down(pane, ix, ev, window, cx);
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                this.on_entry_mouse_down(pane, ix, ev, window, cx);
            }),
        )
        .child(icon)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap_1()
                .child(div().min_w_0().truncate().child(r.name.clone()))
                .children(r.tags.iter().take(4).map(|t| {
                    div()
                        .flex_none()
                        .px_1()
                        .rounded_sm()
                        .text_xs()
                        .bg(theme.accent.opacity(0.18))
                        .text_color(if r.selected {
                            theme.selection_text
                        } else {
                            theme.text
                        })
                        .child(t.clone())
                })),
        );
    if !compact {
        el = el
            .child(
                div()
                    .w(px(90.))
                    .flex_none()
                    .text_right()
                    .text_color(muted)
                    .child(r.size.clone()),
            )
            .child(
                div()
                    .w(px(130.))
                    .flex_none()
                    .text_color(muted)
                    .child(r.modified.clone()),
            );
    }
    el = dnd::draggable(el, shell);
    if let Some(p) = &r.path {
        el = dnd::droppable_dir(el, p.clone(), cx);
    }
    el
}

fn header_cell(
    id: &'static str,
    label: &'static str,
    key: SortKey,
    shell: &Shell,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let spec = shell.sort_spec();
    let indicator = if spec.key == key {
        if spec.ascending { " ▲" } else { " ▼" }
    } else {
        ""
    };
    div()
        .id(id)
        .cursor_pointer()
        .hover(|s| s.text_color(theme.text))
        .on_click(cx.listener(move |this, _, _, cx| this.sort_by(key, cx)))
        .child(format!("{label}{indicator}"))
}

fn header(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> Div {
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
        .child(div().size(px(ICON_PX)).flex_none())
        .child(div().flex_1().child(header_cell(
            "h-name",
            "Name",
            SortKey::Name,
            shell,
            theme,
            cx,
        )))
        .child(
            div()
                .w(px(90.))
                .flex_none()
                .flex()
                .justify_end()
                .child(header_cell(
                    "h-size",
                    "Size",
                    SortKey::Size,
                    shell,
                    theme,
                    cx,
                )),
        )
        .child(div().w(px(130.)).flex_none().child(header_cell(
            "h-mod",
            "Modified",
            SortKey::Modified,
            shell,
            theme,
            cx,
        )))
}

impl PaneView for ListView {
    fn id(&self) -> &'static str {
        crate::view::LIST
    }

    fn title(&self) -> &'static str {
        "List"
    }

    fn render(&mut self, shell: &mut Shell, _: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
        let theme = cx.global::<Theme>().clone();
        let count = shell.ui().list.sorted.len();
        let pane = shell.pane;
        let list = uniform_list(
            "rows",
            count,
            cx.processor(move |this: &mut Shell, range: Range<usize>, _window, cx| {
                this.with_pane(pane, |this| {
                    let theme = cx.global::<Theme>().clone();
                    this.ui_mut().list.visible_rows = range.len().max(1);
                    let rows = this.rows(range, ICON_PX as u16);
                    let need: Vec<EntryId> =
                        rows.iter().filter(|r| !r.hydrated).map(|r| r.id).collect();
                    this.request_hydrate(need);
                    rows.iter()
                        .map(|r| row(this, &theme, r, false, cx))
                        .collect::<Vec<_>>()
                })
            }),
        )
        .track_scroll(self.scroll.clone())
        .size_full();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(header(shell, &theme, cx))
            .child(div().flex_1().min_h_0().w_full().child(list))
            .into_any_element()
    }

    fn scroll_to(&mut self, ix: usize) {
        self.scroll.scroll_to_item(ix, ScrollStrategy::Top);
    }

    fn nav_delta(&self, nav: Nav, shell: &Shell) -> Option<isize> {
        let page = shell.ui().list.visible_rows.saturating_sub(1).max(1) as isize;
        match nav {
            Nav::Up => Some(-1),
            Nav::Down => Some(1),
            Nav::PageUp => Some(-page),
            Nav::PageDown => Some(page),
            Nav::Left | Nav::Right | Nav::Home | Nav::End => None,
        }
    }

    fn reset(&mut self) {
        self.scroll.scroll_to_item_strict(0, ScrollStrategy::Top);
    }
}
