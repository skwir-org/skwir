//! Miller columns: ancestor directories to the left, the pane's listing as
//! the deepest column. The ancestors are locations the pane keeps open
//! beside the one it shows (`PaneView::kept_locations`, `App::keep_locations`):
//! the core keeps their listings current and pumps them with the pane's,
//! and going up or down one level adopts a listing already there instead of
//! listing that folder again. A column draws only its visible rows, from
//! the kept listing, so a parent of 366 000 entries costs no copy of them.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bibli_core::os_path::PathArg;
use bibli_core::{EntryId, Location, SortSpec};
use gpui::{
    AnyElement, Context, MouseButton, ScrollStrategy, UniformListScrollHandle, Window, div,
    prelude::*, px, uniform_list,
};

use crate::dnd;
use crate::icons;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::view::{Nav, PaneView};
use crate::views::list::{row, row_height};

const MAX_ANCESTORS: usize = 3;
const COLUMN_W: f32 = 240.0;

struct Ancestor {
    path: PathBuf,
    location: Location,
    /// Name of the child on the way to the pane's directory.
    child: std::ffi::OsString,
    scroll: UniformListScrollHandle,
    /// The row on the way down was scrolled into view once it listed; until
    /// then, the listing version it was last looked for in.
    shown_child: bool,
    looked_at: Option<u64>,
}

#[derive(Default)]
pub struct ColumnsView {
    base: Option<PathBuf>,
    ancestors: Vec<Ancestor>,
    scroll: UniformListScrollHandle,
}

/// The folders above `base` that are shown as columns, farthest first,
/// each with the name of the child on the way down.
fn chain(base: &Path) -> Vec<(PathBuf, std::ffi::OsString)> {
    let mut chain = Vec::new();
    let mut cur = base.to_path_buf();
    while let Some(parent) = cur.parent().map(Path::to_path_buf) {
        let child = cur.file_name().map(ToOwned::to_owned).unwrap_or_default();
        chain.push((parent.clone(), child));
        cur = parent;
        if chain.len() >= MAX_ANCESTORS {
            break;
        }
    }
    chain.reverse();
    chain
}

impl ColumnsView {
    /// The ancestors of `base`, nearest last. Those already shown (going one
    /// level down or up keeps most of them) keep their scroll.
    fn rebuild(&mut self, base: &Path) {
        let mut old: Vec<Ancestor> = std::mem::take(&mut self.ancestors);
        for (path, child) in chain(base) {
            if let Some(i) = old.iter().position(|a| a.path == path) {
                let mut kept = old.swap_remove(i);
                if kept.child != child {
                    kept.child = child;
                    kept.shown_child = false;
                    kept.looked_at = None;
                }
                self.ancestors.push(kept);
                continue;
            }
            self.ancestors.push(Ancestor {
                location: Location::from_path(&path),
                path,
                child,
                scroll: UniformListScrollHandle::new(),
                shown_child: false,
                looked_at: None,
            });
        }
        self.base = Some(base.to_path_buf());
    }
}

/// One row of an ancestor column, read from its listing as it is drawn.
struct AncestorRow {
    name: std::ffi::OsString,
    label: gpui::SharedString,
    is_dir: bool,
    icon: Option<Arc<std::path::Path>>,
    link: icons::Link,
}

