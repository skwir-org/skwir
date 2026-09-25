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
mod archive_preview;
pub mod choose;
mod code_preview;
pub mod controls;
mod details;
pub mod dnd;
pub mod dock;
mod dragicon;
pub mod elements;
pub mod format;
pub mod icons;
pub mod input;
mod inspector;
mod inspector_pictures;
mod inspector_rows;
mod inspector_text;
pub mod jump;
pub mod keymap;
pub mod layout;
pub mod list;
mod loading;
mod markdown;
mod media;
pub mod menu;
pub mod modal;
mod note_popover;
mod outline;
pub mod palette;
pub mod placeholder;
mod player;
pub mod preview;
mod preview_bar;
mod preview_toolbar;
pub mod prompts;
pub mod reader;
mod reader_scroll;
pub mod renamebulk;
pub mod review;
pub mod search_bar;
pub mod session;
pub mod settings;
mod settings_view;
pub mod shell;
pub mod shortcuts;
pub mod sidebar;
pub mod status;
pub mod tabs;
pub mod theme;
pub mod thumbs;
pub mod timeline;
pub mod toast;
pub mod toolbar;
pub mod treemap;
pub mod typenames;
pub mod usage;
pub mod view;
pub mod views;

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

const MONO_CANDIDATES: [&str; 4] = [
    "JetBrains Mono",
    "DejaVu Sans Mono",
    "Noto Sans Mono",
    "Liberation Mono",
];

fn pick_font(cx: &gpui::App, candidates: &[&'static str], label: &str) -> SharedString {
    let ts = cx.text_system();
    let missing = ts.resolve_font(&font("bibli-no-such-font"));
    for c in candidates {
        if ts.resolve_font(&font(*c)) != missing {
            tracing::info!(family = c, label, "font");
            return (*c).into();
        }
    }
    tracing::warn!("none of {candidates:?} resolved for {label}; using the platform fallback");
    candidates[1].into()
}

/// Dev-only PNG/JPEG decoder used to exercise the views before the platform
/// providers exist. Enabled with `--features bibli-ui/dev-thumbs`.
#[cfg(feature = "dev-thumbs")]
mod dev_thumbs {
    use bibli_core::CancelToken;
    use bibli_core::thumbs::{Image, ThumbRequest, ThumbnailProvider};

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
            let img = image::ImageReader::open(&req.entry.path)?
                .with_guessed_format()?
                .decode()?;
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

    Application::new()
        .with_assets(icons::FsAssets)
        .run(move |cx: &mut gpui::App| {
            tracing::debug!(
                ms = started.elapsed().as_millis() as u64,
                "gpui run callback"
            );
            let font = pick_font(cx, &FONT_CANDIDATES, "ui");
            let mono_font = pick_font(cx, &MONO_CANDIDATES, "mono");
            tracing::debug!(ms = started.elapsed().as_millis() as u64, "font picked");
            let keymap = keymap::install(&app, cx);
            // Debug aid: `RUST_LOG=bibli_ui=trace` shows every keystroke GPUI saw.
            cx.observe_keystrokes(|ev, _, _| {
                tracing::trace!(keystroke = %ev.keystroke.unparse(), "keystroke");
            })
            .detach();
            let stats_app = Arc::clone(&app);
            cx.on_window_closed(move |cx| {
                if cx.windows().is_empty() {
                    remember_location(&stats_app);
                    let (decoded, skipped, bytes) = stats_app.thumbnails.stats();
                    tracing::info!(decoded, skipped, cached_bytes = bytes, "thumbnail stats");
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
                            Shell::new(
                                app, initial, started, font, mono_font, keymap, wake_rx, window, cx,
                            )
                        })
                    },
                )
                .expect("open window");
            tracing::debug!(ms = started.elapsed().as_millis() as u64, "window opened");
            window
                .update(cx, |shell, window, cx| {
                    window.focus(&shell.ui().focus);
                    cx.activate(true);
                })
                .ok();
        });
    Ok(())
}

/// Note where the window is, for "open the folders and tabs left open".
pub(crate) fn remember_location(app: &App) {
    bibli_core::tabs::remember(app);
}
