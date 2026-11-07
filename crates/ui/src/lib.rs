//! skwir UI shell on GPUI (ADR-001).
//!
//! The shell owns no file state: it reads the core's workspace during render
//! and mutates only through commands. It re-renders when the core wakes it,
//! never on a timer.

// Pedantic lints that fight GPUI's closure-heavy API and hex color tokens.
#![allow(
    clippy::unreadable_literal,
    clippy::cast_possible_wrap,
    clippy::too_many_arguments,
    clippy::needless_pass_by_value,
    clippy::unused_self,
    clippy::ignored_unit_patterns
)]

mod about;
mod activity;
pub mod address;
mod archive_preview;
mod backdrop;
pub mod bytes_view;
pub mod choose;
pub mod clashes;
mod clipboard;
mod code_preview;
pub mod color;
mod coloring;
pub mod commands;
mod complete;
pub mod controls;
mod deleting;
mod desktop_style;
pub mod dnd;
pub mod dock;
mod dragicon;
mod dragname;
pub mod elements;
mod finishing;
pub mod fonts;
pub mod form;
pub mod format;
mod fullscreen;
mod held_before;
mod history;
pub mod icons;
pub mod input;
mod inspector;
mod inspector_pages;
mod inspector_pictures;
mod inspector_rows;
mod inspector_text;
pub mod keymap;
mod latest;
pub mod layout;
mod links;
pub mod list;
mod loading;
mod map_preview;
mod map_tiles;
mod map_view;
mod markdown;
mod media;
mod members;
pub mod menu;
pub mod modal;
mod model_preview;
mod name_lines;
mod nav_menu;
mod note_popover;
mod omnibox;
mod outline;
pub mod palette;
pub mod palettes;
pub mod placeholder;
mod player;
pub mod preview;
mod preview_bar;
mod preview_toolbar;
pub mod prompts;
mod properties;
pub mod quick_filters;
pub mod reader;
mod reader_scroll;
pub mod renamebulk;
pub mod review;
pub mod search_bar;
mod sequence;
pub mod session;
pub mod settings;
mod settings_keys;
mod settings_keys_new;
mod settings_keys_view;
mod settings_records;
mod settings_view;
pub mod shell;
pub mod shortcuts;
pub mod sidebar;
pub mod sizes;
pub mod status;
pub mod suggestions;
pub mod tabs;
pub mod tags;
pub mod terminal;
pub mod text_view;
pub mod theme;
pub mod thumbs;
pub mod timeline;
pub mod toast;
pub mod toolbar;
pub mod trail;
pub mod treemap;
pub mod typenames;
pub mod usage;
pub mod view;
pub mod views;
mod weight;
mod work;

use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::{App, Location};

/// How many times the core has woken the window: what a wake brought is
/// told apart from a draw the user asked (the timeline's count in the
/// background, `timeline.rs`).
static WAKES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The wakes so far, counted before each is sent.
pub(crate) fn wakes() -> u64 {
    WAKES.load(std::sync::atomic::Ordering::Acquire)
}

/// Dev-only PNG/JPEG decoder used to exercise the views before the platform
/// providers exist. Enabled with `--features skwir-ui/dev-thumbs`.
#[cfg(feature = "dev-thumbs")]
mod dev_thumbs {
    use skwir_core::CancelToken;
    use skwir_core::thumbs::{Image, ThumbRequest, ThumbnailProvider};

    pub struct DevDecoder;

    impl ThumbnailProvider for DevDecoder {
        fn id(&self) -> &'static str {
            "dev-decoder"
        }

        fn accepts(&self, req: &ThumbRequest) -> bool {
            req.mime.as_deref().is_some_and(|m| m.starts_with("image/"))
        }

        fn thumbnail(
            &self,
            req: &ThumbRequest,
            cancel: &CancelToken,
        ) -> anyhow::Result<Option<Image>> {
            let img =
                skwir_platform::picture::decode(skwir_platform::picture::open(&req.entry.path)?)?;
            if cancel.is_cancelled() {
                return Ok(None);
            }
            let px = req.size.pixels();
            let t = img.thumbnail(px, px).to_rgba8();
            Ok(Some(Image {
                width: t.width(),
                height: t.height(),
                rgba: t.into_raw().into(),
            }))
        }
    }
}

/// Run the UI until the window closes. `initial` is opened in the active pane.
pub fn run(app: Arc<App>, initial: Option<Location>) -> anyhow::Result<()> {
    run_from(app, initial, Instant::now())
}

