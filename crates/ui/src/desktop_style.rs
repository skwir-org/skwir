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

use crate::settings::{COLORS_FROM_KEY, DESKTOP_COLORS_KEY};
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
    use skwir_platform::desktop_style::{COLORS, FONTS};
    if !COLORS && !FONTS {
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

/// What the look takes from the desktop: the scheme `ui.colors_from`
/// chooses (Automatic's, GTK's or Qt's) and the accent, unless skwir's own
/// are chosen; more contrast whenever it asks for it, which is a need, not
/// a style. An accent the system names (the portal's) replaces a scheme's,
/// whose selection is then derived from it, unless it is the scheme's own
/// said again (Plasma's portal); a look without a scheme wears the
/// system's accent, else the scheme's (ADR-071, point 4).
pub fn colors(settings: &Settings) -> Desktop {
    let Some(said) = said() else {
        return Desktop::default();
    };
    let contrast = if said.more_contrast {
        Contrast::More
    } else {
        Contrast::Normal
    };
    let chosen = match Source::of_colors(settings) {
        Source::Own => {
            return Desktop {
                contrast,
                ..Desktop::default()
            };
        }
        Source::Automatic => said.schemes.clone(),
        Source::Gtk => said
            .gtk_colors
            .clone()
            .map(|c| c.schemes)
            .unwrap_or_default(),
        Source::Qt => said
            .qt_colors
            .clone()
            .map(|c| c.schemes)
            .unwrap_or_default(),
    };
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
        light: scheme(chosen.light.as_ref()),
        dark: scheme(chosen.dark.as_ref()),
    };
    let accent = said.accent.or_else(|| {
        [&chosen.light, &chosen.dark]
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
    chosen(settings)
        .map(|chosen| Fonts {
            interface: chosen.interface,
            document: chosen.document,
            monospace: chosen.monospace,
        })
        .unwrap_or_default()
}

/// Where the fonts a font setting's Automatic writes in, and their sizes,
/// are read (`ui.fonts_from`, ADR-117), and where the colors are
/// (`ui.colors_from`, ADR-071): Automatic (the desktop's own, then GTK's,
/// then Qt's; macOS's or Windows' own fonts), skwir's own, or GTK's or
/// Qt's alone where they are read apart (`desktop_style::TOOLKITS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Automatic,
    Own,
    Gtk,
    Qt,
}

impl Source {
    const ALL: [(&'static str, &'static str); 4] = [
        ("auto", "Automatic"),
        (OWN, "Skwir's own"),
        ("gtk", "GTK"),
        ("qt", "Qt"),
    ];

    /// The choices this system offers: GTK and Qt where they are read
    /// apart.
    pub fn choices() -> &'static [(&'static str, &'static str)] {
        if skwir_platform::desktop_style::TOOLKITS {
            &Self::ALL
        } else {
            &Self::ALL[..2]
        }
    }

    /// What the setting `key` chooses; a toolkit this system does not read
    /// apart (a settings file brought from Linux) is Automatic.
    pub fn chosen(settings: &Settings, key: &str) -> Self {
        let toolkits = skwir_platform::desktop_style::TOOLKITS;
        match settings.get::<String>(key).as_deref() {
            Some(OWN) => Source::Own,
            Some("gtk") if toolkits => Source::Gtk,
            Some("qt") if toolkits => Source::Qt,
            _ => Source::Automatic,
        }
    }

    /// What Fonts from chooses.
    pub fn of_fonts(settings: &Settings) -> Self {
        Self::chosen(settings, crate::settings::FONTS_FROM_KEY)
    }

    /// What Colors from chooses.
    pub fn of_colors(settings: &Settings) -> Self {
        Self::chosen(settings, crate::settings::COLORS_FROM_KEY)
    }

    /// What the source's Automatic read, as its choices name it: GTK's
    /// (the portal's are GTK's), Qt's, or the desktop's own by its name
    /// (`Plasma`); empty when nothing was.
    pub fn now(from: Option<&str>) -> String {
        match from {
            Some(from) if from.starts_with("GTK") || from == "the portal" => "gtk".to_owned(),
            Some(from) if from.starts_with("Qt") => "qt".to_owned(),
            Some(from) => from.to_owned(),
            None => String::new(),
        }
    }
}

