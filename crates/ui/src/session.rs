//! What the window looked like, so the next one looks the same.
//!
//! A preference is something the user chose in Settings; this is not that. It
//! is the shape of the last window — which view the panes showed, with Quick
//! filters and the timeline or without (the active pane's, which every pane
//! starts with), whether it was split and how, whether the preview and the
//! terminal were open, where the preview was and how big, how the sidebar's
//! groups were arranged — kept as application data under one key and
//! restored at startup. Nothing here is worth a dialog, and all of it is annoying to set
//! up again every morning.
//!
//! The shell keeps a copy and writes it only when it differs, so ordinary
//! frames neither touch the settings lock nor bump its revision.

use crate::shell::Shell;

pub const SESSION_KEY: &str = "ui.session";

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
// Each is one independent toggle of the window.
#[allow(clippy::struct_excessive_bools)]
pub struct Session {
    /// Id of the view the panes showed, e.g. `list` or `grid`.
    pub view: String,
    /// Icon size of the grid view.
    pub zoom: u32,
    /// Icon size of the list and columns views' rows.
    pub row_zoom: u32,
    /// Levels of folders the Usage map draws; 0 until changed.
    pub levels: u32,
    /// What the Usage map's blocks say.
    pub usage_mode: crate::usage::Mode,
    /// The sidebar put away with F9 (it also gives way to a narrow window).
    pub sidebar_hidden: bool,
    /// How its groups were arranged: their order, those put away or folded.
    pub sidebar_sections: crate::sidebar::Sections,
    /// How wide the sidebar was dragged; 0 until then.
    pub sidebar_width: f32,
    /// The first split pane's share of the room, as its edge was dragged;
    /// 0 until then.
    pub split_share: f32,
    pub split: bool,
    /// Split panes side by side, stacked, or as the window's shape says.
    pub panes: crate::layout::Panes,
    /// The open split's own arrangement, when it was made the other way.
    pub split_panes: Option<crate::layout::Panes>,
    /// The panel's Preview switch (the panel was open, before 2026-11-06).
    pub preview: bool,
    /// The preview at the right, at the bottom, or as the window's shape says.
    pub preview_at: crate::layout::Preview,
    pub preview_width: f32,
    /// A file of an archive previewed at the right of the archive's view,
    /// under it, or as the panel's shape says.
    pub member_at: crate::layout::Member,
    /// The archive's view's share of the room above a file of it, and at
    /// its left, as their edge was dragged each way; 0 until then.
    pub members_height_share: f32,
    pub members_width_share: f32,
    /// How tall the preview is when it is at the bottom, once dragged; 0
    /// until then (the default makes room for a pinned player).
    pub preview_height: f32,
    /// The panel's Properties switch: Properties shown, over the preview
    /// or alone, the panel there while either switch is on.
    pub properties_on: bool,
    /// What windows wrote before 2026-11-06: Properties open under a
    /// preview, shown only while the panel was. Read into `properties_on`
    /// (`upgraded`), never written.
    #[serde(skip_serializing)]
    pub properties: Option<bool>,
    /// Its share of the room above a preview, and at its left, as their
    /// edge was dragged each way.
    pub properties_height_share: f32,
    pub properties_width_share: f32,
    /// Its groups folded or unfolded, by title.
    pub properties_folds: crate::properties::Folds,
    /// The types whose files open on their source.
    pub source_types: crate::preview::SourceTypes,
    /// The types whose models open turned in 3D, or by their picture.
    pub turned_types: crate::preview::TurnedTypes,
    /// The active pane's Quick filters asked for above its files.
    pub quick_filters: bool,
    /// The active pane's timeline shown under its files.
    pub timeline: bool,
    /// Whether its bars measure sizes rather than files.
    pub timeline_size: bool,
    /// The time they count (`timeline::DATES`); modified when empty.
    pub timeline_date: String,
    /// How tall the pinned player's dock was dragged.
    pub player_height: f32,
    /// Whether Activity was folded into the footer's strip by its chevron,
    /// opening over the files only when asked, rather than standing above
    /// the footer once an action lasted (ADR-114, amended 2026-11-07).
    /// What windows wrote before, `activity_pinned` (off by default), is
    /// not read: every window takes the new default.
    pub activity_folded: bool,
    /// Whether the terminal was open under the panes.
    pub terminal: bool,
    /// How tall its edge was dragged; 0 until then.
    pub terminal_height: f32,
    /// The list's detail columns dragged to another width than their own,
    /// by `Column::key`.
    pub column_widths: std::collections::BTreeMap<String, f32>,
    /// Where the window was and how big (ADR-122).
    pub window: Option<Placement>,
}

