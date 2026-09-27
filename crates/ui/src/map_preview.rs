//! A map preview: a GPS track, a route, a file of places (the `geo`
//! plugin's `InspectionKind::Map`), drawn by `elements::map`.
//!
//! It is an inspection like any other, so Details, the switch to the
//! file's source and the way back out of a container are the frame's.
//! What is its own: the map takes the pointer (drag to move, the wheel or a
//! double click to zoom) and, once clicked, the keys (arrows move it, the
//! zoom keys zoom it, Ctrl+0 fits it again). The view belongs to the file
//! shown: a changed file is read again where the map is; another file is
//! fitted.
use std::sync::Arc;

use gpui::{AnyElement, Context, MouseButton, div, prelude::*};

use crate::elements::map::{self, Colors, Handles, Shapes, View};
use crate::inspector::Display;
use crate::preview::PreviewContent;
use crate::preview_bar::{Bar, Item, Slot, label};
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toolbar::Button;

/// A key moves the map by this many pixels.
const PAN_STEP: f64 = 120.;

/// The map's view, kept with the preview (`PreviewState::map`).
pub type MapState = Handles;

fn colors(theme: &Theme) -> Colors {
    Colors {
        paper: theme.bg,
        grid: theme.map_grid,
        grid_text: theme.text_muted,
        track: theme.map_track,
        route: theme.map_route,
        text: theme.text,
    }
}

pub fn content(shell: &Shell, display: &Display, theme: &Theme) -> AnyElement {
    let shapes = display.map.clone().unwrap_or_default();
    let focus = shell.preview.inspection.focus.clone();
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .key_context(crate::keymap::MAP)
        .when_some(focus, |d, focus| {
            let clicked = focus.clone();
            d.track_focus(&focus)
                .on_mouse_down(MouseButton::Left, move |_, window, _| {
                    window.focus(&clicked);
                })
        })
        .child(map::map(
            "preview-map",
            shapes,
            shell.preview.map.clone(),
            colors(theme),
        ))
        .into_any_element()
}

/// The map's tools, in the bar's View slot: zoom out, in, and fit.
pub fn tools(bar: &mut Bar, shell: &Shell) {
    let fitted = shell.preview.map.view.get().is_none();
    let step = |id, icon, title, dir| {
        Button::new(id, icon, title, "ui.preview_zoom").args(serde_json::json!({ "dir": dir }))
    };
    bar.push(Item::button(
        Slot::View,
        1,
        step("map-zoom-out", "minus", "Zoom out", "out"),
    ));
    bar.push(Item::button(
        Slot::View,
        1,
        step("map-zoom-in", "plus", "Zoom in", "in"),
    ));
    bar.push(Item::button(
        Slot::View,
        2,
        step("map-fit", "fit", label::FIT, "reset").active(fitted),
    ));
    bar.push(Item::menu_only(Button::new(
        "map-open-external",
        "open",
        "Open in Maps",
        "ui.map_open",
    )));
}

impl Shell {
    /// The map shown in the preview, if one is.
    fn map_shown(&self) -> Option<&Arc<Shapes>> {
        match &self.preview.content {
            Some(PreviewContent::Inspection(display)) => display.map.as_ref(),
            _ => None,
        }
    }

    /// Where a key zooms and moves from: the view the user chose, else the
    /// one drawn last (a fitted map has no other). `None` before it was
    /// drawn.
    fn map_view(&self) -> Option<View> {
        let handles = &self.preview.map;
        handles.view.get().or_else(|| handles.shown.get())
    }

    /// `ui.preview_zoom` on a map: `in` and `out` by a level around the
    /// middle, `reset` fits it again. False when no map is shown.
    pub(crate) fn map_zoom(&mut self, direction: &str, cx: &mut Context<Self>) -> bool {
        if self.map_shown().is_none() {
            return false;
        }
        let levels = match direction {
            "in" => 1.,
            "out" => -1.,
            _ => {
                self.preview.map.view.set(None);
                cx.notify();
                return true;
            }
        };
        if let Some(view) = self.map_view() {
            self.preview.map.view.set(Some(View {
                zoom: (view.zoom + levels).clamp(0., map::MAX_ZOOM),
                ..view
            }));
            cx.notify();
        }
        true
    }

    /// `ui.map_pan { dx, dy }`: move the map by steps of `PAN_STEP` pixels
    /// (the arrows), as a drag would.
    pub(crate) fn map_pan(&mut self, dx: f64, dy: f64, cx: &mut Context<Self>) {
        if self.map_shown().is_none() {
            return;
        }
        if let Some(view) = self.map_view() {
            self.preview
                .map
                .view
                .set(Some(view.pan(-dx * PAN_STEP, -dy * PAN_STEP)));
            cx.notify();
        }
    }

    /// `ui.map_open`: the place the map shows, in the desktop's maps
    /// application, as a `geo:` URI (RFC 5870).
    pub(crate) fn map_open_external(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.map_view().filter(|_| self.map_shown().is_some()) else {
            return;
        };
        let (lat, lon) = map::math::unproject(view.x, view.y);
        let uri = format!(
            "geo:{lat:.6},{:.6}?z={:.0}",
            map::math::wrap_lon(lon),
            view.zoom.round()
        );
        let app = self.app.clone();
        self.app.scheduler.spawn(
            bibli_core::scheduler::Priority::Visible,
            "map-open",
            self.app.lifetime().child(),
            move || {
                if let Err(error) = bibli_platform::open::open_geo_uri(&uri) {
                    app.notify(
                        bibli_core::app::NotifyLevel::Error,
                        format!("No application opens map locations: {error}"),
                    );
                }
            },
        );
        cx.notify();
    }
}
