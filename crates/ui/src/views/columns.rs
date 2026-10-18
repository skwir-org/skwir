//! Miller columns: ancestor directories to the left, the pane's listing as
//! the deepest column; for a location that reads as a trail (a tag's,
//! ADR-032), the places of its trail. The ancestors are locations the pane keeps open
//! beside the one it shows (`PaneView::kept_locations`, `App::keep_locations`):
//! the core keeps their listings current and pumps them with the pane's,
//! and going up or down one level adopts a listing already there instead of
//! listing that folder again. A column draws only its visible rows, from
//! the kept listing, so a parent of 366 000 entries costs no copy of them.
//!
//! After the pane's column, the folder under the cursor (`Next`): yazi's
//! preview of it, Finder's next column. It is kept open beside the others,
//! so going into it adopts its listing. Right goes into it, never opens a
//! file, and lands on its first entry (`land`); Left goes up, the folder
//! left selected (the `up` command).
//!
//! Wider than the pane, the columns scroll sideways, as Finder's do, and
//! follow their right end, where the folder shown is (`follow_end`).

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, Context, MouseButton, ScrollHandle, ScrollStrategy, UniformListScrollHandle,
    Window, div, point, prelude::*, px, uniform_list,
};
use skwir_core::location::Trail;
use skwir_core::{EntryId, Location, SortSpec};

use crate::dnd;
use crate::elements::zoom_wheel::{self, Zoomed};
use crate::icons;
use crate::list::Step;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::view::{Nav, PaneView};
use crate::views::list::{row_elements, row_height};

const MAX_ANCESTORS: usize = 3;
const COLUMN_W: f32 = 240.0;
/// How long the cursor rests on a folder before its column is listed: a
/// key held down the column (a repeat every 30 to 40 ms) lists none of the
/// folders it passes.
const SETTLE: Duration = Duration::from_millis(100);

struct Ancestor {
    location: Location,
    /// Name of the child on the way to the pane's directory.
    child: std::ffi::OsString,
    scroll: UniformListScrollHandle,
    /// The row on the way down was scrolled into view once it listed; until
    /// then, the listing version it was last looked for in.
    shown_child: bool,
    looked_at: Option<u64>,
}

/// The column of the folder under the cursor, once the cursor rested on it
/// (`SETTLE`).
#[derive(Default)]
struct Next {
    /// The folder under the cursor at the last frame.
    under: Option<Location>,
    /// The folder the column shows: `under`, once it rested there. Kept open
    /// beside the pane's (`kept_locations`).
    shown: Rc<RefCell<Option<Location>>>,
    scroll: UniformListScrollHandle,
    /// Waits out `SETTLE`, then shows `under`; dropped by the next move.
    settle: Option<gpui::Task<()>>,
}

#[derive(Default)]
pub struct ColumnsView {
    base: Option<Location>,
    ancestors: Vec<Ancestor>,
    scroll: UniformListScrollHandle,
    next: Next,
    /// A folder gone into from the column of the folder under the cursor:
    /// once listed, its first entry is selected, unless something else was
    /// (a row clicked in that column, an arrow pressed meanwhile).
    land: Option<Location>,
    /// The columns scrolled sideways.
    row: ScrollHandle,
    /// Another folder is shown: the strip goes to its end, where it is.
    to_end: Rc<Cell<bool>>,
}

/// The folder or place the cursor of the pane drawn is on: where going into
/// it leads, as Open goes. `None` on a file.
fn folder_under_cursor(shell: &Shell) -> Option<Location> {
    let ws = shell.app.workspace.read();
    let p = ws.pane(shell.pane)?;
    let id = p.selection.cursor?;
    if !p.entry(id)?.is_dir() {
        return None;
    }
    p.location_of(id)
        .or_else(|| p.path_of(id).map(Location::from_path))
}

