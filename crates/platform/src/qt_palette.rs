//! The colors Qt's applications are given outside Plasma (ADR-071,
//! amended 2026-11-05: Colors from; 2026-11-07: the platform themes after
//! qt6ct): the color scheme of the platform theme `QT_QPA_PLATFORMTHEME`
//! names (qt5ct, qt6ct, qtengine, hyprqt6engine), and LXQt's palette.
//! Plasma's own scheme is `kde_scheme`'s.
//!
//! qt5ct and qt6ct name a scheme while their custom palette is on
//! (`[Appearance] custom_palette`, `color_scheme_path`): a file of their
//! own, its `[ColorScheme] active_colors` a list of the palette's colors
//! in Qt's order of roles (`QPalette::ColorRole`), or a KDE `.colors`
//! scheme, which qt6ct reads too. qtengine (`theme.colorScheme` of its
//! JSON) and hyprqt6engine (`theme:color_scheme`) name a KDE scheme
//! alone. LXQt keeps its palette in `lxqt.conf` (`[Palette]
//! window_color`, `base_color`…), Qt making what it leaves out from the
//! window's color (`lxqt-qtplugin`).

use std::path::{Path, PathBuf};

use crate::desktop_settings::{QtSettings, hyprlang_value, keyfile_value};
use crate::desktop_style::{Scheme, over};

/// The roles of a Qt palette, as `active_colors` lists them.
mod role {
    pub const TEXT: usize = 6;
    pub const BASE: usize = 9;
    pub const WINDOW: usize = 10;
    pub const HIGHLIGHT: usize = 12;
    pub const HIGHLIGHTED_TEXT: usize = 13;
    pub const ALTERNATE_BASE: usize = 16;
    pub const PLACEHOLDER_TEXT: usize = 20;
    /// Qt 6.6's, which a scheme written before it leaves out.
    pub const ACCENT: usize = 21;
}

/// The color scheme file a platform theme's settings name, an absolute
/// path: qt5ct's and qt6ct's while their custom palette is on, qtengine's
/// (`~` its user's `home`, as it reads it), hyprqt6engine's.
pub(crate) fn scheme_path(settings: &QtSettings, home: Option<&Path>) -> Option<PathBuf> {
    let text = settings.text.as_str();
    let named = match settings.program {
        "qtengine" => {
            let json: serde_json::Value = serde_json::from_str(text).ok()?;
            let named = json.get("theme")?.get("colorScheme")?.as_str()?;
            match (named.strip_prefix('~'), home) {
                (Some(""), Some(home)) => home.to_path_buf(),
                (Some(rest), Some(home)) if rest.starts_with('/') => home.join(&rest[1..]),
                _ => PathBuf::from(named),
            }
        }
        "hyprqt6engine" => PathBuf::from(hyprlang_value(text, "theme:color_scheme")?),
        _ => {
            let on = keyfile_value(text, "Appearance", "custom_palette")?;
            if !on.eq_ignore_ascii_case("true") {
                return None;
            }
            PathBuf::from(keyfile_value(text, "Appearance", "color_scheme_path")?)
        }
    };
    named.is_absolute().then_some(named)
}

/// The scheme a platform theme's settings name: the file `scheme_path`
/// says, a KDE scheme (one of qt5ct's or qt6ct's own unless it is a
/// `.colors`).
pub(crate) fn of_settings(settings: &QtSettings, home: Option<&Path>) -> Option<Scheme> {
    let path = scheme_path(settings, home)?;
    let text = crate::file::read_regular_text(&path, crate::file::DESKTOP_FILE).ok()?;
    let kde = !matches!(settings.program, "qt5ct" | "qt6ct")
        || path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("colors"));
    if kde {
        // A KDE scheme alone: nothing of Breeze's fills it in.
        crate::kde_scheme::scheme_of(&[text], &[])
    } else {
        palette(&keyfile_value(&text, "ColorScheme", "active_colors")?)
    }
}

/// A palette as qt5ct and qt6ct write it: its colors, `#aarrggbb`, in the
/// order of Qt's roles.
pub(crate) fn palette(active_colors: &str) -> Option<Scheme> {
    let colors: Vec<[f32; 4]> = active_colors
        .split(',')
        .map(|color| color_of(color.trim()))
        .collect::<Option<_>>()?;
    let at = |role: usize| colors.get(role).copied();
    let opaque = |color: [f32; 4]| [color[0], color[1], color[2]];
    let bg = opaque(at(role::BASE)?);
    let on_bg = |role: usize| at(role).map(|color| over(color, bg));
    let highlight = on_bg(role::HIGHLIGHT);
    let selection = highlight.zip(
        at(role::HIGHLIGHTED_TEXT)
            .zip(highlight)
            .map(|(text, ground)| over(text, ground)),
    );
    Some(Scheme {
        bg,
        surface: opaque(at(role::WINDOW)?),
        text: on_bg(role::TEXT)?,
        text_muted: on_bg(role::PLACEHOLDER_TEXT),
        zebra: on_bg(role::ALTERNATE_BASE),
        border: None,
        accent: on_bg(role::ACCENT).or(highlight),
        selection,
        error: None,
        warning: None,
        success: None,
        sidebar: None,
    })
}

