//! The skwir binary. Wires the core, the plugins, and the UI shell (Phase 2).

// What ships for Windows (ADR-074) is a program of the window subsystem: no
// console opens behind it from Explorer or the Start menu. A debug build
// keeps its console, for the tests' output under Wine.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod gpu;

use std::time::Instant;

use skwir_core::app::AppConfig;
use skwir_core::{App, Location};

/// A program of Windows' window subsystem starts without a console: one
/// started from a terminal prints to it (`skwir --version`, `skwir cmd`),
/// unless its output already goes elsewhere (a pipe, as an agent's `skwir
/// mcp` has). Started from Explorer, there is none to take. The terminal
/// does not wait for it: its prompt may come back before the output.
#[cfg(all(windows, not(debug_assertions)))]
#[allow(unsafe_code)]
fn attach_console() {
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
    };
    // SAFETY: neither call takes a pointer; the handle asked for is only
    // compared with null, before any other thread exists.
    unsafe {
        if GetStdHandle(STD_OUTPUT_HANDLE).is_null() {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

fn main() -> anyhow::Result<()> {
    let started = Instant::now();
    // Before anything allocates much: freed memory goes back (ADR-011),
    // and a worker idle after its jobs gives back what they left.
    skwir_alloc::tune();
    // Run by ssh (`SSH_ASKPASS`) or rclone (`--password-command`) while the
    // window connects to a server (ADR-077): ask the window over the
    // connection's socket, print the answer, and nothing else.
    if let Some(socket) = std::env::var_os(skwir_platform::askpass::SOCKET_VAR) {
        let prompt = std::env::args().nth(1);
        let answered = skwir_platform::askpass::answer_as_askpass(
            std::path::Path::new(&socket),
            prompt.as_deref(),
        );
        std::process::exit(i32::from(!answered));
    }
    skwir_core::scheduler::release_when_idle(skwir_alloc::collect);
    // Which build this is, for a helper's failure to say when the helper
    // is of another (`process::explain`).
    skwir_platform::process::set_build(env!("SKWIR_BUILD"));
    #[cfg(all(windows, not(debug_assertions)))]
    attach_console();
    // `skwir mcp` and `skwir cmd` (ADR-043), before anything logs: their
    // standard output is the protocol, or the JSON a script reads.
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if let Some(code) = skwir::cli::run(&args) {
        std::process::exit(code);
    }
    // A Skwir finishing what runs without its window takes this launch
    // over: it opens its window again, showing what this was asked to
    // (ADR-115). Where none is, this costs a look at one file.
    let cwd = std::env::current_dir().unwrap_or_default();
    let opening = skwir_platform::handover::Opening {
        location: args.first().cloned(),
        cwd: cwd.clone(),
        token: skwir_platform::handover::launch_token(),
    };
    if skwir_platform::handover::hand_over(&opening) {
        return Ok(());
    }
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
    for plugin in skwir::plugins() {
        app.plugins.add(plugin, &app);
    }

    // A location on the command line wins: one a plugin opens (`trash:`,
    // `tags:/family`), else a folder. Otherwise the startup preference: a
    // chosen folder, or the windows and tabs left open (ADR-122).
    let folder =
        |path: std::path::PathBuf| Location::from_path(std::path::absolute(&path).unwrap_or(path));
    let initial = match args.first() {
        Some(arg) => Some(skwir_ui::location_of(&app, arg, &cwd)),
        None => match skwir_core::tabs::last_windows(&app.settings) {
            Some(saved) if app.restore_windows(&saved) => None,
            _ => {
                let home =
                    skwir_core::os_path::home().unwrap_or_else(|| std::path::PathBuf::from("/"));
                Some(folder(skwir_core::settings::startup_folder(
                    &app.settings,
                    &home,
                )))
            }
        },
    };
    // The folder is not logged: it can come from a preference, and paths
    // are private.
    tracing::info!("starting");
    skwir_ui::run_from(app, initial, started)
}
