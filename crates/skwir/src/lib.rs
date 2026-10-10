//! What the binary wires, shared with its tests: the plugin list lives here
//! once, so a new plugin cannot be registered without also being covered by
//! `tests/plugins_toggle.rs`.

use std::sync::Arc;

use skwir_core::plugin::Plugin;

pub mod cli;

/// Every plugin, in registration order. Each registers its commands,
/// bindings, menus and places, and can be toggled at runtime through
/// `plugins.<id>.enabled` in settings.
pub fn plugins() -> Vec<Arc<dyn Plugin>> {
    let mut plugins = built_in();
    // Then those from outside skwir its folders of plugins hold, each
    // off until the user turns it on (ADR-045).
    let taken: Vec<&str> = plugins.iter().map(|plugin| plugin.id()).collect();
    let outside = skwir_extend::discover(&skwir_extend::folders(), &taken);
    plugins.extend(outside.into_iter().map(|plugin| plugin as Arc<dyn Plugin>));
    plugins
}

/// The plugins built into skwir, in registration order.
fn built_in() -> Vec<Arc<dyn Plugin>> {
    vec![
        Arc::new(skwir_plugin_ops_basic::OpsBasicPlugin::default()),
        Arc::new(skwir_plugin_open_with::OpenWithPlugin::default()),
        Arc::new(skwir_plugin_places::PlacesPlugin::default()),
        Arc::new(skwir_plugin_thumbnails::ThumbnailsPlugin::default()),
        // Phase 4 power features, each a plugin with a runtime toggle.
        Arc::new(skwir_plugin_query::QueryPlugin::default()),
        Arc::new(skwir_plugin_shelf::ShelfPlugin::default()),
        Arc::new(skwir_plugin_rename_bulk::RenameBulkPlugin),
        Arc::new(skwir_plugin_edit_as_buffer::EditAsBufferPlugin::default()),
        Arc::new(skwir_plugin_jump::JumpPlugin::default()),
        Arc::new(skwir_plugin_trash::TrashPlugin::default()),
        Arc::new(skwir_plugin_tags::TagsPlugin::default()),
        Arc::new(skwir_plugin_xmp::XmpPlugin),
        Arc::new(skwir_plugin_tagspaces::TagSpacesPlugin),
        Arc::new(skwir_plugin_name_tags::NameTagsPlugin),
        Arc::new(skwir_plugin_shell::ShellPlugin),
        Arc::new(skwir_plugin_terminal::TerminalPlugin::default()),
        Arc::new(skwir_plugin_usage::UsagePlugin::default()),
        Arc::new(skwir_plugin_timeline::TimelinePlugin::default()),
        Arc::new(skwir_plugin_preview_media::PreviewPlugin::default()),
        Arc::new(skwir_plugin_preview_documents::PreviewPlugin::default()),
        Arc::new(skwir_plugin_preview_markdown::PreviewPlugin),
        Arc::new(skwir_plugin_preview_archives::PreviewPlugin),
        Arc::new(skwir_plugin_preview_epub::PreviewPlugin),
        Arc::new(skwir_plugin_preview_tables::PreviewPlugin),
        Arc::new(skwir_plugin_preview_playlists::PreviewPlugin),
        // The free desktops' media keys (D-Bus): not on macOS or Windows,
        // whose own are plugins still to write (ADR-035, step I).
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        Arc::new(skwir_plugin_mpris::MprisPlugin::default()),
        Arc::new(skwir_plugin_control::Control::default()),
        Arc::new(skwir_plugin_preview_structured::PreviewPlugin),
        Arc::new(skwir_plugin_preview_fonts::PreviewPlugin),
        Arc::new(skwir_plugin_preview_models::PreviewPlugin),
        Arc::new(skwir_plugin_preview_circuits::PreviewPlugin),
        Arc::new(skwir_plugin_preview_email::PreviewPlugin),
        Arc::new(skwir_plugin_preview_comics::PreviewPlugin),
        Arc::new(skwir_plugin_geo::GeoPlugin::default()),
        Arc::new(skwir_plugin_map_tiles::MapTilesPlugin::default()),
        Arc::new(skwir_plugin_preview_hex::PreviewPlugin),
        // A key scheme: off until the user turns it on.
        Arc::new(skwir_plugin_vim::VimPlugin::default()),
    ]
}
