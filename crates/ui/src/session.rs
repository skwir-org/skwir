//! What the window looked like, so the next one looks the same.
//!
//! A preference is something the user chose in Settings; this is not that. It
//! is the shape of the last window — which view each pane showed, whether it
//! was split and how, whether the preview and the timeline were open, where
//! the preview was and how big — kept as application data under one key and
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
    /// The sidebar put away with F9 (it also gives way to a narrow window).
    pub sidebar_hidden: bool,
    pub split: bool,
    /// Split panes side by side, stacked, or as the window's shape says.
    pub panes: crate::layout::Panes,
    /// The open split's own arrangement, when it was made the other way.
    pub split_panes: Option<crate::layout::Panes>,
    pub preview: bool,
    /// The preview at the right, at the bottom, or as the window's shape says.
    pub preview_at: crate::layout::Preview,
    pub preview_width: f32,
    /// How tall the preview is when it is at the bottom, once dragged; 0
    /// until then (the default makes room for a pinned player).
    pub preview_height: f32,
    /// Whether Details is open under the preview.
    pub details: bool,
    pub timeline: bool,
    /// How tall the pinned player's dock was dragged.
    pub player_height: f32,
}

impl Shell {
    /// This window's shape right now.
    pub(crate) fn session(&self) -> Session {
        Session {
            view: self.ui().view_id.to_owned(),
            zoom: self.zoom,
            row_zoom: self.row_zoom,
            sidebar_hidden: self.sidebar_hidden,
            split: self.is_split(),
            panes: self.layout.panes,
            split_panes: self.layout.split,
            preview: self.preview.open,
            preview_at: self.layout.preview,
            preview_width: self.preview.width,
            preview_height: self.preview.height.unwrap_or(0.),
            details: self.preview.details_open,
            timeline: self.timeline.open,
            player_height: self.player.height,
        }
    }

    /// Remember the shape, if it changed. Called after the handful of actions
    /// that change it, never per frame.
    pub(crate) fn remember_session(&mut self) {
        let session = self.session();
        if session == self.stored_session {
            return;
        }
        self.stored_session = session.clone();
        match toml::Value::try_from(&session) {
            Ok(value) => {
                self.app.settings.set(SESSION_KEY, value);
                self.save_settings_later();
            }
            Err(error) => tracing::warn!("cannot record the window's shape: {error:#}"),
        }
    }
}

/// What was remembered, if anything was.
pub fn restore(app: &bibli_core::App) -> Option<Session> {
    let session = app.settings.get::<Session>(SESSION_KEY)?;
    (session != Session::default()).then_some(session)
}

pub fn declare(app: &bibli_core::App) {
    app.settings
        .declare(bibli_core::settings::SettingSpec::data(
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
        // A file written before the arrangement existed reads as automatic.
        let old: Session = toml::from_str("split = true\npreview = true").unwrap();
        assert_eq!((old.panes, old.preview_at), (Panes::Auto, Preview::Auto));
    }
}
