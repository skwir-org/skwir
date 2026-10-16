//! Virtual source lines. Fold coordinates stay in the provider's logical lines.
//!
//! Code is colored with bat's grammars and themes, in skwir's look or the
//! one the Reading page keeps it in (`reading.code_appearance`). Unless
//! the page chooses a theme for that look (`CodeTheme`), the colors are
//! skwir's own: base16 Ocean's, on that look's background. A theme
//! chosen, one of bat's or one its user added to bat (`bat::themes_dir`),
//! brings the background it is meant for, and its bold and italic: not
//! its underline, which in skwir is a link's. Code in a look that is not
//! skwir's is drawn with that look's tokens (`Theme::in_look`).
use crate::{inspector_text::Selection, shell::Shell, theme::Theme};
use gpui::{AnyElement, Hsla, WeakEntity, div, prelude::*, px};
use parking_lot::Mutex;
use skwir_core::{
    inspection::{Block, Inspection, Run},
    workspace::PaneId,
};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock, Weak};
use std::time::{Duration, Instant};
use syntect::highlighting::{Color, FontStyle, Theme as Colors, ThemeSet};
use syntect::parsing::SyntaxSet;
use two_face::theme::{EmbeddedLazyThemeSet, EmbeddedThemeName as Bat};

/// How long bat's grammars stay compiled after code was last colored, so
/// the next file looked at finds its grammar ready. Then they are let go
/// of: some 30 MiB once twenty languages were seen. Reading them again
/// takes 2 ms, and a grammar compiles again in tens of milliseconds when
/// a file next needs it (ADR-060).
const GRAMMARS_KEPT: Duration = Duration::from_secs(60);

/// bat's grammars as compiled so far, shared by whatever colors code.
struct Grammars {
    /// The set in use. Highlighters and resume points hold it, since their
    /// parser states index into it: there is never more than one.
    shared: Weak<SyntaxSet>,
    /// The set kept after its last use, until `GRAMMARS_KEPT` has passed.
    kept: Option<Arc<SyntaxSet>>,
    last_use: Option<Instant>,
    /// Whether a thread waits to let go of `kept`.
    waiting: bool,
}

static GRAMMARS: Mutex<Grammars> = Mutex::new(Grammars {
    shared: Weak::new(),
    kept: None,
    last_use: None,
    waiting: false,
});

/// bat's grammars, syntect's defaults among them, matched by Ferroni:
/// each grammar compiled when a file first needs it, all of them let go
/// of once no code was colored for `GRAMMARS_KEPT`, and read again when
/// code next is. Hold the set only while coloring, never in a static.
pub(crate) fn syntaxes() -> Arc<SyntaxSet> {
    let mut grammars = GRAMMARS.lock();
    let set = grammars.shared.upgrade().unwrap_or_else(|| {
        let set = Arc::new(two_face::syntax::extra_newlines());
        grammars.shared = Arc::downgrade(&set);
        set
    });
    grammars.kept = Some(Arc::clone(&set));
    grammars.last_use = Some(Instant::now());
    if !grammars.waiting {
        // A thread lives only while grammars are kept. Without one, they
        // stay until the next use starts it.
        grammars.waiting = std::thread::Builder::new()
            .name("skwir-grammars".into())
            .spawn(let_go_of_grammars)
            .is_ok();
    }
    set
}

/// Wait until the grammars have been unused for `GRAMMARS_KEPT`, let go
/// of them, and give back what they took.
fn let_go_of_grammars() {
    while let Some(wait) = let_go_if_idle(Instant::now()) {
        std::thread::sleep(wait);
    }
    // Workers compiled them: what is freed here stays with each worker
    // until it releases.
    skwir_core::scheduler::release_on_workers();
    skwir_core::scheduler::release_now();
}

/// Let go of the grammars kept if unused for `GRAMMARS_KEPT` at `now`;
/// else how long until they may be. A highlighter or a resume point that
/// still holds them keeps them, and they are shared again when asked for.
fn let_go_if_idle(now: Instant) -> Option<Duration> {
    let mut grammars = GRAMMARS.lock();
    let due = grammars.last_use.map_or(now, |at| at + GRAMMARS_KEPT);
    if due > now {
        return Some(due - now);
    }
    let kept = grammars.kept.take();
    grammars.waiting = false;
    drop(grammars);
    // Thousands of compiled patterns are freed outside the lock.
    drop(kept);
    None
}

