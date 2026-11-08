//! Omarchy's theme (ADR-071, amended 2026-11-07): the scheme of the
//! desktop where Omarchy's session runs, read from the theme it applies
//! (`colors.toml`), which no toolkit's settings carry: Omarchy names GTK's
//! built-in Adwaita and Qt nothing, and writes its colors into each
//! program's own file from that one. Read on the desktop style's thread,
//! and again when Omarchy puts another theme in place.
//!
//! Omarchy keeps the theme it applies in `~/.local/state/omarchy/current/
//! theme` (in `~/.config/omarchy/current/theme` before), its home folder's
//! whatever the XDG variables say, and puts another in place whole: the
//! folder removed, the next one renamed to it (`omarchy-theme-set`).
//! `colors.toml` is read as Omarchy reads it (`omarchy-theme-color`): a
//! line a key, `=`, a value quoted or not; a name it once wrote otherwise
//! (`bg`, the terminal's `color0`…) taken where the newer one is missing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::desktop_style::{Rgb, Scheme};

/// Whether this session is Omarchy's: its folder named, as its session
/// exports it to every program (`OMARCHY_PATH`). A state folder left from
/// an Omarchy no longer run says nothing then.
pub(crate) fn is_session() -> bool {
    std::env::var_os("OMARCHY_PATH").is_some_and(|path| !path.is_empty())
}

/// Where Omarchy keeps the theme it applies, newest first, under `home`:
/// what is followed, each in its folder.
pub(crate) fn themes(home: &Path) -> [PathBuf; 2] {
    [
        home.join(".local/state/omarchy/current/theme"),
        home.join(".config/omarchy/current/theme"),
    ]
}

/// The scheme of the theme Omarchy applies now.
pub(crate) fn read() -> Option<Scheme> {
    read_in(&skwir_core::os_path::home()?)
}

/// The scheme of the theme Omarchy applies under `home`: the first of
/// `themes` that holds a `colors.toml`.
pub(crate) fn read_in(home: &Path) -> Option<Scheme> {
    themes(home).iter().find_map(|theme| {
        let text =
            crate::file::read_regular_text(&theme.join("colors.toml"), crate::file::DESKTOP_FILE)
                .ok()?;
        scheme_of(&text)
    })
}

/// A `colors.toml`'s scheme, once it names its ground and its text.
///
/// Its selection is not read: Omarchy's `selection` is what a terminal or
/// an editor marks text with (Omarchy's VS Code theme marks a list's
/// chosen rows with its accent), so the selection is derived from the
/// accent, as the chosen rows of Skwir's own palettes are.
pub(crate) fn scheme_of(text: &str) -> Option<Scheme> {
    let colors = entries(text);
    let color = |names: &[&str]| {
        names
            .iter()
            .find_map(|name| colors.get(*name).and_then(|value| hex(value)))
    };
    let bg = color(&["background", "bg", "color0"])?;
    Some(Scheme {
        bg,
        // Its own step below the ground, where it names one; else the
        // ground, which the theme sets apart for the chrome.
        surface: color(&["dark_background", "dark_bg"]).unwrap_or(bg),
        text: color(&["foreground", "fg", "color7"])?,
        text_muted: color(&["dark_foreground", "dark_fg", "color8"]),
        zebra: None,
        border: color(&["muted", "color8"]),
        accent: color(&["accent", "blue", "color4"]),
        selection: None,
        error: color(&["red", "color1"]),
        warning: color(&["yellow", "color3"]),
        success: color(&["green", "color2"]),
        sidebar: None,
    })
}

/// Its keys and values, as Omarchy reads them: a key's quotes and spaces
/// taken out, a quoted value up to its closing quote (what follows, a
/// comment, left out), another trimmed; a line without a key, or one that
/// begins with `#`, skipped. A key said twice is its last.
fn entries(text: &str) -> HashMap<String, String> {
    let mut entries = HashMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key: String = key
            .chars()
            .filter(|c| !matches!(c, '"' | '\'') && !c.is_whitespace())
            .collect();
        if key.is_empty() || key.starts_with('#') {
            continue;
        }
        let value = match value.find(['"', '\'']) {
            Some(start) => value[start + 1..]
                .split(['"', '\''])
                .next()
                .unwrap_or_default(),
            None => value.trim(),
        };
        entries.insert(key, value.to_owned());
    }
    entries
}

/// `#rrggbb` or `#rgb`.
fn hex(value: &str) -> Option<Rgb> {
    let digits = value.trim().strip_prefix('#')?;
    let channel = |i: usize, width: usize| {
        let digits = digits.get(i * width..(i + 1) * width)?;
        let value = u8::from_str_radix(digits, 16).ok()?;
        Some(if width == 1 {
            f32::from(value * 17) / 255.
        } else {
            f32::from(value) / 255.
        })
    };
    let width = match digits.len() {
        3 => 1,
        6 => 2,
        _ => return None,
    };
    Some([channel(0, width)?, channel(1, width)?, channel(2, width)?])
}

