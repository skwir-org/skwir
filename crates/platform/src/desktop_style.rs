//! What the user chose on their desktop for how applications look
//! (ADR-071): an accent, a preference for more contrast, a color scheme,
//! the families of its fonts (the interface's, documents', monospace:
//! ADR-073). Skwir's own controls wear them; it never imitates a
//! desktop's palette.
//!
//! On Linux the accent and the contrast are the settings portal's
//! (`org.freedesktop.appearance`, over D-Bus). The color scheme is
//! Plasma's (`kde_scheme`: `kdeglobals`, followed as Plasma replaces it)
//! on Plasma, elsewhere the GTK3 theme the desktop names (`gtk_theme`),
//! read again when it names another. The fonts are read where the
//! desktop keeps them (`desktop_settings`: `GSettings`, `kdeglobals`,
//! xfconf, LXQt's `lxqt.conf`), then GTK's (`settings.ini`, then the
//! portal's `org.gnome.desktop.interface`, which it answers outside GNOME
//! too), then Qt's (qt6ct's or qt5ct's, as `QT_QPA_PLATFORMTHEME` names
//! it); a fontconfig alias among them (`Monospace`) is the family
//! fontconfig matches it to (`fonts::unalias`). Each font's size is read
//! with its family, from the same settings, and GTK's and Qt's fonts are
//! also said apart, for the user to choose (ADR-117).
//!
//! On macOS the interface font is the system's (San Francisco) at 13 px,
//! as its controls are written, and its colors those `AppKit` draws its own
//! windows in (`NSColor`'s, each look's as its appearance resolves them:
//! the accent the user chose, which its lists' selection follows, and
//! Increase contrast's appearances when it is on). `AppKit` tells only its own windows of a
//! change, so they are read again when the window's look changes or it is
//! activated (`FOLLOWED`). On Windows the family it writes messages in
//! (`Segoe UI`), the Text size chosen in its Accessibility settings, the
//! accent (`UISettings`) and a contrast theme's colors (`GetSysColor`),
//! followed in the registry (`windows::style`). Windows names no other
//! colors: its own applications' are their toolkit's, which Skwir does
//! not copy (ADR-071, point 2).

use skwir_core::CancelToken;

/// Whether this system's colors are read (its scheme, its accent):
/// Linux's desktops', macOS's, Windows' accent and contrast themes
/// (ADR-071).
pub const COLORS: bool = cfg!(any(target_os = "linux", target_os = "macos", windows));

/// Whether `follow` follows what the system says by itself; where not
/// (macOS, whose notices of its colors reach only `AppKit`'s own windows),
/// the window reads it again (`read`) when its look changes or it is
/// activated.
pub const FOLLOWED: bool = !cfg!(target_os = "macos");

/// Whether this system's fonts and their sizes are read (ADR-117): Linux's
/// desktops, macOS's interface font, Windows' and its Text size.
pub const FONTS: bool = cfg!(any(target_os = "linux", target_os = "macos", windows));

/// Whether GTK's and Qt's settings are read apart, for the user to choose
/// (`Fonts`, `DesktopStyle::gtk` and `qt`): where both toolkits set
/// applications up, the free systems'.
pub const TOOLKITS: bool = cfg!(target_os = "linux");

/// A color as sRGB channels from 0 to 1.
pub type Rgb = [f32; 3];

/// What the desktop says of how applications look; `Default` when it
/// says nothing.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DesktopStyle {
    /// The accent the system names (the portal's), apart from any scheme.
    pub accent: Option<Rgb>,
    /// The desktop asks for more contrast.
    pub more_contrast: bool,
    /// The interface font's family (`Noto Sans`), without its size or
    /// style.
    pub font: Option<String>,
    /// The family documents are read in (GNOME's `document-font-name`).
    #[serde(default)]
    pub document_font: Option<String>,
    /// The monospace font's family (`monospace-font-name`, Plasma's
    /// `fixed`).
    #[serde(default)]
    pub monospace_font: Option<String>,
    /// The colors of the scheme the user chose, a look's each.
    #[serde(default)]
    pub schemes: Schemes,
    /// The interface font's size and the monospace font's, in pixels, as
    /// the settings that name `font` and `monospace_font` say them, and
    /// which settings those are (ADR-117).
    #[serde(default)]
    pub font_px: Option<f32>,
    #[serde(default)]
    pub monospace_px: Option<f32>,
    #[serde(default)]
    pub fonts_from: Option<String>,
    /// The fonts GTK's settings name, and Qt's, each apart: what the user
    /// may choose to follow instead (`Fonts from`, ADR-117).
    #[serde(default)]
    pub gtk: Option<Fonts>,
    #[serde(default)]
    pub qt: Option<Fonts>,
    /// Which settings named `schemes` (`Plasma`, `GNOME`, `GTK`, `Qt
    /// (qt6ct)`), and GTK's colors and Qt's, each apart: what the user may
    /// choose to follow instead (`Colors from`, ADR-071).
    #[serde(default)]
    pub colors_from: Option<String>,
    #[serde(default)]
    pub gtk_colors: Option<Colors>,
    #[serde(default)]
    pub qt_colors: Option<Colors>,
    /// The desktop's text scaling factor (GNOME's Large Text), where it
    /// names one.
    #[serde(default)]
    pub text_scaling: Option<f64>,
}

/// The fonts one source names: their families, the interface's and the
/// monospace font's sizes in pixels, and which source it is (`GTK`,
/// `the portal`, `Qt (qt6ct)`).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Fonts {
    pub from: String,
    #[serde(default)]
    pub interface: Option<String>,
    #[serde(default)]
    pub document: Option<String>,
    #[serde(default)]
    pub monospace: Option<String>,
    #[serde(default)]
    pub interface_px: Option<f32>,
    #[serde(default)]
    pub monospace_px: Option<f32>,
}

/// The colors one source names (GTK's theme, Qt's scheme), and which
/// source it is.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Colors {
    pub from: String,
    #[serde(default)]
    pub schemes: Schemes,
}

/// The colors a desktop's scheme names for the base of a look (ADR-071,
/// point 4): what is read on (`bg`, `text`), chrome (`surface`), and what
/// it says of the rest, each `None` it does not say derived from these. A
/// color with transparency is laid over `bg`, as it would be drawn.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Scheme {
    pub bg: Rgb,
    pub surface: Rgb,
    pub text: Rgb,
    #[serde(default)]
    pub text_muted: Option<Rgb>,
    #[serde(default)]
    pub zebra: Option<Rgb>,
    #[serde(default)]
    pub border: Option<Rgb>,
    #[serde(default)]
    pub accent: Option<Rgb>,
    /// The selection's background and its text.
    #[serde(default)]
    pub selection: Option<(Rgb, Rgb)>,
    #[serde(default)]
    pub error: Option<Rgb>,
    #[serde(default)]
    pub warning: Option<Rgb>,
    #[serde(default)]
    pub success: Option<Rgb>,
}

impl Scheme {
    /// Whether it is a dark look's: its text lighter than what it is read
    /// on.
    pub fn is_dark(&self) -> bool {
        luminance(self.bg) < luminance(self.text)
    }
}

/// The schemes a desktop names, one a look at most: Plasma's is one look's,
/// a GTK3 theme may have a dark variant beside it (`gtk-dark.css`).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Schemes {
    #[serde(default)]
    pub light: Option<Scheme>,
    #[serde(default)]
    pub dark: Option<Scheme>,
}

impl Schemes {
    /// Keep `scheme` as its look's, unless that look has one already.
    pub fn add(&mut self, scheme: Scheme) {
        let slot = if scheme.is_dark() {
            &mut self.dark
        } else {
            &mut self.light
        };
        if slot.is_none() {
            *slot = Some(scheme);
        }
    }

    /// The scheme of the look `dark` names.
    pub fn of(&self, dark: bool) -> Option<&Scheme> {
        if dark {
            self.dark.as_ref()
        } else {
            self.light.as_ref()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.light.is_none() && self.dark.is_none()
    }
}

/// `color`'s relative luminance (WCAG 2).
fn luminance(color: Rgb) -> f32 {
    let linear = color.map(|c| {
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    });
    0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2]
}

/// `color` (with its opacity, from 0 to 1) laid over `ground`, as it is
/// drawn: mixed in sRGB.
pub fn over(color: [f32; 4], ground: Rgb) -> Rgb {
    let a = color[3].clamp(0., 1.);
    [0, 1, 2].map(|i| (color[i] * a + ground[i] * (1. - a)).clamp(0., 1.))
}

/// The colors `AppKit` draws its own windows in, as one appearance resolves
/// them, as sRGB with their opacity: a list's ground (`controlBackground`),
/// the window's (`windowBackground`), labels and secondary labels,
/// separators, the second of the alternating rows, the accent, the
/// selection (`selectedContentBackground`) and its text, and the system's
/// red, orange and green.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, Debug)]
struct SystemColors {
    ground: [f32; 4],
    chrome: [f32; 4],
    text: [f32; 4],
    secondary_text: [f32; 4],
    separator: [f32; 4],
    alternate_row: Option<[f32; 4]>,
    accent: [f32; 4],
    selection: [f32; 4],
    selection_text: [f32; 4],
    red: [f32; 4],
    orange: [f32; 4],
    green: [f32; 4],
}

/// macOS's colors as the base of a look: its translucent labels and
/// lines laid over the ground, as they are drawn; the ground taken opaque.
#[cfg(any(target_os = "macos", test))]
fn system_scheme(colors: &SystemColors) -> Scheme {
    let bg = [colors.ground[0], colors.ground[1], colors.ground[2]];
    let on = |color: [f32; 4]| over(color, bg);
    let selection = on(colors.selection);
    Scheme {
        bg,
        surface: on(colors.chrome),
        text: on(colors.text),
        text_muted: Some(on(colors.secondary_text)),
        zebra: colors.alternate_row.map(on),
        border: Some(on(colors.separator)),
        accent: Some(on(colors.accent)),
        selection: Some((selection, over(colors.selection_text, selection))),
        error: Some(on(colors.red)),
        warning: Some(on(colors.orange)),
        success: Some(on(colors.green)),
    }
}

