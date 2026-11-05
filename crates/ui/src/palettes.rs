//! Skwir's palettes (ADR-071, amended 2026-11-05): the colors a look is
//! made of when Colors from chooses one rather than the desktop's. Each is
//! a `Base` a look (`theme.rs`), from which every other token is derived
//! and its contrast guarded, as a desktop's scheme is: a palette names no
//! token of its own. Skwir's own four are tints of its look; the others
//! are palettes their makers publish for every application, under
//! permissive licences, credited in the notices. A platform's palette
//! (Adwaita's, macOS's, Fluent's) or a company's colors is never one.
//!
//! Each has both looks. One with a single look would be a scheme of one
//! look's: Automatic would take its look, and the other look forced be
//! Skwir's own wearing its accent (`Theme::look`, `Theme::of`). A
//! community palette brings the sixteen colors its terminal ports use,
//! and the code theme its makers publish, which code is drawn in while
//! the Reading page says Default (`Theme::resolve`; Skwir carries those
//! bat lacks, `highlight::CARRIED`). Where its makers publish none for a
//! look (Nord's light, Dracula's Alucard), that look keeps Skwir's.

use gpui::{Hsla, rgb};

use crate::theme::{Base, Contrast, Desktop, Schemes};

/// A palette: its id (the value of `ui.colors_from`), its name, who made
/// it, and its looks.
#[derive(Debug, PartialEq, Eq)]
pub struct Palette {
    pub id: &'static str,
    pub name: &'static str,
    /// Who made it and under what licence; `None` for Skwir's own.
    pub by: Option<(&'static str, &'static str)>,
    pub light: Option<Look>,
    pub dark: Option<Look>,
}

/// One look of a palette, as sRGB `0xRRGGBB`: what `Base` takes, named
/// where the palette designs it for that use (its secondary text, its
/// selection), else derived.
#[derive(Debug, PartialEq, Eq)]
pub struct Look {
    pub bg: u32,
    pub surface: u32,
    pub text: u32,
    pub muted: Option<u32>,
    pub accent: u32,
    pub selection: Option<(u32, u32)>,
    pub error: u32,
    pub warning: u32,
    pub success: u32,
    /// Black, red, green, yellow, blue, magenta, cyan, white, then their
    /// bright kin: the palette's own terminal colors.
    pub terminal: Option<[u32; 16]>,
    /// The code theme its makers publish (`highlight::choices`' value),
    /// drawn while the Reading page says Default.
    pub code: Option<&'static str>,
}

impl Look {
    fn base(&self, dark: bool) -> Base {
        let color = |value: u32| Hsla::from(rgb(value));
        Base {
            dark,
            bg: color(self.bg),
            surface: color(self.surface),
            text: color(self.text),
            text_muted: self.muted.map(color),
            zebra: None,
            border: None,
            accent: color(self.accent),
            selection: self.selection.map(|(bg, text)| (color(bg), color(text))),
            error: color(self.error),
            warning: color(self.warning),
            success: color(self.success),
            terminal: self.terminal.map(|colors| colors.map(color)),
        }
    }
}

impl Palette {
    /// Its look `dark` names, if it has one.
    pub fn look(&self, dark: bool) -> Option<&Look> {
        if dark {
            self.dark.as_ref()
        } else {
            self.light.as_ref()
        }
    }

    /// The base of its look `dark` names.
    pub fn base(&self, dark: bool) -> Option<Base> {
        self.look(dark).map(|look| look.base(dark))
    }

    /// What the look takes when it is worn: its looks as schemes, and
    /// the accent of the one it has for a look it has not. Skwir's own
    /// colors are no scheme: what nothing chosen gives.
    pub fn desktop(&self, contrast: Contrast) -> Desktop {
        if self.id == SKWIR.id {
            return Desktop {
                contrast,
                ..Desktop::default()
            };
        }
        let schemes = Schemes {
            light: self.base(false),
            dark: self.base(true),
        };
        Desktop {
            accent: schemes.light.or(schemes.dark).map(|base| base.accent),
            contrast,
            schemes,
        }
    }

    /// The code theme of its look `dark` names, if it publishes one.
    pub fn code(&self, dark: bool) -> Option<&'static str> {
        self.look(dark)?.code
    }
}

/// The palette `id` names.
pub fn by_id(id: &str) -> Option<&'static Palette> {
    PALETTES.iter().find(|palette| palette.id == id)
}

