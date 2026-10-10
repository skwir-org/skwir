//! Virtual source lines. Fold coordinates stay in the provider's logical lines.
//!
//! Code is colored with bat's grammars and themes. Unless the Reading page
//! chooses a theme for the look bibli has (`CodeTheme`), the colors are
//! bibli's own: base16 Ocean's, on bibli's background. A theme chosen,
//! one of bat's or one its user added to bat (`bat::themes_dir`), brings
//! the background it is meant for, and its bold and italic: not its
//! underline, which in bibli is a link's.
use crate::{inspector_text::Selection, shell::Shell, theme::Theme};
use bibli_core::{
    inspection::{Inspection, Run},
    workspace::PaneId,
};
use gpui::{AnyElement, Hsla, WeakEntity, div, prelude::*, px};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};
use syntect::highlighting::{Color, FontStyle, Theme as Colors, ThemeSet};
use syntect::parsing::SyntaxSet;
use two_face::theme::{EmbeddedLazyThemeSet, EmbeddedThemeName as Bat};

/// bat's grammars, syntect's defaults among them: read once, each
/// grammar compiled when a file first needs it.
pub(crate) fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

/// bat's themes: read once, each unpacked when it is first drawn.
fn bat_themes() -> &'static EmbeddedLazyThemeSet {
    static SET: OnceLock<EmbeddedLazyThemeSet> = OnceLock::new();
    SET.get_or_init(two_face::theme::extra)
}

/// The choice of `reading.code_light` and `reading.code_dark` that keeps
/// bibli's own colors.
pub const DEFAULT: &str = "default";

/// bat's themes bibli offers: bat's name for each (what `bat --theme`
/// takes, and the value stored), the name shown, and whether it is dark,
/// so each look offers its own (a test holds this to the background).
/// A pair's light and dark are one name in their two lists. bat's
/// terminal themes (ANSI, Base16, Base16 256) are left out: they name
/// the terminal's palette, which a window has not.
const BAT: [(Bat, &str, bool); 29] = [
    (Bat::Base16EightiesDark, "Base16 Eighties", true),
    (Bat::Base16MochaDark, "Base16 Mocha", true),
    (Bat::Base16OceanDark, "Base16 Ocean", true),
    (Bat::Base16OceanLight, "Base16 Ocean", false),
    (Bat::CatppuccinFrappe, "Catppuccin Frappé", true),
    (Bat::CatppuccinLatte, "Catppuccin Latte", false),
    (Bat::CatppuccinMacchiato, "Catppuccin Macchiato", true),
    (Bat::CatppuccinMocha, "Catppuccin Mocha", true),
    (Bat::ColdarkCold, "Coldark Cold", false),
    (Bat::ColdarkDark, "Coldark Dark", true),
    (Bat::DarkNeon, "DarkNeon", true),
    (Bat::Dracula, "Dracula", true),
    (Bat::Github, "GitHub", false),
    (Bat::GruvboxDark, "Gruvbox", true),
    (Bat::GruvboxLight, "Gruvbox", false),
    (Bat::InspiredGithub, "Inspired GitHub", false),
    (Bat::Leet, "1337", true),
    (Bat::MonokaiExtended, "Monokai Extended", true),
    (Bat::MonokaiExtendedBright, "Monokai Extended Bright", true),
    (Bat::MonokaiExtendedLight, "Monokai Extended", false),
    (Bat::MonokaiExtendedOrigin, "Monokai Extended Origin", true),
    (Bat::Nord, "Nord", true),
    (Bat::OneHalfDark, "One Half", true),
    (Bat::OneHalfLight, "One Half", false),
    (Bat::SolarizedDark, "Solarized", true),
    (Bat::SolarizedLight, "Solarized", false),
    (Bat::SublimeSnazzy, "Sublime Snazzy", true),
    (Bat::TwoDark, "TwoDark", true),
    (Bat::Zenburn, "Zenburn", true),
];

/// The colors code is drawn in, as chosen for the look bibli has
/// (`Theme::code`, read with the theme). A preview is highlighted with
/// it, so a new choice shows the file again (`preview::Version`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeTheme {
    pub dark: bool,
    /// bat's name for the theme, the name of a theme the user added to
    /// bat, or [`DEFAULT`].
    pub name: Arc<str>,
}

impl CodeTheme {
    /// bibli's own colors, for a look.
    pub fn own(dark: bool) -> Self {
        Self {
            dark,
            name: DEFAULT.into(),
        }
    }