/// A Windows contrast theme as the base of a look (ADR-071, point 4), from
/// its system colors (`windows::style::contrast_colors`, `0x00BBGGRR`):
/// the window's ground and text, a button's face as the chrome, text
/// grayed, the highlight as the accent and the selection. Its outcomes
/// are derived: a contrast theme names none.
#[cfg(any(windows, test))]
fn contrast_scheme(
    [
        window,
        window_text,
        face,
        gray_text,
        highlight,
        highlight_text,
    ]: [u32; 6],
) -> Scheme {
    let rgb =
        |color: u32| [0, 8, 16].map(|shift| f32::from(((color >> shift) & 0xff) as u8) / 255.);
    Scheme {
        bg: rgb(window),
        surface: rgb(face),
        text: rgb(window_text),
        text_muted: Some(rgb(gray_text)),
        zebra: None,
        border: Some(rgb(window_text)),
        accent: Some(rgb(highlight)),
        selection: Some((rgb(highlight), rgb(highlight_text))),
        error: None,
        warning: None,
        success: None,
    }
}

/// What the desktop says now. It asks the session's bus and may run a
/// desktop's settings program: on a worker.
pub fn read() -> DesktopStyle {
    imp::read()
}

/// What the desktop says, then what it says each time it changes:
/// `changed` is called from a thread of its own, at once and on each
/// change, until `stop`. The session's bus ended (started again), what it
/// says is read again once it is back, and followed there. The thread
/// waits on the bus between changes, on a timer only to connect again
/// while the bus is gone.
pub fn follow(changed: impl FnMut(DesktopStyle) + Send + 'static, stop: CancelToken) {
    imp::follow(changed, stop);
}

/// The family of a font as Pango describes it (`Noto Sans Bold 10`,
/// `Cantarell, Noto Sans 11`): the first family named, without the size
/// or the style that follow it.
pub fn pango_family(description: &str) -> Option<String> {
    // Pango's words for a style, a weight, a stretch or a variant.
    const STYLE: &[&str] = &[
        "normal",
        "roman",
        "oblique",
        "italic",
        "small-caps",
        "all-small-caps",
        "petite-caps",
        "all-petite-caps",
        "unicase",
        "title-caps",
        "thin",
        "ultra-light",
        "extra-light",
        "light",
        "semi-light",
        "demi-light",
        "book",
        "regular",
        "medium",
        "semi-bold",
        "demi-bold",
        "bold",
        "ultra-bold",
        "extra-bold",
        "heavy",
        "black",
        "ultra-black",
        "extra-black",
        "ultra-heavy",
        "extra-heavy",
        "ultra-condensed",
        "extra-condensed",
        "condensed",
        "semi-condensed",
        "semi-expanded",
        "expanded",
        "extra-expanded",
        "ultra-expanded",
    ];
    let mut words: Vec<&str> = description.split_whitespace().collect();
    if words
        .last()
        .is_some_and(|w| w.trim_end_matches("px").parse::<f32>().is_ok())
    {
        words.pop();
    }
    while words
        .last()
        .is_some_and(|w| STYLE.contains(&w.to_ascii_lowercase().as_str()))
    {
        words.pop();
    }
    let families = words.join(" ");
    let first = families.split(',').next()?.trim();
    (!first.is_empty()).then(|| first.to_owned())
}

/// The size of a font as Pango describes it, in pixels: its last word,
/// in points (`Noto Sans 10`: 13.3 px at 96 dpi) or pixels (`Sans 12px`).
pub fn pango_size(description: &str) -> Option<f32> {
    let last = description.split_whitespace().last()?;
    let (number, pixels) = match last.strip_suffix("px") {
        Some(number) => (number, true),
        None => (last, false),
    };
    let size: f32 = number.parse().ok()?;
    (size > 0. && size.is_finite()).then(|| if pixels { size } else { points(size) })
}

/// A size in points, in pixels at 96 dpi: as the desktops draw it, before
/// the display's scale.
fn points(size: f32) -> f32 {
    size * 96. / 72.
}

/// A font as Qt writes it, unquoted; `None` for one an old qt5ct wrote
/// as Qt's binary form (`@Variant(…)`), which names nothing read here:
/// qt5ct writes it again as text when its fonts are next saved.
fn qt_written(font: &str) -> Option<&str> {
    let font = font.trim().trim_matches('"');
    (!font.starts_with("@Variant(")).then_some(font)
}

/// The family of a font as Qt writes it in `kdeglobals` (`Noto
/// Sans,10,-1,5,400,0,0,0,0,0`), quoted or not.
pub fn qt_family(font: &str) -> Option<String> {
    let family = qt_written(font)?.split(',').next()?.trim();
    (!family.is_empty()).then(|| family.to_owned())
}

/// The size of a font as Qt writes it, in pixels: its points (the second
/// field), else its pixels (the third) where the points say -1.
pub fn qt_size(font: &str) -> Option<f32> {
    let mut fields = qt_written(font)?.split(',').skip(1);
    let points_said: f32 = fields.next()?.trim().parse().ok()?;
    if points_said > 0. {
        return Some(points(points_said));
    }
    let pixels: f32 = fields.next()?.trim().parse().ok()?;
    (pixels > 0.).then_some(pixels)
}

#[cfg(target_os = "linux")]
mod imp {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    use skwir_core::CancelToken;
    use zbus::blocking::Connection;
    use zbus::zvariant::{OwnedValue, Value};

    use super::{DesktopStyle, Schemes};
    use crate::desktop_settings::{self as desktop, Chosen};

    const DESTINATION: &str = "org.freedesktop.portal.Desktop";
    const PATH: &str = "/org/freedesktop/portal/desktop";
    const SETTINGS: &str = "org.freedesktop.portal.Settings";
    const APPEARANCE: &str = "org.freedesktop.appearance";
    const INTERFACE: &str = "org.gnome.desktop.interface";
    /// How long Plasma's writes of `kdeglobals` are let settle before it
    /// is read again: choosing a scheme writes it more than once.
    const SETTLE: Duration = Duration::from_millis(200);

    /// The session bus, the program's own connection (`bus::session`).
    pub(super) fn read() -> DesktopStyle {
        let bus = crate::bus::session().ok();
        read_from(bus.as_ref(), &TheDesktop)
    }

    pub(super) fn follow(changed: impl FnMut(DesktopStyle) + Send + 'static, stop: CancelToken) {
        let thread = std::thread::Builder::new()
            .name("skwir-desktop-style".into())
            .spawn(move || follow_on(crate::bus::session, Arc::new(TheDesktop), changed, &stop));
        if let Err(error) = thread {
            tracing::debug!("the desktop's style is not followed: {error}");
        }
    }

    /// The fonts a source names, each a family, the interface's and the
    /// monospace font's with their sizes in pixels, and the source.
    #[derive(Clone, Debug, Default)]
    pub(super) struct Named {
        pub(super) interface: Option<String>,
        pub(super) document: Option<String>,
        pub(super) monospace: Option<String>,
        pub(super) interface_px: Option<f32>,
        pub(super) monospace_px: Option<f32>,
        pub(super) from: Option<&'static str>,
    }

    impl Named {
        /// Each font as this names it, else as `other` does, a size with
        /// its family; the source the interface's.
        pub(super) fn or(self, other: Named) -> Named {
            let mut named = self;
            if named.interface.is_none() {
                named.interface = other.interface;
                named.interface_px = other.interface_px;
                named.from = other.from;
            }
            if named.document.is_none() {
                named.document = other.document;
            }
            if named.monospace.is_none() {
                named.monospace = other.monospace;
                named.monospace_px = other.monospace_px;
            }
            named
        }

        fn names_any(&self) -> bool {
            self.interface.is_some() || self.document.is_some() || self.monospace.is_some()
        }

        /// Apart, as the user may choose it: its families as fontconfig
        /// matches them.
        fn apart(&self) -> Option<super::Fonts> {
            self.names_any().then(|| super::Fonts {
                from: self.from.unwrap_or_default().to_owned(),
                interface: self.interface.as_deref().map(crate::fonts::unalias),
                document: self.document.as_deref().map(crate::fonts::unalias),
                monospace: self.monospace.as_deref().map(crate::fonts::unalias),
                interface_px: self.interface_px,
                monospace_px: self.monospace_px,
            })
        }
    }

    /// The colors a source names, a look's each, and the source.
    #[derive(Clone, Debug, Default)]
    pub(super) struct Painted {
        pub(super) schemes: Schemes,
        pub(super) from: Option<&'static str>,
    }

    impl Painted {
        /// One scheme, said by `from`.
        pub(super) fn one(scheme: Option<super::Scheme>, from: &'static str) -> Painted {
            let mut schemes = Schemes::default();
            if let Some(scheme) = scheme {
                schemes.add(scheme);
            }
            Painted {
                schemes,
                from: Some(from),
            }
        }

        /// These colors where they name any, else `other`'s.
        pub(super) fn or(self, other: Painted) -> Painted {
            if self.schemes.is_empty() { other } else { self }
        }

        /// Apart, as the user may choose it.
        fn apart(&self) -> Option<super::Colors> {
            (!self.schemes.is_empty()).then(|| super::Colors {
                from: self.from.unwrap_or_default().to_owned(),
                schemes: self.schemes.clone(),
            })
        }
    }

