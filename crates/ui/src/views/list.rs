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
/// The name never gets narrower than this; detail columns give way first.
const NAME_MIN: f32 = 150.0;
/// Size, Modified, Type.
pub const DEFAULT_WIDTHS: [f32; 3] = [80.0, 136.0, 150.0];
const DETAIL_GAP: f32 = 12.0;

pub fn clamp_width(width: f32) -> f32 {
    width.clamp(56.0, 360.0)
}

/// Payload of a header edge drag.
#[derive(Clone)]
pub struct ColumnResize(pub usize);

/// Nothing follows the pointer while an edge is dragged.
struct NoDragPreview;
impl gpui::Render for NoDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl Shell {
    /// The columns are right-aligned, so a column grows as its left edge
    /// moves left; the name takes what is left.
    pub fn on_column_drag(
        &mut self,
        ev: &gpui::DragMoveEvent<ColumnResize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let x = f32::from(ev.event.position.x);
        let column = ev.drag(cx).0;
        let (_, start_x, start_width) =
            *self
                .column_drag
                .get_or_insert((column, x, self.column_widths[column]));
        self.column_widths[column] = clamp_width(start_width + (start_x - x));
        cx.notify();
    }

    /// The drag ended: remember the widths. Saved on a worker.
    pub fn finish_column_drag(&mut self) {
        if self.column_drag.take().is_none() {
            return;
        }
        self.app.settings.set(
            crate::settings::COLUMN_WIDTHS_KEY,
            toml::Value::Array(
                self.column_widths
                    .iter()
                    .map(|width| toml::Value::Integer(width.round() as i64))
                    .collect(),
            ),
        );
        let weak = self.app.weak();
        self.app.scheduler.spawn(
            bibli_core::scheduler::Priority::Background,
            "column-widths-save",
            self.app.lifetime().child(),
            move || {
                if let Some(app) = weak.upgrade()
                    && let Err(error) = app.settings.save()
                {
                    tracing::warn!("cannot save column widths: {error:#}");
                }
            },
        );
    }
}

/// The draggable left edge of a header cell.
fn resize_edge(column: usize) -> Stateful<Div> {
    div()
        .id(("column-edge", column))
        .absolute()
        .top_0()
        // Inside the cell: the columns' container clips what leaves it.
        .left_0()
        .w(px(7.))
        .h_full()
        .cursor(gpui::CursorStyle::ResizeLeftRight)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_drag(ColumnResize(column), |_, _, _, cx| {
            cx.new(|_| NoDragPreview)
        })
}

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
    /// Exact timestamp, shown as the date's tooltip.
    pub modified_exact: String,
    pub kind: std::sync::Arc<str>,
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
        let friendly = self.friendly_dates;
        let now = jiff::Zoned::now();
        let app = std::sync::Arc::clone(&self.app);
        let ws = app.workspace.read();
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
            let time = e
                .metadata
                .value(keys::MODIFIED)
                .and_then(bibli_core::Value::as_time);
            let modified_exact = time.map_or_else(String::new, format::time);
            let modified = match time {
                None => "—".to_owned(),
                Some(time) if friendly => format::friendly_time(time, &now),
                Some(_) => modified_exact.clone(),
            };
            let kind: std::sync::Arc<str> = if e.is_dir() {
                "Folder".into()
            } else {
                bibli_platform::mime::by_name(&e.name)
                    .and_then(|mime| self.type_names.get(&mime))
                    .unwrap_or_else(|| crate::typenames::fallback(&e.name_str()).into())
            };
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
                modified_exact,
                kind,
                selected: pane.selection.contains(id),
                is_cursor: cursor == Some(id),
                icon: self.icons.for_entry(e, icon_px),
                hydrated: e.metadata.is_hydrated(),
                tags: tags_of(e),
            });
        }
        drop(ws);
        self.type_names.request(&app);
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
        .when(!r.selected, |d| d.hover(|s| s.bg(theme.hover)))
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
                .min_w(px(if compact { 0. } else { NAME_MIN }))
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
        let font = shell.font.clone();
        let exact: gpui::SharedString = r.modified_exact.clone().into();
        let [size, modified, kind] = shell.columns;
        el = el.child(
            details(muted, shell.columns, shell.column_widths)
                .when(size, |d| {
                    d.child(
                        div()
                            .w(px(shell.column_widths[0]))
                            .flex_none()
                            .text_right()
                            .child(r.size.clone()),
                    )
                })
                .when(modified, |d| {
                    d.child(
                        div()
                            .id(("modified", ix))
                            .w(px(shell.column_widths[1]))
                            .flex_none()
                            .truncate()
                            .when(!exact.is_empty(), |d| {
                                d.tooltip(move |_, cx| {
                                    crate::toolbar::tooltip(exact.clone(), font.clone(), cx)
                                })
                            })
                            .child(r.modified.clone()),
                    )
                })
                .when(kind, |d| {
                    d.child(
                        div()
                            .w(px(shell.column_widths[2]))
                            .flex_none()
                            .truncate()
                            .child(gpui::SharedString::from(r.kind.clone())),
                    )
                }),
        );
    }
    el = dnd::draggable(el, shell);
    if let Some(p) = &r.path {
        el = dnd::droppable_dir(el, p.clone(), cx);
    }
    el
}

