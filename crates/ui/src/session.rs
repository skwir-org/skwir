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
    /// Whether Details is open under the preview.
    pub details: bool,
    pub timeline: bool,
    /// How tall the pinned player's dock was dragged.
    pub player_height: f32,
    /// Whether the terminal was open under the panes.
    pub terminal: bool,
    /// How tall its edge was dragged; 0 until then.
    pub terminal_height: f32,
    /// The list's detail columns dragged to another width than their own,
    /// by `Column::key`.
    #[serde(deserialize_with = "widths")]
    pub column_widths: std::collections::BTreeMap<String, f32>,
}

/// Widths by column; a list is the setting they were kept in before
/// (`ui.column_widths`, moved here by `declare`), in its order.
fn widths<'de, D: serde::Deserializer<'de>>(
    from: D,
) -> Result<std::collections::BTreeMap<String, f32>, D::Error> {
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Saved {
        Named(std::collections::BTreeMap<String, f32>),
        Listed(Vec<f32>),
    }
    Ok(match serde::Deserialize::deserialize(from)? {
        Saved::Named(widths) => widths,
        Saved::Listed(widths) => ["size", "modified", "type", "rating", "folder"]
            .into_iter()
            .map(str::to_owned)
            .zip(widths)
            .collect(),
    })
}

impl Shell {
    /// This window's shape right now.
    pub(crate) fn session(&self) -> Session {
        Session {
            view: self.ui().view_id.to_owned(),
            zoom: self.zoom,
            row_zoom: self.row_zoom,
            levels: self.levels,
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
            details: self.preview.details_open,
            timeline: self.timeline.open,
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

/// What was remembered, if anything was.
pub fn restore(app: &bibli_core::App) -> Option<Session> {
    let session = app.settings.get::<Session>(SESSION_KEY)?;
    (session != Session::default()).then_some(session)
}

pub fn declare(app: &bibli_core::App) {
    // The columns' widths were a setting of their own.
    app.settings
        .rename("ui.column_widths", &format!("{SESSION_KEY}.column_widths"));
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

    #[test]
    fn column_widths_kept_as_a_setting_are_read_by_name() {
        let old: Session = toml::from_str("column_widths = [90, 136, 150]").unwrap();
        let widths: Vec<(&str, f32)> = old
            .column_widths
            .iter()
            .map(|(key, width)| (key.as_str(), *width))
            .collect();
        assert_eq!(widths, [("modified", 136.), ("size", 90.), ("type", 150.)]);
        let named: Session = toml::from_str(
            "[column_widths]
folder = 200.0",
        )
        .unwrap();
        assert_eq!(named.column_widths.get("folder"), Some(&200.));
    }
}
