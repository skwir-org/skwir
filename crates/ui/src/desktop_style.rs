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
use skwir_platform::desktop_style::{DesktopStyle, Scheme};

use crate::settings::DESKTOP_COLORS_KEY;
use crate::theme::{Base, Contrast, Desktop, Schemes};

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

/// What the look takes from the desktop: while `ui.desktop_colors` is on,
/// its scheme and its accent; more contrast whenever it asks for it, which
/// is a need, not a style. An accent the system names (the portal's)
/// replaces a scheme's, whose selection is then derived from it, unless it
/// is the scheme's own said again (Plasma's portal); a look without a
/// scheme wears the system's accent, else the scheme's (ADR-071, point 4).
pub fn colors(settings: &Settings) -> Desktop {
    let Some(said) = said() else {
        return Desktop::default();
    };
    let contrast = if said.more_contrast {
        Contrast::More
    } else {
        Contrast::Normal
    };
    if !on(settings, DESKTOP_COLORS_KEY) {
        return Desktop {
            contrast,
            ..Desktop::default()
        };
    }
    let scheme = |scheme: Option<&Scheme>| {
        let scheme = scheme?;
        let mut base = base(scheme);
        let its_own = scheme.accent.map(eight_bit);
        if let Some(accent) = said.accent.filter(|a| Some(eight_bit(*a)) != its_own) {
            base.accent = hsla(accent);
            base.selection = None;
        }
        Some(base)
    };
    let schemes = Schemes {
        light: scheme(said.schemes.light.as_ref()),
        dark: scheme(said.schemes.dark.as_ref()),
    };
    let accent = said.accent.or_else(|| {
        [&said.schemes.light, &said.schemes.dark]
            .into_iter()
            .find_map(|scheme| scheme.as_ref()?.accent)
    });
    Desktop {
        accent: accent.map(hsla),
        contrast,
        schemes,
    }
}

fn hsla([r, g, b]: [f32; 3]) -> Hsla {
    Hsla::from(Rgba { r, g, b, a: 1. })
}

/// `color` as the eight bits a channel a desktop writes it in: what two
/// sayings of one color are compared by.
fn eight_bit(color: [f32; 3]) -> [f32; 3] {
    color.map(|c| (c * 255.).round() / 255.)
}

