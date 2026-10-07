//! What the binary wires, shared with its tests: the plugin list lives here
//! once, so a new plugin cannot be registered without also being covered by
//! `tests/plugins_toggle.rs`.

use std::sync::Arc;

use bibli_core::plugin::Plugin;

pub mod cli;

/// Every plugin, in registration order. Each registers its commands,
/// bindings, menus and places, and can be toggled at runtime through
/// `plugins.<id>.enabled` in settings.
pub fn plugins() -> Vec<Arc<dyn Plugin>> {
    let mut plugins = built_in();
    // Then those from outside bibli its folders of plugins hold, each
    // off until the user turns it on (ADR-045).
    let taken: Vec<&str> = plugins.iter().map(|plugin| plugin.id()).collect();
    let outside = bibli_extend::discover(&bibli_extend::folders(), &taken);
    plugins.extend(outside.into_iter().map(|plugin| plugin as Arc<dyn Plugin>));
    plugins
}

/// The plugins built into bibli, in registration order.
fn built_in() -> Vec<Arc<dyn Plugin>> {
    vec![
        Arc::new(bibli_plugin_ops_basic::OpsBasicPlugin::default()),
        Arc::new(bibli_plugin_open_with::OpenWithPlugin::default()),
        Arc::new(bibli_plugin_places::PlacesPlugin::default()),
        Arc::new(bibli_plugin_thumbnails::ThumbnailsPlugin::default()),
        // Phase 4 power features, each a plugin with a runtime toggle.
        Arc::new(bibli_plugin_query::QueryPlugin::default()),
        Arc::new(bibli_plugin_shelf::ShelfPlugin::default()),
        Arc::new(bibli_plugin_rename_bulk::RenameBulkPlugin),
        Arc::new(bibli_plugin_edit_as_buffer::EditAsBufferPlugin::default()),
        Arc::new(bibli_plugin_jump::JumpPlugin::default()),
        Arc::new(bibli_plugin_trash::TrashPlugin::default()),
        Arc::new(bibli_plugin_tags::TagsPlugin::default()),
        Arc::new(bibli_plugin_xmp::XmpPlugin),
        Arc::new(bibli_plugin_tagspaces::TagSpacesPlugin),
        Arc::new(bibli_plugin_name_tags::NameTagsPlugin),
        Arc::new(bibli_plugin_shell::ShellPlugin),
        Arc::new(bibli_plugin_terminal::TerminalPlugin::default()),
        Arc::new(bibli_plugin_usage::UsagePlugin::default()),
        Arc::new(bibli_plugin_timeline::TimelinePlugin::default()),
        Arc::new(bibli_plugin_preview_media::PreviewPlugin::default()),
        Arc::new(bibli_plugin_preview_documents::PreviewPlugin::default()),
        Arc::new(bibli_plugin_preview_markdown::PreviewPlugin),
        Arc::new(bibli_plugin_preview_archives::PreviewPlugin),
        Arc::new(bibli_plugin_preview_epub::PreviewPlugin),
        Arc::new(bibli_plugin_preview_tables::PreviewPlugin),
        Arc::new(bibli_plugin_preview_playlists::PreviewPlugin),
        // The free desktops' media keys (D-Bus): not on macOS or Windows,
        // whose own are plugins still to write (ADR-035, step I).
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        Arc::new(bibli_plugin_mpris::MprisPlugin::default()),
        Arc::new(bibli_plugin_control::Control::default()),
        Arc::new(bibli_plugin_preview_structured::PreviewPlugin),
        Arc::new(bibli_plugin_preview_fonts::PreviewPlugin),
        Arc::new(bibli_plugin_preview_models::PreviewPlugin),
        Arc::new(bibli_plugin_preview_circuits::PreviewPlugin),
        Arc::new(bibli_plugin_preview_email::PreviewPlugin),
        Arc::new(bibli_plugin_preview_comics::PreviewPlugin),
        Arc::new(bibli_plugin_geo::GeoPlugin::default()),
        Arc::new(bibli_plugin_map_tiles::MapTilesPlugin::default()),
        Arc::new(bibli_plugin_preview_hex::PreviewPlugin),
        // A key scheme: off until the user turns it on.
        Arc::new(bibli_plugin_vim::VimPlugin::default()),
    ]
}
