//! A map preview: a GPS track, a route, a file of places (the `geo`
//! plugin's `InspectionKind::Map`), drawn by `elements::map`.
//!
//! It is an inspection like any other, so Properties, the switch to the
//! file's source and the way back out of a container are the frame's.
//! What is its own: the map takes the pointer (drag to move, the wheel or a
//! double click to zoom) and, once clicked, the keys (arrows move it, the
//! zoom keys zoom it, Ctrl+0 fits it again). The view belongs to the file
//! shown: a changed file is read again where the map is; another file is
//! fitted.
use std::rc::Rc;
use std::sync::Arc;

use gpui::{AnyElement, Context, MouseButton, Window, div, prelude::*};
use skwir_core::tiles::Layer;

use crate::elements::map::{self, Colors, Handles, Shapes, View};
use crate::inspector::Display;
use crate::map_tiles::Backgrounds;
use crate::preview::PreviewContent;
use crate::preview_bar::{Bar, Item, Slot, label};
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toolbar::Button;

/// A key moves the map by this many pixels.
const PAN_STEP: f64 = 120.;
/// Zoomed in this far on a background without streets, the map says,
/// once, where streets can be had.
const DETAIL_ZOOM: f64 = 10.;

/// What the maps said once and need not say again this session.
#[derive(Default)]
pub struct Notes {
    /// That the background has no streets.
    hinted: bool,
    /// That the tiles of this source revision failed.
    failed: Option<u64>,
    /// That a file's tiles failed (its `Layer::id`).
    failed_file: Option<u64>,
}

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

/// A map of `shapes` over the registered background, and over it the
/// tiles of the `file` shown when it holds some: the preview's and the Map
/// view's, which differ only in what they place and how it answers.
pub fn element(
    shell: &Shell,
    id: impl Into<gpui::ElementId>,
    shapes: Arc<Shapes>,
    handles: Handles,
    theme: &Theme,
    file: Option<Layer>,
) -> map::Map {
    let app = Arc::clone(&shell.app);
    let tiles = map::TileLayer {
        store: Rc::clone(&shell.map_tiles),
        palette: crate::map_tiles::palette(theme),
        file,
        // The attribution's link reaches the desktop as every address does.
        on_link: Rc::new(move |url: String, _: &mut Window, _: &mut gpui::App| {
            app.request_front(skwir_core::command::Invocation::with_args(
                "ui.open_url",
                &serde_json::json!({ "url": url }),
            ));
        }),
    };
    map::map(id, shapes, handles, colors(theme), Some(tiles))
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
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    window.focus(&clicked, cx);
                })
        })
        .child(div().flex_1().min_h_0().child(element(
            shell,
            "preview-map",
            shapes,
            shell.preview.map.clone(),
            theme,
            display.data.tiles.clone(),
        )))
        .when_some(display.profile.clone(), |d, profile| {
            let handles = &shell.preview.map;
            d.child(
                div()
                    .flex_none()
                    .h(gpui::px(PROFILE_HEIGHT))
                    .border_t_1()
                    .border_color(theme.border)
                    .child(crate::elements::profile::chart(
                        "preview-profile",
                        profile,
                        crate::elements::profile::Colors {
                            line: theme.map_track,
                            text: theme.text,
                            muted: theme.text_muted,
                            grid: theme.map_grid,
                            paper: theme.bg,
                        },
                        Rc::clone(&handles.mark),
                        Rc::clone(&handles.profile_hover),
                    )),
            )
        })
        .into_any_element()
}

/// The elevation profile under a map, in pixels.
const PROFILE_HEIGHT: f32 = 96.;

/// The popup that chooses the background.
pub const BACKGROUND_POPUP: &str = "background";

/// The button that opens the popup choosing the background, which it
/// names: the preview's (`ui.preview_menu`) and the Map view's
/// (`ui.toolbar_menu`), each with the popup's `kind`.
pub fn background_button(
    id: impl Into<gpui::SharedString>,
    backgrounds: &Backgrounds,
    command: &'static str,
    kind: &str,
) -> Button {
    Button::new(id, "layers", "Map background", command)
        .args(serde_json::json!({ "kind": kind }))
        .label(backgrounds.label().to_owned())
}