    /// What the desktop's own settings and files say beside the portal,
    /// read again at each change.
    pub(super) trait Own: Send + Sync + 'static {
        /// The fonts its settings name.
        fn fonts(&self) -> Named;
        /// The fonts GTK's `settings.ini` names.
        fn gtk_fonts(&self) -> Named {
            Named::default()
        }
        /// The fonts Qt's applications are given: Plasma's, LXQt's,
        /// qt6ct's or qt5ct's.
        fn qt_fonts(&self) -> Named {
            Named::default()
        }
        /// Whether GTK's applications here draw through Wayland: there GTK
        /// takes its settings from the settings portal (else `GSettings`)
        /// before `settings.ini`, as GTK 3's and 4's Wayland backends do;
        /// on X11 without an XSETTINGS daemon, from `settings.ini`.
        fn wayland(&self) -> bool {
            std::env::var_os("WAYLAND_DISPLAY").is_some_and(|display| !display.is_empty())
        }
        /// The colors its own settings name: Plasma's scheme, LXQt's
        /// palette, or the GTK3 theme GNOME's, MATE's, Cinnamon's or Xfce's
        /// settings name, read by `theme`.
        fn own_colors(&self, theme: &dyn Fn(&str) -> Schemes) -> Painted;
        /// The GTK3 theme GTK's own settings name (`settings.ini`, GTK 3's
        /// before GTK 4's, as the themes read are GTK 3's).
        fn gtk_theme(&self) -> Option<String> {
            None
        }
        /// The GTK3 theme `GTK_THEME` forces on GTK's applications.
        fn forced_gtk_theme(&self) -> Option<String> {
            desktop::forced_gtk_theme()
        }
        /// The colors Qt's applications are given: Plasma's scheme, LXQt's
        /// palette, qt6ct's or qt5ct's scheme.
        fn qt_colors(&self) -> Painted {
            Painted::default()
        }
        /// The colors of the GTK3 theme `name` (`gtk_theme::schemes`).
        fn theme(&self, name: &str) -> Schemes {
            crate::gtk_theme::schemes(name, &crate::gtk_theme::folders())
        }
        /// The files it says a change in by writing or replacing them
        /// (Plasma's `kdeglobals`, LXQt's `lxqt.conf`, qt5ct's and qt6ct's
        /// files, GTK's `settings.ini`), each followed in its folder.
        fn followed(&self) -> Vec<PathBuf>;
    }

    /// This session's desktop.
    struct TheDesktop;

    impl Own for TheDesktop {
        fn fonts(&self) -> Named {
            own_fonts()
        }

        fn gtk_fonts(&self) -> Named {
            let named = desktop::gtk_settings("gtk-font-name");
            let named = named.first();
            Named {
                interface: named.and_then(|name| super::pango_family(name)),
                interface_px: named.and_then(|name| super::pango_size(name)),
                from: Some("GTK"),
                ..Named::default()
            }
        }

        fn qt_fonts(&self) -> Named {
            qt_fonts()
        }

        fn own_colors(&self, theme: &dyn Fn(&str) -> Schemes) -> Painted {
            own_colors(theme)
        }

        fn gtk_theme(&self) -> Option<String> {
            desktop::gtk_settings("gtk-theme-name").pop()
        }

        fn qt_colors(&self) -> Painted {
            match desktop::chosen() {
                Some(Chosen::Kde | Chosen::Lxqt) => own_colors(&|_| Schemes::default()),
                _ => match desktop::qt_ct() {
                    Some((program, text)) => Painted::one(
                        crate::qt_palette::qt_ct(&text),
                        if program == "qt6ct" {
                            "Qt (qt6ct)"
                        } else {
                            "Qt (qt5ct)"
                        },
                    ),
                    None => Painted::default(),
                },
            }
        }

        fn followed(&self) -> Vec<PathBuf> {
            let Some(config) = crate::xdg::config_home() else {
                return Vec::new();
            };
            let desktop = match desktop::chosen() {
                Some(Chosen::Kde) => crate::kde_scheme::user_file(),
                Some(Chosen::Lxqt) => Some(config.join("lxqt/lxqt.conf")),
                _ => None,
            };
            desktop
                .into_iter()
                .chain(
                    [
                        "qt5ct/qt5ct.conf",
                        "qt6ct/qt6ct.conf",
                        "gtk-3.0/settings.ini",
                        "gtk-4.0/settings.ini",
                    ]
                    .map(|file| config.join(file)),
                )
                .collect()
        }
    }

    /// The portal's keys of the fonts, in `INTERFACE`.
    const FONT_KEYS: [&str; 3] = ["font-name", "document-font-name", "monospace-font-name"];

    /// What the desktop says: each font as `own` names it where the
    /// desktop keeps it, else as the portal on `bus` names it; its scheme
    /// as `own` finds it.
    pub(super) fn read_from(bus: Option<&Connection>, own: &dyn Own) -> DesktopStyle {
        let portal =
            |namespace: &str, key: &str| bus.and_then(|bus| portal_read(bus, namespace, key));
        let portal_string = |namespace: &str, key: &str| {
            portal(namespace, key).and_then(|value| String::try_from(value).ok())
        };
        let [interface, document, monospace] = FONT_KEYS;
        // What the portal answers, which GTK reads where `settings.ini`
        // names nothing.
        let said = |key: &str| portal_string(INTERFACE, key);
        let (font_name, monospace_name) = (said(interface), said(monospace));
        let portal_named = Named {
            interface: font_name.as_deref().and_then(super::pango_family),
            interface_px: font_name.as_deref().and_then(super::pango_size),
            document: said(document).as_deref().and_then(super::pango_family),
            monospace: monospace_name.as_deref().and_then(super::pango_family),
            monospace_px: monospace_name.as_deref().and_then(super::pango_size),
            from: Some("the portal"),
        };
        // As GTK's applications here take them (`Own::wayland`).
        let gtk = if own.wayland() {
            portal_named.or(own.gtk_fonts())
        } else {
            own.gtk_fonts().or(portal_named)
        };
        let qt = own.qt_fonts();
        // The desktop's own, then GTK's, then Qt's (ADR-117).
        let named = own.fonts().or(gtk.clone()).or(qt.clone());
        let unaliased =
            |family: Option<String>| family.map(|family| crate::fonts::unalias(&family));
        let text_scaling = portal(INTERFACE, "text-scaling-factor")
            .and_then(|value| f64::try_from(value).ok())
            .filter(|factor| *factor > 0. && factor.is_finite());
        // A GTK3 theme read once a reading, however many name it.
        let read = std::cell::RefCell::new(std::collections::HashMap::<String, Schemes>::new());
        let theme = |name: &str| {
            read.borrow_mut()
                .entry(name.to_owned())
                .or_insert_with(|| own.theme(name))
                .clone()
        };
        // GTK's theme as GTK's applications here take it (`Own::wayland`).
        let portal_theme = portal_string(INTERFACE, "gtk-theme").filter(|theme| !theme.is_empty());
        let gtk_theme = own.forced_gtk_theme().or_else(|| {
            if own.wayland() {
                portal_theme.or_else(|| own.gtk_theme())
            } else {
                own.gtk_theme().or(portal_theme)
            }
        });
        let gtk_colors = Painted {
            schemes: gtk_theme.as_deref().map(theme).unwrap_or_default(),
            from: Some("GTK"),
        };
        let qt_colors = own.qt_colors();
        // The desktop's own, then GTK's, then Qt's, as the fonts.
        let colors = own
            .own_colors(&theme)
            .or(gtk_colors.clone())
            .or(qt_colors.clone());
        DesktopStyle {
            accent: portal(APPEARANCE, "accent-color").and_then(accent),
            more_contrast: portal(APPEARANCE, "contrast")
                .and_then(|value| u32::try_from(value).ok())
                == Some(1),
            font_px: named.interface_px,
            monospace_px: named.monospace_px,
            fonts_from: named.from.map(str::to_owned),
            gtk: gtk.apart(),
            qt: qt.apart(),
            text_scaling,
            font: unaliased(named.interface),
            document_font: unaliased(named.document),
            monospace_font: unaliased(named.monospace),
            schemes: colors.schemes.clone(),
            colors_from: colors.from.map(str::to_owned),
            gtk_colors: gtk_colors.apart(),
            qt_colors: qt_colors.apart(),
        }
    }

    /// What was said last, and whom to tell of a change: read again at
    /// the bus's word and the file's alike, on a thread of its own.
    struct Following {
        /// The bus the portal is asked on: the last connection made.
        bus: Mutex<Option<Connection>>,
        own: Arc<dyn Own>,
        said: Mutex<Option<DesktopStyle>>,
        changed: Mutex<Box<dyn FnMut(DesktopStyle) + Send>>,
        /// Whether a reading runs, and whether it reads once more after.
        reading: Mutex<Option<bool>>,
    }

    impl Following {
        /// Read again, and tell what differs from what was said. What the
        /// reading freed (a theme's sheets) is given back before the
        /// thread waits again, which may be for hours.
        fn read_again(&self) {
            let bus = self
                .bus
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            let now = read_from(bus.as_ref(), &*self.own);
            let mut said = self.said.lock().unwrap_or_else(PoisonError::into_inner);
            if said.as_ref() != Some(&now) {
                *said = Some(now.clone());
                (self.changed.lock().unwrap_or_else(PoisonError::into_inner))(now);
            }
            drop(said);
            skwir_core::scheduler::release_now();
        }

        /// [`Following::read_again`] on a thread of its own, as a listener
        /// on the shared bus takes each message at once and the portal may
        /// answer slowly: one reading at a time, a change said meanwhile
        /// read once more after.
        fn read_soon(self: &Arc<Self>) {
            {
                let mut reading = self.reading.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(again) = reading.as_mut() {
                    *again = true;
                    return;
                }
                *reading = Some(false);
            }
            let this = Arc::clone(self);
            let started = std::thread::Builder::new()
                .name("skwir-desktop-read".into())
                .spawn(move || {
                    loop {
                        this.read_again();
                        let mut reading =
                            this.reading.lock().unwrap_or_else(PoisonError::into_inner);
                        if *reading != Some(true) {
                            *reading = None;
                            break;
                        }
                        *reading = Some(false);
                    }
                });
            if let Err(error) = started {
                tracing::debug!("the desktop's style is not read again: {error}");
                *self.reading.lock().unwrap_or_else(PoisonError::into_inner) = None;
            }
        }
    }

    /// Read, tell `changed`, then again at each change of what is read:
    /// a setting the portal says changed on the bus `connect` makes, the
    /// file `own` follows replaced. The bus ended, it is made again once
    /// back (`bus::keep`), and what changed meanwhile told. Ends with
    /// `stop`, or at once with neither a bus nor a file followed. Never
    /// wakes on a timer but to connect again while the bus is gone.
    pub(super) fn follow_on<E: std::fmt::Display>(
        connect: impl FnMut() -> Result<Connection, E>,
        own: Arc<dyn Own>,
        changed: impl FnMut(DesktopStyle) + Send + 'static,
        stop: &CancelToken,
    ) {
        let following = Arc::new(Following {
            bus: Mutex::new(None),
            own,
            said: Mutex::new(None),
            changed: Mutex::new(Box::new(changed)),
            reading: Mutex::new(None),
        });
        let files = follow_files(&following, &following.own.followed(), stop);
        let mut read = false;
        crate::bus::keep(connect, stop, |bus| {
            *following.bus.lock().unwrap_or_else(PoisonError::into_inner) = Some(bus.clone());
            // Read once listened to: at first, then, the bus back, what
            // changed while it was gone. Not listened to, read all the same.
            let mut listened = false;
            listen(bus, &following, stop, || {
                listened = true;
                following.read_soon();
            });
            if !listened {
                following.read_soon();
            }
            read = true;
        });
        if !read {
            following.read_soon();
        }
        if files.is_some() {
            // No bus, the files are followed still.
            let _ = stop.subscribe().receiver().recv();
        }
    }

    /// Follow `files` being written or replaced, each in its folder
    /// (access dropped, as `fs::Settled` does), reading again once their
    /// writes settle. A folder not there is not followed.
    fn follow_files(
        following: &Arc<Following>,
        files: &[PathBuf],
        stop: &CancelToken,
    ) -> Option<skwir_core::fs::Settled> {
        let wanted: Vec<(PathBuf, std::ffi::OsString)> = files
            .iter()
            .filter_map(|file| Some((file.parent()?.to_path_buf(), file.file_name()?.to_owned())))
            .collect();
        if wanted.is_empty() {
            return None;
        }
        let weak = Arc::downgrade(following);
        let stop = stop.clone();
        let relevant = wanted.clone();
        let mut settled = skwir_core::fs::Settled::start(
            "skwir-desktop-files",
            SETTLE,
            move |path| {
                relevant.iter().any(|(folder, name)| {
                    path.file_name() == Some(name.as_os_str())
                        && path.parent() == Some(folder.as_path())
                })
            },
            move || {
                let Some(following) = weak.upgrade().filter(|_| !stop.is_cancelled()) else {
                    return false;
                };
                following.read_soon();
                true
            },
        )?;
        let mut folders: Vec<&PathBuf> = wanted.iter().map(|(folder, _)| folder).collect();
        folders.sort();
        folders.dedup();
        let mut followed = false;
        for folder in folders {
            if settled.watch(folder) {
                followed = true;
            } else {
                tracing::debug!("{} is read, not followed", folder.display());
            }
        }
        followed.then_some(settled)
    }

    /// Read again at each change the portal on `bus` says of what is read,
    /// until the bus goes or `stop`: `ready` once listened to.
    fn listen(
        bus: &Connection,
        following: &Arc<Following>,
        stop: &CancelToken,
        ready: impl FnOnce(),
    ) {
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(SETTINGS)
            .and_then(|rule| rule.member("SettingChanged"))
            .map(zbus::match_rule::Builder::build);
        let rule = match rule {
            Ok(rule) => rule,
            Err(error) => {
                tracing::debug!("the desktop's style is read, not followed: {error}");
                return;
            }
        };
        crate::bus::listen(bus, vec![rule], stop, ready, |message| {
            let Ok((namespace, key, _)) =
                message.body().deserialize::<(String, String, OwnedValue)>()
            else {
                return;
            };
            let read = match key.as_str() {
                "accent-color" | "contrast" => namespace == APPEARANCE,
                key if FONT_KEYS.contains(&key) || key == "gtk-theme" => {
                    namespace.ends_with(".interface")
                }
                _ => false,
            };
            if read {
                following.read_soon();
            }
        });
    }

    /// A setting the portal on `bus` answers: `ReadOne`, or `Read` for a
    /// portal before its second version, which wraps the value once more.
    fn portal_read(bus: &Connection, namespace: &str, key: &str) -> Option<OwnedValue> {
        let call = |method: &str| {
            bus.call_method(
                Some(DESTINATION),
                PATH,
                Some(SETTINGS),
                method,
                &(namespace, key),
            )
            .ok()
        };
        if let Some(reply) = call("ReadOne") {
            return reply.body().deserialize::<OwnedValue>().ok();
        }
        let value = call("Read")?.body().deserialize::<OwnedValue>().ok()?;
        match &*value {
            Value::Value(inner) => inner.try_to_owned().ok(),
            _ => Some(value),
        }
    }

    /// An accent as the portal says it: three channels from 0 to 1, any
    /// outside them meaning none chosen.
    fn accent(value: OwnedValue) -> Option<[f32; 3]> {
        let (r, g, b) = <(f64, f64, f64)>::try_from(value).ok()?;
        let channels = [r, g, b];
        channels
            .iter()
            .all(|c| (0. ..=1.).contains(c))
            .then(|| channels.map(|c| c as f32))
    }

    /// The fonts the desktop's own settings name, with their sizes.
    fn own_fonts() -> Named {
        let pango = |described: &Option<String>| described.as_deref().and_then(super::pango_family);
        let size = |described: &Option<String>| described.as_deref().and_then(super::pango_size);
        match desktop::chosen() {
            // GNOME's and MATE's schemas name the three; Cinnamon's its
            // interface font alone, the others kept in GNOME's.
            Some(Chosen::GSettings(schema)) => {
                let other = |key| {
                    desktop::gsettings(schema, key)
                        .filter(|named| super::pango_family(named).is_some())
                        .or_else(|| {
                            (schema != INTERFACE)
                                .then(|| desktop::gsettings(INTERFACE, key))
                                .flatten()
                        })
                };
                let interface = desktop::gsettings(schema, "font-name");
                let monospace = other("monospace-font-name");
                Named {
                    interface: pango(&interface),
                    interface_px: size(&interface),
                    document: pango(&other("document-font-name")),
                    monospace: pango(&monospace),
                    monospace_px: size(&monospace),
                    from: Some(schema_name(schema)),
                }
            }
            Some(Chosen::Kde) => {
                let kdeglobals = desktop::kdeglobals();
                qt_named(
                    kdeglobals.as_deref(),
                    "General",
                    ("font", "fixed"),
                    "Plasma",
                )
            }
            Some(Chosen::Lxqt) => {
                let conf = desktop::lxqt_conf();
                qt_named(conf.as_deref(), "Qt", ("font", "fixed"), "LXQt")
            }
            Some(Chosen::Xfce) => {
                let interface = desktop::xfconf("/Gtk/FontName");
                let monospace = desktop::xfconf("/Gtk/MonospaceFontName");
                Named {
                    interface: pango(&interface),
                    interface_px: size(&interface),
                    monospace: pango(&monospace),
                    monospace_px: size(&monospace),
                    from: Some("Xfce"),
                    ..Named::default()
                }
            }
            None => Named::default(),
        }
    }

    /// The fonts a Qt settings file names (`text`), its interface font and
    /// its fixed one under `keys` in `group`, as Qt writes them.
    pub(super) fn qt_named(
        text: Option<&str>,
        group: &str,
        (interface, fixed): (&str, &str),
        from: &'static str,
    ) -> Named {
        let value = |key| text.and_then(|text| desktop::keyfile_value(text, group, key));
        let (interface, fixed) = (value(interface), value(fixed));
        Named {
            interface: interface.as_deref().and_then(super::qt_family),
            interface_px: interface.as_deref().and_then(super::qt_size),
            monospace: fixed.as_deref().and_then(super::qt_family),
            monospace_px: fixed.as_deref().and_then(super::qt_size),
            from: Some(from),
            ..Named::default()
        }
    }

    /// The fonts Qt's applications are given: Plasma's and LXQt's own on
    /// those desktops, elsewhere qt6ct's or qt5ct's, as
    /// `QT_QPA_PLATFORMTHEME` names it.
    /// The colors the desktop's own settings name (`Own::own_colors`), a
    /// GTK3 theme read by `theme`, `GTK_THEME` before the settings as GTK
    /// takes it.
    fn own_colors(theme: &dyn Fn(&str) -> Schemes) -> Painted {
        let gtk = |named: Option<String>, from: &'static str| Painted {
            schemes: desktop::forced_gtk_theme()
                .or(named)
                .map(|name| theme(&name))
                .unwrap_or_default(),
            from: Some(from),
        };
        match desktop::chosen() {
            Some(Chosen::Kde) => Painted::one(crate::kde_scheme::read(), "Plasma"),
            Some(Chosen::Lxqt) => Painted::one(
                desktop::lxqt_conf().and_then(|conf| crate::qt_palette::lxqt(&conf)),
                "LXQt",
            ),
            Some(Chosen::GSettings(schema)) => gtk(desktop::own_gtk_theme(), schema_name(schema)),
            Some(Chosen::Xfce) => gtk(desktop::own_gtk_theme(), "Xfce"),
            None => Painted::default(),
        }
    }

    /// The desktop a `GSettings` schema of interface settings is.
    fn schema_name(schema: &str) -> &'static str {
        match schema {
            "org.cinnamon.desktop.interface" => "Cinnamon",
            "org.mate.interface" => "MATE",
            _ => "GNOME",
        }
    }

    fn qt_fonts() -> Named {
        match desktop::chosen() {
            Some(Chosen::Kde | Chosen::Lxqt) => own_fonts(),
            _ => match desktop::qt_ct() {
                Some(("qt6ct", text)) => {
                    qt_named(Some(&text), "Fonts", ("general", "fixed"), "Qt (qt6ct)")
                }
                Some((_, text)) => {
                    qt_named(Some(&text), "Fonts", ("general", "fixed"), "Qt (qt5ct)")
                }
                None => Named::default(),
            },
        }
    }
}

