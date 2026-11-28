//! The skwir binary. Wires the core, the plugins, and the UI shell (Phase 2).

// What ships for Windows (ADR-074) is a program of the window subsystem: no
// console opens behind it from Explorer or the Start menu. A debug build
// keeps its console, for the tests' output under Wine.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod catalogs;
mod gpu;

/// The argument that makes a launch a program of its own (ADR-122).
const NEW_INSTANCE: &str = "--new-instance";

/// The status of a program the session bus started to show files for
/// another program that ended asked nothing (ADR-140, 5).
const ASKED_NOTHING: i32 = 3;

use std::time::Instant;

use skwir_core::App;
use skwir_core::app::AppConfig;

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
    // The picture formats `image` is taught (JPEG XL, ADR-148), before any
    // picture is read.
    skwir_platform::picture::formats();
    // Which build this is, for a helper's failure to say when the helper
    // is of another (`process::explain`).
    skwir_platform::process::set_build(env!("SKWIR_BUILD"));
    #[cfg(all(windows, not(debug_assertions)))]
    attach_console();
    // What skwir starts inherits none of its standard handles (ADR-074,
    // amended): before any program is started.
    skwir_platform::process::keep_own_handles();
    // `skwir mcp` and `skwir cmd` (ADR-043), before anything logs: their
    // standard output is the protocol, or the JSON a script reads.
    let program = std::env::args_os().next().unwrap_or_default();
    let mut args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    // `skwir view` and `skw` (ADR-151): handed to the Skwir running, this
    // launch waiting until that window closes; else a program of its own,
    // which takes nothing over, started with what viewing needs.
    if let Some(asked) = skwir::view::asked(&program, &args) {
        let cwd = std::env::current_dir().unwrap_or_default();
        let (asked, apart) = skwir::view::apart(&asked);
        if apart {
            skwir_platform::handover::keep_apart();
        }
        let mut viewing = match skwir::view::read(&asked, &cwd) {
            Ok(viewing) => viewing,
            Err(code) => std::process::exit(code),
        };
        viewing.token = skwir_platform::handover::launch_token();
        match skwir_platform::handover::view_elsewhere(&viewing) {
            skwir_platform::handover::ViewedElsewhere::NotTaken => {}
            // What standard input gave is this launch's to remove once its
            // window closed (ADR-151, 7).
            skwir_platform::handover::ViewedElsewhere::Closed => {
                skwir::view::let_go_of_piped(&viewing);
                return Ok(());
            }
            skwir_platform::handover::ViewedElsewhere::Ended => {
                skwir::view::let_go_of_piped(&viewing);
                eprintln!("skwir view: the Skwir showing it ended");
                std::process::exit(1);
            }
        }
        return view(started, viewing);
    }
    if let Some(code) = skwir::cli::run(&args) {
        std::process::exit(code);
    }
    // A program of its own: no launch handed to another Skwir, none taken
    // over by this one (ADR-122).
    let before = args.len();
    args.retain(|arg| arg != NEW_INSTANCE);
    if args.len() != before {
        skwir_platform::handover::keep_apart();
    }
    // Started by the session bus to show files for another program: no
    // window until the request opens one (ADR-140). Never handed over as
    // a launch would be, which would only bring a window forward: the
    // program takes the request, and hands it on if another takes
    // launches over.
    // Started by the bus for another program's dialog: the same, the
    // request opening a window answering it (ADR-141, 3).
    let before = args.len();
    args.retain(|arg| arg != skwir_platform::file_manager::STARTED_BY_BUS);
    let mut windowless = (args.len() != before).then_some(skwir_ui::Windowless::FileManager);
    let before = args.len();
    args.retain(|arg| arg != skwir_platform::chooser::STARTED_BY_PORTAL);
    if args.len() != before {
        windowless = Some(skwir_ui::Windowless::Portal);
    }
    if windowless.is_some() && !cfg!(target_os = "linux") {
        // What is not written for this system yet (ADR-140, 7), in
        // English, as what the command line says is.
        eprintln!(
            "skwir: {}",
            skwir_platform::file_manager::not_yet().english()
        );
        std::process::exit(2);
    }
    // The running Skwir of this build, these settings and this display
    // takes this launch over: it opens a window on what this was asked to,
    // or its windows again while it finishes what runs without one
    // (ADR-115, ADR-122). Where none is, this costs a look at one file.
    let cwd = std::env::current_dir().unwrap_or_default();
    // A file named opens its folder, the file selected (ADR-151, 5: what a
    // viewer's Show in Skwir starts), as another program's ShowItems does
    // (ADR-140).
    let (location, select) = match args.first() {
        Some(arg) => {
            let (location, select) = skwir_platform::file_manager::named(arg, &cwd);
            (Some(location), select.into_iter().collect())
        }
        None => (None, Vec::new()),
    };
    let opening = skwir_platform::handover::Opening {
        location,
        cwd: cwd.clone(),
        token: skwir_platform::handover::launch_token(),
        select,
        ..skwir_platform::handover::Opening::default()
    };
    if windowless.is_none() && skwir_platform::handover::hand_over(&opening) {
        return Ok(());
    }
    // Before any GPU work: keep the Vulkan loader from mapping drivers for
    // hardware that is not here (ADR-002).
    let restricted = gpu::restrict_vulkan_drivers();
    log();
    if let Some(disabled) = restricted {
        tracing::debug!(%disabled, "restricting Vulkan drivers");
    }
    speak();
    let app = App::new(AppConfig::default())?;
    // Type-to-find's table for the collation, made on a worker before a
    // key is typed, never on the window's thread.
    app.scheduler.spawn(
        skwir_core::scheduler::Priority::Background,
        "collate-prewarm",
        app.lifetime().child(),
        skwir_core::collate::prewarm,
    );

    // Plugins: each registers its commands, bindings, menus, and places, and
    // can be toggled at runtime through `plugins.<id>.enabled` in settings.
    for plugin in skwir::plugins() {
        app.plugins.add(plugin, &app);
    }
    // The variables the exclusions' paths start from, as the login shell
    // exports them too (ADR-125): kept from the last run, asked again on a
    // worker once the shell's files change.
    skwir_platform::shell_environment::follow(&app);

    // A location on the command line wins: one a plugin opens (`trash:`,
    // `tags:/family`), else a folder. Otherwise the startup preference (a
    // chosen folder, or the windows and tabs left open, ADR-122), chosen
    // as the user's windows first come up (`finishing::show`): started by
    // the bus, they come up for a launch later, if one comes. What another
    // program asks the bus is that location, as a launch naming its
    // folder: nothing restored, the request's folder in the one window
    // (ADR-140).
    let initial = opening
        .location
        .as_ref()
        .map(|arg| skwir_ui::location_of(&app, arg, &cwd));
    // The folder is not logged: it can come from a preference, and paths
    // are private.
    tracing::info!("starting");
    let launch = windowless.is_none().then_some(opening);
    let ran = skwir_ui::run_from(app, initial, started, launch, windowless);
    // The program ends with its last viewer (ADR-151, 4): the launch that
    // waited on it reads that it closed before the bus would tell it this
    // Skwir ended. At once where none waits.
    skwir_platform::handover::wait_told();
    if windowless.is_some() && skwir_ui::ended_unasked() {
        // Started by the bus and ended asked nothing: not 0, which
        // dbus-daemon reads as a program gone to the background and
        // waits for, the asking program with it (ADR-140, 5).
        std::process::exit(ASKED_NOTHING);
    }
    ran
}

