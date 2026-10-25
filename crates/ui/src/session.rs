//! What the window looked like, so the next one looks the same.
//!
//! A preference is something the user chose in Settings; this is not that. It
//! is the shape of the last window — which view each pane showed, whether it
//! was split and how, whether the preview, the timeline and the terminal were
//! open, where the preview was and how big, how the sidebar's groups were
//! arranged — kept as application data under one key and
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
    /// Whether Properties is open under the preview.
    pub properties: bool,
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
    pub timeline: bool,
    /// Whether its bars measure sizes rather than files.
    pub timeline_size: bool,
    /// The time they count (`timeline::DATES`); modified when empty.
    pub timeline_date: String,
    /// How tall the pinned player's dock was dragged.
    pub player_height: f32,
    /// Whether the terminal was open under the panes.
    pub terminal: bool,
    /// How tall its edge was dragged; 0 until then.
    pub terminal_height: f32,
    /// The list's detail columns dragged to another width than their own,
    /// by `Column::key`.
    pub column_widths: std::collections::BTreeMap<String, f32>,
}

impl Shell {
    /// This window's shape right now.
    pub(crate) fn session(&self) -> Session {
        Session {
            view: self.ui().view_id.to_owned(),
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
            preview: self.preview.open,
            preview_at: self.layout.preview,
            preview_width: self.preview.width,
            member_at: self.layout.member,
            members_height_share: self.preview.members_share.above,
            members_width_share: self.preview.members_share.beside,
            preview_height: self.preview.height.unwrap_or(0.),
            properties: self.preview.properties_open,
            properties_height_share: self.preview.properties_share.above,
            properties_width_share: self.preview.properties_share.beside,
            properties_folds: self.preview.properties_folds.clone(),
            source_types: self.preview.source_types.clone(),
            turned_types: self.preview.turned_types.clone(),
            timeline: self.timeline.open,
            timeline_size: self.timeline.measure == crate::timeline::Measure::Size,
            timeline_date: crate::timeline::DATES[self.timeline.date].0.to_owned(),
            player_height: self.player.height,
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
        }
    }

    /// Remember the shape, if it changed. Called after the handful of actions
    /// that change it, never per frame.
    pub(crate) fn remember_session(&mut self) {
        // Nothing is written before the last window's shape was read: the
        // tabs are put in place (and present themselves) before the shape
        // is restored, and writing then would replace it with defaults.
        let Some(stored) = &self.stored_session else {
            return;
        };
        let session = self.session();
        if session == *stored {
            return;
        }
        self.stored_session = Some(session.clone());
        match toml::Value::try_from(&session) {
            Ok(value) => {
                self.app.settings.set(SESSION_KEY, value);
                self.save_settings_later();
            }
            Err(error) => tracing::warn!("cannot record the window's shape: {error:#}"),
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
        self
    }
}

/// What was remembered, if anything was, as a window could have written it.
pub fn restore(app: &skwir_core::App) -> Option<Session> {
    let session = app.settings.get::<Session>(SESSION_KEY)?.written();
    (session != Session::default()).then_some(session)
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
}