/// Where a window was and how big, as file managers and browsers keep it
/// (ADR-122): its size and whether it was maximized, on every system; its
/// place, on the display it was on, where the system says where a window
/// is and lets it say where it goes (X11, Windows, macOS: Wayland's
/// compositor places windows itself and says nothing of where).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Placement {
    /// Its size as a window, not maximized nor on the whole screen, in
    /// logical pixels.
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub maximized: bool,
    /// Its place, where the system says it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<At>,
}

/// A window's place on a display, in the coordinates the system gave for
/// both: it holds while that display is there as it was.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct At {
    pub x: f32,
    pub y: f32,
    /// The display (`PlatformDisplay::uuid`).
    pub display: String,
    /// The display's bounds then: x, y, width, height.
    pub display_bounds: [f32; 4],
}

/// How small a window opens, whatever was written.
const SMALLEST: (f32, f32) = (360., 240.);

impl Placement {
    /// Where `window` is now and how big, as gpui says; maximized or on
    /// the whole screen, the size and place it had as a window `before`,
    /// which it goes back to: X11 says only the bounds it has.
    pub fn of(window: &gpui::Window, before: Option<&Placement>, cx: &gpui::App) -> Placement {
        let (bounds, maximized) = match window.window_bounds() {
            gpui::WindowBounds::Windowed(bounds) => (bounds, false),
            gpui::WindowBounds::Maximized(bounds) => match before {
                Some(before) => {
                    return Placement {
                        maximized: true,
                        ..before.clone()
                    };
                }
                None => (bounds, true),
            },
            gpui::WindowBounds::Fullscreen(bounds) => match before {
                Some(before) => return before.clone(),
                None => (bounds, false),
            },
        };
        let at = window.display(cx).and_then(|display| {
            let uuid = display.uuid().ok()?;
            Some(At {
                x: f32::from(bounds.origin.x),
                y: f32::from(bounds.origin.y),
                display: uuid.to_string(),
                display_bounds: bounds_of(display.bounds()),
            })
        });
        Placement {
            width: f32::from(bounds.size.width),
            height: f32::from(bounds.size.height),
            maximized,
            at,
        }
    }

