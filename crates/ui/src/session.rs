//! What the window looked like, so the next one looks the same.
//!
//! A preference is something the user chose in Settings; this is not that. It
//! is the shape of the last window — which view each pane showed, whether it
//! was split, whether the preview and the timeline were open and how wide the
//! preview was — kept as application data under one key and restored at
//! startup. Nothing here is worth a dialog, and all of it is annoying to set
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
    pub split: bool,
    pub preview: bool,
    pub preview_width: f32,
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
            split: self.is_split(),
            preview: self.preview.open,
            preview_width: self.preview.width,
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