/// LXQt's palette, from its `lxqt.conf` (`conf`, its text): what it names
/// once it names what is read on and in (the base and the window's color
/// and a text color), as LXQt's appearance settings write them all.
pub(crate) fn lxqt(conf: &str) -> Option<Scheme> {
    let color = |key: &str| keyfile_value(conf, "Palette", key).and_then(|c| color_of(&c));
    let opaque = |color: [f32; 4]| [color[0], color[1], color[2]];
    let bg = opaque(color("base_color")?);
    let on_bg = |key: &str| color(key).map(|color| over(color, bg));
    let highlight = on_bg("highlight_color");
    let selection = highlight.zip(
        color("highlighted_text_color")
            .zip(highlight)
            .map(|(text, ground)| over(text, ground)),
    );
    Some(Scheme {
        bg,
        surface: opaque(color("window_color")?),
        text: on_bg("text_color").or_else(|| on_bg("window_text_color"))?,
        text_muted: None,
        zebra: None,
        border: None,
        accent: highlight,
        selection,
        error: None,
        warning: None,
        success: None,
        sidebar: None,
    })
}

/// A color as Qt writes it: `#rrggbb`, or `#aarrggbb` with its opacity
/// first.
fn color_of(value: &str) -> Option<[f32; 4]> {
    let hex = value.trim().strip_prefix('#')?;
    let channel = |i: usize| {
        u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16)
            .ok()
            .map(|v| f32::from(v) / 255.)
    };
    match hex.len() {
        6 => Some([channel(0)?, channel(1)?, channel(2)?, 1.]),
        8 => Some([channel(1)?, channel(2)?, channel(3)?, channel(0)?]),
        _ => None,
    }
}