// macOS: its interface font, at the size its controls are written in,
// and the colors AppKit draws its own windows in, each look's as its
// appearance resolves them, Increase contrast's when it is on (ADR-071).
// AppKit tells only its own windows of a change: the window reads it
// again as its look changes or it is activated (`FOLLOWED`).
// The system's calls are Objective-C messages and extern statics: each
// `unsafe` block says why it is sound, and nothing unsafe leaves the module.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod imp {
    use std::cell::RefCell;

    use block2::StackBlock;
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::{
        NSAppearance, NSAppearanceName, NSAppearanceNameAccessibilityHighContrastAqua,
        NSAppearanceNameAccessibilityHighContrastDarkAqua, NSAppearanceNameAqua,
        NSAppearanceNameDarkAqua, NSColor, NSColorSpace, NSWorkspace,
    };
    use skwir_core::CancelToken;

    use super::{DesktopStyle, Scheme, Schemes, SystemColors};

    /// The family `NSFont.systemFont(ofSize:)` names: San Francisco, kept
    /// apart from the families listed (a leading `.`), which `CoreText`
    /// finds by this name.
    const SYSTEM_FONT: &str = ".AppleSystemUIFont";
    /// `NSFont.systemFontSize`, in points, which are pixels at 1×.
    const SYSTEM_FONT_SIZE: f32 = 13.;

    pub(super) fn read() -> DesktopStyle {
        autoreleasepool(|_| {
            let more_contrast =
                NSWorkspace::sharedWorkspace().accessibilityDisplayShouldIncreaseContrast();
            // SAFETY: names AppKit defines, read as the statics they are.
            let (light, dark) = unsafe {
                if more_contrast {
                    (
                        NSAppearanceNameAccessibilityHighContrastAqua,
                        NSAppearanceNameAccessibilityHighContrastDarkAqua,
                    )
                } else {
                    (NSAppearanceNameAqua, NSAppearanceNameDarkAqua)
                }
            };
            let schemes = Schemes {
                light: scheme(light),
                dark: scheme(dark),
            };
            DesktopStyle {
                // Each look's scheme carries the accent as that look
                // resolves it: none is said apart to replace it.
                accent: None,
                more_contrast,
                colors_from: (!schemes.is_empty()).then(|| "macOS".to_owned()),
                schemes,
                font: Some(SYSTEM_FONT.to_owned()),
                font_px: Some(SYSTEM_FONT_SIZE),
                fonts_from: Some("macOS".to_owned()),
                ..DesktopStyle::default()
            }
        })
    }

    /// The scheme of the appearance `name` names: its colors resolved
    /// while it is the thread's drawing appearance.
    fn scheme(name: &NSAppearanceName) -> Option<Scheme> {
        let appearance = NSAppearance::appearanceNamed(name)?;
        let read = RefCell::new(None);
        let block = StackBlock::new(|| {
            *read.borrow_mut() = colors();
        });
        appearance.performAsCurrentDrawingAppearance(&block);
        read.into_inner()
            .map(|colors| super::system_scheme(&colors))
    }

    fn colors() -> Option<SystemColors> {
        let alternating = NSColor::alternatingContentBackgroundColors();
        Some(SystemColors {
            ground: rgba(&NSColor::controlBackgroundColor())?,
            chrome: rgba(&NSColor::windowBackgroundColor())?,
            text: rgba(&NSColor::labelColor())?,
            secondary_text: rgba(&NSColor::secondaryLabelColor())?,
            separator: rgba(&NSColor::separatorColor())?,
            alternate_row: alternating.iter().nth(1).and_then(|color| rgba(&color)),
            accent: rgba(&NSColor::controlAccentColor())?,
            selection: rgba(&NSColor::selectedContentBackgroundColor())?,
            selection_text: rgba(&NSColor::alternateSelectedControlTextColor())?,
            red: rgba(&NSColor::systemRedColor())?,
            orange: rgba(&NSColor::systemOrangeColor())?,
            green: rgba(&NSColor::systemGreenColor())?,
        })
    }

    /// `color` in sRGB, with its opacity, as the drawing appearance
    /// resolves it.
    fn rgba(color: &NSColor) -> Option<[f32; 4]> {
        let srgb = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
        let (mut r, mut g, mut b, mut a) = (0., 0., 0., 0.);
        // SAFETY: four numbers this owns, which the call writes.
        unsafe { srgb.getRed_green_blue_alpha(&raw mut r, &raw mut g, &raw mut b, &raw mut a) };
        Some([r, g, b, a].map(|c| c as f32))
    }

    pub(super) fn follow(mut changed: impl FnMut(DesktopStyle) + Send + 'static, _: CancelToken) {
        // Read on a thread of its own: AppKit's colors are asked off the
        // window's thread, as every reading is.
        let thread = std::thread::Builder::new()
            .name("skwir-desktop-style".into())
            .spawn(move || changed(read()));
        if let Err(error) = thread {
            tracing::debug!("the system's style is not read: {error}");
        }
    }
}