/// Like [`run`], with the instant the process started, for first-frame timing.
pub fn run_from(app: Arc<App>, initial: Option<Location>, started: Instant) -> anyhow::Result<()> {
    // Commands that may wait move their waiting off this thread.
    skwir_core::scheduler::mark_window_thread(&app.scheduler);
    let (wake_tx, wake_rx) = flume::bounded::<()>(1);
    app.set_waker(Some(Arc::new(move || {
        WAKES.fetch_add(1, std::sync::atomic::Ordering::Release);
        let _ = wake_tx.try_send(());
    })));

    tracing::debug!(
        ms = started.elapsed().as_millis() as u64,
        "gpui application init"
    );
    #[cfg(feature = "dev-thumbs")]
    {
        app.thumbnails
            .register("ui-dev", Arc::new(dev_thumbs::DevDecoder));
        tracing::warn!("dev-thumbs: registered the dev PNG/JPEG decoder as a thumbnail provider");
    }
    for spec in views::specs() {
        app.registry.views.add("ui", spec);
    }
    // The shell's own commands, known to every surface as the others are.
    commands::declare_all(&app);
    // What the shell offers the address field (ADR-098).
    suggestions::register(&app);
    // What the desktop said when skwir last ran, then what it says
    // (ADR-071): its colors and fonts are the theme's (ADR-073).
    desktop_style::start(&app);
    // The fonts skwir's packages carry that it will draw in, read while
    // the window opens (ADR-118).
    fonts::read_ahead(&app);

    // The system's backend, with AccessKit (ADR-064).
    let application = gpui_platform::application().with_assets(icons::FsAssets);
    // macOS: the Dock's icon clicked with no window up (ADR-115).
    application.on_reopen(|cx| finishing::reopen(cx, None, false));
    // macOS: folders the Finder or the Dock opens in the running program
    // (ADR-122), handed in as a launch's are, at the next wake.
    let openings: finishing::Openings = Arc::default();
    {
        let (openings, wake) = (Arc::clone(&openings), app.waker_handle());
        application.on_open_urls(move |urls| {
            let opened: Vec<skwir_platform::handover::Opening> = urls
                .iter()
                .filter_map(|url| skwir_platform::uri::local_path_of(url))
                .map(|path| skwir_platform::handover::Opening {
                    location: Some(path.into_os_string()),
                    cwd: std::path::PathBuf::new(),
                    token: None,
                })
                .collect();
            if !opened.is_empty() {
                openings.lock().extend(opened);
                wake();
            }
        });
    }
    // macOS: Quit from the Dock, a log out, a restart, asked first while
    // something is under way (gpui patch 0044).
    application.on_should_quit(finishing::may_quit);
    application.run(move |cx: &mut gpui::App| {
        tracing::debug!(
            ms = started.elapsed().as_millis() as u64,
            "gpui run callback"
        );
        let keymap = keymap::install(&app, skwir_core::keymap::user_bindings(&app.settings), cx);
        // A key that begins a binding of several strokes waits for the
        // next as long as it takes; `sequence.rs` shows what can follow.
        cx.set_key_sequence_timeout(None);
        // A key that types a character hides the pointer until the mouse
        // moves, as Windows and macOS do while typing; keys that only move
        // (the arrows through files) leave it, which gpui's default would not.
        cx.set_cursor_hide_mode(gpui::CursorHideMode::OnTyping);
        // Debug aid: `RUST_LOG=skwir_ui=trace` shows every keystroke GPUI saw.
        cx.observe_keystrokes(|ev, _, _| {
            tracing::trace!(keystroke = %keymap::unparse(&ev.keystroke), "keystroke");
        })
        .detach();
        // Every way out passes here: Quit and Stop and quit, the last
        // window closing with nothing left, a signal (`finishing.rs`).
        let quitting = Arc::clone(&app);
        cx.on_app_quit(move |_| {
            wind_down(&quitting);
            async {}
        })
        .detach();
        // The window, and what outlives it (ADR-115).
        finishing::start(cx, app, keymap, started, wake_rx, openings, initial);
    });
    Ok(())
}

/// How long the process may stay after its last window closed, for the
/// plans it cancels to take back their partial work: a copy removes its
/// staging folder file by file, perhaps tens of thousands of them from a
/// slow drive. Past it the process ends anyway, what is left staying, so a
/// step stuck on a drive that stopped answering never keeps it alive
/// unseen.
const WIND_DOWN: Duration = Duration::from_secs(30);

/// Stop every plan before the process ends: a copy cut short by Stop and
/// quit, or a signal, leaves nothing in its destination. What the program
/// finishes without its window it finishes before it gets here
/// (`finishing.rs`, ADR-115).
///
/// Run by GPUI's quit handler, the one place every system passes through
/// as the application ends. GPUI gives the future a handler returns
/// 100 ms; the handler itself it runs to the end, which is why the waiting
/// is here and not in that future. The windows are gone by then.
fn wind_down(app: &App) {
    if !app.executor.wind_down(WIND_DOWN) {
        tracing::warn!("quitting while a plan still runs: what it left stays");
    }
}

/// Note where the windows are, for "open the windows and tabs left open".
pub(crate) fn remember_location(app: &App) {
    skwir_core::tabs::remember(app);
}

/// What a launch's argument opens (`skwir <arg>`), read against `cwd`: a
/// location a plugin opens (`trash:`, `tags:/family`), else a folder.
pub fn location_of(app: &App, arg: &std::ffi::OsStr, cwd: &std::path::Path) -> Location {
    let location = Location::parse_os(arg);
    if !location.is_path() && app.provider(&location.scheme).is_some() {
        return location;
    }
    let path = cwd.join(arg);
    Location::from_path(std::path::absolute(&path).unwrap_or(path))
}
