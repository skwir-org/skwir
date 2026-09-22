//! Miller columns: ancestor directories to the left, the pane's listing as
//! the deepest column. Ancestors are opened by the view through
//! `App::open_location` (which only enqueues the listing) when the pane's
//! folder changes, kept while they stay ancestors, pumped here, and closed
//! when the view drops them.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bibli_core::location::OpenLocation;
use bibli_core::os_path::PathArg;
use bibli_core::{EntryId, Listing, Location, SortSpec};
use gpui::{
    AnyElement, Context, MouseButton, ScrollStrategy, UniformListScrollHandle, Window, div,
    prelude::*, px, uniform_list,
};

use crate::dnd;
use crate::icons;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::view::{Nav, PaneView};
use crate::views::list::{ROW_HEIGHT, row};

const MAX_ANCESTORS: usize = 3;
const COLUMN_W: f32 = 240.0;
const ICON_PX: u16 = 18;

/// One row of an ancestor column, built when its listing changes.
struct AncestorRow {
    name: std::ffi::OsString,
    label: gpui::SharedString,
    is_dir: bool,
}

struct Ancestor {
    path: PathBuf,
    /// Name of the child on the way to the pane's directory.
    child: std::ffi::OsString,
    open: Arc<dyn OpenLocation>,
    listing: Listing,
    rows: Arc<[AncestorRow]>,
    sorted_at: Option<(u64, SortSpec)>,
    scroll: UniformListScrollHandle,
    /// The row on the way down was scrolled into view once it listed.
    shown_child: bool,
}

impl Drop for Ancestor {
    fn drop(&mut self) {
        self.open.close();
    }
}

#[derive(Default)]
pub struct ColumnsView {
    base: Option<PathBuf>,
    ancestors: Vec<Ancestor>,
    scroll: UniformListScrollHandle,
}

impl ColumnsView {
    /// The ancestors of `base`, nearest last. Those already open (going one
    /// level down or up keeps most of them) are kept with their listing and
    /// scroll; only the new ones are opened.
    fn rebuild(&mut self, shell: &Shell, base: &Path) {
        let mut chain: Vec<(PathBuf, std::ffi::OsString)> = Vec::new();
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
        let mut old: Vec<Ancestor> = std::mem::take(&mut self.ancestors);
        for (path, child) in chain {
            if let Some(i) = old.iter().position(|a| a.path == path) {
                let mut kept = old.swap_remove(i);
                if kept.child != child {
                    kept.child = child;
                    kept.shown_child = false;
                }
                self.ancestors.push(kept);
                continue;
            }
            match shell.app.open_location(&Location::from_path(&path)) {
                Ok(open) => self.ancestors.push(Ancestor {
                    path,
                    child,
                    open,
                    listing: Listing::default(),
                    rows: Arc::from([]),
                    sorted_at: None,
                    scroll: UniformListScrollHandle::new(),
                    shown_child: false,
                }),
                Err(e) => tracing::warn!(path = %path.display(), "columns: {e:#}"),
            }
        }
        self.base = Some(base.to_path_buf());
    }

    fn pump(&mut self, spec: SortSpec) {
        for a in &mut self.ancestors {
            a.listing.drain(a.open.events());
            let v = a.listing.version();
            if a.sorted_at == Some((v, spec)) {
                continue;
            }
            a.sorted_at = Some((v, spec));
            let listing = &a.listing;
            a.rows = listing
                .sorted(spec)
                .iter()
                .filter_map(|id| listing.get(*id))
                .map(|e| AncestorRow {
                    name: e.name.clone(),
                    label: e.name_str().into_owned().into(),
                    is_dir: e.is_dir(),
                })
                .collect();
            if !a.shown_child
                && let Some(ix) = a.rows.iter().position(|row| row.name == a.child)
            {
                a.scroll.scroll_to_item(ix, ScrollStrategy::Center);
                a.shown_child = true;
            }
        }
    }
}

fn ancestor_column(
    view: &ColumnsView,
    k: usize,
    pane: bibli_core::workspace::PaneId,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let a = &view.ancestors[k];
    let rows = Arc::clone(&a.rows);
    let path = a.path.clone();
    let child = a.child.clone();
    let list = uniform_list(
        ("column", k),
        rows.len(),
        cx.processor(move |this: &mut Shell, range: Range<usize>, _window, cx| {
            let theme = cx.global::<Theme>().clone();
            let mut out = Vec::with_capacity(range.len());
            for ix in range {
                let Some(row) = rows.get(ix) else {
                    break;
                };
                let target = path.join(&row.name);
                let is_on_path = row.name == child;
                let icon = if row.is_dir {
                    this.icons.folder(ICON_PX)
                } else {
                    this.icons.for_name(&row.name, ICON_PX)
                };
                let icon: AnyElement = match icon {
                    Some(p) => icons::element(&p, f32::from(ICON_PX)),
                    None => div()
                        .size(px(f32::from(ICON_PX)))
                        .rounded_sm()
                        .bg(theme.text_muted)
                        .into_any_element(),
                };
                let dir_target = row.is_dir.then(|| target.clone());
                let mut el = div()
                    .id(ix)
                    .h(px(ROW_HEIGHT))
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
                self.rebuild(shell, b);
            } else {
                self.ancestors.clear();
                self.base = None;
            }
        }
        self.pump(spec);

        let mut columns = div().size_full().flex().flex_row().overflow_hidden();
        for k in 0..self.ancestors.len() {
            columns = columns.child(ancestor_column(self, k, shell.pane, &theme, cx));
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
                    let rows = this.rows(range, ICON_PX, true);
                    let need: Vec<EntryId> =
                        rows.iter().filter(|r| !r.hydrated).map(|r| r.id).collect();
                    this.request_hydrate(need);
                    rows.iter()
                        .map(|r| row(this, &theme, r, true, cx))
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
        columns.into_any_element()
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