// Windows: the family it writes messages in and the Text size chosen in
// its settings (`windows::style`), the accent, and a contrast theme,
// followed in the registry; the size itself is WinUI's (ADR-117),
// Windows naming none. A contrast theme is a palette the user chose: its
// colors are a scheme, under the stricter guard, and keep their own
// accent (ADR-071, points 4 and 6).
#[cfg(windows)]
mod imp {
    use skwir_core::CancelToken;

    use super::{DesktopStyle, Schemes};

    pub(super) fn read() -> DesktopStyle {
        use crate::windows::style;
        let contrast = style::high_contrast();
        let mut schemes = Schemes::default();
        if contrast {
            schemes.add(super::contrast_scheme(style::contrast_colors()));
        }
        DesktopStyle {
            accent: if contrast { None } else { style::accent() },
            more_contrast: contrast,
            colors_from: contrast.then(|| "Windows".to_owned()),
            schemes,
            font: style::message_font(),
            fonts_from: Some("Windows".to_owned()),
            text_scaling: style::text_scale(),
            ..DesktopStyle::default()
        }
    }

    pub(super) fn follow(
        mut changed: impl FnMut(DesktopStyle) + Send + 'static,
        stop: CancelToken,
    ) {
        let thread = std::thread::Builder::new()
            .name("skwir-desktop-style".into())
            .spawn(move || {
                let mut said = read();
                changed(said.clone());
                crate::windows::style::follow(&stop, || {
                    let now = read();
                    if now != said {
                        said = now.clone();
                        changed(now);
                    }
                });
            });
        if let Err(error) = thread {
            tracing::debug!("the desktop's style is not followed: {error}");
        }
    }
}

// The other systems say nothing: skwir's own colors and fonts.
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod imp {
    use skwir_core::CancelToken;

    use super::DesktopStyle;

    pub(super) fn read() -> DesktopStyle {
        DesktopStyle::default()
    }

    pub(super) fn follow(mut changed: impl FnMut(DesktopStyle) + Send + 'static, _: CancelToken) {
        changed(DesktopStyle::default());
    }
}

#[cfg(test)]
mod tests {
    // Colors are written as their hex codes are, `0xrrggbb`.
    #![allow(clippy::unreadable_literal)]

    use super::*;

    fn rgba(hex: u32, alpha: f32) -> [f32; 4] {
        let [r, g, b] = [16, 8, 0].map(|shift| f32::from(((hex >> shift) & 0xff) as u8) / 255.);
        [r, g, b, alpha]
    }

    #[test]
    fn macos_s_colors_are_a_scheme_its_labels_laid_on_its_ground() {
        // AppKit's own, as its two appearances resolve them.
        let light = system_scheme(&SystemColors {
            ground: rgba(0xffffff, 1.),
            chrome: rgba(0xececec, 1.),
            text: rgba(0x000000, 0.85),
            secondary_text: rgba(0x000000, 0.5),
            separator: rgba(0x000000, 0.1),
            alternate_row: Some(rgba(0xf4f5f5, 1.)),
            accent: rgba(0x007aff, 1.),
            selection: rgba(0x0064e1, 1.),
            selection_text: rgba(0xffffff, 1.),
            red: rgba(0xff3b30, 1.),
            orange: rgba(0xff9500, 1.),
            green: rgba(0x28cd41, 1.),
        });
        assert!(!light.is_dark());
        assert_eq!(light.bg, [1., 1., 1.]);
        assert!(
            (light.text[0] - 0.15).abs() < 1e-3,
            "black at 85 % on white"
        );
        assert!((light.text_muted.unwrap()[0] - 0.5).abs() < 1e-3);
        assert!(light.zebra.is_some() && light.selection.is_some());
        let dark = system_scheme(&SystemColors {
            ground: rgba(0x1e1e1e, 1.),
            chrome: rgba(0x323232, 1.),
            text: rgba(0xffffff, 0.85),
            secondary_text: rgba(0xffffff, 0.55),
            separator: rgba(0xffffff, 0.1),
            alternate_row: None,
            accent: rgba(0x0a84ff, 1.),
            selection: rgba(0x0058d0, 1.),
            selection_text: rgba(0xffffff, 1.),
            red: rgba(0xff453a, 1.),
            orange: rgba(0xff9f0a, 1.),
            green: rgba(0x32d74b, 1.),
        });
        assert!(dark.is_dark());
        assert_eq!(dark.zebra, None, "derived where AppKit says none");
        let mut schemes = Schemes::default();
        schemes.add(light);
        schemes.add(dark);
        assert!(schemes.light.is_some() && schemes.dark.is_some());
    }

    #[test]
    fn a_windows_contrast_theme_is_a_scheme_of_its_system_colors() {
        // Windows 11's Aquatic and Desert, as `GetSysColor` says them
        // (0x00BBGGRR): window, its text, a button's face, gray text,
        // highlight, its text.
        let aquatic = contrast_scheme([
            0x00202020, 0x00ffffff, 0x00202020, 0x00a6a6a6, 0x00f0e38e, 0x00503b26,
        ]);
        assert!(aquatic.is_dark());
        let close = |a: Rgb, hex: u32| {
            let [r, g, b, _] = rgba(hex, 1.);
            a.iter().zip([r, g, b]).all(|(a, b)| (a - b).abs() < 1e-3)
        };
        assert!(close(aquatic.accent.unwrap(), 0x8ee3f0), "BGR read as RGB");
        assert!(close(aquatic.selection.unwrap().1, 0x263b50));
        assert_eq!(aquatic.error, None, "outcomes derived");
        let desert = contrast_scheme([
            0x00effaff, 0x003d3d3d, 0x00effaff, 0x006d6d6d, 0x00093990, 0x00e3f5ff,
        ]);
        assert!(!desert.is_dark());
        assert!(close(desert.bg, 0xfffaef));
    }

    #[cfg(windows)]
    #[test]
    fn windows_says_its_contrast_theme_or_its_accent() {
        // Under Wine no contrast theme is on, and its accent may be none.
        let style = read();
        assert_eq!(style.schemes.is_empty(), !style.more_contrast);
        assert_eq!(style.colors_from.is_some(), style.more_contrast);
        if style.more_contrast {
            assert_eq!(style.accent, None, "a contrast theme keeps its own");
        }
        assert!(
            crate::windows::style::contrast_colors()
                .iter()
                .any(|color| *color != 0)
        );
    }

