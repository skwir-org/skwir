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
pub mod address;
mod archive_preview;
mod backdrop;
pub mod bytes_view;
pub mod choose;
pub mod clashes;
mod clipboard;
mod code_preview;
pub mod color;
pub mod commands;
mod complete;
pub mod controls;
mod deleting;
mod desktop_style;
pub mod dnd;
pub mod dock;
mod dragicon;
pub mod elements;
pub mod fonts;
pub mod form;
pub mod format;
mod fullscreen;
pub mod icons;
pub mod input;
mod inspector;
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
pub mod narrow;
mod nav_menu;
mod note_popover;
mod omnibox;
mod outline;
pub mod palette;
pub mod placeholder;
mod player;
pub mod preview;
mod preview_bar;
mod preview_toolbar;
pub mod prompts;
mod properties;
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

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{Bounds, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size};
use skwir_core::{App, Location};

use crate::shell::Shell;

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

    // The system's backend, with AccessKit (ADR-064).
    gpui_platform::application()
        .with_assets(icons::FsAssets)
        .run(move |cx: &mut gpui::App| {
            tracing::debug!(
                ms = started.elapsed().as_millis() as u64,
                "gpui run callback"
            );
            let keymap =
                keymap::install(&app, skwir_core::keymap::user_bindings(&app.settings), cx);
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
            // Every way out (Quit, the window's close button) closes the
            // last window first, so nothing is left on screen while the
            // plans wind down.
            let stats_app = Arc::clone(&app);
            cx.on_window_closed(move |cx, _| {
                if cx.windows().is_empty() {
                    remember_location(&stats_app);
                    let (decoded, skipped, bytes) = stats_app.thumbnails.stats();
                    tracing::info!(decoded, skipped, cached_bytes = bytes, "thumbnail stats");
                    cx.quit();
                }
            })
            .detach();
            let quitting = Arc::clone(&app);
            cx.on_app_quit(move |_| {
                wind_down(&quitting);
                async {}
            })
            .detach();

            let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        titlebar: Some(TitlebarOptions {
                            title: Some("skwir".into()),
                            ..Default::default()
                        }),
                        app_id: Some(skwir_core::APP_ID.into()),
                        ..Default::default()
                    },
                    |window, cx| {
                        cx.new(|cx| Shell::new(app, initial, started, keymap, wake_rx, window, cx))
                    },
                )
                .expect("open window");
            tracing::debug!(ms = started.elapsed().as_millis() as u64, "window opened");
            window
                .update(cx, |shell, window, cx| {
                    window.focus(&shell.ui().focus, cx);
                    cx.activate(true);
                })
                .ok();
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

/// Stop every plan before the process ends: a copy cut short by Quit
/// leaves nothing in its destination. A plan is cancelled, never finished
/// unseen: with no window, nothing would show it or let it be stopped.
///
/// Run by GPUI's quit handler, the one place every system passes through
/// as the application ends (on Linux the event loop stops by itself when
/// the last window closes, so nothing can wait on it). GPUI gives the
/// future a handler returns 100 ms; the handler itself it runs to the end,
/// which is why the waiting is here and not in that future. The windows
/// are gone by then: Quit closes them first.
fn wind_down(app: &App) {
    if !app.executor.wind_down(WIND_DOWN) {
        tracing::warn!("quitting while a plan still runs: what it left stays");
    }
}

/// Note where the window is, for "open the folders and tabs left open".
pub(crate) fn remember_location(app: &App) {
    skwir_core::tabs::remember(app);
}