    /// The bounds a window opens with, taking this placement, and the
    /// display it opens on: its own place while its display is there as it
    /// was, else centered on the main display; `cascade` sets it a step
    /// down and right of it, back to the display's corner where that
    /// leaves it (a window made beside the one it comes from). Its size
    /// fits on the display. Where the system places windows itself, it
    /// does so whatever is asked.
    pub fn opening(&self, cascade: f32, displays: &[Screen], primary: Option<&Screen>) -> Opening {
        use gpui::{Bounds, point, px, size};
        let kept = self.at.as_ref().and_then(|at| {
            let display = displays.iter().find(|display| {
                display.uuid.as_deref() == Some(at.display.as_str())
                    && bounds_of(display.bounds)
                        .iter()
                        .zip(at.display_bounds)
                        .all(|(now, then)| (now - then).abs() < 0.5)
            })?;
            Some((display, point(px(at.x), px(at.y))))
        });
        let display = kept.as_ref().map(|(display, _)| *display).or(primary);
        // Where the system names no main display (Wayland), the size still
        // fits one: the first it says it has.
        let room = display.or(displays.first()).map(|display| display.visible);
        let fits = |wanted: f32, smallest: f32, room: Option<f32>| {
            let wanted = if wanted.is_finite() { wanted } else { 0. };
            let at_most = room.unwrap_or(f32::MAX).max(smallest);
            wanted.clamp(smallest, at_most)
        };
        let window = size(
            px(fits(
                self.width,
                SMALLEST.0,
                room.map(|r| f32::from(r.size.width)),
            )),
            px(fits(
                self.height,
                SMALLEST.1,
                room.map(|r| f32::from(r.size.height)),
            )),
        );
        let origin = match (&kept, room) {
            (Some((_, origin)), _) => *origin,
            (None, Some(room)) => room.center() - point(window.width / 2., window.height / 2.),
            (None, None) => point(px(0.), px(0.)),
        };
        let mut bounds = Bounds::new(origin, window);
        if cascade > 0. {
            bounds.origin += point(px(cascade), px(cascade));
            if let Some(room) = room
                && (bounds.right() > room.right() || bounds.bottom() > room.bottom())
            {
                bounds.origin = room.origin;
            }
        }
        let asked = Placement {
            width: f32::from(bounds.size.width),
            height: f32::from(bounds.size.height),
            maximized: self.maximized,
            at: display.and_then(|display| {
                Some(At {
                    x: f32::from(bounds.origin.x),
                    y: f32::from(bounds.origin.y),
                    display: display.uuid.clone()?,
                    display_bounds: bounds_of(display.bounds),
                })
            }),
        };
        Opening {
            bounds: if self.maximized {
                gpui::WindowBounds::Maximized(bounds)
            } else {
                gpui::WindowBounds::Windowed(bounds)
            },
            display: display.map(|display| display.id),
            asked,
        }
    }
}

/// How a window opens (`Placement::opening`).
#[derive(Clone, Debug)]
pub struct Opening {
    pub bounds: gpui::WindowBounds,
    pub display: Option<gpui::DisplayId>,
    /// The placement that asks, which the window keeps while it opens
    /// maximized (`Placement::of`).
    pub asked: Placement,
}

/// What `Placement::opening` needs of a display, read from gpui's
/// (`Screen::all`).
#[derive(Clone, Debug)]
pub struct Screen {
    pub id: gpui::DisplayId,
    /// `PlatformDisplay::uuid`, where the system names it.
    pub uuid: Option<String>,
    pub bounds: gpui::Bounds<gpui::Pixels>,
    /// Without the desktop's panels and docks.
    pub visible: gpui::Bounds<gpui::Pixels>,
}

impl Screen {
    fn of(display: &dyn gpui::PlatformDisplay) -> Screen {
        Screen {
            id: display.id(),
            uuid: display.uuid().ok().map(|uuid| uuid.to_string()),
            bounds: display.bounds(),
            visible: display.visible_bounds(),
        }
    }

    /// The displays, and the main one.
    pub fn all(cx: &gpui::App) -> (Vec<Screen>, Option<Screen>) {
        let all = cx
            .displays()
            .iter()
            .map(|display| Screen::of(&**display))
            .collect();
        (
            all,
            cx.primary_display().map(|display| Screen::of(&*display)),
        )
    }
}

fn bounds_of(bounds: gpui::Bounds<gpui::Pixels>) -> [f32; 4] {
    [
        f32::from(bounds.origin.x),
        f32::from(bounds.origin.y),
        f32::from(bounds.size.width),
        f32::from(bounds.size.height),
    ]
}

