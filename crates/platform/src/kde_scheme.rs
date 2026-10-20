//! Plasma's color scheme (ADR-071, point 4), as Plasma's applications read
//! it: `kdeglobals`, the user's before the system's (`XDG_CONFIG_DIRS`),
//! and for what they leave out the scheme they name (`[General]
//! ColorScheme`, a `color-schemes/<name>.colors` of the data folders),
//! Breeze's when they name none. Read on the desktop style's thread, and
//! again each time Plasma replaces the user's `kdeglobals`.

use std::path::{Path, PathBuf};

use crate::desktop_settings::keyfile_value;
use crate::desktop_style::{Scheme, over};

/// The scheme names Plasma takes when `kdeglobals` names none: Plasma 6's,
/// then Plasma 5's.
const DEFAULT: [&str; 2] = ["BreezeLight", "Breeze"];

/// The `kdeglobals` Plasma replaces when its user chooses: the one
/// followed.
pub(crate) fn user_file() -> Option<PathBuf> {
    crate::xdg::config_home().map(|config| config.join("kdeglobals"))
}

/// The scheme Plasma's settings name now.
pub(crate) fn read() -> Option<Scheme> {
    let files: Vec<String> = user_file()
        .into_iter()
        .chain(
            crate::xdg::config_dirs()
                .into_iter()
                .map(|config| config.join("kdeglobals")),
        )
        .filter_map(|path| crate::file::read_regular_text(&path, crate::file::DESKTOP_FILE).ok())
        .collect();
    scheme_of(&files, &crate::xdg::data_search())
}

/// The scheme `kdeglobals` (each file's text, the first winning) says, the
/// scheme file it names in `data` filling in what it leaves out.
pub(crate) fn scheme_of(kdeglobals: &[String], data: &[PathBuf]) -> Option<Scheme> {
    let said = |group: &str, key: &str| {
        kdeglobals
            .iter()
            .find_map(|text| keyfile_value(text, group, key))
    };
    let named = said("General", "ColorScheme");
    let candidates: Vec<&str> = match &named {
        Some(name) => vec![name.as_str()],
        None => DEFAULT.to_vec(),
    };
    let file = candidates.iter().find_map(|name| scheme_file(name, data));
    let value = |group: &str, key: &str| {
        said(group, key).or_else(|| {
            file.as_deref()
                .and_then(|text| keyfile_value(text, group, key))
        })
    };
    let color = |group: &str, key: &str| value(group, key).as_deref().and_then(color);
    let opaque = |color: [f32; 4]| [color[0], color[1], color[2]];
    let bg = opaque(color("Colors:View", "BackgroundNormal")?);
    let on_bg = |group: &str, key: &str| color(group, key).map(|c| over(c, bg));
    let selection_bg = on_bg("Colors:Selection", "BackgroundNormal");
    let selection = selection_bg.zip(
        color("Colors:Selection", "ForegroundNormal")
            .zip(selection_bg)
            .map(|(fg, ground)| over(fg, ground)),
    );
    Some(Scheme {
        bg,
        surface: opaque(color("Colors:Window", "BackgroundNormal")?),
        text: on_bg("Colors:View", "ForegroundNormal")?,
        text_muted: on_bg("Colors:View", "ForegroundInactive"),
        zebra: on_bg("Colors:View", "BackgroundAlternate"),
        border: None,
        accent: on_bg("General", "AccentColor").or(selection_bg),
        selection,
        error: on_bg("Colors:View", "ForegroundNegative"),
        warning: on_bg("Colors:View", "ForegroundNeutral"),
        success: on_bg("Colors:View", "ForegroundPositive"),
    })
}

/// The text of the scheme `name` in the first of `data` that has it.
fn scheme_file(name: &str, data: &[PathBuf]) -> Option<String> {
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return None;
    }
    data.iter().find_map(|folder| {
        let path: &Path = &folder.join("color-schemes").join(format!("{name}.colors"));
        crate::file::read_regular_text(path, crate::file::DESKTOP_FILE).ok()
    })
}