    #[test]
    fn a_font_s_size_is_read_in_points_or_pixels() {
        let close = |a: Option<f32>, b: f32| a.is_some_and(|a| (a - b).abs() < 1e-3);
        assert!(close(pango_size("Noto Sans  10"), 13.333));
        assert!(close(pango_size("Cantarell 11"), 14.667));
        assert!(close(pango_size("Sans 12px"), 12.));
        assert_eq!(pango_size("Noto Sans"), None);
        assert_eq!(pango_size("Sans 0"), None);
        assert!(close(qt_size("Noto Sans,10,-1,5,400,0,0,0,0,0"), 13.333));
        assert!(close(
            qt_size("\"Sans Serif,10,-1,5,400,0,0,0,0,0,0,0,0,0,0,1\""),
            13.333
        ));
        assert!(close(qt_size("Hack,-1,15,5,50,0,0,0,0,0"), 15.));
        assert_eq!(
            qt_family("\"Sans Serif,10,-1\"").as_deref(),
            Some("Sans Serif")
        );
        assert_eq!(qt_size("Noto Sans"), None);
    }

    #[test]
    fn a_pango_font_s_family_is_its_name_without_size_or_style() {
        let family = |description| pango_family(description);
        assert_eq!(family("Noto Sans  10").as_deref(), Some("Noto Sans"));
        assert_eq!(family("Cantarell 11").as_deref(), Some("Cantarell"));
        assert_eq!(family("Noto Sans Bold 10").as_deref(), Some("Noto Sans"));
        assert_eq!(family("Sans Bold Italic 12").as_deref(), Some("Sans"));
        assert_eq!(
            family("DejaVu Sans Book 10").as_deref(),
            Some("DejaVu Sans")
        );
        assert_eq!(
            family("Cantarell, Noto Sans 11").as_deref(),
            Some("Cantarell")
        );
        assert_eq!(
            family("Inter Variable 11").as_deref(),
            Some("Inter Variable")
        );
        assert_eq!(family("Monospace 13px").as_deref(), Some("Monospace"));
        assert_eq!(family("Adwaita Sans").as_deref(), Some("Adwaita Sans"));
        assert_eq!(family("12"), None);
        assert_eq!(family(""), None);
    }

    #[test]
    fn a_qt_font_s_family_is_what_comes_before_its_first_comma() {
        assert_eq!(
            qt_family("Noto Sans,10,-1,5,400,0,0,0,0,0").as_deref(),
            Some("Noto Sans")
        );
        assert_eq!(qt_family("Hack"), Some("Hack".to_owned()));
        assert_eq!(qt_family(",10"), None);
        // An old qt5ct's binary form names nothing, its size neither.
        let variant = r"@Variant(\0\0\0@\0\0\0\x12\0N\0o\0t\0o\0 \0S\0\x61\0n\0s@$\0\0\0\0\0\0\xff\xff\xff\xff\x5\x1\0\x32\x10)";
        assert_eq!(qt_family(variant), None);
        assert_eq!(qt_size(variant), None);
        assert_eq!(qt_family(&format!("\"{variant}\"")), None);
    }

    #[cfg(target_os = "linux")]
    mod portal {
        use std::collections::HashMap;
        use std::path::{Path, PathBuf};
        use std::sync::{Arc, Mutex, mpsc};
        use std::time::Duration;

        use skwir_core::CancelToken;
        use zbus::blocking::Connection;
        use zbus::object_server::SignalEmitter;
        use zbus::zvariant::{OwnedValue, Value};

        use super::super::{DesktopStyle, Schemes, imp};

        type Values = Arc<Mutex<HashMap<(String, String), OwnedValue>>>;

        /// A settings portal that answers what `values` holds.
        struct Portal {
            values: Values,
        }

        #[zbus::interface(name = "org.freedesktop.portal.Settings")]
        impl Portal {
            fn read_one(&self, namespace: String, key: String) -> zbus::fdo::Result<OwnedValue> {
                let values = self.values.lock().unwrap();
                let value = values.get(&(namespace, key));
                let value =
                    value.ok_or_else(|| zbus::fdo::Error::Failed("no such setting".into()))?;
                value
                    .try_clone()
                    .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
            }

            #[zbus(signal)]
            async fn setting_changed(
                emitter: &SignalEmitter<'_>,
                namespace: &str,
                key: &str,
                value: Value<'_>,
            ) -> zbus::Result<()>;
        }

        /// A connection to a portal of the test's own, and the portal's
        /// end, over a pair of sockets: no bus, never the session's.
        /// A portal before its second version: `Read` alone, the value in
        /// a variant of its own.
        struct OldPortal {
            values: Values,
        }

        #[zbus::interface(name = "org.freedesktop.portal.Settings")]
        impl OldPortal {
            fn read(&self, namespace: String, key: String) -> zbus::fdo::Result<OwnedValue> {
                let values = self.values.lock().unwrap();
                let value = values.get(&(namespace, key));
                let value =
                    value.ok_or_else(|| zbus::fdo::Error::Failed("no such setting".into()))?;
                let failed = |e: zbus::zvariant::Error| zbus::fdo::Error::Failed(e.to_string());
                let inner = Value::from(value.try_clone().map_err(failed)?);
                OwnedValue::try_from(Value::Value(Box::new(inner))).map_err(failed)
            }
        }

        fn portal(values: &Values) -> (Connection, Connection) {
            serve(Portal {
                values: Arc::clone(values),
            })
        }

        fn serve(answering: impl zbus::object_server::Interface) -> (Connection, Connection) {
            let (theirs, ours) = std::os::unix::net::UnixStream::pair().unwrap();
            let server = std::thread::spawn(move || {
                zbus::blocking::connection::Builder::async_io_unix_stream(theirs)
                    .server(zbus::Guid::generate())
                    .unwrap()
                    .p2p()
                    .serve_at("/org/freedesktop/portal/desktop", answering)
                    .unwrap()
                    .build()
                    .unwrap()
            });
            let client = zbus::blocking::connection::Builder::async_io_unix_stream(ours)
                .p2p()
                .build()
                .unwrap();
            (client, server.join().unwrap())
        }

        /// The connections `follow_on` is given, one each time it asks,
        /// then none: the bus not back.
        fn handed(
            mut buses: Vec<Connection>,
        ) -> impl FnMut() -> Result<Connection, &'static str> + Send + 'static {
            buses.reverse();
            move || buses.pop().ok_or("no bus")
        }

        fn set(values: &Values, namespace: &str, key: &str, value: &Value<'_>) {
            values.lock().unwrap().insert(
                (namespace.to_owned(), key.to_owned()),
                value.try_to_owned().unwrap(),
            );
        }

        fn say(server: &Connection, namespace: &str, key: &str, value: Value<'_>) {
            let emitter =
                SignalEmitter::new(server.inner(), "/org/freedesktop/portal/desktop").unwrap();
            zbus::block_on(Portal::setting_changed(&emitter, namespace, key, value)).unwrap();
        }

        const APPEARANCE: &str = "org.freedesktop.appearance";
        const INTERFACE: &str = "org.gnome.desktop.interface";

        /// A desktop of the test's own: the fonts its settings name, its
        /// `kdeglobals` when it is Plasma (followed), else the GTK3 themes
        /// in a folder of the test's own, named by the portal.
        struct Fixed {
            fonts: fn() -> imp::Named,
            kdeglobals: Option<PathBuf>,
            themes: Vec<PathBuf>,
        }

        impl Fixed {
            fn fonts(fonts: fn() -> imp::Named) -> Arc<Self> {
                Arc::new(Fixed {
                    fonts,
                    kdeglobals: None,
                    themes: Vec::new(),
                })
            }
        }

        impl imp::Own for Fixed {
            fn fonts(&self) -> imp::Named {
                (self.fonts)()
            }

            fn own_colors(&self, _: &dyn Fn(&str) -> Schemes) -> imp::Painted {
                match &self.kdeglobals {
                    Some(file) => {
                        let text = std::fs::read_to_string(file).unwrap_or_default();
                        imp::Painted::one(crate::kde_scheme::scheme_of(&[text], &[]), "Plasma")
                    }
                    None => imp::Painted::default(),
                }
            }

            fn forced_gtk_theme(&self) -> Option<String> {
                None
            }

            fn theme(&self, name: &str) -> Schemes {
                crate::gtk_theme::schemes(name, &self.themes)
            }

            fn followed(&self) -> Vec<PathBuf> {
                self.kdeglobals.clone().into_iter().collect()
            }
        }

        fn nothing_named() -> imp::Named {
            imp::Named::default()
        }

        #[test]
        fn what_the_portal_answers_is_the_desktop_s_style() {
            let values = Values::default();
            set(
                &values,
                APPEARANCE,
                "accent-color",
                &Value::from((0.2_f64, 0.4, 0.6)),
            );
            set(&values, APPEARANCE, "contrast", &Value::from(1_u32));
            for (key, font) in [
                ("font-name", "Noto Sans 10"),
                ("document-font-name", "Noto Serif 11"),
                ("monospace-font-name", "Hack Bold 10"),
            ] {
                set(&values, INTERFACE, key, &Value::from(font));
            }
            let (bus, _server) = portal(&values);
            let style = imp::read_from(Some(&bus), &*Fixed::fonts(nothing_named));
            assert_eq!(
                style,
                DesktopStyle {
                    accent: Some([0.2, 0.4, 0.6]),
                    more_contrast: true,
                    font: Some("Noto Sans".into()),
                    document_font: Some("Noto Serif".into()),
                    monospace_font: Some("Hack".into()),
                    schemes: Schemes::default(),
                    font_px: Some(10. * 96. / 72.),
                    monospace_px: Some(10. * 96. / 72.),
                    fonts_from: Some("the portal".into()),
                    gtk: Some(super::super::Fonts {
                        from: "the portal".into(),
                        interface: Some("Noto Sans".into()),
                        document: Some("Noto Serif".into()),
                        monospace: Some("Hack".into()),
                        interface_px: Some(10. * 96. / 72.),
                        monospace_px: Some(10. * 96. / 72.),
                    }),
                    qt: None,
                    colors_from: None,
                    gtk_colors: None,
                    qt_colors: None,
                    text_scaling: None,
                }
            );
            // The desktop's own settings name a font before the portal.
            let own = imp::read_from(
                Some(&bus),
                &*Fixed::fonts(|| imp::Named {
                    interface: Some("Cantarell".into()),
                    monospace: Some("Source Code Pro".into()),
                    ..imp::Named::default()
                }),
            );
            assert_eq!(own.font.as_deref(), Some("Cantarell"));
            assert_eq!(own.document_font.as_deref(), Some("Noto Serif"));
            assert_eq!(own.monospace_font.as_deref(), Some("Source Code Pro"));
        }