/// The strip of columns follows its right end, where the folder shown is,
/// while it is there: a folder opened lands at the end, and the end stays
/// in view as the pane narrows (a preview opened). Scrolled away from it,
/// it stays where it was put. Decided before the strip's prepaint, which
/// clamps the offset to the end, whatever the strip's width now.
fn follow_end(row: ScrollHandle, to_end: Rc<Cell<bool>>) -> impl IntoElement {
    gpui::canvas(
        move |_, _, _| {
            // Where the last frame left it, moved since by the wheel or
            // the bar.
            let at_end = row.offset().x <= px(0.5) - row.max_offset().x;
            if to_end.replace(false) || at_end {
                row.set_offset(point(px(-f32::MAX), px(0.)));
            }
        },
        |_, (), _, _| {},
    )
    .absolute()
    .size_0()
}

/// The places above `shown` drawn as columns, farthest first, each with
/// the name of the one on the way down: a folder's parents, or the places
/// of a location's trail (a tag's categories).
fn chain(shown: &Location, trail: Option<&Trail>) -> Vec<(Location, std::ffi::OsString)> {
    if let Some(base) = shown.path() {
        return folders(&base)
            .into_iter()
            .map(|(path, child)| (Location::from_path(path), child))
            .collect();
    }
    let Some(trail) = trail else {
        return Vec::new();
    };
    let crumbs = &trail.crumbs;
    let from = crumbs.len().saturating_sub(MAX_ANCESTORS + 1);
    crumbs
        .windows(2)
        .skip(from)
        .map(|pair| (pair[0].location.clone(), pair[1].label.clone().into()))
        .collect()
}

/// The folders above `base` that are shown as columns, farthest first,
/// each with the name of the child on the way down.
fn folders(base: &Path) -> Vec<(PathBuf, std::ffi::OsString)> {
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
    /// The column of the folder under the cursor follows it: emptied as
    /// the cursor leaves a folder, filled once it has rested on another
    /// (`SETTLE`), when a wake lists it beside the pane's (`kept_locations`).
    /// The folder just `left` going up is there at once: its listing was
    /// kept.
    fn follow_cursor(&mut self, shell: &Shell, left: Option<&Location>, cx: &mut Context<Shell>) {
        let under = folder_under_cursor(shell);
        if under == self.next.under {
            return;
        }
        self.next.under.clone_from(&under);
        self.next.scroll = UniformListScrollHandle::new();
        if under.is_some() && under.as_ref() == left {
            *self.next.shown.borrow_mut() = under;
            self.next.settle = None;
            return;
        }
        *self.next.shown.borrow_mut() = None;
        self.next.settle = under.map(|under| {
            let shown = Rc::clone(&self.next.shown);
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SETTLE).await;
                *shown.borrow_mut() = Some(under);
                let _ = this.update(cx, |_, cx| cx.notify());
            })
        });
    }

    /// The ancestors of `base`, nearest last. Those already shown (going one
    /// level down or up keeps most of them) keep their scroll.
    fn rebuild(&mut self, base: &Location, trail: Option<&Trail>) {
        let mut old: Vec<Ancestor> = std::mem::take(&mut self.ancestors);
        for (location, child) in chain(base, trail) {
            if let Some(i) = old.iter().position(|a| a.location == location) {
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
                location,
                child,
                scroll: UniformListScrollHandle::new(),
                shown_child: false,
                looked_at: None,
            });
        }
        self.base = Some(base.clone());
    }
}

/// Which column beside the pane's own.
enum Side {
    /// The `k`th folder above, farthest first: the row on the way down is
    /// marked, and a press goes where a row leads.
    Above(usize),
    /// The folder under the cursor: a press goes into it, the row pressed
    /// selected there, as a click in Finder's next column.
    Next(Location),
}

/// One row of a column beside the pane's, read from its listing as it is
/// drawn.
struct SideRow {
    id: EntryId,
    name: std::ffi::OsString,
    label: gpui::SharedString,
    is_dir: bool,
    icon: Option<icons::Pic>,
    link: icons::Link,
    /// Where it leads, for a place (a tag's), and its tag's color.
    place: Option<(Location, Option<usize>)>,
}

