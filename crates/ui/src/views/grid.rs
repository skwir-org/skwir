//! The grid (gallery) view: thumbnail cells in virtualized rows.

use std::ops::Range;

use bibli_core::EntryId;
use bibli_core::thumbs::ThumbSize;
use gpui::{
    AnyElement, Context, MouseButton, MouseDownEvent, ScrollStrategy, UniformListScrollHandle,
    Window, div, img, prelude::*, px, uniform_list,
};

use crate::dnd;
use crate::icons;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::thumbs::ThumbView;
use crate::view::{Nav, PaneView};

/// Zoom steps for `ui.zoom`.
pub const ZOOMS: [u32; 4] = [96, 128, 192, 256];

#[derive(Default)]
pub struct GridView {
    scroll: UniformListScrollHandle,
    cols: usize,
    rows_visible: usize,
}

impl GridView {
    fn cell_w(zoom: u32) -> f32 {
        zoom as f32 + 24.0
    }

    fn cell_h(zoom: u32) -> f32 {
        zoom as f32 + 44.0
    }

    fn compute_cols(&mut self, shell: &Shell) -> usize {
        let width = shell.list.content_width.get() - 8.0;
        let cols = (width / Self::cell_w(shell.zoom)).floor().max(1.0) as usize;
        if cols != self.cols {
            // Row indices change meaning; keep the cursor in view.
            self.cols = cols;
            if let Some(ix) = shell.cursor_index() {
                self.scroll.scroll_to_item(ix / cols, ScrollStrategy::Top);
            }
        }
        cols
    }
}

fn cell(
    shell: &mut Shell,
    theme: &Theme,
    ix: usize,
    id: EntryId,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let zoom = shell.zoom;
    let (name, is_dir, selected, is_cursor, icon, path, hydrated) = {
        let ws = shell.app.workspace.read();
        let Some(pane) = ws.pane(shell.pane) else {
            return div().into_any_element();
        };
        let Some(e) = pane.listing.get(id) else {
            return div().into_any_element();
        };
        (
            e.name_str().into_owned(),
            e.is_dir(),
            pane.selection.contains(id),
            pane.selection.cursor == Some(id),
            shell.icons.for_entry(e, if zoom > 128 { 64 } else { 48 }),
            if e.is_dir() {
                pane.open.as_ref().and_then(|o| o.path_of(id))
            } else {
                None
            },
            e.metadata.is_hydrated(),
        )
    };
    if !hydrated {
        shell.request_hydrate(vec![id]);
    }
    let size = if zoom > 128 {
        ThumbSize::Large
    } else {
        ThumbSize::Normal
    };
    let thumb = if is_dir {
        ThumbView::Unavailable
    } else {
        shell.thumb(id, size, window, cx)
    };
    let icon_px = if zoom > 128 { 64.0 } else { 48.0 };
    let picture: AnyElement = match thumb {
        ThumbView::Ready(ri) => img(ri)
            .max_w(px(zoom as f32))
            .max_h(px(zoom as f32))
            .object_fit(gpui::ObjectFit::Contain)
            .rounded_sm()
            .into_any_element(),
        ThumbView::Pending => div()
            .size(px(zoom as f32 * 0.6))
            .rounded_md()
            .bg(theme.border.opacity(0.6))
            .into_any_element(),
        ThumbView::Unavailable => match &icon {
            Some(p) => icons::element(p, icon_px),
            None => div()
                .size(px(icon_px))
                .rounded_md()
                .bg(if is_dir {
                    theme.accent
                } else {
                    theme.text_muted
                })
                .into_any_element(),
        },
    };
    let mut el = div()
        .id(ix)
        .w(px(GridView::cell_w(zoom)))
        .h(px(GridView::cell_h(zoom)))
        .flex()
        .flex_col()
        .items_center()
        .p_1()
        .gap_1()
        .rounded_md()
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
                this.on_entry_mouse_down(ix, ev, window, cx);
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                this.on_entry_mouse_down(ix, ev, window, cx);
            }),
        )
        .child(
            div()
                .size(px(zoom as f32))
                .flex()
                .items_center()
                .justify_center()
                .child(picture),
        )
        .child(
            div()
                .w_full()
                .text_xs()
                .text_center()
                .line_clamp(2)
                .child(name),
        );
    el = dnd::draggable(el, shell);
    if let Some(p) = path {
        el = dnd::droppable_dir(el, p, cx);
    }
    el.into_any_element()
}

impl PaneView for GridView {
    fn id(&self) -> &'static str {
        crate::view::GRID
    }

    fn title(&self) -> &'static str {
        "Grid"
    }

    fn render(&mut self, shell: &mut Shell, _: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
        let cols = self.compute_cols(shell);
        let count = shell.list.sorted.len();
        let rows = count.div_ceil(cols);
        let zoom = shell.zoom;
        let list = uniform_list(
            "grid-rows",
            rows,
            cx.processor(move |this: &mut Shell, range: Range<usize>, window, cx| {
                let theme = cx.global::<Theme>().clone();
                this.list.visible_rows = range.len().max(1) * cols;
                if let Some(v) = this.views.get_mut(crate::view::GRID) {
                    let _ = v;
                }
                this.grid_rows_visible = range.len().max(1);
                let mut out = Vec::with_capacity(range.len());
                for r in range {
                    let start = r * cols;
                    let end = ((r + 1) * cols).min(this.list.sorted.len());
                    let ids: Vec<(usize, EntryId)> =
                        (start..end).map(|ix| (ix, this.list.sorted[ix])).collect();
                    let mut row = div()
                        .h(px(GridView::cell_h(zoom)))
                        .w_full()
                        .flex()
                        .flex_row()
                        .px_1();
                    for (ix, id) in ids {
                        row = row.child(cell(this, &theme, ix, id, window, cx));
                    }
                    out.push(row);
                }
                out
            }),
        )
        .track_scroll(self.scroll.clone())
        .size_full();
        self.rows_visible = shell.grid_rows_visible;
        div().size_full().child(list).into_any_element()
    }

    fn scroll_to(&mut self, ix: usize) {
        let cols = self.cols.max(1);
        self.scroll.scroll_to_item(ix / cols, ScrollStrategy::Top);
    }

    fn nav_delta(&self, nav: Nav, _: &Shell) -> Option<isize> {
        let cols = self.cols.max(1) as isize;
        let page = (self.rows_visible.saturating_sub(1).max(1) as isize) * cols;
        match nav {
            Nav::Up => Some(-cols),
            Nav::Down => Some(cols),
            Nav::Left => Some(-1),
            Nav::Right => Some(1),
            Nav::PageUp => Some(-page),
            Nav::PageDown => Some(page),
            Nav::Home | Nav::End => None,
        }
    }

    fn reset(&mut self) {
        self.scroll.scroll_to_item_strict(0, ScrollStrategy::Top);
    }
}
