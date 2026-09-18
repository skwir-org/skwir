//! Miller columns: ancestor directories to the left, the pane's listing as
//! the deepest column. Ancestors are opened by the view through
//! `App::open_location` and pumped here; they close when the view drops them.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bibli_core::location::OpenLocation;
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

struct Ancestor {
    path: PathBuf,
    /// Name of the child on the way to the pane's directory.
    child: String,
    open: Arc<dyn OpenLocation>,
    listing: Listing,
    ids: Arc<[EntryId]>,
    sorted_at: Option<(u64, SortSpec)>,
    scroll: UniformListScrollHandle,
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
    fn rebuild(&mut self, shell: &Shell, base: &Path) {
        self.ancestors.clear();
        let mut chain: Vec<(PathBuf, String)> = Vec::new();
        let mut cur = base.to_path_buf();
        while let Some(parent) = cur.parent().map(Path::to_path_buf) {
            let child = cur
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            chain.push((parent.clone(), child));
            cur = parent;
            if chain.len() >= MAX_ANCESTORS {
                break;
            }
        }
        chain.reverse();
        for (path, child) in chain {
            match shell.app.open_location(&Location::from_path(&path)) {
                Ok(open) => self.ancestors.push(Ancestor {
                    path,
                    child,
                    open,
                    listing: Listing::default(),
                    ids: Arc::from([]),
                    sorted_at: None,
                    scroll: UniformListScrollHandle::new(),
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
            if a.sorted_at != Some((v, spec)) {
                a.ids = a.listing.sorted(spec);
                a.sorted_at = Some((v, spec));
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
    let ids = Arc::clone(&a.ids);
    let path = a.path.clone();
    let child = a.child.clone();
    // Ancestor rows are read from the view's own listing, so they are built
    // here rather than through the shell's processor.
    let mut rows: Vec<(EntryId, String, bool, Option<Arc<Path>>)> = Vec::with_capacity(ids.len());
    {
        let _ = cx;
    }
    for id in ids.iter() {
        if let Some(e) = a.listing.get(*id) {
            rows.push((*id, e.name_str().into_owned(), e.is_dir(), None));
        }
    }
    let rows = Arc::new(rows);
    let list = uniform_list(
        ("column", k),
        rows.len(),
        cx.processor(move |this: &mut Shell, range: Range<usize>, _window, cx| {
            let theme = cx.global::<Theme>().clone();
            let mut out = Vec::with_capacity(range.len());
            for ix in range {
                let Some((_, name, is_dir, _)) = rows.get(ix) else {
                    break;
                };
                let target = path.join(name);
                let is_on_path = *name == child;
                let icon: AnyElement = {
                    let e = bibli_core::Entry::new(
                        EntryId { dev: 0, ino: 0 },
                        name.as_str(),
                        if *is_dir {
                            bibli_core::EntryKind::Directory
                        } else {
                            bibli_core::EntryKind::File
                        },
                    );
                    match this.icons.for_entry(&e, 18) {
                        Some(p) => icons::element(&p, 18.0),
                        None => div()
                            .size(px(18.))
                            .rounded_sm()
                            .bg(theme.text_muted)
                            .into_any_element(),
                    }
                };
                let dir_target = is_dir.then(|| target.clone());
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
                    .when(!is_on_path, |d| d.hover(|s| s.bg(theme.surface)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.activate_pane(pane, cx);
                            if let Some(t) = &dir_target {
                                this.run(
                                    "navigate",
                                    serde_json::json!({ "location": t.to_string_lossy() }),
                                    window,
                                    cx,
                                );
                            }
                        }),
                    )
                    .child(icon)
                    .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                    .when(*is_dir, |d| {
                        d.child(div().text_color(theme.text_muted).child("›"))
                    });
                if *is_dir {
                    el = dnd::droppable_dir(el, target, cx);
                }
                out.push(el);
            }
            out
        }),
    )
    .track_scroll(a.scroll.clone())
    .size_full();
    let mut col = div()
        .id(("colwrap", k))
        .w(px(COLUMN_W))
        .h_full()
        .flex_none()
        .border_r_1()
        .border_color(theme.border)
        .child(list);
    col = dnd::droppable_dir(col, a.path.clone(), cx);
    col.into_any_element()
}

impl PaneView for ColumnsView {
    fn id(&self) -> &'static str {
        crate::view::COLUMNS
    }

    fn title(&self) -> &'static str {
        "Columns"
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
                    let rows = this.rows(range, 18);
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
            .w(px(COLUMN_W * 1.25))
            .h_full()
            .flex_none()
            .border_r_1()
            .border_color(theme.border)
            .child(last);
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