/// A column beside the pane's, a list box named `title` for assistive
/// technology (`None` while it does not listen).
fn side_column(
    view: &ColumnsView,
    side: Side,
    ids: Arc<[EntryId]>,
    pane: skwir_core::workspace::PaneId,
    title: Option<gpui::SharedString>,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let (location, child, scroll, k) = match side {
        Side::Above(k) => {
            let a = &view.ancestors[k];
            (
                a.location.clone(),
                Some(a.child.clone()),
                a.scroll.clone(),
                k,
            )
        }
        Side::Next(location) => (
            location,
            None,
            view.next.scroll.clone(),
            view.ancestors.len(),
        ),
    };
    let next = child.is_none();
    let path = location.path();
    let ids_of = |name: &'static str| (name, k);
    // What the rows read their listing by, in the closure that draws them.
    let at = location.clone();
    let count = ids.len();
    let list = uniform_list(
        ids_of(if next { "column-next" } else { "column" }),
        ids.len(),
        cx.processor(move |this: &mut Shell, range: Range<usize>, _window, cx| {
            let theme = cx.global::<Theme>().clone();
            let icon_px = u16::try_from(this.row_zoom).unwrap_or(u16::MAX);
            let height = row_height(this.row_zoom);
            // Only the rows drawn are read from the kept listing.
            let rows: Vec<SideRow> = {
                let ws = this.app.workspace.read();
                let listing = ws.pane(pane).and_then(|p| p.kept(&at));
                let p = ws.pane(pane);
                ids[range.clone()]
                    .iter()
                    .filter_map(|id| listing?.get(*id))
                    .map(|e| SideRow {
                        id: e.id,
                        label: e.name_str().into_owned().into(),
                        name: e.name.clone(),
                        is_dir: e.is_dir(),
                        icon: this.icons.for_entry(
                            e,
                            || path.as_ref().map(|dir| dir.join(&e.name)),
                            icon_px,
                        ),
                        link: icons::Link::of(e),
                        place: p.and_then(|p| p.kept_location_of(&at, e.id)).map(|leads| {
                            let tag = e
                                .metadata
                                .value(skwir_core::entry::keys::PLACE_TAG)
                                .and_then(skwir_core::Value::as_str);
                            (leads, tag.map(|tag| this.tag_colors.of(tag)))
                        }),
                    })
                    .collect()
            };
            let a11y = this.a11y;
            let mut out = Vec::with_capacity(rows.len());
            for (ix, row) in range.zip(rows) {
                let target = path.as_ref().map(|dir| dir.join(&row.name));
                let is_on_path = child.as_ref().is_some_and(|child| row.name == *child);
                let mark =
                    icons::Mark::of(row.link, row.place.as_ref().and_then(|(_, color)| *color));
                let icon = icons::entry_picture(
                    row.icon.as_ref(),
                    row.is_dir,
                    f32::from(icon_px),
                    mark,
                    &theme,
                );
                // Where a press goes: into the folder under the cursor, the
                // row selected there; from a column above, where the row
                // leads (a folder, or where a place leads).
                let press = if next {
                    Some(serde_json::json!({
                        "location": at.arg(),
                        "select": skwir_core::os_path::PathArg::from(Path::new(&row.name)),
                        "pane": pane,
                    }))
                } else {
                    match (&row.place, &target) {
                        (Some((place, _)), _) => Some(serde_json::json!(place.arg())),
                        (None, Some(target)) if row.is_dir => Some(serde_json::json!(
                            skwir_core::os_path::PathArg::from(target.as_path())
                        )),
                        _ => None,
                    }
                    .map(|leads| serde_json::json!({ "location": leads, "pane": pane }))
                };
                // The folder the pane went into from here is this column's
                // selected option.
                let mut el = div()
                    .id(ix)
                    .role(gpui::Role::ListBoxOption)
                    .when(a11y, |d| {
                        d.aria_label(row.label.clone())
                            .aria_selected(is_on_path)
                            .aria_position_in_set(ix)
                    })
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
                    // Cut, it is dimmed here as in the pane's own column.
                    .when(
                        this.pending.cut && this.pending.holds(row.id.file, &row.name),
                        |d| d.opacity(crate::views::list::CUT_OPACITY),
                    )
                    // Not the pane's entry, so not its selection's menu: a
                    // right press here opens nothing, rather than the menu
                    // of the folder shown.
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.activate_pane(pane, cx);
                            if let Some(press) = &press {
                                this.run("navigate", press.clone(), window, cx);
                            }
                        }),
                    )
                    .child(icon)
                    .child(crate::elements::clip::clipped(
                        div()
                            .id(("name", ix))
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_middle(),
                        row.label.clone(),
                        this.font.clone(),
                    ))
                    .when(row.is_dir, |d| {
                        d.child(div().text_color(theme.text_muted).child("›"))
                    });
                // A tag dropped on it is its own: these are not the
                // pane's entries, so never its selection.
                if let Some(target) = &target
                    && row.place.is_none()
                {
                    el = dnd::takes_gift_for(
                        el,
                        skwir_core::op::EntryRef::new(row.id.file, target.clone()),
                        cx,
                    );
                }
                match (row.place, target) {
                    (Some((place, _)), _) => el = dnd::droppable_place(el, place, cx),
                    (None, Some(target)) if row.is_dir => {
                        el = dnd::droppable_dir(el, target, cx);
                    }
                    _ => {}
                }
                out.push(el);
            }
            let app = Arc::clone(&this.app);
            this.icons.request(&app);
            out
        }),
    )
    .track_scroll(&scroll)
    .size_full();
    let mut col = div()
        .id(ids_of(if next { "colwrap-next" } else { "colwrap" }))
        .role(gpui::Role::ListBox)
        .when_some(title, |d, title| {
            d.aria_label(title).aria_size_of_set(count)
        })
        .relative()
        .w(px(COLUMN_W))
        .h_full()
        .flex_none()
        .border_r_1()
        .border_color(theme.border)
        .child(list)
        .child(
            crate::elements::scrollbar::vertical(
                ids_of(if next {
                    "column-scrollbar-next"
                } else {
                    "column-scrollbar"
                }),
                crate::elements::scrollbar::Scrolls::Offset(scroll.0.borrow().base_handle.clone()),
            )
            .across(view.row.clone()),
        );
    col = match location.path() {
        Some(dir) => dnd::droppable_dir(col, dir, cx),
        None => dnd::droppable_place(col, location, cx),
    };
    col.into_any_element()
}