/// bat's themes: read once, each unpacked when it is first drawn.
fn bat_themes() -> &'static EmbeddedLazyThemeSet {
    static SET: OnceLock<EmbeddedLazyThemeSet> = OnceLock::new();
    SET.get_or_init(two_face::theme::extra)
}

/// The choice of `reading.code_light` and `reading.code_dark` that keeps
/// skwir's own colors.
pub const DEFAULT: &str = "default";

/// bat's themes skwir offers: bat's name for each (what `bat --theme`
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

/// The colors code is drawn in and the look they are for, as the
/// settings choose them while skwir has its look (`Theme::code`, read
/// with the theme). A preview is highlighted with it, so a new choice
/// shows the file again (`preview::Version`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeTheme {
    /// Whether code is drawn dark: skwir's look, unless
    /// `reading.code_appearance` keeps code in one.
    pub dark: bool,
    /// bat's name for the theme, the name of a theme the user added to
    /// bat, or [`DEFAULT`].
    pub name: Arc<str>,
}

impl CodeTheme {
    /// skwir's own colors, for a look.
    pub fn own(dark: bool) -> Self {
        Self {
            dark,
            name: DEFAULT.into(),
        }
    }

    /// The theme the settings choose while skwir is dark, or light: the
    /// look `reading.code_appearance` keeps code in, Automatic following
    /// skwir's, and the theme chosen for that look.
    pub fn chosen(app: &skwir_core::App, skwir_dark: bool) -> Self {
        let dark = match app
            .settings
            .get::<String>(crate::settings::CODE_APPEARANCE_KEY)
            .as_deref()
        {
            Some("light") => false,
            Some("dark") => true,
            _ => skwir_dark,
        };
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
    let themes: Arc<[_]> = skwir_platform::bat::themes_dir()
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
            let bytes = skwir_platform::file::read_regular(&path, LIMIT).ok()?;
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

/// The choices of a look's setting, `(value, label)`: skwir's own colors,
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
/// skwir's own, the background they are meant for and their bold and
/// italic.
pub(crate) struct Palette {
    colors: Painted,
    /// The theme's background, as a run's color is kept (`Run::color`);
    /// `None` draws on skwir's own.
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
    /// of bat's, as bat looks them up; skwir's own colors when none has
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
    syntaxes: Arc<SyntaxSet>,
    lines: syntect::easy::HighlightLines<'a>,
    styled: bool,
}

/// Where a highlighter is in a text, to go on from there later: what a
/// chunk of a text read where it is shown ends in, for the next (ADR-051).
/// It holds the grammars its parser state indexes into, so they are the
/// ones shared when it is resumed.
#[derive(Clone)]
pub(crate) struct Resume(
    syntect::highlighting::HighlightState,
    syntect::parsing::ParseState,
    Arc<SyntaxSet>,
);

impl<'a> Highlighter<'a> {
    pub fn new(language: Option<&str>, palette: &'a Palette) -> Self {
        Self::resume(language, palette, None)
    }

    /// One that goes on from `from`, where another stopped; from the
    /// start of the text without.
    pub fn resume(language: Option<&str>, palette: &'a Palette, from: Option<Resume>) -> Self {
        let syntaxes = syntaxes();
        let lines = if let Some(Resume(highlight, parse, held)) = from {
            debug_assert!(
                Arc::ptr_eq(&held, &syntaxes),
                "one set of grammars at a time"
            );
            syntect::easy::HighlightLines::from_state(palette.colors(), highlight, parse)
        } else {
            let syntax = language
                .and_then(|language| syntaxes.find_syntax_by_token(language))
                .unwrap_or_else(|| syntaxes.find_syntax_plain_text());
            syntect::easy::HighlightLines::new(syntax, palette.colors())
        };
        Self {
            syntaxes,
            lines,
            styled: palette.styled,
        }
    }

    /// Where it is, to resume from.
    pub fn stop(self) -> Resume {
        let (highlight, parse) = self.lines.state();
        Resume(highlight, parse, self.syntaxes)
    }

    /// The coloured runs of the next line, `text` without its line ending,
    /// in the line's own coordinates. The grammar needs the ending, but
    /// selection and copy use the text as it is: the extra byte never shows.
    pub fn line(&mut self, text: &str) -> Vec<Run> {
        self.row(text, true)
    }

    /// The runs of the next row of a text: a line, or a part of one that
    /// the next row goes on with (`ends_line` false), given no ending.
    pub fn row(&mut self, text: &str, ends_line: bool) -> Vec<Run> {
        let mut runs = Vec::new();
        let ending = if ends_line { "\n" } else { "" };
        if let Ok(ranges) = self
            .lines
            .highlight_line(&format!("{text}{ending}"), &self.syntaxes)
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
        if line % 256 == 0 && skwir_core::scheduler::job_cancelled() {
            return;
        }
        block.runs = highlighter.line(&block.text);
        skwir_platform::links::link_block(block, true);
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

/// A row of the code view: a line of code, or a row of a text read where
/// it is shown (ADR-051), its number in the gutter.
pub struct CodeRow<'a> {
    pub block: &'a Block,
    /// Its index among the rows: what selection, links and ids count.
    pub index: usize,
    /// The line number in the gutter (from 1), on a line's first row.
    pub number: Option<u64>,
    /// How many digits the gutter holds: the most any row of it needs.
    pub digits: usize,
    /// The fold it opens, and whether it is closed.
    pub fold: Option<(&'a std::ops::Range<usize>, bool)>,
    /// How far its text is scrolled sideways, when lines do not wrap: the
    /// gutter stays.
    pub shift: f32,
    pub selected: bool,
    pub selection: Option<Selection>,
    pub font_size: f32,
    pub mono: &'a gpui::SharedString,
    pub theme: &'a Theme,
    /// The inspector's style for this frame, built once, not per row.
    pub style: &'a crate::markdown::MarkdownStyle,
    pub pane: PaneId,
    pub entity: WeakEntity<Shell>,
}

/// The digits a gutter holds for `rows` lines: five at least, so it does
/// not widen as a short text's count grows.
pub fn digits(rows: u64) -> usize {
    rows.max(1).ilog10() as usize + 1
}

impl CodeRow<'_> {
    pub fn render(self) -> AnyElement {
        let Self {
            block,
            index,
            number,
            digits,
            fold,
            shift,
            selected,
            selection,
            font_size,
            mono,
            theme,
            style,
            pane,
            entity,
        } = self;
        let folded = fold.is_some_and(|(_, folded)| folded);
        // A digit of the monospaced font is about 0.6 of its size.
        let numbers = (digits.max(5) as f32 * (font_size - 2.) * 0.62).ceil();
        let mut gutter = div()
            .id(("code-fold", index))
            .flex_none()
            .flex()
            .items_center()
            .w(px(numbers + 30.))
            .pl_1()
            .pr_2()
            .gap_1()
            .text_color(theme.text_muted)
            .child(
                div()
                    .w(px(numbers))
                    .text_right()
                    .children(number.map(|number| number.to_string())),
            )
            .child(div().w(px(14.)).child(match fold {
                Some((_, true)) => "▸",
                Some((_, false)) => "▾",
                None => "",
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
                                serde_json::json!({"action":"collapse", "index":index}),
                                window,
                                cx,
                            );
                        });
                    });
                });
        }
        div()
            .id(("code-line", index))
            .w_full()
            .flex()
            .items_start()
            .font_family(mono.clone())
            .text_size(px(font_size - 2.))
            .line_height(px(font_size + 4.))
            .when(selected, |d| d.bg(theme.highlight))
            .child(gutter)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pr_2()
                    .when(shift > 0., gpui::Styled::overflow_hidden)
                    // Moved, not laid out shorter: the row measures as
                    // wide however far it is scrolled, so the rows know
                    // how far they can go (`elements::rows`).
                    .child(
                        div()
                            .relative()
                            .left(px(-shift))
                            .child(crate::inspector_text::render(
                                block, index, selection, style, pane, entity,
                            )),
                    ),
            )
            .when_some(fold.filter(|_| folded), |d, (range, _)| {
                d.child(
                    div()
                        .flex_none()
                        .px_2()
                        .rounded_sm()
                        .bg(theme.code_bg)
                        .text_color(theme.text_muted)
                        .child(format!("… {} lines", range.end - range.start - 1)),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::inspection::{Block, InspectionKind};

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
            // In Oniguruma's set of bat's grammars, not in fancy-regex's.
            ("ps1", "$answer = Get-Answer -Name 'life' # a note"),
        ] {
            let palette = Palette::of(&CodeTheme::own(false));
            let runs = Highlighter::new(Some(language), &palette).line(line);
            let colors: BTreeSet<_> = runs.iter().filter_map(|run| run.color).collect();
            assert!(colors.len() > 1, "{language} is highlighted: {runs:?}");
        }
    }

    #[test]
    fn a_pattern_ferroni_refuses_never_matches_and_its_grammar_still_highlights() {
        // Vue Component's, which Oniguruma 6.9.10 takes and its last
        // source, ported by Ferroni, refuses (`\x7f-\x9f` in a class).
        // It must not panic its grammar.
        let refused = r#"(?=(?:[^ "'>/=\x00-\x1f\x7f-\x9f]))"#;
        assert!(syntect::parsing::Regex::try_compile(refused).is_some());
        // Oniguruma's own syntax, as syntect's onig backend: Ruby's would
        // refuse VimHelp's look-behind, as Oniguruma does under it.
        assert!(syntect::parsing::Regex::try_compile(r"(?<=^\s*)(vim?|ex):").is_none());
        let regex = syntect::parsing::Regex::new(refused.to_owned());
        assert!(!regex.is_match("class"));
        assert!(!regex.search("class", 0, 5, None));
        let palette = Palette::of(&CodeTheme::own(false));
        let mut vue = Highlighter::new(Some("vue"), &palette);
        let mut colors = BTreeSet::new();
        for line in [
            "<template>",
            "  <div class=\"answer\" v-if=\"ok\" @click=\"go(42)\">{{ answer }}</div>",
            "</template>",
            "<script setup lang=\"ts\">",
            "const answer: number = 42;",
            "</script>",
        ] {
            colors.extend(vue.line(line).iter().filter_map(|run| run.color));
        }
        assert!(colors.len() > 2, "Vue is highlighted: {colors:?}");
    }

    #[test]
    fn grammars_are_let_go_of_once_unused_and_read_again() {
        let grammars = Arc::downgrade(&syntaxes());
        // Used a moment ago: kept, nothing holding them but the keeper.
        assert!(let_go_if_idle(Instant::now()).is_some());
        assert!(grammars.upgrade().is_some());
        // Unused for `GRAMMARS_KEPT`: let go of, read again when asked.
        assert_eq!(let_go_if_idle(Instant::now() + GRAMMARS_KEPT), None);
        assert!(grammars.upgrade().is_none());
        let palette = Palette::of(&CodeTheme::own(false));
        let runs = Highlighter::new(Some("rs"), &palette).line("let answer = 42;");
        assert!(
            runs.iter()
                .filter_map(|run| run.color)
                .collect::<BTreeSet<_>>()
                .len()
                > 1
        );
    }

    #[test]
    fn a_resume_point_keeps_its_grammars_past_their_release() {
        let palette = Palette::of(&CodeTheme::own(false));
        let mut first = Highlighter::new(Some("toml"), &palette);
        first.line("text = \"\"\"");
        let resume = first.stop();
        let held = Arc::downgrade(&resume.2);
        assert_eq!(let_go_if_idle(Instant::now() + GRAMMARS_KEPT), None);
        assert!(held.upgrade().is_some(), "the resume point holds them");
        // The state indexes into the grammars it held: they are the ones
        // shared, and the string goes on.
        let mut again = Highlighter::resume(Some("toml"), &palette, Some(resume));
        let string = again.line("# still a string");
        let comment = Highlighter::new(Some("toml"), &palette).line("# a comment");
        assert_ne!(string[0].color, comment[0].color);
        assert!(Arc::ptr_eq(&held.upgrade().unwrap(), &syntaxes()));
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
    fn default_keeps_the_colors_skwir_drew_code_in_before() {
        // syntect's own base16 Ocean, which skwir drew with before it
        // offered bat's themes.
        let before = ThemeSet::load_defaults();
        let grammars = syntaxes();
        let markdown = grammars.find_syntax_by_token("md").unwrap();
        for (dark, name) in [(false, "base16-ocean.light"), (true, "base16-ocean.dark")] {
            let palette = Palette::of(&CodeTheme::own(dark));
            assert_eq!(palette.background, None, "drawn on skwir's background");
            let text = "# Title\n**bold** and _italic_ `code`";
            let ours = Highlighter::new(Some("md"), &palette).text(text);
            let mut theirs = syntect::easy::HighlightLines::new(markdown, &before.themes[name]);
            let colors: Vec<_> = text
                .split('\n')
                .flat_map(|line| {
                    theirs
                        .highlight_line(&format!("{line}\n"), &grammars)
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
        // Dracula underlines a path's modules; in skwir an underline is a
        // link's.
        let dracula = theme(true, "Dracula");
        let runs = Highlighter::new(Some("rs"), &dracula).line("use std::collections::HashMap;");
        assert!(runs.iter().all(|run| !run.underline), "{runs:?}");
        // A name no theme has any more is drawn in skwir's own colors.
        assert_eq!(theme(true, "Taken out of bat's folder").background, None);
    }

    /// Code is as light or dark as skwir unless the settings keep it in a
    /// look, and wears the theme chosen for the look it is in.
    #[test]
    fn code_is_in_skwir_s_look_unless_the_settings_keep_it_in_one() {
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        crate::settings::declare(&app);
        app.settings
            .set(crate::settings::CODE_LIGHT_KEY, "GitHub".into());
        app.settings
            .set(crate::settings::CODE_DARK_KEY, "Dracula".into());
        let chosen = |skwir_dark| {
            let code = CodeTheme::chosen(&app, skwir_dark);
            (code.dark, code.name.to_string())
        };
        let (light, dark) = ((false, "GitHub".to_owned()), (true, "Dracula".to_owned()));
        assert_eq!((chosen(false), chosen(true)), (light.clone(), dark.clone()));
        app.settings
            .set(crate::settings::CODE_APPEARANCE_KEY, "light".into());
        assert_eq!(
            (chosen(false), chosen(true)),
            (light.clone(), light.clone())
        );
        app.settings
            .set(crate::settings::CODE_APPEARANCE_KEY, "dark".into());
        assert_eq!((chosen(false), chosen(true)), (dark.clone(), dark));
        app.settings
            .set(crate::settings::CODE_APPEARANCE_KEY, "auto".into());
        assert_eq!(chosen(false), light);
    }

    /// Code kept in the other look is drawn with that look's tokens, its
    /// sizes and measure those of the document around it.
    #[test]
    fn code_in_the_other_look_is_drawn_with_its_tokens() {
        let dark = Theme::dark();
        assert!(matches!(dark.in_look(true), std::borrow::Cow::Borrowed(_)));
        let light = dark.in_look(false);
        assert!(!light.is_dark);
        assert_eq!(
            (light.bg, light.text),
            (Theme::light().bg, Theme::light().text)
        );
        assert_eq!(light.code, dark.code, "what code is drawn in stays");
        let document = crate::markdown::MarkdownStyle::new(&dark, 16., true, Some(42.));
        let code = document.drawn_in(&light);
        assert_eq!(
            (code.text, code.selection_bg, code.code_bg),
            (light.text, light.selection_bg, light.code_bg)
        );
        assert_eq!(
            (code.code_size, code.measure, code.line_height),
            (document.code_size, document.measure, document.line_height)
        );
        assert_eq!(document.code().text, dark.text, "no look of its own");
        let mut document = document;
        document.code_look = Some(std::rc::Rc::new(code));
        assert_eq!(document.code().text, light.text);
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