fn ancestor_column(
    view: &ColumnsView,
    k: usize,
    ids: Arc<[EntryId]>,
    pane: bibli_core::workspace::PaneId,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let a = &view.ancestors[k];
    let path = a.path.clone();
    let child = a.child.clone();
    let location = a.location.clone();
    let list = uniform_list(
        ("column", k),
        ids.len(),
        cx.processor(move |this: &mut Shell, range: Range<usize>, _window, cx| {
            let theme = cx.global::<Theme>().clone();
            let icon_px = u16::try_from(this.row_zoom).unwrap_or(u16::MAX);
            let height = row_height(this.row_zoom);
            // Only the rows drawn are read from the kept listing.
            let rows: Vec<AncestorRow> = {
                let ws = this.app.workspace.read();
                let listing = ws.pane(pane).and_then(|p| p.kept(&location));
                ids[range.clone()]
                    .iter()
                    .filter_map(|id| listing?.get(*id))
                    .map(|e| AncestorRow {
                        label: e.name_str().into_owned().into(),
                        name: e.name.clone(),
                        is_dir: e.is_dir(),
                        icon: this.icons.for_entry(e, icon_px),
                        link: icons::Link::of(e),
                    })
                    .collect()
            };
            let mut out = Vec::with_capacity(rows.len());
            for (ix, row) in range.zip(rows) {
                let target = path.join(&row.name);
                let is_on_path = row.name == child;
                let icon: AnyElement = match row.icon {
                    Some(p) => icons::element(&p, f32::from(icon_px)),
                    None => div()
                        .size(px(f32::from(icon_px)))
                        .rounded_sm()
                        .bg(theme.text_muted)
                        .into_any_element(),
                };
                let icon = icons::with_link(icon, f32::from(icon_px), row.link, &theme);
                let dir_target = row.is_dir.then(|| target.clone());
                let mut el = div()
                    .id(ix)
                    .h(px(height))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .when(is_on_path, |d| {
                        d.bg(theme.selection_bg).text_color(theme.selection_text)
                    })
                    .when(!is_on_path, |d| d.hover(|s| s.bg(theme.hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.activate_pane(pane, cx);
                            if let Some(t) = &dir_target {
                                this.run(
                                    "navigate",
                                    serde_json::json!({
                                        "location": PathArg::from(t.as_path()),
                                        "pane": pane,
                                    }),
                                    window,
                                    cx,
                                );
                            }
                        }),
                    )
                    .child(icon)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_middle()
                            .child(row.label.clone()),
                    )
                    .when(row.is_dir, |d| {
                        d.child(div().text_color(theme.text_muted).child("›"))
                    });
                if row.is_dir {
                    el = dnd::droppable_dir(el, target, cx);
                }
                out.push(el);
            }
            let app = Arc::clone(&this.app);
            this.icons.request(&app);
            out
        }),
    )
    .track_scroll(a.scroll.clone())
    .size_full();
    let mut col = div()
        .id(("colwrap", k))
        .relative()
        .w(px(COLUMN_W))
        .h_full()
        .flex_none()
        .border_r_1()
        .border_color(theme.border)
        .child(list)
        .child(crate::elements::scrollbar::vertical(
            ("column-scrollbar", k),
            crate::elements::scrollbar::Scrolls::Offset(a.scroll.0.borrow().base_handle.clone()),
        ));
    col = dnd::droppable_dir(col, a.path.clone(), cx);
    col.into_any_element()
}