impl PaneView for ColumnsView {
    fn zooms(&self) -> crate::view::Zooms {
        crate::view::Zooms::Rows
    }

    fn id(&self) -> &'static str {
        crate::view::COLUMNS
    }

    fn render(
        &mut self,
        shell: &mut Shell,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) -> AnyElement {
        let theme = cx.global::<Theme>().clone();
        let pane = shell.pane;
        let (base, spec, cursor, complete) = {
            let ws = shell.app.workspace.read();
            let p = ws.pane(pane);
            (
                p.and_then(|p| p.location().cloned()),
                p.map_or_else(SortSpec::default, |p| p.sort),
                p.and_then(|p| p.selection.cursor),
                p.is_some_and(|p| p.listing.is_complete()),
            )
        };
        let mut left = None;
        if base != self.base {
            // Gone into the folder under the cursor (Right, a double click,
            // Open): its first entry is the cursor's once it is listed.
            self.land = base
                .clone()
                .filter(|base| self.next.under.as_ref() == Some(base));
            left = self.base.take();
            if let Some(b) = &base {
                let trail = shell.ui().place.trail.clone();
                self.rebuild(b, trail.as_ref());
            } else {
                self.ancestors.clear();
                self.base = None;
            }
            self.to_end.set(true);
        }
        if self.land.is_some() && (cursor.is_some() || complete) {
            // Something chose first (a row pressed in that column, an
            // arrow), or the listing is complete.
            if self.land.take().is_some() && cursor.is_none() && !shell.ui().list.sorted.is_empty()
            {
                cx.defer_in(window, move |shell, window, cx| {
                    shell.with_pane(pane, |shell| shell.cursor_to(0, Step::Select, window, cx));
                });
            }
        }
        self.follow_cursor(shell, left.as_ref(), cx);
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
                    // Looked for again only when its names change: every
                    // batch of metadata moved the listing's version.
                    if !a.shown_child && a.looked_at != Some(listing.names_version()) {
                        a.looked_at = Some(listing.names_version());
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

        // The column of the folder under the cursor, once listed beside.
        let next = self.next.shown.borrow().clone().map(|next| {
            let ids = {
                let ws = shell.app.workspace.read();
                ws.pane(pane)
                    .and_then(|p| p.kept(&next))
                    .map_or_else(|| Arc::from([]), |listing| listing.sorted(spec))
            };
            (next, ids)
        });

        // The columns from the pane's left; wider than the pane, they scroll
        // sideways and follow their end (`follow_end`).
        let mut columns = div()
            .id(("columns", pane.0 as usize))
            .size_full()
            .flex()
            .flex_row()
            .overflow_x_scroll()
            .track_scroll(&self.row);
        // Each column a list box named by its folder, for assistive
        // technology alone.
        let title = |location: &Location| {
            shell
                .a11y
                .then(|| gpui::SharedString::from(shell.app.location_title(location)))
        };
        for (k, ids) in orders.into_iter().enumerate() {
            let named = title(&self.ancestors[k].location);
            columns = columns.child(side_column(
                self,
                Side::Above(k),
                ids,
                pane,
                named,
                &theme,
                cx,
            ));
        }
        let count = shell.ui().list.sorted.len();
        let last = uniform_list(
            "column-last",
            count,
            cx.processor(move |this: &mut Shell, range: Range<usize>, _window, cx| {
                this.with_pane(pane, |this| {
                    let theme = cx.global::<Theme>().clone();
                    this.ui_mut().list.visible_rows = range.len().max(1);
                    let icon_px = u16::try_from(this.row_zoom).unwrap_or(u16::MAX);
                    let rows = this.rows(range.clone(), icon_px, true);
                    this.request_hydrate(rows.iter().map(|r| r.look.id).collect::<Vec<_>>());
                    row_elements(this, &theme, range, &rows, (true, false), cx)
                })
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        let mut last_col = div()
            .id("column-last-wrap")
            .role(gpui::Role::ListBox)
            .when(shell.a11y, |d| {
                d.aria_label(shell.ui().place.title.clone())
                    .aria_size_of_set(count)
            })
            .relative()
            .w(px(COLUMN_W * 1.25))
            .h_full()
            .flex_none()
            .border_r_1()
            .border_color(theme.border)
            .child(last)
            .child(
                crate::elements::scrollbar::vertical(
                    ("column-scrollbar-last", pane.0 as usize),
                    crate::elements::scrollbar::Scrolls::Offset(
                        self.scroll.0.borrow().base_handle.clone(),
                    ),
                )
                .across(self.row.clone()),
            );
        match base.as_ref().map(|b| (b.path(), b)) {
            Some((Some(dir), _)) => last_col = dnd::droppable_dir(last_col, dir, cx),
            Some((None, place)) => last_col = dnd::droppable_place(last_col, place.clone(), cx),
            None => {}
        }
        columns = columns.child(last_col);
        // Always there, empty on a file or while the cursor moves on, so the
        // columns never shift as it goes.
        columns = columns.child(match next {
            Some((next, ids)) => {
                let named = title(&next);
                side_column(self, Side::Next(next), ids, pane, named, &theme, cx)
            }
            None => div()
                .w(px(COLUMN_W))
                .h_full()
                .flex_none()
                .into_any_element(),
        });
        div()
            .relative()
            .size_full()
            // Before the strip, which it moves.
            .child(follow_end(self.row.clone(), Rc::clone(&self.to_end)))
            .child(columns)
            .child(crate::elements::scrollbar::horizontal(
                ("columns-bar", pane.0 as usize),
                self.row.clone(),
            ))
            // Ctrl and the wheel size the rows, over any column.
            .child(zoom_wheel::overlay(Zoomed::View, cx))
            .into_any_element()
    }

    fn scroll_to(&mut self, ix: usize) {
        self.scroll.scroll_to_item(ix, ScrollStrategy::Top);
    }

    fn kept_locations(&self, shown: Option<&Location>, trail: Option<&Trail>) -> Vec<Location> {
        let Some(shown) = shown else {
            return Vec::new();
        };
        let mut kept: Vec<Location> = chain(shown, trail)
            .into_iter()
            .map(|(location, _)| location)
            .collect();
        // The folder under the cursor, never the one shown: the pane has
        // just gone into it, and adopted its listing.
        if let Some(next) = self.next.shown.borrow().as_ref()
            && next != shown
            && !kept.contains(next)
        {
            kept.push(next.clone());
        }
        // Just gone up a folder (the view has not drawn since): the one
        // left, which `up` puts the cursor on, keeps its listing for the
        // column of the folder under the cursor.
        if let Some(left) = self.base.as_ref().filter(|left| {
            left.path()
                .and_then(|path| path.parent().map(Location::from_path))
                .as_ref()
                == Some(shown)
        }) && !kept.contains(left)
        {
            kept.push(left.clone());
        }
        kept
    }

    fn nav_delta(&self, nav: Nav, shell: &Shell) -> Option<isize> {
        crate::views::list::rows_nav(nav, shell)
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
            // Into the folder under the cursor, as yazi's `l` and Finder's
            // Right: a file is not opened (Enter opens).
            Nav::Right => {
                if let Some(folder) = folder_under_cursor(shell) {
                    let pane = shell.pane;
                    shell.run(
                        "navigate",
                        serde_json::json!({ "location": folder.arg(), "pane": pane }),
                        window,
                        cx,
                    );
                }
                true
            }
            _ => false,
        }
    }

    fn reset(&mut self) {
        self.scroll.scroll_to_item_strict(0, ScrollStrategy::Top);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::location::Crumb;

    #[test]
    fn a_trail_is_walked_in_columns_and_a_folder_by_its_parents() {
        let crumb = |label: &str, rest: &str| Crumb::new(label, Location::new("tags", rest));
        let trail = Trail {
            base: Some("/home/someone".into()),
            crumbs: vec![
                crumb("Tags", "/"),
                crumb("Places", "/Places"),
                crumb("France", "/Places/France"),
                crumb("Paris", "/Places/France/Paris"),
                crumb("Old town", "/Places/France/Paris/Old town"),
            ],
            filters: Vec::new(),
            toggles: Vec::new(),
            holds_places: true,
            one_folder: false,
        };
        let shown = trail.crumbs[4].location.clone();
        let columns: Vec<(String, String)> = chain(&shown, Some(&trail))
            .into_iter()
            .map(|(l, child)| (l.rest, child.to_string_lossy().into_owned()))
            .collect();
        assert_eq!(
            columns,
            [
                ("/Places".to_owned(), "France".to_owned()),
                ("/Places/France".to_owned(), "Paris".to_owned()),
                ("/Places/France/Paris".to_owned(), "Old town".to_owned()),
            ],
            "the nearest places, each with the one on the way down"
        );
        assert!(chain(&shown, None).is_empty(), "no trail, no columns");
        let folder = Location::from_path("/a/b/c");
        let parents: Vec<Location> = chain(&folder, None).into_iter().map(|(l, _)| l).collect();
        assert_eq!(
            parents,
            [
                Location::from_path("/"),
                Location::from_path("/a"),
                Location::from_path("/a/b")
            ]
        );
    }
}
