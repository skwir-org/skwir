//! What the binary wires, shared with its tests: the plugin list lives here
//! once, so a new plugin cannot be registered without also being covered by
//! `tests/plugins_toggle.rs`.

use std::sync::Arc;

use bibli_core::plugin::Plugin;

/// Every plugin, in registration order. Each registers its commands,
/// bindings, menus and places, and can be toggled at runtime through
/// `plugins.<id>.enabled` in settings.
pub fn plugins() -> Vec<Arc<dyn Plugin>> {
    vec![
        Arc::new(bibli_plugin_ops_basic::OpsBasicPlugin::default()),
        Arc::new(bibli_plugin_open_with::OpenWithPlugin),
        Arc::new(bibli_plugin_places::PlacesPlugin::default()),
        Arc::new(bibli_plugin_thumbnails::ThumbnailsPlugin::default()),
        // Phase 4 power features, each a plugin with a runtime toggle.
        Arc::new(bibli_plugin_query::QueryPlugin::default()),
        Arc::new(bibli_plugin_shelf::ShelfPlugin::default()),
        Arc::new(bibli_plugin_rename_bulk::RenameBulkPlugin),
        Arc::new(bibli_plugin_edit_as_buffer::EditAsBufferPlugin::default()),
        Arc::new(bibli_plugin_jump::JumpPlugin::default()),
        Arc::new(bibli_plugin_tags::TagsPlugin),
        Arc::new(bibli_plugin_shell::ShellPlugin),
        Arc::new(bibli_plugin_usage::UsagePlugin::default()),
        Arc::new(bibli_plugin_timeline::TimelinePlugin::default()),
        Arc::new(bibli_plugin_preview_media::PreviewPlugin),
        Arc::new(bibli_plugin_preview_documents::PreviewPlugin),
        Arc::new(bibli_plugin_preview_audio::PreviewPlugin::default()),
        Arc::new(bibli_plugin_preview_markdown::PreviewPlugin),
        Arc::new(bibli_plugin_preview_archives::PreviewPlugin),
        Arc::new(bibli_plugin_preview_epub::PreviewPlugin),
        Arc::new(bibli_plugin_preview_tables::PreviewPlugin),
        Arc::new(bibli_plugin_preview_playlists::PreviewPlugin),
        Arc::new(bibli_plugin_mpris::MprisPlugin::default()),
        Arc::new(bibli_plugin_preview_structured::PreviewPlugin),
        Arc::new(bibli_plugin_preview_fonts::PreviewPlugin),
        Arc::new(bibli_plugin_preview_email::PreviewPlugin),
        Arc::new(bibli_plugin_preview_comics::PreviewPlugin),
        Arc::new(bibli_plugin_preview_hex::PreviewPlugin),
    ]
}