    /// The theme the settings choose for a look.
    pub fn chosen(app: &bibli_core::App, dark: bool) -> Self {
        let key = if dark {
            crate::settings::CODE_DARK_KEY
        } else {
            crate::settings::CODE_LIGHT_KEY
        };
        app.settings.get::<String>(key).map_or_else(
            || Self::own(dark),
            |name| Self {
                dark,
                name: name.into(),
            },
        )
    }
}

/// A theme the user added to bat, named as bat names it: by its file.
#[derive(Debug)]
pub struct UserTheme {
    pub name: String,
    pub dark: bool,
    colors: Colors,
}

/// The themes the user added to bat, once read (`user_themes`).
static USER: RwLock<Option<Arc<[Arc<UserTheme>]>>> = RwLock::new(None);

/// The themes the user added to bat, read from its folder the first time
/// and again when `again` asks (at start, and when the settings open: a
/// file edited since is read there). It reads files: a worker's work.
pub fn user_themes(again: bool) -> Arc<[Arc<UserTheme>]> {
    if !again && let Some(themes) = USER.read().ok().and_then(|themes| themes.clone()) {
        return themes;
    }
    let themes: Arc<[_]> = bibli_platform::bat::themes_dir()
        .map(|dir| read_user_themes(&dir))
        .unwrap_or_default()
        .into();
    if let Ok(mut user) = USER.write() {
        *user = Some(Arc::clone(&themes));
    }
    themes
}

/// Each `.tmTheme` under `dir`, found as bat finds them and named by its
/// file, in the order of their names. One that does not read as a theme
/// is left out.
fn read_user_themes(dir: &Path) -> Vec<Arc<UserTheme>> {
    /// Far more than a theme holds: a file this large is none.
    const LIMIT: u64 = 4 << 20;
    let mut themes: Vec<_> = ThemeSet::discover_theme_paths(dir)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|path| {
            let name = path.file_stem()?.to_str()?.to_owned();
            let bytes = bibli_platform::file::read_regular(&path, LIMIT).ok()?;
            let colors = ThemeSet::load_from_reader(&mut std::io::Cursor::new(bytes)).ok()?;
            Some(Arc::new(UserTheme {
                name,
                dark: is_dark(&colors),
                colors,
            }))
        })
        .collect();
    themes.sort_by_cached_key(|theme| (theme.name.to_lowercase(), theme.name.clone()));
    themes.dedup_by(|a, b| a.name == b.name);
    themes
}

/// Whether a theme is meant for a dark background: by its background's
/// lightness, else its text's.
fn is_dark(colors: &Colors) -> bool {
    let light = |c: Color| {
        (0.2126 * f32::from(c.r) + 0.7152 * f32::from(c.g) + 0.0722 * f32::from(c.b)) / 255.
    };
    match (colors.settings.background, colors.settings.foreground) {
        (Some(background), _) => light(background) < 0.5,
        (None, Some(text)) => light(text) > 0.5,
        (None, None) => false,
    }
}

/// The choices of a look's setting, `(value, label)`: bibli's own colors,
/// then bat's themes of that look, then the user's once read, a theme of
/// the user's taking the place of bat's of the same name as in bat. The
/// one chosen is always among them, said missing once the user's are
/// read and none has its name.
pub fn choices(dark: bool, chosen: &str, user: Option<&[Arc<UserTheme>]>) -> Vec<(String, String)> {
    let theirs = |name: &str| user.is_some_and(|user| user.iter().any(|theme| theme.name == name));
    let mut bat: Vec<_> = BAT
        .iter()
        .filter(|(name, _, theme_dark)| *theme_dark == dark && !theirs(name.as_name()))
        .map(|(name, label, _)| (name.as_name().to_owned(), (*label).to_owned()))
        .collect();
    bat.sort_by_cached_key(|(_, label)| label.to_lowercase());
    let mut choices = vec![(DEFAULT.to_owned(), "Default".to_owned())];
    choices.extend(bat);
    choices.extend(
        user.unwrap_or_default()
            .iter()
            .filter(|theme| theme.dark == dark)
            .map(|theme| (theme.name.clone(), theme.name.clone())),
    );
    if !choices.iter().any(|(value, _)| value == chosen) {
        let label = if user.is_some() {
            format!("{chosen} (not found)")
        } else {
            chosen.to_owned()
        };
        choices.push((chosen.to_owned(), label));
    }
    choices
}