/// `ui.fonts_from`'s and `ui.colors_from`'s value for skwir's own.
pub const OWN: &str = "skwir";

/// Before `ui.fonts_from` stood apart, Desktop colors turned off gave
/// skwir's own fonts too: someone who turned it off keeps them, as
/// `Settings::rename` moves a value (the file says it at the next save;
/// until then this is done again at each start, to the same effect).
/// Called before `ui.fonts_from` is declared, so only a value stored
/// counts.
pub fn keep_own_fonts(settings: &Settings) {
    if settings.get::<bool>(DESKTOP_COLORS_KEY) == Some(false)
        && settings.text(crate::settings::FONTS_FROM_KEY).is_none()
    {
        settings.set(crate::settings::FONTS_FROM_KEY, OWN.into());
    }
}

/// Colors from replaced the Desktop colors switch: someone who turned it
/// off keeps skwir's own colors (`keep_own_fonts` read it first, for the
/// fonts). The switch's key is then forgotten, and named Colors from's for
/// `Settings::check`; a value it held is not moved, being a switch's.
pub fn keep_own_colors(settings: &Settings) {
    if settings.get::<bool>(DESKTOP_COLORS_KEY) == Some(false)
        && settings.text(COLORS_FROM_KEY).is_none()
    {
        settings.set(COLORS_FROM_KEY, OWN.into());
    }
    if settings.text(DESKTOP_COLORS_KEY).is_some() {
        settings.unset(DESKTOP_COLORS_KEY);
    }
    settings.rename(DESKTOP_COLORS_KEY, COLORS_FROM_KEY);
}

/// Where the colors were read while Colors from is Automatic (`Plasma`,
/// `GTK`): what its Automatic comes to (`settings::show_resolved`).
pub fn colors_source() -> Option<String> {
    said().and_then(|said| said.colors_from)
}

/// The fonts the desktop names as the settings choose to read them, with
/// their sizes and where they were read: none for skwir's own.
fn chosen(settings: &Settings) -> Option<skwir_platform::desktop_style::Fonts> {
    read(Source::of_fonts(settings))
}

/// The fonts `source` names, with their sizes and where they were read:
/// none for skwir's own.
fn read(source: Source) -> Option<skwir_platform::desktop_style::Fonts> {
    let said = said()?;
    match source {
        Source::Own => None,
        Source::Automatic => Some(skwir_platform::desktop_style::Fonts {
            from: said.fonts_from.unwrap_or_default(),
            interface: said.font,
            document: said.document_font,
            monospace: said.monospace_font,
            interface_px: said.font_px,
            monospace_px: said.monospace_px,
        }),
        Source::Gtk => said.gtk,
        Source::Qt => said.qt,
    }
}

/// What the desktop says of text sizes (ADR-117): the interface font's
/// size and the monospace font's, in pixels, and where they were read; and
/// its text scaling factor. Followed whatever fonts are chosen, as more
/// contrast is: how large text is written is a need, not a style.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sizes {
    pub interface: Option<f32>,
    pub monospace: Option<f32>,
    pub from: Option<String>,
    pub scaling: Option<f32>,
}