        /// A desktop whose only fonts are Qt's (qt6ct's file, as on a
        /// compositor with no settings of its own).
        struct QtOnly;

        impl imp::Own for QtOnly {
            fn fonts(&self) -> imp::Named {
                imp::Named::default()
            }

            fn qt_fonts(&self) -> imp::Named {
                imp::qt_named(
                    Some(
                        "[Appearance]\nstyle=Fusion\n\n[Fonts]\n\
                         fixed=\"Hack,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1\"\n\
                         general=\"Fira Sans,12,-1,5,400,0,0,0,0,0,0,0,0,0,0,1\"\n",
                    ),
                    "Fonts",
                    ("general", "fixed"),
                    "Qt (qt6ct)",
                )
            }

            fn own_colors(&self, _: &dyn Fn(&str) -> Schemes) -> imp::Painted {
                imp::Painted::default()
            }

            fn followed(&self) -> Vec<PathBuf> {
                Vec::new()
            }
        }

        /// GTK's `settings.ini` naming one font, on Wayland or X11.
        struct IniOnly(bool);

        impl imp::Own for IniOnly {
            fn fonts(&self) -> imp::Named {
                imp::Named::default()
            }

            fn gtk_fonts(&self) -> imp::Named {
                imp::Named {
                    interface: Some("Noto Sans".into()),
                    interface_px: Some(10. * 96. / 72.),
                    from: Some("GTK"),
                    ..imp::Named::default()
                }
            }

            fn wayland(&self) -> bool {
                self.0
            }

            fn own_colors(&self, _: &dyn Fn(&str) -> Schemes) -> imp::Painted {
                imp::Painted::default()
            }

            fn followed(&self) -> Vec<PathBuf> {
                Vec::new()
            }
        }

        /// GTK's `settings.ini` naming one theme and qt6ct a scheme; the
        /// desktop's own settings none, or Plasma's.
        struct Painters {
            themes: PathBuf,
            wayland: bool,
            plasma: bool,
        }

        impl imp::Own for Painters {
            fn fonts(&self) -> imp::Named {
                imp::Named::default()
            }

            fn own_colors(&self, _: &dyn Fn(&str) -> Schemes) -> imp::Painted {
                if self.plasma {
                    let breeze = "[Colors:View]\nBackgroundNormal=255,255,255\n\
                        ForegroundNormal=35,38,41\n[Colors:Window]\nBackgroundNormal=239,240,241\n";
                    imp::Painted::one(
                        crate::kde_scheme::scheme_of(&[breeze.into()], &[]),
                        "Plasma",
                    )
                } else {
                    imp::Painted::default()
                }
            }

            fn gtk_theme(&self) -> Option<String> {
                Some("Ini".into())
            }

            fn forced_gtk_theme(&self) -> Option<String> {
                None
            }

            fn wayland(&self) -> bool {
                self.wayland
            }

            fn qt_colors(&self) -> imp::Painted {
                imp::Painted::one(
                    crate::qt_palette::palette(
                        "#ff000000, #ffefefef, #ffffffff, #ffcacaca, #ff9f9f9f, #ffb8b8b8, \
                         #ffffffff, #ffffffff, #ff000000, #ff202020, #ff303030",
                    ),
                    "Qt (qt6ct)",
                )
            }

            fn theme(&self, name: &str) -> Schemes {
                crate::gtk_theme::schemes(name, std::slice::from_ref(&self.themes))
            }

            fn followed(&self) -> Vec<PathBuf> {
                Vec::new()
            }
        }

        #[test]
        fn gtk_s_and_qt_s_colors_are_read_apart_after_the_desktop_s() {
            let themes = tempfile::tempdir().unwrap();
            for (name, base) in [("Ini", "#fdf6e3"), ("Portal", "#e0e0e0")] {
                let sheet = themes.path().join(name).join("gtk-3.0/gtk.css");
                std::fs::create_dir_all(sheet.parent().unwrap()).unwrap();
                std::fs::write(
                    &sheet,
                    format!(
                        "@define-color theme_base_color {base};\n\
                         @define-color theme_bg_color #eee8d5;\n\
                         @define-color theme_text_color #586e75;\n"
                    ),
                )
                .unwrap();
            }
            let values = Values::default();
            set(&values, INTERFACE, "gtk-theme", &Value::from("Portal"));
            let (bus, _server) = portal(&values);
            let painters = |wayland, plasma| Painters {
                themes: themes.path().to_path_buf(),
                wayland,
                plasma,
            };
            let bg = |colors: &Option<super::super::Colors>| {
                colors
                    .as_ref()
                    .and_then(|c| c.schemes.light.as_ref())
                    .map(|s| s.bg[0])
            };
            // On X11 GTK takes settings.ini's theme; Automatic is GTK's, the
            // desktop naming none, and Qt's is kept apart.
            let x11 = imp::read_from(Some(&bus), &painters(false, false));
            assert_eq!(x11.colors_from.as_deref(), Some("GTK"));
            assert_eq!(bg(&x11.gtk_colors), Some(253. / 255.));
            assert_eq!(
                x11.schemes.light.as_ref().map(|s| s.bg[0]),
                Some(253. / 255.)
            );
            let qt = x11.qt_colors.as_ref().expect("Qt's colors");
            assert_eq!(qt.from, "Qt (qt6ct)");
            assert!(qt.schemes.dark.is_some());
            // On Wayland the portal's theme first.
            let wayland = imp::read_from(Some(&bus), &painters(true, false));
            assert_eq!(bg(&wayland.gtk_colors), Some(224. / 255.));
            // The desktop's own first: Plasma's, GTK's and Qt's still apart.
            let plasma = imp::read_from(Some(&bus), &painters(false, true));
            assert_eq!(plasma.colors_from.as_deref(), Some("Plasma"));
            assert_eq!(plasma.schemes.light.as_ref().map(|s| s.bg[0]), Some(1.));
            assert!(plasma.gtk_colors.is_some() && plasma.qt_colors.is_some());
        }

        #[test]
        fn gtk_s_fonts_are_read_in_the_order_gtk_reads_them() {
            let values = Values::default();
            set(
                &values,
                INTERFACE,
                "font-name",
                &Value::from("Cantarell 11"),
            );
            let (bus, _server) = portal(&values);
            // On Wayland GTK takes the portal's before settings.ini.
            let wayland = imp::read_from(Some(&bus), &IniOnly(true));
            assert_eq!(wayland.font.as_deref(), Some("Cantarell"));
            assert_eq!(wayland.fonts_from.as_deref(), Some("the portal"));
            // On X11 settings.ini's.
            let x11 = imp::read_from(Some(&bus), &IniOnly(false));
            assert_eq!(x11.font.as_deref(), Some("Noto Sans"));
            assert_eq!(x11.fonts_from.as_deref(), Some("GTK"));
            // Without a portal, settings.ini's either way.
            let alone = imp::read_from(None, &IniOnly(true));
            assert_eq!(alone.font.as_deref(), Some("Noto Sans"));
        }

        #[test]
        fn an_old_qt5ct_s_binary_fonts_name_nothing_and_automatic_goes_on() {
            let conf = r"[Fonts]
fixed=@Variant(\0\0\0@\0\0\0\x10\0H\0\x61\0\x63\0k@$\0\0\0\0\0\0\xff\xff\xff\xff\x5\x1\0\x32\x10)
general=@Variant(\0\0\0@\0\0\0\x12\0N\0o\0t\0o\0 \0S\0\x61\0n\0s@$\0\0\0\0\0\0\xff\xff\xff\xff\x5\x1\0\x32\x10)
";
            // Read as written, it is the binary form that names nothing.
            let general = crate::desktop_settings::keyfile_value(conf, "Fonts", "general");
            assert!(general.is_some_and(|value| value.starts_with("@Variant(")));
            let old = imp::qt_named(Some(conf), "Fonts", ("general", "fixed"), "Qt (qt5ct)");
            assert_eq!((old.interface.as_deref(), old.interface_px), (None, None));
            assert_eq!((old.monospace.as_deref(), old.monospace_px), (None, None));
            // Named nothing, it gives way to what follows.
            let after = imp::Named {
                interface: Some("Noto Sans".into()),
                from: Some("GTK"),
                ..imp::Named::default()
            };
            assert_eq!(old.or(after).from, Some("GTK"));
        }

        #[test]
        fn qt_s_fonts_follow_gtk_s_and_are_said_apart() {
            let close = |a: Option<f32>, b: f32| a.is_some_and(|a| (a - b).abs() < 1e-3);
            // Nothing else names a font: Automatic takes Qt's, with sizes.
            let alone = imp::read_from(None, &QtOnly);
            assert_eq!(alone.font.as_deref(), Some("Fira Sans"));
            assert!(close(alone.font_px, 16.));
            assert_eq!(alone.monospace_font.as_deref(), Some("Hack"));
            assert!(close(alone.monospace_px, 14.667));
            assert_eq!(alone.fonts_from.as_deref(), Some("Qt (qt6ct)"));
            assert_eq!(alone.gtk, None);
            let qt = alone.qt.expect("Qt's fonts, apart");
            assert_eq!(qt.interface.as_deref(), Some("Fira Sans"));
            // The portal names the GTK side's: Automatic takes it, and Qt's
            // stay apart for the user to choose; its scaling is read.
            let values = Values::default();
            set(
                &values,
                INTERFACE,
                "font-name",
                &Value::from("Noto Sans 10"),
            );
            set(
                &values,
                INTERFACE,
                "text-scaling-factor",
                &Value::from(1.25_f64),
            );
            let (bus, _server) = portal(&values);
            let both = imp::read_from(Some(&bus), &QtOnly);
            assert_eq!(both.font.as_deref(), Some("Noto Sans"));
            assert!(close(both.font_px, 13.333));
            assert_eq!(both.fonts_from.as_deref(), Some("the portal"));
            // A role GTK's side leaves out is still Qt's.
            assert_eq!(both.monospace_font.as_deref(), Some("Hack"));
            assert_eq!(
                both.qt.and_then(|qt| qt.interface).as_deref(),
                Some("Fira Sans")
            );
            assert_eq!(both.text_scaling, Some(1.25));
        }

