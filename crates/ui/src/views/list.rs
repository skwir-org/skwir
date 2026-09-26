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
/// How much of a cut entry remains visible until it is pasted.
pub const CUT_OPACITY: f32 = 0.45;

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
        // A drag starts clean (`resize_edge` clears the state), so a release
        // missed outside the window cannot leave another edge's start here.
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
        self.store_column_widths();
    }

    fn store_column_widths(&self) {
        self.app.settings.set(
            crate::settings::COLUMN_WIDTHS_KEY,
            toml::Value::Array(
                self.column_widths
                    .iter()
                    .map(|width| toml::Value::Integer(width.round() as i64))
                    .collect(),
            ),
        );
        self.save_settings_later();
    }

    /// `ui.columns`: show or hide one detail column, from the header's menu.
    /// It goes through `settings.apply` like any other preference, so the
    /// dialog and the header always agree and the choice outlives the window.
    pub fn toggle_column(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(index) = crate::settings::COLUMN_KEYS.iter().position(|k| *k == key) else {
            return;
        };
        let shown = !self.columns[index];
        self.columns[index] = shown;
        cx.notify();
        if let Err(error) = self
            .app
            .dispatch(&bibli_core::command::Invocation::with_args(
                "settings.apply",
                &serde_json::json!({ "changes": { key: shown.to_string() } }),
            ))
        {
            self.set_status(format!("{error:#}"), true, cx);
        }
    }

    /// `ui.columns_reset`: back to the widths the view ships with.
    pub fn reset_column_widths(&mut self, cx: &mut Context<Self>) {
        self.column_widths = DEFAULT_WIDTHS;
        self.store_column_widths();
        cx.notify();
    }
}

/// The draggable left edge of a header cell.
fn resize_edge(column: usize, cx: &mut Context<Shell>) -> Stateful<Div> {
    let shell = cx.weak_entity();
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
        .on_drag(ColumnResize(column), move |_, _, _, cx| {
            // The first move of this drag records where it starts.
            shell.update(cx, |shell, _| shell.column_drag = None).ok();
            cx.new(|_| NoDragPreview)
        })
}

/// The row at the top of rows scrolled to `offset_y`, and how many pixels
/// into it the view begins. Rows are `ROW_HEIGHT` tall, from the top.
fn row_anchor(offset_y: f32) -> (usize, f32) {
    let top = (-offset_y).max(0.);
    let row = (top / ROW_HEIGHT).floor();
    (row as usize, top - row * ROW_HEIGHT)
}

/// The offset that shows `anchor` again (the list clamps it to its rows).
fn row_offset((row, within): (usize, f32)) -> f32 {
    -(row as f32 * ROW_HEIGHT + within.clamp(0., ROW_HEIGHT - 1.))
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
    /// Cut and waiting for a paste: drawn dimmed, as the file is on its way out.
    pub cut: bool,
    pub is_cursor: bool,
    pub icon: Option<std::sync::Arc<std::path::Path>>,
    pub hydrated: bool,
    pub tags: Vec<String>,
}

