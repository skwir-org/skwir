//! The grid (gallery) view: thumbnail cells in a `UniformGrid`, whose
//! column count is decided in prepaint from the element's real width.

use std::ops::Range;

use bibli_core::EntryId;
use bibli_core::thumbs::ThumbSize;
use gpui::{
    AnyElement, Context, MouseButton, MouseDownEvent, Window, div, img, prelude::*, px, size,
};

use crate::dnd;
use crate::elements::uniform_grid::{UniformGridScrollHandle, uniform_grid};
use crate::icons;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::thumbs::ThumbView;
use crate::view::{Nav, PaneView};

/// Zoom steps for `ui.zoom`.
pub const ZOOMS: [u32; 4] = [96, 128, 192, 256];

#[derive(Default)]
pub struct GridView {
    scroll: UniformGridScrollHandle,
}

impl GridView {
    pub fn cell_w(zoom: u32) -> f32 {
        zoom as f32 + 24.0
    }

    pub fn cell_h(zoom: u32) -> f32 {
        zoom as f32 + 44.0
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
    let pane = shell.pane;
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
        .when(!selected, |d| d.hover(|s| s.bg(theme.hover)))
        .when(
            shell.pending.cut && shell.pending.entries.contains(&id),
            |d| d.opacity(crate::views::list::CUT_OPACITY),
        )
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
        let count = shell.ui().list.sorted.len();
        let zoom = shell.zoom;
        let pane = shell.pane;
        let render = cx.processor(
            move |this: &mut Shell, (range, cols): (Range<usize>, usize), window, cx| {
                this.with_pane(pane, |this| {
                    let theme = cx.global::<Theme>().clone();
                    this.ui_mut().list.visible_rows = range.len().div_ceil(cols.max(1)).max(1);
                    let ids: Vec<(usize, EntryId)> = range
                        .filter_map(|ix| this.ui().list.sorted.get(ix).map(|id| (ix, *id)))
                        .collect();
                    ids.into_iter()
                        .map(|(ix, id)| cell(this, &theme, ix, id, window, cx))
                        .collect::<Vec<_>>()
                })
            },
        );
        let grid = uniform_grid(
            ("grid", pane.0 as usize),
            count,
            size(px(GridView::cell_w(zoom)), px(GridView::cell_h(zoom))),
            move |range, cols, window, cx| render((range, cols), window, cx),
        )
        .track_scroll(self.scroll.clone())
        .size_full()
        .px_1();
        let marquee = crate::elements::marquee::Marquee {
            pane,
            shell: cx.weak_entity(),
            geometry: crate::elements::marquee::Geometry::Cells {
                width: GridView::cell_w(zoom),
                height: GridView::cell_h(zoom),
                // The column count of the last prepaint: a band dragged in the
                // frame a resize lands is off by that frame only.
                cols: self.scroll.cols(),
                pad_x: 4.,
            },
            count,
            offset: self.scroll.0.borrow().base_handle.offset(),
            drag: shell.marquee.clone(),
        };
        div()
            .relative()
            .size_full()
            .child(grid)
            .child(marquee)
            .into_any_element()
    }

    fn scroll_to(&mut self, ix: usize) {
        self.scroll.scroll_to_item(ix);
    }

    fn nav_delta(&self, nav: Nav, _: &Shell) -> Option<isize> {
        // Geometry from the last prepaint, never from a measured width.
        let cols = self.scroll.cols() as isize;
        let page = (self.scroll.rows_visible().saturating_sub(1).max(1) as isize) * cols;
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
        self.scroll.scroll_to_item_strict(0);
    }
}