/// A desktop's scheme as the base of a look; skwir's own outcomes where it
/// names none.
fn base(scheme: &Scheme) -> Base {
    let own = Base::own(scheme.is_dark());
    Base {
        dark: scheme.is_dark(),
        bg: hsla(scheme.bg),
        surface: hsla(scheme.surface),
        text: hsla(scheme.text),
        text_muted: scheme.text_muted.map(hsla),
        zebra: scheme.zebra.map(hsla),
        border: scheme.border.map(hsla),
        accent: scheme.accent.map_or(own.accent, hsla),
        selection: scheme.selection.map(|(bg, text)| (hsla(bg), hsla(text))),
        error: scheme.error.map_or(own.error, hsla),
        warning: scheme.warning.map_or(own.warning, hsla),
        success: scheme.success.map_or(own.success, hsla),
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
            schemes: skwir_platform::desktop_style::Schemes::default(),
        }
    }

    fn channels(hex: u32) -> [f32; 3] {
        [16, 8, 0].map(|shift| f32::from(((hex >> shift) & 0xff) as u8) / 255.)
    }

    /// Breeze Light's colors, as `kdeglobals` names them.
    fn breeze() -> Scheme {
        Scheme {
            bg: channels(0xffffff),
            surface: channels(0xeff0f1),
            text: channels(0x232629),
            text_muted: Some(channels(0x707d8a)),
            zebra: Some(channels(0xf7f7f7)),
            border: None,
            accent: Some(channels(0x3daee9)),
            selection: Some((channels(0x3daee9), channels(0xffffff))),
            error: Some(channels(0xda4453)),
            warning: Some(channels(0xf67400)),
            success: Some(channels(0x27ae60)),
        }
    }

    fn with_scheme(light: Option<Scheme>, dark: Option<Scheme>) -> DesktopStyle {
        DesktopStyle {
            accent: None,
            schemes: skwir_platform::desktop_style::Schemes { light, dark },
            ..teal()
        }
    }

    #[test]
    fn a_scheme_fills_its_look_its_contrast_guarded() {
        let (_tmp, app) = app();
        set(with_scheme(Some(breeze()), None));
        let desktop = colors(&app.settings);
        let theme = Theme::of(false, desktop);
        let scheme = breeze();
        assert_eq!(theme.bg, hsla(scheme.bg));
        assert_eq!(theme.surface, hsla(scheme.surface));
        assert_eq!(theme.text, hsla(scheme.text));
        assert_eq!(theme.zebra, hsla(scheme.zebra.unwrap()));
        // Breeze's selection, white on its blue, reads at 2.4:1: the fill
        // gives way. Its muted text, its warning, its accent likewise.
        let ratio = crate::color::ratio;
        assert!(ratio(theme.selection_text, theme.selection_bg) >= 4.5);
        for (name, fg) in [
            ("text", theme.text),
            ("text_muted", theme.text_muted),
            ("warning", theme.warning),
            ("error", theme.error),
            ("success", theme.success),
        ] {
            for ground in [theme.bg, theme.surface] {
                assert!(ratio(fg, ground) >= 4.5, "{name}");
            }
        }
        assert!(ratio(theme.accent, theme.bg) >= 3.);
        // What no scheme says is derived from it; what no desktop changes
        // is kept.
        assert_ne!(theme.hover, Theme::light().hover);
        assert_eq!(theme.tags, Theme::light().tags);

        // More contrast asked: the stricter guard on the same scheme.
        set(DesktopStyle {
            more_contrast: true,
            ..with_scheme(Some(breeze()), None)
        });
        let theme = Theme::of(false, colors(&app.settings));
        assert!(ratio(theme.text_muted, theme.surface) >= 7.);

        // The switch off: skwir's own, but for the contrast asked.
        set(with_scheme(Some(breeze()), None));
        app.settings
            .set(DESKTOP_COLORS_KEY, toml::Value::Boolean(false));
        assert_eq!(Theme::of(false, colors(&app.settings)), Theme::light());
    }

    #[test]
    fn a_scheme_is_its_look_s_and_automatic_takes_it() {
        use gpui::WindowAppearance::{Dark, Light};
        let (_tmp, app) = app();
        set(with_scheme(Some(breeze()), None));
        let desktop = colors(&app.settings);
        // The dark look forced: skwir's dark palette, wearing the scheme's
        // accent.
        let dark = Theme::of(true, desktop);
        assert_eq!(dark.bg, Theme::dark().bg);
        assert_ne!(dark.accent, Theme::dark().accent);
        assert_eq!(desktop.accent, Some(hsla(breeze().accent.unwrap())));
        // The other look drawn from this one (code kept in it) is that.
        assert_eq!(Theme::of(false, desktop).in_look(true).bg, dark.bg);

        // Automatic: the scheme's look, whatever the appearance says,
        // while it has one look's; the appearance when it has both.
        assert!(!Theme::look(None, &desktop, Dark));
        assert!(Theme::look(Some("dark"), &desktop, Light));
        let night = Scheme {
            bg: channels(0x141618),
            surface: channels(0x202326),
            text: channels(0xfcfcfc),
            ..breeze()
        };
        set(with_scheme(None, Some(night.clone())));
        assert!(Theme::look(Some("auto"), &colors(&app.settings), Light));
        set(with_scheme(Some(breeze()), Some(night.clone())));
        let both = colors(&app.settings);
        assert!(Theme::look(None, &both, Dark));
        assert!(!Theme::look(None, &both, Light));
        assert_eq!(Theme::of(true, both).bg, hsla(night.bg));
    }

    #[test]
    fn the_system_s_accent_replaces_a_scheme_s_unless_it_is_the_same() {
        let (_tmp, app) = app();
        // Plasma's portal says the scheme's own accent: the scheme's
        // selection is kept.
        set(DesktopStyle {
            accent: breeze().accent,
            ..with_scheme(Some(breeze()), None)
        });
        let kept = colors(&app.settings).schemes.light.unwrap();
        assert!(kept.selection.is_some());
        // Another accent: worn, the selection derived from it.
        set(DesktopStyle {
            accent: Some([0.9, 0.3, 0.1]),
            ..with_scheme(Some(breeze()), None)
        });
        let desktop = colors(&app.settings);
        let replaced = desktop.schemes.light.unwrap();
        assert_eq!(replaced.accent, hsla([0.9, 0.3, 0.1]));
        assert_eq!(replaced.selection, None);
        assert_eq!(replaced.bg, hsla(breeze().bg), "the rest the scheme's");
        let theme = Theme::of(false, desktop);
        assert_ne!(theme.selection_bg, hsla(breeze().selection.unwrap().0));
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
        assert_eq!(theme.term, theme.accent.opacity(0.18));
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