/// Tags from the entry's `tags` metadata field (a list of text values).
pub fn tags_of(e: &bibli_core::Entry) -> Vec<String> {
    match e.metadata.value(keys::TAGS) {
        Some(bibli_core::Value::List(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        Some(bibli_core::Value::Text(t)) => vec![t.to_string()],
        _ => Vec::new(),
    }
}

impl Shell {
    /// Collect row data for `range` of the sorted order. `compact` rows
    /// (the Columns view) show a name only: their details are not computed.
    pub fn rows(&mut self, range: Range<usize>, icon_px: u16, compact: bool) -> Vec<RowData> {
        let mut out = Vec::with_capacity(range.len());
        let friendly = self.prefs.friendly_dates;
        let now = (!compact && friendly && self.columns[1]).then(jiff::Zoned::now);
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
            let mime = (!e.is_dir())
                .then(|| bibli_platform::mime::by_name(&e.name))
                .flatten();
            let icon = if e.is_dir() {
                self.icons.folder(icon_px)
            } else {
                self.icons.for_mime(mime.as_ref(), icon_px)
            };
            let mut row = RowData {
                ix,
                id,
                name: e.name_str().into_owned(),
                is_dir: e.is_dir(),
                path: if e.is_dir() {
                    pane.open.as_ref().and_then(|o| o.path_of(id))
                } else {
                    None
                },
                size: String::new(),
                modified: String::new(),
                modified_exact: String::new(),
                kind: std::sync::Arc::from(""),
                selected: pane.selection.contains(id),
                cut: self.pending.cut && self.pending.entries.contains(&id),
                is_cursor: cursor == Some(id),
                icon,
                hydrated: e.metadata.is_hydrated(),
                tags: tags_of(e),
            };
            if !compact {
                let [size, modified, kind] = self.columns;
                if size {
                    row.size = match e
                        .metadata
                        .value(keys::SIZE)
                        .and_then(bibli_core::Value::as_u64)
                    {
                        Some(s) if !e.is_dir() => format::size(s),
                        _ => "—".to_owned(),
                    };
                }
                if modified {
                    let time = e
                        .metadata
                        .value(keys::MODIFIED)
                        .and_then(bibli_core::Value::as_time);
                    row.modified_exact = time.map_or_else(String::new, format::time);
                    row.modified = match (time, &now) {
                        (None, _) => "—".to_owned(),
                        (Some(time), Some(now)) => format::friendly_time(time, now),
                        (Some(_), None) => row.modified_exact.clone(),
                    };
                }
                if kind {
                    row.kind = if e.is_dir() {
                        "Folder".into()
                    } else {
                        mime.as_ref()
                            .and_then(|mime| self.type_names.get(mime))
                            .unwrap_or_else(|| crate::typenames::fallback(&e.name_str()).into())
                    };
                }
            }
            out.push(row);
        }
        drop(ws);
        self.type_names.request(&app);
        self.icons.request(&app);
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
    let el = div()
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
        .when(r.cut, |d| d.opacity(CUT_OPACITY))
        .child(icon);
    let el = crate::list::entry_mouse(el, pane, ix, cx);
    let name = div()
        .flex()
        .items_center()
        .gap_1()
        .child(
            // A name keeps its end: the extension tells files apart.
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis_middle()
                .child(r.name.clone()),
        )
        .children(r.tags.iter().take(4).map(|t| {
            div()
                .flex_none()
                .px_1()
                .rounded_sm()
                .text_xs()
                .bg(theme.tag_bg)
                .text_color(if r.selected {
                    theme.selection_text
                } else {
                    theme.text
                })
                .child(t.clone())
        }));
    let mut line = line(name, if compact { 0. } else { NAME_MIN });
    if !compact {
        let font = shell.font.clone();
        let exact: gpui::SharedString = r.modified_exact.clone().into();
        let [size, modified, kind] = shell.columns;
        line = line
            .when(size, |d| {
                d.child(
                    column(shell.column_widths[0], muted)
                        .text_right()
                        .child(r.size.clone()),
                )
            })
            .when(modified, |d| {
                d.child(
                    column(shell.column_widths[1], muted)
                        .id(("modified", ix))
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
                    column(shell.column_widths[2], muted)
                        .truncate()
                        .child(gpui::SharedString::from(r.kind.clone())),
                )
            });
    }
    let mut el = el.child(line);
    el = dnd::draggable(el, pane, cx);
    if let Some(p) = &r.path {
        el = dnd::droppable_dir(el, p.clone(), cx);
    }
    el
}

/// The name and the detail columns of a row, or of the header. A wrapping
/// flex line one row tall: a column that no longer fits wraps below and is
/// clipped, so columns drop whole, last first (Type, then Modified, then
/// Size), and the name, the only child that grows, takes what they leave: it
/// never sits at `NAME_MIN` beside the blank of a column that went. Pure
/// layout: nothing is measured, the header and every row agree by
/// construction, and a pane of any width is laid out correctly in the frame
/// it gets that width.
fn line(name: Div, name_min: f32) -> Div {
    div()
        .h(px(ROW_HEIGHT - 2.))
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .flex_row()
        .flex_wrap()
        .content_start()
        .gap_x(px(DETAIL_GAP))
        .child(
            name.h(px(ROW_HEIGHT - 2.))
                .flex()
                .items_center()
                .flex_1()
                .min_w(px(name_min)),
        )
}

/// One detail column of `line`.
fn column(width: f32, color: gpui::Hsla) -> Div {
    div()
        .w(px(width))
        .flex_none()
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
        // Right-click the header to choose columns, where they are.
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.open_column_menu(ev.position, window, cx);
            }),
        )
        .child(div().size(px(ICON_PX)).flex_none())
        .child(
            line(
                div().child(header_cell(
                    "h-name",
                    "Name",
                    SortKey::Name,
                    shell,
                    theme,
                    cx,
                )),
                NAME_MIN,
            )
            .when(shell.columns[0], |d| {
                d.child(
                    column(shell.column_widths[0], theme.text_muted)
                        .relative()
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
                        .child(resize_edge(0, cx)),
                )
            })
            .when(shell.columns[1], |d| {
                d.child(
                    column(shell.column_widths[1], theme.text_muted)
                        .relative()
                        .child(header_cell(
                            "h-mod",
                            "Modified",
                            SortKey::Modified,
                            shell,
                            theme,
                            cx,
                        ))
                        .child(resize_edge(1, cx)),
                )
            })
            .when(shell.columns[2], |d| {
                d.child(
                    column(shell.column_widths[2], theme.text_muted)
                        .relative()
                        .child("Type")
                        .child(resize_edge(2, cx)),
                )
            }),
        )
}