        #[test]
        fn the_gtk_theme_the_portal_names_gives_its_colors() {
            let themes = tempfile::tempdir().unwrap();
            let sheet = themes.path().join("Named/gtk-3.0/gtk.css");
            std::fs::create_dir_all(sheet.parent().unwrap()).unwrap();
            std::fs::write(
                &sheet,
                "@define-color theme_base_color #fdf6e3;\n\
                 @define-color theme_bg_color #eee8d5;\n\
                 @define-color theme_text_color #586e75;\n",
            )
            .unwrap();
            let values = Values::default();
            set(&values, INTERFACE, "gtk-theme", &Value::from("Named"));
            let (bus, _server) = portal(&values);
            let desktop = Arc::new(Fixed {
                fonts: nothing_named,
                kdeglobals: None,
                themes: vec![themes.path().to_path_buf()],
            });
            let style = imp::read_from(Some(&bus), &*desktop);
            let light = style.schemes.light.expect("the theme's colors");
            assert!((light.bg[0] - 253. / 255.).abs() < 1e-6);
            assert_eq!(style.schemes.dark, None);
            // No portal to name it: none.
            assert!(imp::read_from(None, &*desktop).schemes.is_empty());
        }

        #[test]
        fn a_portal_before_read_one_is_read_too() {
            let values = Values::default();
            set(
                &values,
                APPEARANCE,
                "accent-color",
                &Value::from((0.2_f64, 0.4, 0.6)),
            );
            set(&values, APPEARANCE, "contrast", &Value::from(1_u32));
            let (bus, _server) = serve(OldPortal {
                values: Arc::clone(&values),
            });
            let style = imp::read_from(Some(&bus), &*Fixed::fonts(nothing_named));
            assert_eq!(style.accent, Some([0.2, 0.4, 0.6]));
            assert!(style.more_contrast);
        }

        #[test]
        fn an_accent_out_of_range_or_a_setting_not_answered_is_none() {
            let values = Values::default();
            set(
                &values,
                APPEARANCE,
                "accent-color",
                &Value::from((-1_f64, -1., -1.)),
            );
            let (bus, _server) = portal(&values);
            let nothing = Fixed::fonts(nothing_named);
            assert_eq!(
                imp::read_from(Some(&bus), &*nothing),
                DesktopStyle::default()
            );
            // No bus at all: skwir's own.
            assert_eq!(imp::read_from(None, &*nothing), DesktopStyle::default());
        }

        #[test]
        fn a_change_the_portal_says_is_followed_and_only_one_that_changes_something() {
            let values = Values::default();
            set(
                &values,
                APPEARANCE,
                "accent-color",
                &Value::from((0.2_f64, 0.4, 0.6)),
            );
            let (bus, server) = portal(&values);
            let (sent, got) = mpsc::channel();
            let stop = CancelToken::new();
            let following = {
                let stop = stop.clone();
                std::thread::spawn(move || {
                    imp::follow_on(
                        handed(vec![bus]),
                        Fixed::fonts(nothing_named),
                        move |style| sent.send(style).unwrap(),
                        &stop,
                    );
                })
            };
            let next = || got.recv_timeout(Duration::from_secs(10)).unwrap();
            assert_eq!(next().accent, Some([0.2, 0.4, 0.6]));

            // A setting it does not read, and one that says what it said:
            // nothing. Then the accent and the contrast change.
            say(&server, APPEARANCE, "color-scheme", Value::from(1_u32));
            say(
                &server,
                APPEARANCE,
                "accent-color",
                Value::from((0.2_f64, 0.4, 0.6)),
            );
            assert!(got.recv_timeout(Duration::from_millis(300)).is_err());
            set(
                &values,
                APPEARANCE,
                "accent-color",
                &Value::from((0.9_f64, 0.5, 0.1)),
            );
            set(&values, APPEARANCE, "contrast", &Value::from(1_u32));
            say(&server, APPEARANCE, "contrast", Value::from(1_u32));
            let changed = next();
            assert_eq!(changed.accent, Some([0.9, 0.5, 0.1]));
            assert!(changed.more_contrast);
            assert!(got.recv_timeout(Duration::from_millis(300)).is_err());

            // A font the desktop names for code, followed as the rest is.
            set(
                &values,
                INTERFACE,
                "monospace-font-name",
                &Value::from("Hack 10"),
            );
            say(
                &server,
                INTERFACE,
                "monospace-font-name",
                Value::from("Hack 10"),
            );
            assert_eq!(next().monospace_font.as_deref(), Some("Hack"));

            // The portal gone, asked for again until stopped: the thread
            // ends.
            drop(server);
            stop.cancel();
            following.join().unwrap();
        }

        #[test]
        fn the_bus_back_what_changed_while_it_was_gone_is_told_and_followed() {
            let values = Values::default();
            set(
                &values,
                APPEARANCE,
                "accent-color",
                &Value::from((0.2_f64, 0.4, 0.6)),
            );
            let ((bus, server), (back, server_back)) = (portal(&values), portal(&values));
            let (sent, got) = mpsc::channel();
            let stop = CancelToken::new();
            let following = {
                let stop = stop.clone();
                std::thread::spawn(move || {
                    imp::follow_on(
                        handed(vec![bus, back]),
                        Fixed::fonts(nothing_named),
                        move |style| sent.send(style).unwrap(),
                        &stop,
                    );
                })
            };
            let next = || got.recv_timeout(Duration::from_secs(10)).unwrap();
            assert_eq!(next().accent, Some([0.2, 0.4, 0.6]));

            // The bus ends, and the accent changes while it is gone, said
            // to no one: once back, read again and told.
            set(
                &values,
                APPEARANCE,
                "accent-color",
                &Value::from((0.9_f64, 0.5, 0.1)),
            );
            drop(server);
            assert_eq!(next().accent, Some([0.9, 0.5, 0.1]));

            // And followed there.
            set(&values, APPEARANCE, "contrast", &Value::from(1_u32));
            say(&server_back, APPEARANCE, "contrast", Value::from(1_u32));
            assert!(next().more_contrast);
            stop.cancel();
            following.join().unwrap();
        }

        /// Write `text` to `file` as Plasma does: beside it, then over it.
        fn replace(file: &Path, text: &str) {
            let staged = file.with_extension("new");
            std::fs::write(&staged, text).unwrap();
            std::fs::rename(&staged, file).unwrap();
        }

        const PLASMA: &str = "[Colors:View]\nBackgroundNormal=255,255,255\nForegroundNormal=35,38,41\n\
                              [Colors:Window]\nBackgroundNormal=239,240,241\n";

        #[test]
        fn plasma_s_scheme_is_followed_as_kdeglobals_is_replaced() {
            let config = tempfile::tempdir().unwrap();
            let kdeglobals = config.path().join("kdeglobals");
            replace(&kdeglobals, PLASMA);
            let desktop = Arc::new(Fixed {
                fonts: nothing_named,
                kdeglobals: Some(kdeglobals.clone()),
                themes: Vec::new(),
            });
            let (sent, got) = mpsc::channel();
            let stop = CancelToken::new();
            let following = {
                let stop = stop.clone();
                std::thread::spawn(move || {
                    imp::follow_on(
                        handed(Vec::new()),
                        desktop,
                        move |style| sent.send(style).unwrap(),
                        &stop,
                    );
                })
            };
            let next = || got.recv_timeout(Duration::from_secs(10)).unwrap();
            let first = next().schemes;
            assert!(first.light.is_some() && first.dark.is_none());

            // Another file of the folder, and kdeglobals written again the
            // same: nothing. Then a dark scheme.
            std::fs::write(config.path().join("other"), "x").unwrap();
            replace(&kdeglobals, PLASMA);
            assert!(got.recv_timeout(Duration::from_millis(800)).is_err());
            let dark = PLASMA
                .replace("BackgroundNormal=255,255,255", "BackgroundNormal=35,38,41")
                .replace("ForegroundNormal=35,38,41", "ForegroundNormal=252,252,252")
                .replace("BackgroundNormal=239,240,241", "BackgroundNormal=32,35,38");
            replace(&kdeglobals, &dark);
            let changed = next().schemes;
            assert!(changed.light.is_none() && changed.dark.is_some());
            assert!(
                got.recv_timeout(Duration::from_millis(800)).is_err(),
                "once"
            );

            // No bus, the file followed still; stopped, the thread ends.
            stop.cancel();
            following.join().unwrap();
        }

        /// qt6ct's file in a folder of the test's own, its fonts read from
        /// it each time.
        struct Qt6ct(PathBuf);

        impl imp::Own for Qt6ct {
            fn fonts(&self) -> imp::Named {
                imp::Named::default()
            }

            fn qt_fonts(&self) -> imp::Named {
                let text = std::fs::read_to_string(&self.0).ok();
                imp::qt_named(text.as_deref(), "Fonts", ("general", "fixed"), "Qt (qt6ct)")
            }

            fn own_colors(&self, _: &dyn Fn(&str) -> Schemes) -> imp::Painted {
                imp::Painted::default()
            }

            fn followed(&self) -> Vec<PathBuf> {
                vec![self.0.clone()]
            }
        }

        #[test]
        fn qt6ct_s_file_is_followed_as_it_is_written() {
            let config = tempfile::tempdir().unwrap();
            let conf = config.path().join("qt6ct.conf");
            let fonts =
                |size: u32| format!("[Fonts]\ngeneral=\"Fira Sans,{size},-1,5,400,0,0,0,0,0\"\n");
            std::fs::write(&conf, fonts(10)).unwrap();
            let (sent, got) = mpsc::channel();
            let stop = CancelToken::new();
            let following = {
                let stop = stop.clone();
                let desktop = Arc::new(Qt6ct(conf.clone()));
                std::thread::spawn(move || {
                    imp::follow_on(
                        handed(Vec::new()),
                        desktop,
                        move |style| sent.send(style).unwrap(),
                        &stop,
                    );
                })
            };
            let next = || got.recv_timeout(Duration::from_secs(10)).unwrap();
            assert_eq!(next().font_px, Some(10. * 96. / 72.));
            // Written in place, as qt6ct saves it: read again.
            std::fs::write(&conf, fonts(12)).unwrap();
            assert_eq!(next().font_px, Some(16.));
            stop.cancel();
            following.join().unwrap();
        }
    }
}