#[cfg(test)]
// The channels compared are the same numbers divided alike: equal exactly.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::desktop_style::Rgb;

    fn rgb(r: u8, g: u8, b: u8) -> Rgb {
        [r, g, b].map(|c| f32::from(c) / 255.)
    }

    /// qt6ct's own "darker" scheme, as it ships it.
    const DARKER: &str = "#ffffffff, #ff424245, #ff979797, #ff5e5c5b, #ff302f2e, #ff4a4947, \
        #ffffffff, #ffffffff, #ffffffff, #ff3d3d3d, #ff222020, #ffe7e4e0, #ff12608a, #fff9f9f9, \
        #ff0986d3, #ffa70b06, #ff5c5b5a, #ffffffff, #ff3f3f36, #ffffffff, #80ffffff";

    #[test]
    fn a_qt_ct_palette_is_read_by_its_roles() {
        let scheme = palette(DARKER).expect("a scheme");
        assert_eq!(scheme.bg, rgb(0x3d, 0x3d, 0x3d));
        assert_eq!(scheme.surface, rgb(0x22, 0x20, 0x20));
        assert_eq!(scheme.text, rgb(0xff, 0xff, 0xff));
        assert_eq!(scheme.zebra, Some(rgb(0x5c, 0x5b, 0x5a)));
        assert_eq!(scheme.accent, Some(rgb(0x12, 0x60, 0x8a)));
        assert_eq!(
            scheme.selection,
            Some((rgb(0x12, 0x60, 0x8a), rgb(0xf9, 0xf9, 0xf9)))
        );
        // The placeholder's half white laid over the base.
        let muted = scheme.text_muted.unwrap();
        assert!((muted[0] - (128. / 255. + 61. / 255. * (1. - 128. / 255.))).abs() < 0.01);
        assert!(scheme.is_dark());
        // Too short to name what is read on: none.
        assert!(palette("#ff000000, #ffefefef").is_none());
        assert!(palette("not a color").is_none());
    }

    #[test]
    fn qt_ct_names_a_scheme_only_while_its_custom_palette_is_on() {
        let folder = tempfile::tempdir().unwrap();
        let own = folder.path().join("darker.conf");
        std::fs::write(&own, format!("[ColorScheme]\nactive_colors={DARKER}\n")).unwrap();
        let kde = folder.path().join("Solarized.colors");
        std::fs::write(
            &kde,
            "[Colors:View]\nBackgroundNormal=253,246,227\nForegroundNormal=88,110,117\n\
             [Colors:Window]\nBackgroundNormal=238,232,213\n",
        )
        .unwrap();
        let conf = |on: &str, path: &Path| {
            format!(
                "[Appearance]\ncolor_scheme_path={}\ncustom_palette={on}\nstyle=Fusion\n",
                path.display()
            )
        };
        let qt_ct = |text: &str| {
            of_settings(
                &QtSettings {
                    program: "qt6ct",
                    path: folder.path().join("qt6ct.conf"),
                    text: text.to_owned(),
                },
                None,
            )
        };
        let darker = qt_ct(&conf("true", &own)).expect("its own scheme");
        assert_eq!(darker.bg, rgb(0x3d, 0x3d, 0x3d));
        let solarized = qt_ct(&conf("true", &kde)).expect("a KDE scheme");
        assert_eq!(solarized.bg, rgb(253, 246, 227));
        assert_eq!(solarized.surface, rgb(238, 232, 213));
        // Off, Qt's style draws its own palette: none named.
        assert!(qt_ct(&conf("false", &own)).is_none());
        assert!(qt_ct("[Appearance]\ncustom_palette=true\n").is_none());
        assert!(qt_ct(&conf("true", Path::new("relative.conf"))).is_none());
    }

    #[test]
    fn qtengine_and_hyprqt6engine_name_a_kde_scheme() {
        let folder = tempfile::tempdir().unwrap();
        let home = folder.path().join("home");
        std::fs::create_dir_all(home.join(".config/qtengine")).unwrap();
        let kde = home.join(".config/qtengine/shell.colors");
        std::fs::write(
            &kde,
            "[Colors:View]\nBackgroundNormal=#1b1b1f\nForegroundNormal=#e4e2e6\n\
             [Colors:Window]\nBackgroundNormal=#202024\n\
             [Colors:Selection]\nBackgroundNormal=#b0c6ff\nForegroundNormal=#002d6e\n",
        )
        .unwrap();
        let settings = |program: &'static str, text: String| QtSettings {
            program,
            path: folder.path().join("settings"),
            text,
        };
        // qtengine's JSON, its path from the user's home as qtengine
        // takes it, or written whole.
        let engine = |named: &str| {
            settings(
                "qtengine",
                format!("{{\"theme\": {{\"colorScheme\": \"{named}\", \"style\": \"Darkly\"}}}}"),
            )
        };
        for named in [
            "~/.config/qtengine/shell.colors",
            &kde.display().to_string(),
        ] {
            let scheme = of_settings(&engine(named), Some(&home)).expect(named);
            assert_eq!(scheme.bg, rgb(0x1b, 0x1b, 0x1f));
            assert_eq!(scheme.surface, rgb(0x20, 0x20, 0x24));
            assert_eq!(scheme.accent, Some(rgb(0xb0, 0xc6, 0xff)));
            assert!(scheme.is_dark());
        }
        assert_eq!(scheme_path(&engine("~/x.colors"), None), None, "no home");
        assert_eq!(scheme_path(&engine("relative.colors"), Some(&home)), None);
        assert_eq!(
            scheme_path(&settings("qtengine", "not json".into()), None),
            None
        );
        // hyprqt6engine's, in a block or by its whole name, the last one.
        let hypr = settings(
            "hyprqt6engine",
            format!(
                "# The theme ## its colors\ntheme {{\n    color_scheme = /elsewhere.colors\n    \
                 style = Fusion # a comment\n}}\ntheme:color_scheme = {}\n",
                kde.display()
            ),
        );
        assert_eq!(scheme_path(&hypr, None).as_deref(), Some(kde.as_path()));
        assert!(of_settings(&hypr, None).is_some_and(|s| s.is_dark()));
    }

    #[test]
    fn lxqt_s_palette_is_read_once_it_names_what_is_read_on() {
        let conf = "[General]\ntheme=frost\n\n[Palette]\nbase_color=#ffffff\n\
            highlight_color=#3daee9\nhighlighted_text_color=#ffffff\nlink_color=#2980b9\n\
            text_color=#232629\nwindow_color=#eff0f1\nwindow_text_color=#232629\n";
        let scheme = lxqt(conf).expect("a scheme");
        assert_eq!(scheme.bg, rgb(255, 255, 255));
        assert_eq!(scheme.surface, rgb(0xef, 0xf0, 0xf1));
        assert_eq!(scheme.text, rgb(0x23, 0x26, 0x29));
        assert_eq!(scheme.accent, Some(rgb(0x3d, 0xae, 0xe9)));
        assert_eq!(
            scheme.selection,
            Some((rgb(0x3d, 0xae, 0xe9), rgb(255, 255, 255)))
        );
        assert!(!scheme.is_dark());
        // The window's color alone (LXQt's default): Qt derives the rest,
        // no scheme named.
        assert!(lxqt("[Palette]\nwindow_color=#efefef\n").is_none());
        assert!(lxqt("[General]\ntheme=frost\n").is_none());
    }
}