#[cfg(test)]
// The channels compared are the same numbers divided alike: equal exactly.
// Colors are written as the themes write them, `0xrrggbb`.
#[allow(clippy::float_cmp, clippy::unreadable_literal)]
mod tests {
    use super::*;

    fn rgb(hex: u32) -> Rgb {
        [16, 8, 0].map(|shift| f32::from(((hex >> shift) & 0xff) as u8) / 255.)
    }

    /// A theme as Omarchy writes one now, of the test's own colors.
    const DARK: &str = "mode = \"dark\"\n\n\
        accent = \"#7a9ef0\"\n\
        selection = \"#2a2e40\"\n\
        muted = \"#43496a\"\n\n\
        background = \"#1b1c27\" # the ground\n\
        dark_background = \"#14151d\"\n\
        lighter_background = \"#25293a\"\n\n\
        foreground = \"#aab2d5\"\n\
        dark_foreground = \"#575f88\"\n\n\
        red = \"#f5778f\"\n\
        yellow = \"#dfae69\"\n\
        green = \"#9dcd6b\"\n";

    #[test]
    fn a_theme_s_colors_are_its_scheme() {
        let scheme = scheme_of(DARK).expect("a scheme");
        assert_eq!(scheme.bg, rgb(0x1b1c27));
        assert_eq!(scheme.surface, rgb(0x14151d));
        assert_eq!(scheme.text, rgb(0xaab2d5));
        assert_eq!(scheme.text_muted, Some(rgb(0x575f88)));
        assert_eq!(scheme.border, Some(rgb(0x43496a)));
        assert_eq!(scheme.accent, Some(rgb(0x7a9ef0)));
        assert_eq!(scheme.selection, None, "derived from the accent");
        assert_eq!(scheme.error, Some(rgb(0xf5778f)));
        assert_eq!(scheme.warning, Some(rgb(0xdfae69)));
        assert_eq!(scheme.success, Some(rgb(0x9dcd6b)));
        assert!(scheme.is_dark());
    }

    #[test]
    fn older_names_unquoted_values_and_comments_are_read_as_omarchy_reads_them() {
        // The terminal's names, as themes written before the newer ones.
        let ansi = "# An older theme\n\
            color0 = \"#fafafa\"\ncolor1='#c01c28'\ncolor4 = \"#1c71d8\"\n\
            color7 = #202020\ncolor8 = \"#8a8a8a\"\n";
        let scheme = scheme_of(ansi).expect("a scheme");
        assert_eq!(scheme.bg, rgb(0xfafafa));
        assert_eq!(scheme.surface, scheme.bg, "the ground, set apart later");
        assert_eq!(scheme.text, rgb(0x202020), "unquoted");
        assert_eq!(scheme.accent, Some(rgb(0x1c71d8)));
        assert_eq!(scheme.text_muted, Some(rgb(0x8a8a8a)));
        assert_eq!(scheme.error, Some(rgb(0xc01c28)));
        assert!(!scheme.is_dark());
        // The short names, and the newer one first where both are there.
        let short = "bg = \"#fff\"\nfg = \"#000\"\nbackground = \"#eeeeee\"\n";
        let scheme = scheme_of(short).expect("a scheme");
        assert_eq!((scheme.bg, scheme.text), (rgb(0xeeeeee), rgb(0)));
        // Without a ground or a text, or with one that is no color: none.
        assert!(scheme_of("foreground = \"#ffffff\"\n").is_none());
        assert!(scheme_of("background = \"#ffffff\"\nforeground = \"white\"\n").is_none());
        assert!(scheme_of("").is_none());
    }

    #[test]
    fn the_theme_applied_is_the_newest_place_s_then_the_older_one_s() {
        let home = tempfile::tempdir().unwrap();
        assert!(read_in(home.path()).is_none());
        let [newer, older] = themes(home.path());
        std::fs::create_dir_all(&older).unwrap();
        std::fs::write(
            older.join("colors.toml"),
            "background = \"#ffffff\"\nforeground = \"#111111\"\n",
        )
        .unwrap();
        assert_eq!(read_in(home.path()).unwrap().bg, rgb(0xffffff));
        std::fs::create_dir_all(&newer).unwrap();
        std::fs::write(newer.join("colors.toml"), DARK).unwrap();
        assert_eq!(read_in(home.path()).unwrap().bg, rgb(0x1b1c27));
    }
}