impl Shell {
    /// This window's shape right now.
    pub(crate) fn session(&self) -> Session {
        Session {
            view: self.ui().own_view().to_owned(),
            zoom: self.zoom,
            row_zoom: self.row_zoom,
            levels: self.levels,
            usage_mode: self.usage_mode,
            sidebar_hidden: self.sidebar_hidden,
            sidebar_sections: self.sidebar_sections.clone(),
            sidebar_width: self.sidebar_width,
            split_share: self.split_share,
            split: self.is_split(),
            panes: self.layout.panes,
            split_panes: self.layout.split,
            preview: self.preview.preview_on,
            preview_at: self.layout.preview,
            preview_width: self.preview.width,
            member_at: self.layout.member,
            members_height_share: self.preview.members_share.above,
            members_width_share: self.preview.members_share.beside,
            preview_height: self.preview.height.unwrap_or(0.),
            properties_on: self.preview.properties_on,
            properties: None,
            properties_height_share: self.preview.properties_share.above,
            properties_width_share: self.preview.properties_share.beside,
            properties_folds: self.preview.properties_folds.clone(),
            source_types: self.preview.source_types.clone(),
            turned_types: self.preview.turned_types.clone(),
            quick_filters: self.ui().quick_filters_on,
            timeline: self.timeline().open,
            timeline_size: self.timeline().measure == crate::timeline::Measure::Size,
            timeline_date: crate::timeline::DATES[self.timeline().date].0.to_owned(),
            player_height: self.player.height,
            activity_folded: self.activity.folded,
            terminal: self.terminal.open,
            terminal_height: self.terminal.height.unwrap_or(0.),
            column_widths: crate::views::list::COLUMNS
                .iter()
                .filter(|column| (self.column_widths[column.detail] - column.width).abs() >= 0.5)
                .map(|column| {
                    (
                        column.key.to_owned(),
                        self.column_widths[column.detail].round(),
                    )
                })
                .collect(),
            window: Some(self.placement.clone()),
        }
    }

    /// Remember the shape, if it changed: this window's, kept with its tabs
    /// (`Workspace::set_shape`, ADR-122), and, as the window used last,
    /// the shape a new window takes (`SESSION_KEY`). Called after the
    /// handful of actions that change it and as the window comes forward,
    /// never per frame.
    pub(crate) fn remember_session(&mut self) {
        // Nothing is written before the window's shape was read: the
        // tabs are put in place (and present themselves) before the shape
        // is restored, and writing then would replace it with defaults.
        let Some(stored) = &self.stored_session else {
            return;
        };
        let session = self.session();
        let value = match toml::Value::try_from(&session) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!("cannot record the window's shape: {error:#}");
                return;
            }
        };
        if session != *stored {
            self.stored_session = Some(session.clone());
            self.app
                .workspace
                .write()
                .set_shape(self.window_id, Some(value.clone()));
        }
        let used = self.app.workspace.read().active_window_id() == self.window_id;
        if used && self.app.settings.get::<Session>(SESSION_KEY).as_ref() != Some(&session) {
            self.app.settings.set(SESSION_KEY, value);
            self.save_settings_later();
        }
    }
}

impl Session {
    /// What a window could have written, of a file edited by hand: a length
    /// dragged is finite and above 0, a share between 0 and 1, a column's
    /// width finite; anything else is 0, not dragged, as before the first
    /// drag (`inf` and `nan` are TOML too, and nothing draws them).
    fn written(mut self) -> Self {
        let length = |value: &mut f32| {
            if !(value.is_finite() && *value > 0.) {
                *value = 0.;
            }
        };
        let share = |value: &mut f32| {
            if !(*value > 0. && *value < 1.) {
                *value = 0.;
            }
        };
        for value in [
            &mut self.sidebar_width,
            &mut self.preview_width,
            &mut self.preview_height,
            &mut self.player_height,
            &mut self.terminal_height,
        ] {
            length(value);
        }
        for value in [
            &mut self.split_share,
            &mut self.members_height_share,
            &mut self.members_width_share,
            &mut self.properties_height_share,
            &mut self.properties_width_share,
        ] {
            share(value);
        }
        self.column_widths.retain(|_, width| width.is_finite());
        // A place that is no number is no place; a size is fitted where it
        // opens (`Placement::opening`).
        if let Some(window) = &mut self.window
            && window
                .at
                .as_ref()
                .is_some_and(|at| !(at.x.is_finite() && at.y.is_finite()))
        {
            window.at = None;
        }
        self
    }

    /// What an older window wrote, in today's words: Properties open under
    /// a preview showed only while the panel was open, so its switch is on
    /// only where the panel was; a closed panel stays closed.
    fn upgraded(mut self) -> Self {
        if let Some(under) = self.properties.take() {
            self.properties_on |= under && self.preview;
        }
        self
    }
}

/// What was remembered, if anything was, as a window could have written it.
pub fn restore(app: &skwir_core::App) -> Option<Session> {
    let session = app
        .settings
        .get::<Session>(SESSION_KEY)?
        .written()
        .upgraded();
    (session != Session::default()).then_some(session)
}

