//! The bibli binary. Wires the core, the plugins, and the UI shell (Phase 2).

mod gpu;

use std::time::Instant;

use bibli_core::app::AppConfig;
use bibli_core::{App, Location};

fn main() -> anyhow::Result<()> {
    let started = Instant::now();
    // Before anything allocates much: freed memory goes back (ADR-011).
    bibli_alloc::tune();
    // Before any GPU work: keep the Vulkan loader from mapping drivers for
    // hardware that is not here (ADR-002).
    let restricted = gpu::restrict_vulkan_drivers();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    if let Some(disabled) = restricted {
        tracing::debug!(%disabled, "restricting Vulkan drivers");
    }

    let app = App::new(AppConfig::default())?;

    // Plugins: each registers its commands, bindings, menus, and places, and
    // can be toggled at runtime through `plugins.<id>.enabled` in settings.
    for plugin in bibli::plugins() {
        app.plugins.add(plugin, &app);
    }

    // A folder on the command line wins; otherwise the startup preference:
    // a chosen folder, or the folders and tabs left open.
    let initial = match std::env::args_os().nth(1) {
        Some(arg) => {
            let path = std::path::PathBuf::from(arg);
            Some(std::path::absolute(&path).unwrap_or(path))
        }
        None => match bibli_core::tabs::last_tabs(&app.settings) {
            Some(saved) if app.restore_tabs(&saved) => None,
            _ => {
                let home =
                    std::env::var_os("HOME").map_or_else(|| "/".into(), std::path::PathBuf::from);
                let path = bibli_core::settings::startup_folder(&app.settings, &home);
                Some(std::path::absolute(&path).unwrap_or(path))
            }
        },
    };
    // The folder is not logged: it can come from a preference, and paths
    // are private.
    tracing::info!("starting");
    bibli_ui::run_from(app, initial.map(Location::from_path), started)
}