/// What code is drawn with: a theme's colors and, unless they are
/// bibli's own, the background they are meant for and their bold and
/// italic.
pub(crate) struct Palette {
    colors: Painted,
    /// The theme's background, as a run's color is kept (`Run::color`);
    /// `None` draws on bibli's own.
    pub background: Option<[u8; 3]>,
    /// Whether its font styles are drawn.
    styled: bool,
}

enum Painted {
    Bat(&'static Colors),
    User(Arc<UserTheme>),
}

impl Palette {
    /// The palette `code` names: a theme the user added to bat, then one
    /// of bat's, as bat looks them up; bibli's own colors when none has
    /// its name (a file taken out of bat's folder). It may read the
    /// user's themes: a worker's work.
    pub fn of(code: &CodeTheme) -> Self {
        let own = || Palette {
            colors: Painted::Bat(bat_themes().get(if code.dark {
                Bat::Base16OceanDark
            } else {
                Bat::Base16OceanLight
            })),
            background: None,
            styled: false,
        };
        if &*code.name == DEFAULT {
            return own();
        }
        let colors = if let Some(theme) = user_themes(false)
            .iter()
            .find(|theme| *theme.name == *code.name)
        {
            Painted::User(Arc::clone(theme))
        } else if let Some((name, ..)) = BAT.iter().find(|(name, ..)| name.as_name() == &*code.name)
        {
            Painted::Bat(bat_themes().get(*name))
        } else {
            return own();
        };
        let mut palette = Palette {
            colors,
            background: None,
            styled: true,
        };
        palette.background = palette
            .colors()
            .settings
            .background
            .map(|c| [c.r, c.g, c.b]);
        palette
    }

    fn colors(&self) -> &Colors {
        match &self.colors {
            Painted::Bat(colors) => colors,
            Painted::User(theme) => &theme.colors,
        }
    }
}

/// A color kept as a run's is (`Run::color`), to draw with.
pub fn color([r, g, b]: [u8; 3]) -> Hsla {
    gpui::rgb((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)).into()
}

/// The one syntax highlighter, for the code view and for a document's code
/// blocks alike. It runs on the worker that builds the `Display` and keeps
/// the grammar's state from line to line (a multiline string, a comment).
pub(crate) struct Highlighter<'a> {
    syntaxes: &'static SyntaxSet,
    lines: syntect::easy::HighlightLines<'a>,
    styled: bool,
}

impl<'a> Highlighter<'a> {
    pub fn new(language: Option<&str>, palette: &'a Palette) -> Self {
        let syntaxes = syntaxes();
        let syntax = language
            .and_then(|language| syntaxes.find_syntax_by_token(language))
            .unwrap_or_else(|| syntaxes.find_syntax_plain_text());
        Self {
            syntaxes,
            lines: syntect::easy::HighlightLines::new(syntax, palette.colors()),
            styled: palette.styled,
        }
    }

    /// The coloured runs of the next line, `text` without its line ending,
    /// in the line's own coordinates. The grammar needs the ending, but
    /// selection and copy use the text as it is: the extra byte never shows.
    pub fn line(&mut self, text: &str) -> Vec<Run> {
        let mut runs = Vec::new();
        if let Ok(ranges) = self
            .lines
            .highlight_line(&format!("{text}\n"), self.syntaxes)
        {
            let mut offset = 0;
            for (style, part) in ranges {
                let end = (offset + part.len()).min(text.len());
                if offset < end {
                    let styled = |font: FontStyle| self.styled && style.font_style.contains(font);
                    runs.push(Run {
                        range: offset..end,
                        color: Some([style.foreground.r, style.foreground.g, style.foreground.b]),
                        bold: styled(FontStyle::BOLD),
                        italic: styled(FontStyle::ITALIC),
                        ..Run::default()
                    });
                }
                offset += part.len();
            }
        }
        runs
    }

    /// The runs of a block of several lines, in the block's coordinates.
    pub fn text(&mut self, text: &str) -> Vec<Run> {
        let mut runs = Vec::new();
        let mut offset = 0;
        for line in text.split('\n') {
            runs.extend(self.line(line).into_iter().map(|mut run| {
                run.range = run.range.start + offset..run.range.end + offset;
                run
            }));
            offset += line.len() + 1;
        }
        runs
    }
}