/// Skwir's own colors: what Skwir looks like when nothing else is chosen.
pub static SKWIR: &Palette = &PALETTES[0];

/// Every palette, in the order Colors from offers them: Skwir's own, then
/// the community's.
pub static PALETTES: [Palette; 12] = [
    Palette {
        id: "skwir",
        name: "Skwir",
        by: None,
        light: Some(Look {
            bg: 0xfafafa,
            surface: 0xf0f0f0,
            text: 0x1f1f1f,
            muted: None,
            accent: 0x2f6feb,
            selection: None,
            error: 0xc62828,
            warning: 0xb26a00,
            success: 0x2e7d32,
            terminal: None,
            code: None,
        }),
        dark: Some(Look {
            bg: 0x1e1e1e,
            surface: 0x262626,
            text: 0xe6e6e6,
            muted: None,
            accent: 0x6ea8fe,
            selection: None,
            error: 0xff6b6b,
            warning: 0xf2b84b,
            success: 0x6fcf7f,
            terminal: None,
            code: None,
        }),
    },
    // Warm grounds, as paper under a lamp, with a deep teal accent: far
    // from the error's red.
    Palette {
        id: "paper",
        name: "Paper",
        by: None,
        light: Some(Look {
            bg: 0xfbf8f2,
            surface: 0xf1ece3,
            text: 0x24201b,
            muted: None,
            accent: 0x17766b,
            selection: None,
            error: 0xb3261e,
            warning: 0x9a6200,
            success: 0x3b7a3a,
            terminal: None,
            code: None,
        }),
        dark: Some(Look {
            bg: 0x1f1d1a,
            surface: 0x28251f,
            text: 0xe9e4dc,
            muted: None,
            accent: 0x5cc4b5,
            selection: None,
            error: 0xf2766b,
            warning: 0xe8b860,
            success: 0x8fc98a,
            terminal: None,
            code: None,
        }),
    },
    // Cool blue-gray grounds, an indigo accent.
    Palette {
        id: "slate",
        name: "Slate",
        by: None,
        light: Some(Look {
            bg: 0xf8fafc,
            surface: 0xecf0f5,
            text: 0x1b2129,
            muted: None,
            accent: 0x4860be,
            selection: None,
            error: 0xc0262d,
            warning: 0xa15f00,
            success: 0x2a7a45,
            terminal: None,
            code: None,
        }),
        dark: Some(Look {
            bg: 0x1b1e22,
            surface: 0x23272d,
            text: 0xdde5ee,
            muted: None,
            accent: 0x89a9f1,
            selection: None,
            error: 0xf47a7a,
            warning: 0xe9b75a,
            success: 0x77c890,
            terminal: None,
            code: None,
        }),
    },
    // White and true black, for screens that turn black pixels off and for
    // dark rooms.
    Palette {
        id: "black",
        name: "Black",
        by: None,
        light: Some(Look {
            bg: 0xffffff,
            surface: 0xf2f2f2,
            text: 0x0d0d0d,
            muted: None,
            accent: 0x2f6feb,
            selection: None,
            error: 0xc62828,
            warning: 0xa35f00,
            success: 0x2e7d32,
            terminal: None,
            code: None,
        }),
        dark: Some(Look {
            bg: 0x000000,
            surface: 0x121212,
            text: 0xececec,
            muted: None,
            accent: 0x6ea8fe,
            selection: None,
            error: 0xff6b6b,
            warning: 0xf2b84b,
            success: 0x6fcf7f,
            terminal: None,
            code: None,
        }),
    },
    // Its README's sixteen; one set of terminal colors for both looks.
    Palette {
        id: "solarized",
        name: "Solarized",
        by: Some(("Ethan Schoonover", "MIT")),
        light: Some(Look {
            bg: 0xfdf6e3,
            surface: 0xeee8d5,
            text: 0x657b83,
            muted: Some(0x93a1a1),
            accent: 0x268bd2,
            selection: None,
            error: 0xdc322f,
            warning: 0xb58900,
            success: 0x859900,
            terminal: Some([
                0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5,
                0x002b36, 0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
            ]),
            code: Some("Solarized (light)"),
        }),
        dark: Some(Look {
            bg: 0x002b36,
            surface: 0x073642,
            text: 0x839496,
            muted: Some(0x586e75),
            accent: 0x268bd2,
            selection: None,
            error: 0xdc322f,
            warning: 0xb58900,
            success: 0x859900,
            terminal: Some([
                0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5,
                0x002b36, 0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
            ]),
            code: Some("Solarized (dark)"),
        }),
    },
    // nord.css's roles: nord1 for bars, nord8 the core accent, nord13 for
    // warnings. Snow Storm's grounds for the light look, which has no terminal
    // colors or code theme of its own.
    Palette {
        id: "nord",
        name: "Nord",
        by: Some(("Sven Greb", "MIT")),
        light: Some(Look {
            bg: 0xeceff4,
            surface: 0xe5e9f0,
            text: 0x2e3440,
            muted: None,
            accent: 0x5e81ac,
            selection: None,
            error: 0xbf616a,
            warning: 0xebcb8b,
            success: 0xa3be8c,
            terminal: None,
            code: None,
        }),
        dark: Some(Look {
            bg: 0x2e3440,
            surface: 0x3b4252,
            text: 0xd8dee9,
            muted: None,
            accent: 0x88c0d0,
            selection: None,
            error: 0xbf616a,
            warning: 0xebcb8b,
            success: 0xa3be8c,
            terminal: Some([
                0x3b4252, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x88c0d0, 0xe5e9f0,
                0x4c566a, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x8fbcbb, 0xeceff4,
            ]),
            code: Some("Nord"),
        }),
    },
    // gruvbox.vim's, at medium contrast: faded colors for the light look,
    // bright for the dark, its terminal colors its own.
    Palette {
        id: "gruvbox",
        name: "Gruvbox",
        by: Some(("Pavel Pertsev", "MIT")),
        light: Some(Look {
            bg: 0xfbf1c7,
            surface: 0xf2e5bc,
            text: 0x3c3836,
            muted: Some(0x7c6f64),
            accent: 0xaf3a03,
            selection: None,
            error: 0x9d0006,
            warning: 0xb57614,
            success: 0x79740e,
            terminal: Some([
                0xfbf1c7, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0x7c6f64,
                0x928374, 0x9d0006, 0x79740e, 0xb57614, 0x076678, 0x8f3f71, 0x427b58, 0x3c3836,
            ]),
            code: Some("gruvbox-light"),
        }),
        dark: Some(Look {
            bg: 0x282828,
            surface: 0x32302f,
            text: 0xebdbb2,
            muted: Some(0xa89984),
            accent: 0xfe8019,
            selection: None,
            error: 0xfb4934,
            warning: 0xfabd2f,
            success: 0xb8bb26,
            terminal: Some([
                0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984,
                0x928374, 0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c, 0xebdbb2,
            ]),
            code: Some("gruvbox-dark"),
        }),
    },
    // Latte and Mocha, as its style guide gives the roles (Base, Mantle,
    // Subtext 0); the terminal colors palette.json's.
    Palette {
        id: "catppuccin",
        name: "Catppuccin",
        by: Some(("Catppuccin", "MIT")),
        light: Some(Look {
            bg: 0xeff1f5,
            surface: 0xe6e9ef,
            text: 0x4c4f69,
            muted: Some(0x6c6f85),
            accent: 0x8839ef,
            selection: None,
            error: 0xd20f39,
            warning: 0xdf8e1d,
            success: 0x40a02b,
            terminal: Some([
                0x5c5f77, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xacb0be,
                0x6c6f85, 0xde293e, 0x49af3d, 0xeea02d, 0x456eff, 0xfe85d8, 0x2d9fa8, 0xbcc0cc,
            ]),
            code: Some("Catppuccin Latte"),
        }),
        dark: Some(Look {
            bg: 0x1e1e2e,
            surface: 0x181825,
            text: 0xcdd6f4,
            muted: Some(0xa6adc8),
            accent: 0xcba6f7,
            selection: None,
            error: 0xf38ba8,
            warning: 0xf9e2af,
            success: 0xa6e3a1,
            terminal: Some([
                0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xa6adc8,
                0x585b70, 0xf37799, 0x89d88b, 0xebd391, 0x74a8fc, 0xf2aede, 0x6bd7ca, 0xbac2de,
            ]),
            code: Some("Catppuccin Mocha"),
        }),
    },
    // Its spec's: Alucard for the light look (no code theme in bat), the
    // classic for the dark, with its selection. Orange warns: the spec's red
    // is for errors.
    Palette {
        id: "dracula",
        name: "Dracula",
        by: Some(("Dracula Theme", "MIT")),
        light: Some(Look {
            bg: 0xfffbeb,
            surface: 0xece9df,
            text: 0x1f1f1f,
            muted: Some(0x6c664b),
            accent: 0x644ac9,
            selection: Some((0xcfcfde, 0x1f1f1f)),
            error: 0xcb3a2a,
            warning: 0xa34d14,
            success: 0x14710a,
            terminal: Some([
                0xfffbeb, 0xcb3a2a, 0x14710a, 0x846e15, 0x644ac9, 0xa3144d, 0x036a96, 0x1f1f1f,
                0x6c664b, 0xd74c3d, 0x198d0c, 0x9e841a, 0x7862d0, 0xbf185a, 0x047fb4, 0x2c2b31,
            ]),
            code: None,
        }),
        dark: Some(Look {
            bg: 0x282a36,
            surface: 0x21222c,
            text: 0xf8f8f2,
            muted: Some(0x6272a4),
            accent: 0xbd93f9,
            selection: Some((0x44475a, 0xf8f8f2)),
            error: 0xff5555,
            warning: 0xffb86c,
            success: 0x50fa7b,
            terminal: Some([
                0x21222c, 0xff5555, 0x50fa7b, 0xf1fa8c, 0xbd93f9, 0xff79c6, 0x8be9fd, 0xf8f8f2,
                0x6272a4, 0xff6e6e, 0x69ff94, 0xffffa5, 0xd6acff, 0xff92df, 0xa4ffff, 0xffffff,
            ]),
            code: Some("Dracula"),
        }),
    },
    // Dawn and the main variant. Its own `surface` is lighter than Dawn's
    // ground, as close as 1.03:1: the chrome is `overlay`, as the dark look's
    // is `surface`. Pine, its "terminal green", is success in both looks.
    Palette {
        id: "rose-pine",
        name: "Rosé Pine",
        by: Some(("Rosé Pine", "MIT")),
        light: Some(Look {
            bg: 0xfaf4ed,
            surface: 0xf2e9e1,
            text: 0x464261,
            muted: Some(0x797593),
            accent: 0x907aa9,
            selection: None,
            error: 0xb4637a,
            warning: 0xea9d34,
            success: 0x286983,
            terminal: Some([
                0xf2e9e1, 0xb4637a, 0x286983, 0xea9d34, 0x56949f, 0x907aa9, 0xd7827e, 0x575279,
                0x9893a5, 0xb4637a, 0x286983, 0xea9d34, 0x56949f, 0x907aa9, 0xd7827e, 0x575279,
            ]),
            code: Some("rose-pine-dawn"),
        }),
        dark: Some(Look {
            bg: 0x191724,
            surface: 0x1f1d2e,
            text: 0xe0def4,
            muted: Some(0x908caa),
            accent: 0xc4a7e7,
            selection: None,
            error: 0xeb6f92,
            warning: 0xf6c177,
            success: 0x31748f,
            terminal: Some([
                0x26233a, 0xeb6f92, 0x31748f, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xebbcba, 0xe0def4,
                0x6e6a86, 0xeb6f92, 0x31748f, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xebbcba, 0xe0def4,
            ]),
            code: Some("rose-pine"),
        }),
    },
    // Day and Night, as tokyonight.nvim makes them (Day exists only there):
    // its `bg_dark` the chrome, its `error` role the errors.
    Palette {
        id: "tokyo-night",
        name: "Tokyo Night",
        by: Some(("Enkia (MIT) and Folke Lemaitre", "Apache-2.0")),
        light: Some(Look {
            bg: 0xe1e2e7,
            surface: 0xd0d5e3,
            text: 0x3760bf,
            muted: None,
            accent: 0x2e7de9,
            selection: None,
            error: 0xc64343,
            warning: 0x8c6c3e,
            success: 0x587539,
            terminal: Some([
                0xb4b5b9, 0xf52a65, 0x587539, 0x8c6c3e, 0x2e7de9, 0x9854f1, 0x007197, 0x6172b0,
                0xa1a6c5, 0xff4774, 0x5c8524, 0xa27629, 0x358aff, 0xa463ff, 0x007ea8, 0x3760bf,
            ]),
            code: Some("tokyonight_day"),
        }),
        dark: Some(Look {
            bg: 0x1a1b26,
            surface: 0x16161e,
            text: 0xc0caf5,
            muted: None,
            accent: 0x7aa2f7,
            selection: None,
            error: 0xdb4b4b,
            warning: 0xe0af68,
            success: 0x9ece6a,
            terminal: Some([
                0x15161e, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xa9b1d6,
                0x414868, 0xff899d, 0x9fe044, 0xfaba4a, 0x8db0ff, 0xc7a9ff, 0xa4daff, 0xc0caf5,
            ]),
            code: Some("tokyonight_night"),
        }),
    },
    // everforest.vim's, at medium contrast; its terminal colors its maker's
    // own, the bright as the others.
    Palette {
        id: "everforest",
        name: "Everforest",
        by: Some(("Sainnhe Park", "MIT")),
        light: Some(Look {
            bg: 0xfdf6e3,
            surface: 0xefebd4,
            text: 0x5c6a72,
            muted: None,
            accent: 0x8da101,
            selection: None,
            error: 0xf85552,
            warning: 0xdfa000,
            success: 0x35a77c,
            terminal: Some([
                0x5c6a72, 0xf85552, 0x8da101, 0xdfa000, 0x3a94c5, 0xdf69ba, 0x35a77c, 0xe0dcc7,
                0x5c6a72, 0xf85552, 0x8da101, 0xdfa000, 0x3a94c5, 0xdf69ba, 0x35a77c, 0xe0dcc7,
            ]),
            code: Some("everforest-light"),
        }),
        dark: Some(Look {
            bg: 0x2d353b,
            surface: 0x232a2e,
            text: 0xd3c6aa,
            muted: Some(0x9da9a0),
            accent: 0xa7c080,
            selection: None,
            error: 0xe67e80,
            warning: 0xdbbc7f,
            success: 0x83c092,
            terminal: Some([
                0x475258, 0xe67e80, 0xa7c080, 0xdbbc7f, 0x7fbbb3, 0xd699b6, 0x83c092, 0xd3c6aa,
                0x475258, 0xe67e80, 0xa7c080, 0xdbbc7f, 0x7fbbb3, 0xd699b6, 0x83c092, 0xd3c6aa,
            ]),
            code: Some("everforest-dark"),
        }),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palettes_are_named_once_skwir_s_first() {
        let ids: std::collections::HashSet<_> = PALETTES.iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), PALETTES.len());
        assert_eq!(SKWIR.id, crate::desktop_style::OWN);
        assert!(SKWIR.light.is_some() && SKWIR.dark.is_some());
        for palette in &PALETTES {
            assert!(
                palette.light.is_some() || palette.dark.is_some(),
                "{}",
                palette.id
            );
            assert!(
                palette
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'-'),
                "{}: a value of the settings file",
                palette.id
            );
            // A look's ground and text are on its side of middle gray.
            for dark in [false, true] {
                if let Some(base) = palette.base(dark) {
                    let lighter =
                        crate::color::luminance(base.text) > crate::color::luminance(base.bg);
                    assert_eq!(lighter, dark, "{} dark: {dark}", palette.id);
                }
            }
        }
    }

    #[test]
    fn a_palette_s_code_theme_is_one_offered_for_its_look() {
        let none: &[std::sync::Arc<crate::code_preview::UserTheme>] = &[];
        for palette in &PALETTES {
            for dark in [false, true] {
                let Some(name) = palette.code(dark) else {
                    continue;
                };
                let choices = crate::code_preview::choices(dark, name, Some(none));
                assert!(
                    choices
                        .iter()
                        .any(|(value, label)| value == name && !label.contains("not found")),
                    "{name}, for {} dark: {dark}",
                    palette.id
                );
            }
        }
    }

    #[test]
    fn skwir_s_colors_are_no_scheme_a_palette_s_are() {
        assert_eq!(
            SKWIR.desktop(Contrast::More),
            Desktop {
                contrast: Contrast::More,
                ..Desktop::default()
            }
        );
        let nord = by_id("nord").unwrap();
        let desktop = nord.desktop(Contrast::Normal);
        assert_eq!(desktop.schemes.light, nord.base(false));
        assert_eq!(desktop.schemes.dark, nord.base(true));
        for palette in &PALETTES {
            assert_eq!(palette.desktop(Contrast::Normal).schemes.one_look(), None);
        }
    }
}