/// A window's shape as the core keeps it with its tabs (ADR-122,
/// `Workspace::set_shape`).
pub fn from_value(value: toml::Value) -> Option<Session> {
    let session = value.try_into::<Session>().ok()?.written().upgraded();
    (session != Session::default()).then_some(session)
}

/// Quick filters as Settings kept them, before each pane had its own (a
/// switch under Folders and views until 2026-11-07): `Some` once, while the
/// file still says it, which is forgotten from then on.
pub fn former_quick_filters(app: &skwir_core::App) -> Option<bool> {
    let key = crate::settings::QUICK_FILTERS_FORMER_KEY;
    let value = app.settings.get::<toml::Value>(key)?;
    app.settings.unset(key);
    Some(value.as_bool() == Some(true))
}

pub fn declare(app: &skwir_core::App) {
    app.settings
        .declare(skwir_core::settings::SettingSpec::state(
            SESSION_KEY,
            "The last window's view, split, preview and timeline",
            toml::Value::Table(toml::map::Map::new()),
        ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Panes, Preview};

    #[test]
    fn the_arrangement_survives_the_settings_file() {
        for split_panes in [None, Some(Panes::Column)] {
            let session = Session {
                split: true,
                panes: Panes::Row,
                split_panes,
                preview_at: Preview::Bottom,
                preview_height: 300.,
                quick_filters: true,
                timeline: true,
                ..Session::default()
            };
            let value = toml::Value::try_from(&session).unwrap();
            assert_eq!(value.try_into::<Session>().unwrap(), session);
        }
        // What a hand could write and no window would: not dragged.
        let edited: Session = toml::from_str(
            "sidebar_width = inf\npreview_width = -3.0\npreview_height = nan\n\
             split_share = 1.5\nplayer_height = 120.0\n\
             [column_widths]\nsize = nan\nmodified = 140.0",
        )
        .unwrap();
        let edited = edited.written();
        assert_eq!(
            (
                edited.sidebar_width,
                edited.preview_width,
                edited.preview_height
            ),
            (0., 0., 0.)
        );
        assert_eq!((edited.split_share, edited.player_height), (0., 120.));
        assert_eq!(
            edited.column_widths.into_iter().collect::<Vec<_>>(),
            [("modified".to_owned(), 140.)]
        );
        // What a file does not say reads as the default: automatic.
        let sparse: Session = toml::from_str("split = true\npreview = true").unwrap();
        assert_eq!(
            (sparse.panes, sparse.preview_at),
            (Panes::Auto, Preview::Auto)
        );
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> gpui::Bounds<gpui::Pixels> {
        gpui::Bounds::new(
            gpui::point(gpui::px(x), gpui::px(y)),
            gpui::size(gpui::px(width), gpui::px(height)),
        )
    }

    fn screen(id: u64, name: &str, bounds: gpui::Bounds<gpui::Pixels>) -> Screen {
        Screen {
            id: gpui::DisplayId::new(id),
            uuid: Some(name.to_owned()),
            bounds,
            visible: bounds,
        }
    }

    #[test]
    fn a_window_opens_where_it_was_while_its_display_is_there() {
        let main = screen(1, "main", rect(0., 0., 1920., 1080.));
        let side = screen(2, "side", rect(1920., 0., 1280., 1024.));
        let displays = [main.clone(), side.clone()];
        let on_side = Placement {
            width: 800.,
            height: 600.,
            maximized: false,
            at: Some(At {
                x: 2000.,
                y: 100.,
                display: "side".into(),
                display_bounds: [1920., 0., 1280., 1024.],
            }),
        };
        let windowed = |bounds: gpui::WindowBounds| match bounds {
            gpui::WindowBounds::Windowed(bounds) => bounds,
            other => panic!("not windowed: {other:?}"),
        };

        // Where it was, on its display.
        let opened = on_side.opening(0., &displays, Some(&main));
        assert_eq!(windowed(opened.bounds), rect(2000., 100., 800., 600.));
        assert_eq!(opened.display, Some(side.id));
        assert_eq!(opened.asked, on_side);

        // A step from it; past the display's edge, back to its corner.
        let opened = on_side.opening(32., &displays, Some(&main));
        assert_eq!(windowed(opened.bounds), rect(2032., 132., 800., 600.));
        let low = Placement {
            at: Some(At {
                y: 420.,
                ..on_side.at.clone().unwrap()
            }),
            ..on_side.clone()
        };
        let opened = low.opening(32., &displays, Some(&main));
        assert_eq!(windowed(opened.bounds), rect(1920., 0., 800., 600.));

        // The display gone, or no longer as it was: centered on the main one.
        let opened = on_side.opening(0., std::slice::from_ref(&main), Some(&main));
        assert_eq!(windowed(opened.bounds), rect(560., 240., 800., 600.));
        assert_eq!(opened.display, Some(main.id));
        assert_eq!(
            opened.asked.at.map(|at| at.display),
            Some("main".to_owned())
        );
        let moved = screen(2, "side", rect(0., 1080., 1280., 1024.));
        let opened = on_side.opening(0., &[main.clone(), moved], Some(&main));
        assert_eq!(opened.display, Some(main.id));

        // Its size fits the display; what is no size is the smallest.
        let huge = Placement {
            width: 5000.,
            height: f32::NAN,
            maximized: true,
            at: None,
        };
        let opened = huge.opening(0., &displays, Some(&main));
        let gpui::WindowBounds::Maximized(bounds) = opened.bounds else {
            panic!("not maximized: {:?}", opened.bounds);
        };
        assert_eq!(bounds.size, rect(0., 0., 1920., 240.).size);

        // No main display named (Wayland): the size fits one all the same.
        let opened = huge.opening(0., &displays, None);
        let gpui::WindowBounds::Maximized(bounds) = opened.bounds else {
            panic!("not maximized: {:?}", opened.bounds);
        };
        assert_eq!(bounds.size, rect(0., 0., 1920., 240.).size);
        assert_eq!(opened.display, None, "no display asked for");
    }

    #[test]
    fn a_place_survives_the_state_file_and_a_hand_that_breaks_it() {
        let session = Session {
            window: Some(Placement {
                width: 900.,
                height: 700.,
                maximized: true,
                at: Some(At {
                    x: 10.,
                    y: 20.,
                    display: "a display".into(),
                    display_bounds: [0., 0., 1920., 1080.],
                }),
            }),
            ..Session::default()
        };
        let value = toml::Value::try_from(&session).unwrap();
        assert_eq!(value.try_into::<Session>().unwrap(), session);
        // Written before windows were placed: no place, as before.
        let before: Session = toml::from_str("split = true").unwrap();
        assert_eq!(before.window, None);
        let edited: Session = toml::from_str(
            "[window]\nwidth = 900.0\nheight = 700.0\n\
             [window.at]\nx = nan\ny = 3.0\ndisplay = \"d\"\ndisplay_bounds = [0.0, 0.0, 10.0, 10.0]",
        )
        .unwrap();
        let edited = edited.written();
        assert_eq!(edited.window.map(|window| window.at), Some(None));
    }

    #[test]
    fn properties_under_a_closed_panel_stay_closed() {
        let read = |text: &str| {
            let session: Session = toml::from_str(text).unwrap();
            let session = session.upgraded();
            (session.preview, session.properties_on)
        };
        // Before the two switches: Properties open under a preview showed
        // only with the panel.
        assert_eq!(read("preview = false\nproperties = true"), (false, false));
        assert_eq!(read("preview = true\nproperties = true"), (true, true));
        assert_eq!(read("preview = true\nproperties = false"), (true, false));
        // Today's: Properties alone, the Preview off.
        assert_eq!(read("preview = false\nproperties_on = true"), (false, true));
        // The old word is never written again.
        let session = Session {
            properties_on: true,
            ..Session::default()
        };
        let text = toml::to_string(&session).unwrap();
        assert!(text.contains("properties_on = true"), "{text}");
        assert!(!text.contains("properties ="), "{text}");
    }
}