/// The system's languages and Skwir's catalogs, before anything says a
/// word (ADR-139, 6, G).
fn speak() {
    skwir_core::locale::set_system(skwir_platform::locale::languages());
    {
        use skwir_platform::locale::{Category, of};
        skwir_core::locale::set_formats(of(Category::Numeric), of(Category::Time));
        // Names in the order of the region's collation (ADR-139, 15), set
        // once before any listing makes a key: C and POSIX keep the folded
        // order.
        skwir_core::collate::set_locale(of(Category::Collate).as_deref());
    }
    // The catalogs shipped (ADR-139, G): the program's own now, a plugin's
    // as it is enabled; and how much of each language they say, counted as
    // they were made, which decides what Automatic speaks (ADR-139, 13).
    skwir_core::words::ship(catalogs::CATALOGS);
    skwir_core::words::ship_shares(catalogs::SHARES);
    skwir_core::words::register_shipped_of_program("skwir");
}

/// Logs to standard error, as `RUST_LOG` says, else what informs.
fn log() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
}

/// A viewer's program (`skwir view`, ADR-151, 1), where no Skwir running
/// took the files over: no handover either way, the plugins that serve a
/// viewer alone, none from outside, and not what the file manager starts
/// (the login shell's variables, the collation's table for typing names).
/// Ends once its window closes.
fn view(started: Instant, viewing: skwir_ui::viewer::Viewing) -> anyhow::Result<()> {
    skwir_platform::handover::keep_apart();
    let restricted = gpu::restrict_vulkan_drivers();
    log();
    if let Some(disabled) = restricted {
        tracing::debug!(%disabled, "restricting Vulkan drivers");
    }
    speak();
    let app = App::new(AppConfig::default())?;
    for plugin in skwir::viewer_plugins() {
        app.plugins.add(plugin, &app);
    }
    tracing::info!("viewing");
    skwir_ui::view(app, viewing, started)
}