/// A color as Plasma writes it: `r,g,b` or `r,g,b,a` from 0 to 255, or
/// `#rrggbb`.
fn color(value: &str) -> Option<[f32; 4]> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        let channel = |i: usize| {
            u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16)
                .ok()
                .map(|v| f32::from(v) / 255.)
        };
        if hex.len() != 6 {
            return None;
        }
        return Some([channel(0)?, channel(1)?, channel(2)?, 1.]);
    }
    let channels: Vec<f32> = value
        .split(',')
        .map(|c| c.trim().parse::<u8>().ok().map(|v| f32::from(v) / 255.))
        .collect::<Option<_>>()?;
    match channels[..] {
        [r, g, b] => Some([r, g, b, 1.]),
        [r, g, b, a] => Some([r, g, b, a]),
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

    const VIEW: &str = "[Colors:View]\n\
        BackgroundAlternate=247,247,247\n\
        BackgroundNormal=255,255,255\n\
        ForegroundInactive=112,125,138\n\
        ForegroundNegative=218,68,83\n\
        ForegroundNeutral=246,116,0\n\
        ForegroundNormal=35,38,41\n\
        ForegroundPositive=39,174,96\n\
        \n\
        [Colors:Window]\n\
        BackgroundNormal=239,240,241\n\
        \n\
        [Colors:Selection]\n\
        BackgroundNormal=61,174,233\n\
        ForegroundNormal=255,255,255\n";

    #[test]
    fn kdeglobals_names_the_scheme_by_the_table() {
        let scheme = scheme_of(&[VIEW.to_owned()], &[]).expect("a scheme");
        assert_eq!(scheme.bg, rgb(255, 255, 255));
        assert_eq!(scheme.surface, rgb(239, 240, 241));
        assert_eq!(scheme.text, rgb(35, 38, 41));
        assert_eq!(scheme.text_muted, Some(rgb(112, 125, 138)));
        assert_eq!(scheme.zebra, Some(rgb(247, 247, 247)));
        assert_eq!(scheme.border, None, "derived");
        assert_eq!(scheme.accent, Some(rgb(61, 174, 233)), "the selection's");
        assert_eq!(
            scheme.selection,
            Some((rgb(61, 174, 233), rgb(255, 255, 255)))
        );
        assert_eq!(scheme.error, Some(rgb(218, 68, 83)));
        assert_eq!(scheme.warning, Some(rgb(246, 116, 0)));
        assert_eq!(scheme.success, Some(rgb(39, 174, 96)));
        assert!(!scheme.is_dark());

        // An accent of its own, and a color with transparency laid over
        // what it is drawn on.
        let accented = format!("[General]\nAccentColor=#e93d58\n\n{VIEW}").replace(
            "ForegroundInactive=112,125,138",
            "ForegroundInactive=0,0,0,128",
        );
        let scheme = scheme_of(&[accented], &[]).unwrap();
        assert_eq!(scheme.accent, Some(rgb(0xe9, 0x3d, 0x58)));
        let muted = scheme.text_muted.unwrap();
        assert!((muted[0] - (1. - 128. / 255.)).abs() < 1e-6);
    }

    #[test]
    fn the_user_s_file_wins_and_the_named_scheme_fills_in() {
        let data = tempfile::tempdir().unwrap();
        let schemes = data.path().join("color-schemes");
        std::fs::create_dir_all(&schemes).unwrap();
        let dark = VIEW
            .replace("BackgroundNormal=255,255,255", "BackgroundNormal=35,38,41")
            .replace("ForegroundNormal=35,38,41", "ForegroundNormal=252,252,252")
            .replace("BackgroundNormal=239,240,241", "BackgroundNormal=32,35,38");
        std::fs::write(schemes.join("Night.colors"), &dark).unwrap();
        std::fs::write(schemes.join("BreezeLight.colors"), VIEW).unwrap();
        let folders = [data.path().to_path_buf()];

        // Naming a scheme and nothing else: that scheme.
        let user = "[General]\nColorScheme=Night\n".to_owned();
        let scheme = scheme_of(std::slice::from_ref(&user), &folders).unwrap();
        assert!(scheme.is_dark());
        assert_eq!(scheme.bg, rgb(35, 38, 41));
        // A key of the user's own over it; the system's file below both.
        let own = format!("{user}\n[Colors:Window]\nBackgroundNormal=1,2,3\n");
        let system = "[Colors:View]\nBackgroundNormal=9,9,9\n".to_owned();
        let scheme = scheme_of(&[own, system.clone()], &folders).unwrap();
        assert_eq!(scheme.surface, rgb(1, 2, 3));
        assert_eq!(scheme.bg, rgb(9, 9, 9), "the system's, before the scheme");

        // Naming none: Breeze's, where it is installed; else none.
        assert_eq!(
            scheme_of(&[String::new()], &folders).unwrap().bg,
            rgb(255, 255, 255)
        );
        assert_eq!(scheme_of(&[String::new()], &[]), None);
        // A scheme not installed, or a name that is a path, gives none.
        for name in ["Absent", "../Night"] {
            let named = format!("[General]\nColorScheme={name}\n");
            assert_eq!(scheme_of(&[named], &folders), None, "{name:?}");
        }
    }

    #[test]
    fn colors_are_read_as_plasma_writes_them() {
        assert_eq!(
            color("61,174,233"),
            Some([61., 174., 233., 255.].map(|c| c / 255.))
        );
        assert_eq!(color(" 0, 0, 0, 0 "), Some([0.; 4]));
        assert_eq!(color("#ffffff"), Some([1.; 4]));
        for wrong in ["", "1,2", "1,2,3,4,5", "256,0,0", "#fff", "red", "-1,0,0"] {
            assert_eq!(color(wrong), None, "{wrong:?}");
        }
    }
}