impl PaneView for ColumnsView {
    fn id(&self) -> &'static str {
        crate::view::COLUMNS
    }

    fn render(&mut self, shell: &mut Shell, _: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
        let theme = cx.global::<Theme>().clone();
        let (base, spec) = {
            let ws = shell.app.workspace.read();
            let p = ws.pane(shell.pane);
            (
                p.and_then(|p| p.location().and_then(Location::path)),
                p.map_or_else(SortSpec::default, |p| p.sort),
            )
        };
        if base != self.base {
            if let Some(b) = &base {
                self.rebuild(b);
            } else {
                self.ancestors.clear();
                self.base = None;
            }
        }
        // Each column's order, from the kept listing; the child on the way
        // down is scrolled into view once, when it has listed.
        let orders: Vec<Arc<[EntryId]>> = {
            let ws = shell.app.workspace.read();
            let pane = ws.pane(shell.pane);
            self.ancestors
                .iter_mut()
                .map(|a| {
                    let Some(listing) = pane.and_then(|p| p.kept(&a.location)) else {
                        return Arc::from([]);
                    };
                    let ids = listing.sorted(spec);
                    if !a.shown_child && a.looked_at != Some(listing.version()) {
                        a.looked_at = Some(listing.version());
                        if let Some(ix) = ids
                            .iter()
                            .position(|id| listing.get(*id).is_some_and(|e| e.name == a.child))
                        {
                            a.scroll.scroll_to_item(ix, ScrollStrategy::Center);
                            a.shown_child = true;
                        }
                    }
                    ids
                })
                .collect()
        };

        let mut columns = div()
            .relative()
            .size_full()
            .flex()
            .flex_row()
            .overflow_hidden();
        for (k, ids) in orders.into_iter().enumerate() {
            columns = columns.child(ancestor_column(self, k, ids, shell.pane, &theme, cx));
        }
        let count = shell.ui().list.sorted.len();
        let pane = shell.pane;
        let last = uniform_list(
            "column-last",
            count,
            cx.processor(move |this: &mut Shell, range: Range<usize>, _window, cx| {
                this.with_pane(pane, |this| {
                    let theme = cx.global::<Theme>().clone();
                    this.ui_mut().list.visible_rows = range.len().max(1);
                    let icon_px = u16::try_from(this.row_zoom).unwrap_or(u16::MAX);
                    let rows = this.rows(range, icon_px, true);
                    let need: Vec<EntryId> =
                        rows.iter().filter(|r| !r.hydrated).map(|r| r.id).collect();
                    this.request_hydrate(need);
                    rows.iter()
                        .map(|r| row(this, &theme, r, true, false, cx))
                        .collect::<Vec<_>>()
                })
            }),
        )
        .track_scroll(self.scroll.clone())
        .size_full();
        let mut last_col = div()
            .id("column-last-wrap")
            .relative()
            .w(px(COLUMN_W * 1.25))
            .h_full()
            .flex_none()
            .border_r_1()
            .border_color(theme.border)
            .child(last)
            .child(crate::elements::scrollbar::vertical(
                ("column-scrollbar-last", pane.0 as usize),
                crate::elements::scrollbar::Scrolls::Offset(
                    self.scroll.0.borrow().base_handle.clone(),
                ),
            ));
        if let Some(b) = &base {
            last_col = dnd::droppable_dir(last_col, b.clone(), cx);
        }
        columns = columns.child(last_col);
        // Ctrl and the wheel size the rows, over any column.
        columns
            .child(crate::views::zoom_wheel(cx))
            .into_any_element()
    }

    fn scroll_to(&mut self, ix: usize) {
        self.scroll.scroll_to_item(ix, ScrollStrategy::Top);
    }

    fn kept_locations(&self, shown: Option<&Location>) -> Vec<Location> {
        shown
            .and_then(Location::path)
            .map(|base| {
                chain(&base)
                    .into_iter()
                    .map(|(path, _)| Location::from_path(&path))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn nav_delta(&self, nav: Nav, shell: &Shell) -> Option<isize> {
        let rows = shell.ui().list.visible_rows;
        let page = rows.saturating_sub(1).max(1) as isize;
        let half = (rows / 2).max(1) as isize;
        match nav {
            Nav::Up => Some(-1),
            Nav::Down => Some(1),
            Nav::PageUp => Some(-page),
            Nav::PageDown => Some(page),
            Nav::HalfPageUp => Some(-half),
            Nav::HalfPageDown => Some(half),
            _ => None,
        }
    }

    fn nav(
        &mut self,
        nav: Nav,
        shell: &mut Shell,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) -> bool {
        match nav {
            Nav::Left => {
                shell.run("up", serde_json::json!({}), window, cx);
                true
            }
            Nav::Right => {
                shell.open_selection(window, cx);
                true
            }
            _ => false,
        }
    }

    fn reset(&mut self) {
        self.scroll.scroll_to_item_strict(0, ScrollStrategy::Top);
    }
}
