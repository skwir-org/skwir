//! The desktop's style as skwir wears it (ADR-071): what
//! `skwir_platform::desktop_style` reads, followed on a thread of its own
//! from the start (`start`) and kept in the cache, so the next start draws
//! its first frame with what was read last rather than flashing from
//! skwir's colors to the desktop's. The theme asks `colors` and `fonts`
//! each time it is resolved, and the shell resolves it again when
//! `revision` moves: a change of the desktop's accent or of its fonts
//! shows at once (ADR-073).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use gpui::{Hsla, Rgba};
use skwir_core::App;
use skwir_core::settings::Settings;
use skwir_platform::desktop_style::DesktopStyle;

use crate::settings::DESKTOP_COLORS_KEY;
use crate::theme::{Contrast, Desktop};

/// What the desktop said last, or when skwir last ran.
static SAID: RwLock<Option<DesktopStyle>> = RwLock::new(None);
/// Moves each time what the desktop says changes.
static REVISION: AtomicU64 = AtomicU64::new(0);

/// Where what the desktop said is kept for the next start.
const KEPT: &[u8] = b"desktop-style";

/// Take what the desktop said when skwir last ran, then follow what it
/// says: the first answer, and each change, wake the window.
pub fn start(app: &Arc<App>) {
    if !skwir_platform::desktop_style::READ {
        return;
    }
    let slot = app.cache.namespace("ui");
    if let Some(kept) = slot
        .get(KEPT)
        .and_then(|bytes| serde_json::from_slice::<DesktopStyle>(&bytes).ok())
    {
        set(kept);
    }
    let weak = Arc::downgrade(app);
    skwir_platform::desktop_style::follow(
        move |style| {
            let Some(app) = weak.upgrade() else { return };
            if let Ok(bytes) = serde_json::to_vec(&style) {
                let _ = slot.put(KEPT, &bytes);
            }
            if set(style) {
                app.wake();
            }
        },
        app.lifetime().child(),
    );
}

/// Keep `style`; whether it differs from what was kept.
fn set(style: DesktopStyle) -> bool {
    let mut said = SAID
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if said.as_ref() == Some(&style) {
        return false;
    }
    *said = Some(style);
    REVISION.fetch_add(1, Ordering::Release);
    true
}

fn said() -> Option<DesktopStyle> {
    SAID.read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Moves each time what the desktop says changes: the theme is resolved
/// again (`Shell::sync`).
pub fn revision() -> u64 {
    REVISION.load(Ordering::Acquire)
}

fn on(settings: &Settings, key: &str) -> bool {
    settings.get::<bool>(key).unwrap_or(true)
}

/// What the look takes from the desktop: its accent while
/// `ui.desktop_colors` is on, and more contrast whenever it asks for it,
/// which is a need, not a style.
pub fn colors(settings: &Settings) -> Desktop {
    let Some(said) = said() else {
        return Desktop::default();
    };
    Desktop {
        accent: said
            .accent
            .filter(|_| on(settings, DESKTOP_COLORS_KEY))
            .map(|[r, g, b]| Hsla::from(Rgba { r, g, b, a: 1. })),
        contrast: if said.more_contrast {
            Contrast::More
        } else {
            Contrast::Normal
        },
    }
}

/// The families the desktop names for its fonts, as read last.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fonts {
    pub interface: Option<String>,
    pub document: Option<String>,
    pub monospace: Option<String>,
}

/// What a font that says Automatic takes from the desktop (`fonts.rs`):
/// its fonts while `ui.desktop_colors` is on, else nothing.
pub fn fonts(settings: &Settings) -> Fonts {
    on(settings, DESKTOP_COLORS_KEY)
        .then(said)
        .flatten()
        .map(|said| Fonts {
            interface: said.font,
            document: said.document_font,
            monospace: said.monospace_font,
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    fn app() -> (tempfile::TempDir, Arc<App>) {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        crate::settings::declare(&app);
        (tmp, app)
    }

    fn teal() -> DesktopStyle {
        DesktopStyle {
            accent: Some([0.1, 0.55, 0.5]),
            more_contrast: false,
            font: Some("Noto Sans".into()),
            document_font: Some("Noto Serif".into()),
            monospace_font: None,
        }
    }

    #[test]
    fn the_look_wears_the_desktop_s_accent_while_desktop_colors_is_on() {
        let (_tmp, app) = app();
        assert_eq!(
            colors(&app.settings),
            Desktop::default(),
            "nothing said yet"
        );
        let before = revision();
        assert!(set(teal()));
        assert!(!set(teal()), "the same again is no change");
        assert_eq!(revision(), before + 1);

        let accent = colors(&app.settings).accent.expect("the desktop's accent");
        let theme = Theme::of(false, colors(&app.settings));
        assert_eq!(theme.desktop.accent, Some(accent));
        assert_ne!(theme.accent, Theme::light().accent);
        // Its marks and its selection follow it; the tags keep theirs.
        assert_eq!(theme.band, theme.accent.opacity(0.14));
        assert_eq!(theme.tags, Theme::light().tags);
        // The other look, for code kept in it, wears it too.
        assert_eq!(theme.in_look(true).desktop, theme.desktop);

        app.settings
            .set(DESKTOP_COLORS_KEY, toml::Value::Boolean(false));
        assert_eq!(colors(&app.settings), Desktop::default());
        assert_eq!(Theme::of(false, colors(&app.settings)), Theme::light());
    }

    #[test]
    fn more_contrast_is_followed_whatever_the_switch_says() {
        let (_tmp, app) = app();
        app.settings
            .set(DESKTOP_COLORS_KEY, toml::Value::Boolean(false));
        set(DesktopStyle {
            more_contrast: true,
            ..teal()
        });
        let desktop = colors(&app.settings);
        assert_eq!(desktop.accent, None);
        assert_eq!(desktop.contrast, Contrast::More);
        let theme = Theme::of(false, desktop);
        assert!(crate::color::ratio(theme.text_muted, theme.bg) >= 7.);
    }

    #[test]
    fn the_desktop_s_fonts_are_lent_while_desktop_colors_is_on() {
        let (_tmp, app) = app();
        assert_eq!(fonts(&app.settings), Fonts::default(), "nothing said yet");
        set(teal());
        assert_eq!(
            fonts(&app.settings),
            Fonts {
                interface: Some("Noto Sans".into()),
                document: Some("Noto Serif".into()),
                monospace: None,
            }
        );
        app.settings
            .set(DESKTOP_COLORS_KEY, toml::Value::Boolean(false));
        assert_eq!(fonts(&app.settings), Fonts::default(), "skwir's own");
    }

    #[test]
    fn the_fonts_are_choices_of_their_own_not_under_the_colors_switch() {
        let (_tmp, app) = app();
        for key in [
            crate::settings::FONT_KEY,
            crate::settings::READING_FONT_KEY,
            crate::settings::CODE_FONT_KEY,
        ] {
            let spec = app.settings.spec(key).unwrap();
            assert_eq!(
                spec.kind,
                skwir_core::settings::SettingKind::Choice,
                "{key}"
            );
            assert!(spec.under.is_empty(), "{key}");
            assert!(!spec.restart_required, "{key}: a change shows at once");
            assert_eq!(
                spec.default,
                toml::Value::String(crate::settings::FONT_AUTO.into())
            );
        }
    }
}
