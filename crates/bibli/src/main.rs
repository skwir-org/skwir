//! The bibli binary. Wires the core, the plugins, and the UI shell (Phase 2).

mod gpu;

use std::time::Instant;

use bibli_core::app::AppConfig;
use bibli_core::{App, Location};

fn main() -> anyhow::Result<()> {
    let started = Instant::now();
    // Before any GPU work: keep the Vulkan loader from mapping drivers for
    // hardware that is not here (ADR-002).
    gpu::restrict_vulkan_drivers();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let app = App::new(AppConfig::default())?;

    // Plugins: each registers its commands, bindings, menus, and places, and
    // can be toggled at runtime through `plugins.<id>.enabled` in settings.
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_ops_basic::OpsBasicPlugin::default()),
        &app,
    );
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_open_with::OpenWithPlugin),
        &app,
    );
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_places::PlacesPlugin::default()),
        &app,
    );
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_thumbnails::ThumbnailsPlugin),
        &app,
    );
    // Phase 4 power features, each a plugin with a runtime toggle.
    app.plugins
        .add(std::sync::Arc::new(bibli_plugin_query::QueryPlugin), &app);
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_shelf::ShelfPlugin::default()),
        &app,
    );
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_rename_bulk::RenameBulkPlugin),
        &app,
    );
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_edit_as_buffer::EditAsBufferPlugin),
        &app,
    );
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_jump::JumpPlugin::default()),
        &app,
    );
    app.plugins
        .add(std::sync::Arc::new(bibli_plugin_tags::TagsPlugin), &app);
    app.plugins
        .add(std::sync::Arc::new(bibli_plugin_shell::ShellPlugin), &app);
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_usage::UsagePlugin::default()),
        &app,
    );
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_timeline::TimelinePlugin::default()),
        &app,
    );

    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_preview_media::PreviewPlugin),
        &app,
    );
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_preview_documents::PreviewPlugin),
        &app,
    );

    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_preview_audio::PreviewPlugin),
        &app,
    );

    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_preview_markdown::PreviewPlugin),
        &app,
    );
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_preview_archives::PreviewPlugin),
        &app,
    );
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_preview_epub::PreviewPlugin),
        &app,
    );
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_preview_tables::PreviewPlugin),
        &app,
    );
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_preview_playlists::PreviewPlugin),
        &app,
    );
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_preview_structured::PreviewPlugin),
        &app,
    );
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_preview_fonts::PreviewPlugin),
        &app,
    );
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_preview_email::PreviewPlugin),
        &app,
    );
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_preview_comics::PreviewPlugin),
        &app,
    );
    app.plugins.add(
        std::sync::Arc::new(bibli_plugin_preview_hex::PreviewPlugin),
        &app,
    );

    // A folder on the command line wins; otherwise the startup preference.
    let path = std::env::args_os().nth(1).map_or_else(
        || {
            let home =
                std::env::var_os("HOME").map_or_else(|| "/".into(), std::path::PathBuf::from);
            bibli_core::settings::startup_folder(&app.settings, &home)
        },
        std::path::PathBuf::from,
    );
    let path = std::path::absolute(&path).unwrap_or(path);
    tracing::info!(path = %path.display(), "starting");
    bibli_ui::run_from(app, Some(Location::from_path(path)), started)
}