/// Highlight the code view: one block per line, parser state kept between
/// them, and the addresses each line writes found as links, followed with
/// Ctrl (`Run::found`). Runs on the preview conversion worker, before any UI
/// rows exist.
pub(crate) fn highlight(data: &mut Inspection, palette: &Palette) {
    let language = data.blocks.first().and_then(|b| b.language.clone());
    let mut highlighter = Highlighter::new(language.as_deref(), palette);
    for (line, block) in data.blocks.iter_mut().enumerate() {
        // A preview moved on from is not colored further: its answer is
        // not taken.
        if line % 256 == 0 && bibli_core::scheduler::job_cancelled() {
            return;
        }
        block.runs = highlighter.line(&block.text);
        bibli_platform::links::link_block(block, true);
    }
}

pub fn visible(data: &Inspection, collapsed: &BTreeSet<usize>) -> Vec<usize> {
    let mut hidden_until = 0;
    let mut folds = data.folds.iter().peekable();
    (0..data.blocks.len())
        .filter(|&line| {
            while folds.peek().is_some_and(|fold| fold.start < line) {
                folds.next();
            }
            if line < hidden_until {
                return false;
            }
            if let Some(fold) = folds.peek().filter(|fold| fold.start == line)
                && collapsed.contains(&line)
            {
                hidden_until = fold.end.min(data.blocks.len());
            }
            true
        })
        .collect()
}