pub fn sizes(settings: &Settings) -> Sizes {
    let scaling = said()
        .and_then(|said| said.text_scaling)
        .map(|factor| factor as f32);
    // Skwir's own fonts are a look: written at the sizes Automatic reads.
    let fonts = read(match Source::of_fonts(settings) {
        Source::Own => Source::Automatic,
        source => source,
    });
    Sizes {
        interface: fonts.as_ref().and_then(|fonts| fonts.interface_px),
        monospace: fonts.as_ref().and_then(|fonts| fonts.monospace_px),
        from: fonts
            .map(|fonts| fonts.from)
            .filter(|from| !from.is_empty()),
        scaling,
    }
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
            ..DesktopStyle::default()
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

        // Skwir's own: its colors, but for the contrast asked.
        set(with_scheme(Some(breeze()), None));
        app.settings.set(COLORS_FROM_KEY, OWN.into());
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

        app.settings.set(COLORS_FROM_KEY, OWN.into());
        assert_eq!(colors(&app.settings), Desktop::default());
        assert_eq!(Theme::of(false, colors(&app.settings)), Theme::light());
    }

    #[test]
    fn more_contrast_is_followed_whatever_the_colors_are_from() {
        let (_tmp, app) = app();
        app.settings.set(COLORS_FROM_KEY, OWN.into());
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
    fn the_desktop_s_fonts_are_lent_unless_skwir_s_own_are_chosen() {
        let (_tmp, app) = app();
        assert_eq!(fonts(&app.settings), Fonts::default(), "nothing said yet");
        set(teal());
        let lent = Fonts {
            interface: Some("Noto Sans".into()),
            document: Some("Noto Serif".into()),
            monospace: None,
        };
        assert_eq!(fonts(&app.settings), lent);
        // Colors from is the colors alone (ADR-117).
        app.settings.set(COLORS_FROM_KEY, OWN.into());
        assert_eq!(fonts(&app.settings), lent);
        app.settings
            .set(crate::settings::FONTS_FROM_KEY, OWN.into());
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

    #[test]
    fn fonts_and_their_sizes_are_read_where_the_user_chose() {
        use skwir_platform::desktop_style::Fonts as Said;
        let (_tmp, app) = app();
        let px = |points: f32| points * 96. / 72.;
        set(DesktopStyle {
            font: Some("Noto Sans".into()),
            font_px: Some(px(10.)),
            fonts_from: Some("GTK".into()),
            gtk: Some(Said {
                from: "GTK".into(),
                interface: Some("Noto Sans".into()),
                interface_px: Some(px(10.)),
                ..Said::default()
            }),
            qt: Some(Said {
                from: "Qt (qt6ct)".into(),
                interface: Some("DejaVu Sans".into()),
                interface_px: Some(px(9.)),
                monospace: Some("Hack".into()),
                monospace_px: Some(px(10.)),
                ..Said::default()
            }),
            text_scaling: Some(1.25),
            ..DesktopStyle::default()
        });
        let choose = |value: &str| {
            app.settings
                .set(crate::settings::FONTS_FROM_KEY, value.into());
        };
        // Automatic: what the desktop's settings name first.
        assert_eq!(fonts(&app.settings).interface.as_deref(), Some("Noto Sans"));
        let automatic = sizes(&app.settings);
        assert_eq!(automatic.interface, Some(px(10.)));
        assert_eq!(automatic.from.as_deref(), Some("GTK"));
        assert_eq!(automatic.scaling, Some(1.25));
        // Qt's alone, families and sizes.
        choose("qt");
        assert_eq!(
            fonts(&app.settings).interface.as_deref(),
            Some("DejaVu Sans")
        );
        assert_eq!(fonts(&app.settings).monospace.as_deref(), Some("Hack"));
        let qt = sizes(&app.settings);
        assert_eq!((qt.interface, qt.monospace), (Some(px(9.)), Some(px(10.))));
        assert_eq!(qt.from.as_deref(), Some("Qt (qt6ct)"));
        // Skwir's own colors are the colors alone: the fonts stay Qt's.
        app.settings.set(COLORS_FROM_KEY, OWN.into());
        assert_eq!(
            fonts(&app.settings).interface.as_deref(),
            Some("DejaVu Sans")
        );
        // Skwir's own: its fonts, at the sizes Automatic reads, the
        // scaling followed too.
        choose(OWN);
        assert_eq!(fonts(&app.settings).interface, None);
        assert_eq!(sizes(&app.settings), automatic);
    }

    #[test]
    fn desktop_colors_turned_off_before_keeps_skwir_s_fonts() {
        use crate::settings::FONTS_FROM_KEY;
        // As a file of 0.3.0 says it, before the settings are declared.
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        app.settings
            .set(DESKTOP_COLORS_KEY, toml::Value::Boolean(false));
        keep_own_fonts(&app.settings);
        assert_eq!(Source::of_fonts(&app.settings), Source::Own);
        // Chosen since, it is never moved again.
        app.settings.set(FONTS_FROM_KEY, "gtk".into());
        keep_own_fonts(&app.settings);
        assert_eq!(Source::of_fonts(&app.settings), Source::Gtk);
        // Desktop colors left on: nothing moves.
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        keep_own_fonts(&app.settings);
        assert_eq!(app.settings.text(FONTS_FROM_KEY), None);
    }

    #[test]
    fn desktop_colors_turned_off_before_keeps_skwir_s_colors() {
        // As a file of 0.3.0 says it, before the settings are declared.
        let file = |desktop_colors: bool| {
            let tmp = tempfile::tempdir().unwrap();
            let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
            app.settings
                .set(DESKTOP_COLORS_KEY, toml::Value::Boolean(desktop_colors));
            keep_own_fonts(&app.settings);
            keep_own_colors(&app.settings);
            (tmp, app)
        };
        let (_tmp, off) = file(false);
        assert_eq!(Source::of_colors(&off.settings), Source::Own);
        assert_eq!(Source::of_fonts(&off.settings), Source::Own);
        assert_eq!(off.settings.text(DESKTOP_COLORS_KEY), None, "forgotten");
        // Left on: Automatic, the switch forgotten, nothing moved into
        // Colors from.
        let (_tmp, on) = file(true);
        assert_eq!(on.settings.text(COLORS_FROM_KEY), None);
        assert_eq!(on.settings.text(DESKTOP_COLORS_KEY), None);
        // Chosen since, it is never moved again.
        off.settings.set(COLORS_FROM_KEY, "qt".into());
        keep_own_colors(&off.settings);
        assert_eq!(Source::of_colors(&off.settings), Source::Qt);
    }

    #[test]
    fn gtk_s_or_qt_s_colors_are_worn_when_chosen() {
        use skwir_platform::desktop_style::Colors;
        let (_tmp, app) = app();
        let night = Scheme {
            bg: channels(0x202020),
            surface: channels(0x303030),
            text: channels(0xf0f0f0),
            ..breeze()
        };
        set(DesktopStyle {
            colors_from: Some("Plasma".into()),
            gtk_colors: Some(Colors {
                from: "GTK".into(),
                schemes: skwir_platform::desktop_style::Schemes {
                    light: None,
                    dark: Some(night.clone()),
                },
            }),
            qt_colors: None,
            ..with_scheme(Some(breeze()), None)
        });
        assert!(colors(&app.settings).schemes.light.is_some(), "Automatic's");
        app.settings.set(COLORS_FROM_KEY, "gtk".into());
        let gtk = colors(&app.settings);
        assert!(gtk.schemes.light.is_none() && gtk.schemes.dark.is_some());
        // Qt names none: skwir's colors, with the accent its scheme had not.
        app.settings.set(COLORS_FROM_KEY, "qt".into());
        assert!(colors(&app.settings).schemes.light.is_none());
        assert_eq!(colors_source().as_deref(), Some("Plasma"));
        assert_eq!(Source::now(Some("Plasma")), "Plasma");
        assert_eq!(Source::now(Some("Qt (qt6ct)")), "qt");
        assert_eq!(Source::now(Some("the portal")), "gtk");
    }
}
