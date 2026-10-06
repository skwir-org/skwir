//! Miller columns: ancestor directories to the left, the pane's listing as
//! the deepest column; for a location that reads as a trail (a tag's,
//! ADR-032), the places of its trail. The ancestors are locations the pane keeps open
//! beside the one it shows (`PaneView::kept_locations`, `App::keep_locations`):
//! the core keeps their listings current and pumps them with the pane's,
//! and going up or down one level adopts a listing already there instead of
//! listing that folder again. A column draws only its visible rows, from
//! the kept listing, so a parent of 366 000 entries costs no copy of them.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bibli_core::location::Trail;
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
    location: Location,
    /// The folder it is, for a folder's column: its rows' paths are in it.
    path: Option<PathBuf>,
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
    base: Option<Location>,
    ancestors: Vec<Ancestor>,
    scroll: UniformListScrollHandle,
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
                path: location.path(),
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

/// One row of an ancestor column, read from its listing as it is drawn.
struct AncestorRow {
    id: EntryId,
    name: std::ffi::OsString,
    label: gpui::SharedString,
    is_dir: bool,
    icon: Option<icons::Pic>,
    link: icons::Link,
    /// Where it leads, for a place (a tag's), and its tag's color.
    place: Option<(Location, Option<usize>)>,
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
                let p = ws.pane(pane);
                ids[range.clone()]
                    .iter()
                    .filter_map(|id| listing?.get(*id))
                    .map(|e| AncestorRow {
                        id: e.id,
                        label: e.name_str().into_owned().into(),
                        name: e.name.clone(),
                        is_dir: e.is_dir(),
                        icon: this.icons.for_entry(e, icon_px),
                        link: icons::Link::of(e),
                        place: p
                            .and_then(|p| p.kept_location_of(&location, e.id))
                            .map(|leads| {
                                let tag = e
                                    .metadata
                                    .value(bibli_core::entry::keys::PLACE_TAG)
                                    .and_then(bibli_core::Value::as_str);
                                (leads, tag.map(|tag| this.tag_colors.of(tag)))
                            }),
                    })
                    .collect()
            };
            let mut out = Vec::with_capacity(rows.len());
            for (ix, row) in range.zip(rows) {
                let target = path.as_ref().map(|dir| dir.join(&row.name));
                let is_on_path = row.name == child;
                let mark =
                    icons::Mark::of(row.link, row.place.as_ref().and_then(|(_, color)| *color));
                let icon = icons::entry_picture(
                    row.icon.as_ref(),
                    row.is_dir,
                    f32::from(icon_px),
                    mark,
                    &theme,
                );
                // What a press opens: a folder, or where a place leads.
                let leads = match (&row.place, &target) {
                    (Some((place, _)), _) => Some(serde_json::json!(place.arg())),
                    (None, Some(target)) if row.is_dir => Some(serde_json::json!(
                        bibli_core::os_path::PathArg::from(target.as_path())
                    )),
                    _ => None,
                };
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
                    // Cut, it is dimmed here as in the pane's own column.
                    .when(
                        this.pending.cut && this.pending.entries.contains(&row.id),
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
                            if let Some(leads) = &leads {
                                this.run(
                                    "navigate",
                                    serde_json::json!({ "location": leads, "pane": pane }),
                                    window,
                                    cx,
                                );
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
                        bibli_core::op::EntryRef::new(row.id, target.clone()),
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
    col = match &a.path {
        Some(dir) => dnd::droppable_dir(col, dir.clone(), cx),
        None => dnd::droppable_place(col, a.location.clone(), cx),
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

    fn render(&mut self, shell: &mut Shell, _: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
        let theme = cx.global::<Theme>().clone();
        let (base, spec) = {
            let ws = shell.app.workspace.read();
            let p = ws.pane(shell.pane);
            (
                p.and_then(|p| p.location().cloned()),
                p.map_or_else(SortSpec::default, |p| p.sort),
            )
        };
        if base != self.base {
            if let Some(b) = &base {
                let trail = shell.ui().place.trail.clone();
                self.rebuild(b, trail.as_ref());
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

        // At least as wide as the pane, the columns from its left; wider,
        // set against its right edge, so a narrow pane cuts the folders
        // farthest up and keeps the one shown (layout only).
        let mut columns = div().h_full().min_w_full().flex_none().flex().flex_row();
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
                    this.request_hydrate(rows.iter().map(|r| r.look.id).collect::<Vec<_>>());
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
        match base.as_ref().map(|b| (b.path(), b)) {
            Some((Some(dir), _)) => last_col = dnd::droppable_dir(last_col, dir, cx),
            Some((None, place)) => last_col = dnd::droppable_place(last_col, place.clone(), cx),
            None => {}
        }
        columns = columns.child(last_col);
        div()
            .relative()
            .size_full()
            .flex()
            .flex_row()
            .justify_end()
            .overflow_hidden()
            .child(columns)
            // Ctrl and the wheel size the rows, over any column.
            .child(crate::views::zoom_wheel(cx))
            .into_any_element()
    }

    fn scroll_to(&mut self, ix: usize) {
        self.scroll.scroll_to_item(ix, ScrollStrategy::Top);
    }

    fn kept_locations(&self, shown: Option<&Location>, trail: Option<&Trail>) -> Vec<Location> {
        shown
            .map(|shown| {
                chain(shown, trail)
                    .into_iter()
                    .map(|(location, _)| location)
                    .collect()
            })
            .unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::location::Crumb;

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