pub struct CodeRow<'a> {
    pub data: &'a Inspection,
    pub line: usize,
    pub collapsed: &'a BTreeSet<usize>,
    pub selected: Option<usize>,
    pub selection: Option<Selection>,
    pub font_size: f32,
    pub mono: &'a gpui::SharedString,
    pub theme: &'a Theme,
    /// The inspector's style for this frame, built once, not per row.
    pub style: &'a crate::markdown::MarkdownStyle,
    pub pane: PaneId,
    pub entity: WeakEntity<Shell>,
}
impl CodeRow<'_> {
    pub fn render(self) -> AnyElement {
        let Self {
            data,
            line,
            collapsed,
            selected,
            selection,
            font_size,
            mono,
            theme,
            style,
            pane,
            entity,
        } = self;
        let fold = data
            .folds
            .binary_search_by_key(&line, |fold| fold.start)
            .ok()
            .map(|i| &data.folds[i]);
        let folded = collapsed.contains(&line);
        let mut gutter = div()
            .id(("code-fold", line))
            .flex_none()
            .flex()
            .items_center()
            .w(px(68.))
            .pr_2()
            .gap_1()
            .text_color(theme.text_muted)
            .child(div().w(px(42.)).text_right().child((line + 1).to_string()))
            .child(div().w(px(14.)).child(if fold.is_some() {
                if folded { "▸" } else { "▾" }
            } else {
                ""
            }));
        if fold.is_some() {
            let weak = entity.clone();
            gutter = gutter
                .cursor_pointer()
                .hover(|d| d.bg(theme.hover))
                .on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |s, cx| {
                        s.with_pane(pane, |s| {
                            s.run(
                                "ui.inspect",
                                serde_json::json!({"action":"collapse", "index":line}),
                                window,
                                cx,
                            );
                        });
                    });
                });
        }
        div()
            .id(("code-line", line))
            .w_full()
            .flex()
            .items_start()
            .font_family(mono.clone())
            .text_size(px(font_size - 2.))
            .line_height(px(font_size + 4.))
            .when(selected == Some(line), |d| d.bg(theme.highlight))
            .child(gutter)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pr_2()
                    .child(crate::inspector_text::render(
                        &data.blocks[line],
                        line,
                        selection,
                        style,
                        pane,
                        entity,
                    )),
            )
            .when(folded && fold.is_some(), |d| {
                d.child(
                    div()
                        .flex_none()
                        .px_2()
                        .rounded_sm()
                        .bg(theme.code_bg)
                        .text_color(theme.text_muted)
                        .child(format!(
                            "… {} lines",
                            fold.map_or(0, |r| r.end - r.start - 1)
                        )),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::inspection::{Block, InspectionKind};

    #[test]
    fn nested_folds_retain_siblings_and_remember_child_state() {
        let data = Inspection {
            kind: InspectionKind::Code,
            blocks: vec![Block::default(); 10],
            folds: vec![0..7, 2..5, 7..9],
            ..Inspection::default()
        };
        assert_eq!(
            visible(&data, &BTreeSet::from([2])),
            [0, 1, 2, 5, 6, 7, 8, 9]
        );
        assert_eq!(visible(&data, &BTreeSet::from([0, 2])), [0, 7, 8, 9]);
        assert_eq!(visible(&data, &BTreeSet::from([0, 2, 7])), [0, 7, 9]);
        assert_eq!(
            visible(&data, &BTreeSet::new()),
            (0..10).collect::<Vec<_>>()
        );
    }

    #[test]
    fn everyday_languages_are_highlighted_by_extension_or_name() {
        for (language, line) in [
            ("ts", "const answer: number = 42;"),
            ("tsx", "const answer: number = 42;"),
            ("nix", "{ pkgs ? import <nixpkgs> {} }: pkgs.hello"),
            ("Dockerfile", "FROM alpine:3 AS build"),
            ("kt", "val answer: Int = 42"),
            ("ini", "name = value ; a note"),
            ("typescript", "const answer: number = 42;"),
        ] {
            let palette = Palette::of(&CodeTheme::own(false));
            let runs = Highlighter::new(Some(language), &palette).line(line);
            let colors: BTreeSet<_> = runs.iter().filter_map(|run| run.color).collect();
            assert!(colors.len() > 1, "{language} is highlighted: {runs:?}");
        }
    }

    #[test]
    fn toml_highlight_keeps_multiline_state_in_both_themes() {
        for dark in [false, true] {
            let mut data = Inspection {
                blocks: [
                    "text = \"\"\"",
                    "# still a string",
                    "\"\"\"",
                    "# a comment",
                    "number = 42",
                ]
                .into_iter()
                .map(|text| Block {
                    text: text.into(),
                    language: Some("toml".into()),
                    ..Block::default()
                })
                .collect(),
                ..Inspection::default()
            };
            highlight(&mut data, &Palette::of(&CodeTheme::own(dark)));
            assert!(data.blocks.iter().all(|b| !b.runs.is_empty()));
            assert_ne!(data.blocks[1].runs[0].color, data.blocks[3].runs[0].color);
            assert!(
                data.blocks[4]
                    .runs
                    .windows(2)
                    .any(|runs| runs[0].color != runs[1].color)
            );
            for block in data.blocks {
                assert!(
                    block
                        .runs
                        .iter()
                        .all(|run| run.range.end <= block.text.len())
                );
            }
        }
    }

    /// A theme as bat's users write them, its background `background`.
    fn tm_theme(background: &str, comment: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>name</key><string>Synthetic</string>
<key>settings</key><array>
<dict><key>settings</key><dict>
<key>background</key><string>{background}</string>
<key>foreground</key><string>#808080</string>
</dict></dict>
<dict><key>scope</key><string>comment</string><key>settings</key><dict>
<key>foreground</key><string>{comment}</string>
<key>fontStyle</key><string>italic</string>
</dict></dict>
</array></dict></plist>"#
        )
    }

    #[test]
    fn bat_s_themes_are_offered_to_the_look_their_background_is_for() {
        for (name, label, dark) in BAT {
            assert_eq!(is_dark(bat_themes().get(name)), dark, "{label}");
        }
        // A theme a new bat brings is offered, or left out with the
        // terminal's.
        for name in EmbeddedLazyThemeSet::theme_names() {
            assert!(
                BAT.iter().any(|(bat, ..)| bat == name)
                    || [Bat::Ansi, Bat::Base16, Bat::Base16_256].contains(name),
                "{name:?}"
            );
        }
        let light = choices(false, DEFAULT, Some(&[]));
        assert_eq!(light[0], (DEFAULT.to_owned(), "Default".to_owned()));
        assert!(light.contains(&("Solarized (light)".to_owned(), "Solarized".to_owned())));
        assert!(!light.iter().any(|(value, _)| value == "Dracula"));
        let dark = choices(true, DEFAULT, Some(&[]));
        assert!(dark.contains(&("Solarized (dark)".to_owned(), "Solarized".to_owned())));
        assert!(dark.contains(&("Dracula".to_owned(), "Dracula".to_owned())));
    }

    #[test]
    fn default_keeps_the_colors_bibli_drew_code_in_before() {
        // syntect's own base16 Ocean, which bibli drew with before it
        // offered bat's themes.
        let before = ThemeSet::load_defaults();
        let markdown = syntaxes().find_syntax_by_token("md").unwrap();
        for (dark, name) in [(false, "base16-ocean.light"), (true, "base16-ocean.dark")] {
            let palette = Palette::of(&CodeTheme::own(dark));
            assert_eq!(palette.background, None, "drawn on bibli's background");
            let text = "# Title\n**bold** and _italic_ `code`";
            let ours = Highlighter::new(Some("md"), &palette).text(text);
            let mut theirs = syntect::easy::HighlightLines::new(markdown, &before.themes[name]);
            let colors: Vec<_> = text
                .split('\n')
                .flat_map(|line| {
                    theirs
                        .highlight_line(&format!("{line}\n"), syntaxes())
                        .unwrap()
                        .into_iter()
                        .filter(|(_, part)| *part != "\n")
                        .map(|(style, _)| {
                            [style.foreground.r, style.foreground.g, style.foreground.b]
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            assert_eq!(
                ours.iter()
                    .map(|run| run.color.unwrap())
                    .collect::<Vec<_>>(),
                colors
            );
            assert!(
                ours.iter()
                    .all(|run| !run.bold && !run.italic && !run.underline)
            );
        }
    }

    #[test]
    fn a_theme_chosen_brings_its_background_and_its_font_styles() {
        let theme = |dark, name: &str| {
            Palette::of(&CodeTheme {
                dark,
                name: name.into(),
            })
        };
        assert_eq!(theme(true, "Dracula").background, Some([0x28, 0x2a, 0x36]));
        let styles = |palette: &Palette| {
            let runs = Highlighter::new(Some("md"), palette).text("**bold** and _italic_");
            (
                runs.iter().any(|run| run.bold),
                runs.iter().any(|run| run.italic),
            )
        };
        assert_eq!(styles(&theme(false, DEFAULT)), (false, false));
        assert_eq!(styles(&theme(false, "Catppuccin Latte")), (true, true));
        // Dracula underlines a path's modules; in bibli an underline is a
        // link's.
        let dracula = theme(true, "Dracula");
        let runs = Highlighter::new(Some("rs"), &dracula).line("use std::collections::HashMap;");
        assert!(runs.iter().all(|run| !run.underline), "{runs:?}");
        // A name no theme has any more is drawn in bibli's own colors.
        assert_eq!(theme(true, "Taken out of bat's folder").background, None);
    }

    #[test]
    fn the_themes_added_to_bat_are_named_by_their_file_and_offered_to_their_look() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir(dir.join("more")).unwrap();
        std::fs::write(dir.join("Paper.tmTheme"), tm_theme("#FFFFFF", "#AA0000")).unwrap();
        std::fs::write(
            dir.join("more/Night Owl's.tmTheme"),
            tm_theme("#011627", "#00AA00"),
        )
        .unwrap();
        // A light theme of the user's under bat's name for a dark one takes
        // its place, as in bat.
        std::fs::write(dir.join("Dracula.tmTheme"), tm_theme("#F8F8F2", "#0000AA")).unwrap();
        std::fs::write(dir.join("broken.tmTheme"), "not a theme").unwrap();
        std::fs::write(dir.join("notes.txt"), tm_theme("#FFFFFF", "#AA0000")).unwrap();
        let user = read_user_themes(dir);
        assert_eq!(
            user.iter()
                .map(|theme| (theme.name.as_str(), theme.dark))
                .collect::<Vec<_>>(),
            [("Dracula", false), ("Night Owl's", true), ("Paper", false)]
        );
        let light = choices(false, DEFAULT, Some(&user));
        assert!(light.ends_with(&[
            ("Dracula".to_owned(), "Dracula".to_owned()),
            ("Paper".to_owned(), "Paper".to_owned()),
        ]));
        let dark = choices(true, DEFAULT, Some(&user));
        assert!(dark.contains(&("Night Owl's".to_owned(), "Night Owl's".to_owned())));
        assert!(!dark.iter().any(|(value, _)| value == "Dracula"));
        // The theme it draws with is the file's, its comments italic.
        let night = Palette {
            colors: Painted::User(Arc::clone(&user[1])),
            background: None,
            styled: true,
        };
        let runs = Highlighter::new(Some("sh"), &night).line("# a comment");
        assert_eq!(runs[0].color, Some([0x00, 0xaa, 0x00]));
        assert!(runs[0].italic);
        // The one chosen stays a choice, said missing once the user's
        // themes are read and none has its name.
        assert_eq!(
            choices(true, "Gone", Some(&user)).last(),
            Some(&("Gone".to_owned(), "Gone (not found)".to_owned()))
        );
        assert_eq!(
            choices(true, "Gone", None).last(),
            Some(&("Gone".to_owned(), "Gone".to_owned()))
        );
    }
}
