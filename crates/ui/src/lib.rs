//! bibli UI shell on GPUI (ADR-001).
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

pub mod address;
pub mod format;
pub mod input;
pub mod keymap;
pub mod list;
pub mod menu;
pub mod palette;
pub mod prompts;
pub mod shell;
pub mod sidebar;
pub mod status;
pub mod theme;

use std::sync::Arc;
use std::time::Instant;

use bibli_core::{App, Location};
use gpui::{
    Application, Bounds, SharedString, TitlebarOptions, WindowBounds, WindowOptions, font,
    prelude::*, px, size,
};

use crate::shell::Shell;

/// Font families tried in order; the first the text system resolves wins.
/// GPUI's `.SystemUIFont` falls back per frame at a large cost when unresolved.
const FONT_CANDIDATES: [&str; 5] = [
    "Inter",
    "DejaVu Sans",
    "Noto Sans",
    "Liberation Sans",
    "Cantarell",
];

fn pick_font(cx: &gpui::App) -> SharedString {
    let ts = cx.text_system();
    let missing = ts.resolve_font(&font("bibli-no-such-font"));
    for c in FONT_CANDIDATES {
        if ts.resolve_font(&font(c)) != missing {
            tracing::info!(family = c, "ui font");
            return c.into();
        }
    }
    tracing::warn!("none of {FONT_CANDIDATES:?} resolved; using the platform fallback");
    FONT_CANDIDATES[1].into()
}

/// Run the UI until the window closes. `initial` is opened in the active pane.
pub fn run(app: Arc<App>, initial: Option<Location>) -> anyhow::Result<()> {
    run_from(app, initial, Instant::now())
}

/// Like [`run`], with the instant the process started, for first-frame timing.
pub fn run_from(app: Arc<App>, initial: Option<Location>, started: Instant) -> anyhow::Result<()> {
    let (wake_tx, wake_rx) = flume::bounded::<()>(1);
    app.set_waker(Some(Arc::new(move || {
        let _ = wake_tx.try_send(());
    })));

    tracing::debug!(
        ms = started.elapsed().as_millis() as u64,
        "gpui application init"
    );
    Application::new().run(move |cx: &mut gpui::App| {
        tracing::debug!(
            ms = started.elapsed().as_millis() as u64,
            "gpui run callback"
        );
        let font = pick_font(cx);
        tracing::debug!(ms = started.elapsed().as_millis() as u64, "font picked");
        let key_hints = keymap::install(&app, cx);
        cx.bind_keys(input::key_bindings());
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("bibli".into()),
                        ..Default::default()
                    }),
                    app_id: Some("bibli".into()),
                    ..Default::default()
                },
                |window, cx| {
                    cx.new(|cx| {
                        Shell::new(app, initial, started, font, key_hints, wake_rx, window, cx)
                    })
                },
            )
            .expect("open window");
        tracing::debug!(ms = started.elapsed().as_millis() as u64, "window opened");
        window
            .update(cx, |shell, window, cx| {
                window.focus(&shell.pane_focus);
                cx.activate(true);
            })
            .ok();
    });
    Ok(())
}