/// The rows of the popup that chooses the background, in a preview or a
/// Map view: each background, the one shown checked, chosen as in
/// Settings (`settings.apply`), then Manage backgrounds….
pub fn background_rows(
    shell: &Shell,
    backgrounds: &Backgrounds,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> gpui::Div {
    let mut list = div().flex().flex_col().gap_1();
    for (value, label) in &backgrounds.choices {
        let chosen = *value == backgrounds.current;
        list = list.child(
            Button::new(
                format!("map-background-{value}"),
                if chosen { "check" } else { "" },
                label.clone(),
                "settings.apply",
            )
            .args(serde_json::json!({ "changes": { backgrounds.setting.clone(): value } }))
            .label(label.clone())
            .active(chosen)
            .menu()
            .render(shell, cx),
        );
    }
    list.child(crate::preview_bar::separator(theme)).child(
        Button::new(
            "map-backgrounds-manage",
            "settings",
            "Manage backgrounds",
            "ui.settings",
        )
        .args(serde_json::json!({ "key": backgrounds.setting }))
        .label("Manage backgrounds…")
        .menu()
        .render(shell, cx),
    )
}

/// The map's tools, in the bar's View slot: zoom out, in, and fit, then
/// the background, chosen here as in Settings.
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
    let backgrounds = shell.map_tiles.borrow_mut().backgrounds();
    if let Some(backgrounds) = backgrounds {
        bar.push(
            Item::button(
                Slot::View,
                1,
                background_button(
                    "map-background",
                    &backgrounds,
                    "ui.preview_menu",
                    BACKGROUND_POPUP,
                ),
            )
            .opens(BACKGROUND_POPUP),
        );
        bar.popups.push((
            BACKGROUND_POPUP,
            Rc::new(move |shell, theme, cx| {
                let popup = shell.preview.menu.as_ref()?;
                Some(
                    crate::toolbar::popup_panel(popup, "ui.preview_menu", theme, cx)
                        .child(background_rows(shell, &backgrounds, theme, cx))
                        .into_any_element(),
                )
            }),
        ));
    }
    bar.push(Item::menu_only(Button::new(
        "map-open-external",
        "open",
        "Open in Maps",
        "ui.map_open",
    )));
}

impl Shell {
    /// Take the tiles that came, let the atlas go of those let go, and say
    /// once what is worth saying: a background that failed, or one without
    /// streets zoomed in to them. No map shown, no tiles kept.
    pub(crate) fn sync_map_tiles(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_map_views();
        let shown = self.any_map();
        let (arrived, evicted, failure, info) = {
            let mut store = self.map_tiles.borrow_mut();
            if !shown && !store.is_empty() {
                store.clear();
            }
            let arrived = store.receive();
            let info = shown.then(|| store.info()).flatten();
            (arrived, store.take_evicted(), store.failure.take(), info)
        };
        for image in evicted {
            cx.drop_image(image, Some(window));
        }
        if arrived {
            cx.notify();
        }
        // The setting that chooses the background is the source's to say.
        let setting = info.as_ref().and_then(|info| info.setting.clone());
        match failure.filter(|_| shown) {
            Some(failure) if failure.source != 0 => {
                // A damaged file: no setting fixes it.
                if self.map_notes.failed_file != Some(failure.source) {
                    self.map_notes.failed_file = Some(failure.source);
                    self.set_status(
                        format!("This file's map tiles could not be read: {}", failure.error),
                        true,
                        cx,
                    );
                }
            }
            Some(failure) if self.map_notes.failed != Some(failure.revision) => {
                self.map_notes.failed = Some(failure.revision);
                let text = format!("Map tiles could not be loaded: {}", failure.error);
                match &setting {
                    Some(setting) => self.set_status_about(text, setting, cx),
                    None => self.set_status(text, true, cx),
                }
            }
            _ => {}
        }
        // A file that holds its own tiles shows its streets where it has
        // them: the background's lack of them is not worth a word there.
        if let Some(info) = info
            && let Some(setting) = &setting
            && !info.detailed
            && !self.map_notes.hinted
            && !self.map_has_file_tiles()
            && self.map_view().is_some_and(|view| view.zoom >= DETAIL_ZOOM)
        {
            self.map_notes.hinted = true;
            self.set_hint_about(
                "Streets are not drawn on this map background; another one can show them",
                setting,
                cx,
            );
        }
    }

    /// Whether a map is drawn anywhere: the preview's or a pane's.
    fn any_map(&self) -> bool {
        self.map_shown().is_some() || !self.map_views.is_empty()
    }

    /// Whether the preview's map draws a file's own tiles.
    fn map_has_file_tiles(&self) -> bool {
        matches!(
            &self.preview.content,
            Some(PreviewContent::Inspection(display)) if display.data.tiles.is_some()
        )
    }

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
            skwir_core::scheduler::Priority::Visible,
            "map-open",
            self.app.lifetime().child(),
            move || {
                if let Err(error) = skwir_platform::open::open_geo_uri(&uri) {
                    app.notify(
                        skwir_core::app::NotifyLevel::Error,
                        format!("No application opens map locations: {error}"),
                    );
                }
            },
        );
        cx.notify();
    }
}