/// The detail columns of a row or of the header. A wrapping flex line one
/// row tall: a column that no longer fits wraps below and is clipped, so
/// columns drop whole, last first (Type, then Modified, then Size), while the
/// name keeps `NAME_MIN`. Pure layout: nothing is measured, the header and
/// every row agree by construction, and a pane of any width is laid out
/// correctly in the frame it gets that width.
fn details(color: gpui::Hsla, shown: [bool; 3], widths: [f32; 3]) -> Div {
    let count = shown.iter().filter(|shown| **shown).count();
    let basis: f32 = widths
        .iter()
        .zip(shown)
        .filter_map(|(width, shown)| shown.then_some(*width))
        .sum::<f32>()
        + DETAIL_GAP * count.saturating_sub(1) as f32;
    div()
        .h(px(ROW_HEIGHT - 2.))
        .flex_shrink()
        .flex_basis(px(basis))
        .min_w_0()
        .overflow_hidden()
        .flex()
        .flex_row()
        .flex_wrap()
        .justify_end()
        .items_center()
        .gap_x(px(DETAIL_GAP))
        .line_height(px(ROW_HEIGHT - 2.))
        .text_color(color)
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
        // Right-click the header to choose columns.
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                this.run(
                    "ui.settings",
                    serde_json::json!({ "key": crate::settings::COLUMN_KEYS[0] }),
                    window,
                    cx,
                );
            }),
        )
        .child(div().size(px(ICON_PX)).flex_none())
        .child(div().flex_1().min_w(px(NAME_MIN)).child(header_cell(
            "h-name",
            "Name",
            SortKey::Name,
            shell,
            theme,
            cx,
        )))
        .child(
            details(theme.text_muted, shell.columns, shell.column_widths)
                .when(shell.columns[0], |d| {
                    d.child(
                        div()
                            .relative()
                            .w(px(shell.column_widths[0]))
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
                            ))
                            .child(resize_edge(0)),
                    )
                })
                .when(shell.columns[1], |d| {
                    d.child(
                        div()
                            .relative()
                            .w(px(shell.column_widths[1]))
                            .flex_none()
                            .child(header_cell(
                                "h-mod",
                                "Modified",
                                SortKey::Modified,
                                shell,
                                theme,
                                cx,
                            ))
                            .child(resize_edge(1)),
                    )
                })
                .when(shell.columns[2], |d| {
                    d.child(
                        div()
                            .relative()
                            .w(px(shell.column_widths[2]))
                            .flex_none()
                            .child("Type")
                            .child(resize_edge(2)),
                    )
                }),
        )
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