impl PaneView for ListView {
    fn id(&self) -> &'static str {
        crate::view::LIST
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
                    let list = &mut this.ui_mut().list;
                    list.visible_rows = range.len().max(1);
                    list.visible_items = range.len();
                    let rows = this.rows(range, ICON_PX as u16, false);
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
        let marquee = crate::elements::marquee::Marquee {
            pane,
            shell: cx.weak_entity(),
            source: crate::elements::marquee::Source::Rows { height: ROW_HEIGHT },
            count,
            scroll: self.scroll.0.borrow().base_handle.clone(),
            drag: shell.marquee.clone(),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(header(shell, &theme, cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(list)
                    .child(marquee)
                    .child(crate::elements::scrollbar::vertical(
                        ("list-scrollbar", pane.0 as usize),
                        crate::elements::scrollbar::Scrolls::Offset(
                            self.scroll.0.borrow().base_handle.clone(),
                        ),
                    )),
            )
            .into_any_element()
    }

    fn scroll_to(&mut self, ix: usize) {
        self.scroll.scroll_to_item(ix, ScrollStrategy::Top);
    }

    fn scroll_anchor(&self) -> Option<(usize, f32)> {
        Some(row_anchor(f32::from(
            self.scroll.0.borrow().base_handle.offset().y,
        )))
    }

    fn restore_anchor(&mut self, anchor: (usize, f32)) {
        let mut state = self.scroll.0.borrow_mut();
        // A pending scroll (the reset of a new listing) would move it again.
        state.deferred_scroll_to_item = None;
        let offset = state.base_handle.offset();
        state
            .base_handle
            .set_offset(gpui::point(offset.x, px(row_offset(anchor))));
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

#[cfg(test)]
mod tests {
    use super::{ROW_HEIGHT, row_anchor, row_offset};

    #[test]
    fn a_scroll_position_comes_back_to_the_pixel() {
        for offset in [0., -1., -13., -ROW_HEIGHT, -(40. * ROW_HEIGHT + 11.5)] {
            let anchor = row_anchor(offset);
            assert!((row_offset(anchor) - offset).abs() < 1e-3, "{offset}");
        }
        // Half a row down: the row at the top and half of it scrolled.
        let (row, within) = row_anchor(-(3.5 * ROW_HEIGHT));
        assert_eq!(row, 3);
        assert!((within - ROW_HEIGHT / 2.).abs() < 1e-3);
        // An entry that moved keeps how far into its row the view began.
        assert!((row_offset((5, 7.)) - -(5. * ROW_HEIGHT + 7.)).abs() < 1e-3);
        // What no row can hold is clamped into the row.
        assert!((row_offset((2, 99.)) - -(3. * ROW_HEIGHT - 1.)).abs() < 1e-3);
    }
}
