//! What colors code: bat's grammars and themes (two-face), matched by
//! Ferroni through syntect (ADR-060), for the code view, a document's code
//! blocks and whatever else draws or prints code. The window draws what
//! it colors (`skwir-ui`'s `code_preview`); a plugin colors with it too. A
//! tree-sitter grammar colors a language bat does not know, or every one
//! it knows where the settings put it first (`tree`, ADR-110), in the
//! same themes.
//!
//! Code is colored in the look the Reading page keeps it in
//! (`reading.code_appearance`), in skwir's own colors unless it chooses a
//! theme for that look (`CodeTheme`): base16 Ocean's, on that look's
//! background. A theme chosen, one of bat's or one its user added to bat
//! (`bat::themes_dir`), brings the background it is meant for, and its
//! bold and italic: not its underline, which in skwir is a link's. Beside
//! bat's, skwir carries the themes of its palettes bat lacks (`CARRIED`).
use parking_lot::{Condvar, Mutex};
use skwir_core::inspection::{Inspection, Run};
use std::path::Path;
use std::sync::{Arc, RwLock, Weak};
use std::time::{Duration, Instant};
use syntect::highlighting::{Color, FontStyle, Theme as Colors, ThemeSet};
use syntect::parsing::SyntaxSet;
use two_face::theme::EmbeddedThemeName as Bat;

pub mod added;
pub mod tree;

pub use tree::{CHOSEN_KEY, Choice, ENGINE_KEY, Engine, First, GrammarChoices, engine};

/// Whether code is drawn light or dark: Automatic is as skwir is
/// (`CodeTheme::chosen`).
pub const APPEARANCE_KEY: &str = "reading.code_appearance";
/// The colors code is drawn in when it is light, and when it is dark
/// (`CodeTheme`).
pub const LIGHT_KEY: &str = "reading.code_light";
pub const DARK_KEY: &str = "reading.code_dark";

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
    /// Whether a thread reads the set, outside the lock: others wait for
    /// it (`READ`) rather than read it too.
    reading: bool,
}

static GRAMMARS: Mutex<Grammars> = Mutex::new(Grammars {
    shared: Weak::new(),
    kept: None,
    last_use: None,
    waiting: false,
    reading: false,
});

/// Told when a set read has been shared, or its reading failed.
static READ: Condvar = Condvar::new();

/// bat's grammars, syntect's defaults among them, with those the user
/// added to bat and Sublime Text (`added`), matched by Ferroni: each
/// grammar compiled when a file first needs it, all of them let go of once
/// no code was colored for `GRAMMARS_KEPT`, and read again when code next
/// is. Hold the set only while coloring, never in a static. Reading it
/// reads files and may build it (`added`): a worker's work, never the
/// window's; what bat knows without coloring is `added::table`.
pub fn syntaxes() -> Arc<SyntaxSet> {
    let set = shared_set();
    let mut grammars = GRAMMARS.lock();
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

/// The set shared now, else read: by this thread outside the lock, files
/// read and a set built there holding no one else up, or by the thread
/// that reads it already, waited for.
fn shared_set() -> Arc<SyntaxSet> {
    let mut grammars = GRAMMARS.lock();
    loop {
        if let Some(set) = grammars.shared.upgrade() {
            return set;
        }
        if !grammars.reading {
            break;
        }
        READ.wait(&mut grammars);
    }
    grammars.reading = true;
    drop(grammars);
    let reading = Reading;
    let set = Arc::new(added::set());
    {
        let mut grammars = GRAMMARS.lock();
        grammars.shared = Arc::downgrade(&set);
        // Kept until the caller keeps it, so a thread woken finds it.
        grammars.kept = Some(Arc::clone(&set));
    }
    drop(reading);
    set
}

/// A set read by this thread (`shared_set`): said over however the
/// reading ends, a panic included.
struct Reading;

impl Drop for Reading {
    fn drop(&mut self) {
        GRAMMARS.lock().reading = false;
        READ.notify_all();
    }
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

/// How long the code themes read stay after one was last asked for (a
/// file about to be colored): then they are let go of, as the grammars
/// are (ADR-060). Reading one again takes well under a millisecond.
const THEMES_KEPT: Duration = GRAMMARS_KEPT;

/// The code themes read so far, bat's and those skwir carries, by the
/// name stored for them: kept until none was asked for `THEMES_KEPT`. A
/// palette holds the one it paints with.
struct Themes {
    kept: std::collections::BTreeMap<&'static str, Arc<Colors>>,
    last_use: Option<Instant>,
    /// Whether a thread waits to let go of `kept`.
    waiting: bool,
}

static THEMES: Mutex<Themes> = Mutex::new(Themes {
    kept: std::collections::BTreeMap::new(),
    last_use: None,
    waiting: false,
});

/// The theme stored as `name`, kept, else read by `read` and kept.
fn kept_theme(name: &'static str, read: impl FnOnce() -> Option<Colors>) -> Option<Arc<Colors>> {
    {
        let mut themes = THEMES.lock();
        themes.last_use = Some(Instant::now());
        if !themes.waiting {
            themes.waiting = std::thread::Builder::new()
                .name("skwir-code-themes".into())
                .spawn(let_go_of_themes)
                .is_ok();
        }
        if let Some(colors) = themes.kept.get(name) {
            return Some(Arc::clone(colors));
        }
    }
    // Read outside the lock: two asking at once may each read it.
    let colors = Arc::new(read()?);
    THEMES.lock().kept.insert(name, Arc::clone(&colors));
    Some(colors)
}

/// bat's theme `name`, unpacked from bat's set, which is read for it
/// (some 60 KB) and let go of.
fn bat_theme(name: Bat) -> Arc<Colors> {
    let read = || two_face::theme::extra().get(name).clone();
    kept_theme(name.as_name(), || Some(read())).unwrap_or_else(|| Arc::new(read()))
}

/// Wait until no code theme was asked for `THEMES_KEPT`, let go of them,
/// and give back what they took.
fn let_go_of_themes() {
    while let Some(wait) = let_go_of_themes_if_idle(Instant::now()) {
        std::thread::sleep(wait);
    }
    // Workers read them: what is freed here stays with each worker until
    // it releases.
    skwir_core::scheduler::release_on_workers();
    skwir_core::scheduler::release_now();
}

/// Let go of the code themes kept if none was asked for `THEMES_KEPT` at
/// `now`; else how long until they may be.
fn let_go_of_themes_if_idle(now: Instant) -> Option<Duration> {
    let mut themes = THEMES.lock();
    let due = themes.last_use.map_or(now, |at| at + THEMES_KEPT);
    if due > now {
        return Some(due - now);
    }
    let kept = std::mem::take(&mut themes.kept);
    themes.waiting = false;
    drop(themes);
    drop(kept);
    None
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

/// The code themes skwir carries: those of its palettes bat does not ship
/// (ADR-071, amended 2026-11-05; `assets/code-themes`, each under its
/// licence there), each its makers' own. The value stored (named as bat
/// would name the file, so a theme the user added to bat under that name
/// replaces it), the name shown, whether it is dark, and the file, read
/// the first time it is drawn.
const CARRIED: [(&str, &str, bool, &[u8]); 6] = [
    (
        "everforest-dark",
        "Everforest",
        true,
        include_bytes!("../assets/code-themes/everforest-dark.tmTheme"),
    ),
    (
        "everforest-light",
        "Everforest",
        false,
        include_bytes!("../assets/code-themes/everforest-light.tmTheme"),
    ),
    (
        "rose-pine",
        "Rosé Pine",
        true,
        include_bytes!("../assets/code-themes/rose-pine.tmTheme"),
    ),
    (
        "rose-pine-dawn",
        "Rosé Pine Dawn",
        false,
        include_bytes!("../assets/code-themes/rose-pine-dawn.tmTheme"),
    ),
    (
        "tokyonight_day",
        "Tokyo Night Day",
        false,
        include_bytes!("../assets/code-themes/tokyonight_day.tmTheme"),
    ),
    (
        "tokyonight_night",
        "Tokyo Night",
        true,
        include_bytes!("../assets/code-themes/tokyonight_night.tmTheme"),
    ),
];

/// The theme of `CARRIED` named `name`, read when asked unless kept
/// (`kept_theme`); `None` for another name, or for one that does not
/// read (a test holds them all to).
fn carried(name: &str) -> Option<Arc<Colors>> {
    let (value, _, _, bytes) = CARRIED.iter().find(|(value, ..)| *value == name)?;
    kept_theme(value, || {
        ThemeSet::load_from_reader(&mut std::io::Cursor::new(*bytes)).ok()
    })
}

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
    /// What colors each language (`reading.grammars`, and each language's
    /// own choice, `reading.grammar_for`): the colors change with it too.
    pub grammars: GrammarChoices,
}

impl CodeTheme {
    /// skwir's own colors, for a look.
    pub fn own(dark: bool) -> Self {
        Self {
            dark,
            name: DEFAULT.into(),
            grammars: GrammarChoices::default(),
        }
    }

    /// The theme the settings choose while skwir is dark, or light: the
    /// look `reading.code_appearance` keeps code in, Automatic following
    /// skwir's, the theme chosen for that look, and the grammars chosen
    /// first.
    pub fn chosen(app: &skwir_core::App, skwir_dark: bool) -> Self {
        let dark = match app.settings.get::<String>(APPEARANCE_KEY).as_deref() {
            Some("light") => false,
            Some("dark") => true,
            _ => skwir_dark,
        };
        let key = if dark { DARK_KEY } else { LIGHT_KEY };
        Self {
            name: app
                .settings
                .get::<String>(key)
                .map_or_else(|| DEFAULT.into(), Into::into),
            grammars: GrammarChoices::chosen(app),
            ..Self::own(dark)
        }
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
    let themes: Arc<[_]> = crate::bat::themes_dir()
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
            let bytes = crate::file::read_regular(&path, LIMIT).ok()?;
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
        .map(|(name, label, theme_dark)| (name.as_name(), *label, *theme_dark))
        .chain(
            CARRIED
                .iter()
                .map(|(name, label, theme_dark, _)| (*name, *label, *theme_dark)),
        )
        .filter(|(name, _, theme_dark)| *theme_dark == dark && !theirs(name))
        .map(|(name, label, _)| (name.to_owned(), label.to_owned()))
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
pub struct Palette {
    colors: Painted,
    /// The theme's background, as a run's color is kept (`Run::color`);
    /// `None` draws on skwir's own.
    pub background: Option<[u8; 3]>,
    /// Whether its font styles are drawn.
    styled: bool,
}

enum Painted {
    /// bat's, or one skwir carries (`CARRIED`), as kept (`kept_theme`).
    Bat(Arc<Colors>),
    User(Arc<UserTheme>),
}

impl Palette {
    /// The palette `code` names: a theme the user added to bat, then one
    /// of bat's, as bat looks them up; skwir's own colors when none has
    /// its name (a file taken out of bat's folder). It may read the
    /// user's themes: a worker's work.
    pub fn of(code: &CodeTheme) -> Self {
        let own = || Palette {
            colors: Painted::Bat(bat_theme(if code.dark {
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
            Painted::Bat(bat_theme(*name))
        } else if let Some(colors) = carried(&code.name) {
            Painted::Bat(colors)
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
            Painted::Bat(colors) => colors.as_ref(),
            Painted::User(theme) => &theme.colors,
        }
    }
}

/// What the code view colors the file at `path` of type `mime` as, a token
/// as [`Highlighter`] takes it: the name's extension, or the name where it
/// has none. Most languages have no signature, so the name stays the first
/// word: a grammar that knows it colors the file. A name none knows says
/// nothing of how the file reads, and its type speaks instead, the bytes
/// having settled it or the database knowing the name
/// (`language_of_type`: a `.ofx` that declares itself XML, a script without
/// an extension, a `.geojson`). May read what bat's set holds: a worker's
/// work.
pub fn language_of(path: &Path, mime: Option<&str>) -> Option<String> {
    let token = path
        .extension()
        .or_else(|| path.file_name())?
        .to_string_lossy()
        .into_owned();
    let by_type = mime
        .filter(|_| !grammar_knows(path, &token))
        .and_then(|mime| language_of_type(&crate::mime::Mime(mime.to_owned())));
    Some(by_type.unwrap_or(token))
}

/// What a file of type `mime` is colored as where its name says nothing.
/// What descends from XML is XML, before any extension of its own: SMIL's
/// `.sml` is also Standard ML's. Else the nearest type it descends from
/// that is text and has an extension a grammar colors and the database
/// gives to that very type: `.t` is Perl's, for all that troff claims it.
fn language_of_type(mime: &crate::mime::Mime) -> Option<String> {
    use crate::mime::{Kind, by_name, extensions, is_kind, is_xml, lineage};
    if is_xml(mime) {
        return Some("xml".to_owned());
    }
    let table = added::table_now();
    lineage(mime)
        .iter()
        .filter(|kin| is_kind(kin, Kind::Text))
        .find_map(|kin| {
            extensions(kin).into_iter().find(|extension| {
                table.colors(extension).is_some()
                    && by_name(std::ffi::OsStr::new(&format!("file.{extension}"))).as_ref()
                        == Some(kin)
            })
        })
}

/// Whether a grammar colors the file at `path` by its name: bat's, for
/// `token`, or a tree-sitter grammar found for it, as `engine` asks them.
fn grammar_knows(path: &Path, token: &str) -> bool {
    added::table_now().colors(token).is_some()
        || path.file_name().is_some_and(|name| {
            let found = crate::grammars::all().unwrap_or_default();
            crate::grammars::for_file(&found, &name.to_string_lossy(), None).is_some()
        })
}

/// The one syntax highlighter, for the code view and for a document's code
/// blocks alike. It runs on the worker that builds the `Display` and keeps
/// the grammar's state from line to line (a multiline string, a comment).
pub struct Highlighter<'a> {
    syntaxes: Arc<SyntaxSet>,
    lines: syntect::easy::HighlightLines<'a>,
    styled: bool,
}

/// Where a highlighter is in a text, to go on from there later: what a
/// chunk of a text read where it is shown ends in, for the next (ADR-051).
/// It holds the grammars its parser state indexes into, so they are the
/// ones shared when it is resumed.
#[derive(Clone)]
pub struct Resume(
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

/// Highlight a code view whole: one block per line, parser state kept
/// between them, and the addresses each line writes found as links,
/// followed with Ctrl (`Run::found`). What a code view colored where it is
/// shown (`coloring::CodeColors`) must come to.
pub fn highlight(data: &mut Inspection, palette: &Palette) {
    let language = data.blocks.first().and_then(|b| b.language.clone());
    let mut highlighter = Highlighter::new(language.as_deref(), palette);
    for (line, block) in data.blocks.iter_mut().enumerate() {
        // A preview moved on from is not colored further: its answer is
        // not taken.
        if line % 256 == 0 && skwir_core::scheduler::job_cancelled() {
            return;
        }
        block.runs = highlighter.line(&block.text);
        crate::links::link_block(block, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::inspection::Block;
    use std::collections::BTreeSet;

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
    fn code_themes_are_let_go_of_once_unused_and_read_again() {
        let first = carried("rose-pine").unwrap();
        assert!(Arc::ptr_eq(&first, &carried("rose-pine").unwrap()), "kept");
        let ocean = bat_theme(Bat::Base16OceanDark);
        assert!(Arc::ptr_eq(&ocean, &bat_theme(Bat::Base16OceanDark)));
        assert!(let_go_of_themes_if_idle(Instant::now()).is_some());
        // Unused for `THEMES_KEPT`: let go of; a palette still holds its own.
        assert_eq!(let_go_of_themes_if_idle(Instant::now() + THEMES_KEPT), None);
        let again = carried("rose-pine").unwrap();
        assert!(!Arc::ptr_eq(&first, &again), "read again");
        assert_eq!(first.settings.background, again.settings.background);
        assert!(!Arc::ptr_eq(&ocean, &bat_theme(Bat::Base16OceanDark)));
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

    /// Threads asking at once while none is shared wait for the one that
    /// reads the set, and share it.
    #[test]
    fn threads_asking_at_once_share_one_set() {
        assert_eq!(let_go_if_idle(Instant::now() + GRAMMARS_KEPT), None);
        let sets: Vec<Arc<SyntaxSet>> = std::thread::scope(|scope| {
            let asked: Vec<_> = (0..8).map(|_| scope.spawn(syntaxes)).collect();
            asked.into_iter().map(|one| one.join().unwrap()).collect()
        });
        assert!(sets.windows(2).all(|two| Arc::ptr_eq(&two[0], &two[1])));
        assert!(!GRAMMARS.lock().reading);
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
            assert_eq!(is_dark(&bat_theme(name)), dark, "{label}");
        }
        // A theme a new bat brings is offered, or left out with the
        // terminal's.
        for name in two_face::theme::EmbeddedLazyThemeSet::theme_names() {
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
    fn the_themes_skwir_carries_read_and_are_offered_to_their_look() {
        for (name, label, dark, _) in CARRIED {
            let colors = carried(name).unwrap_or_else(|| panic!("{name} reads"));
            assert_eq!(is_dark(&colors), dark, "{label}");
            assert!(!BAT.iter().any(|(bat, ..)| bat.as_name() == name), "{name}");
            assert!(
                choices(dark, DEFAULT, Some(&[])).contains(&(name.to_owned(), label.to_owned())),
                "{name}"
            );
            let palette = Palette::of(&CodeTheme {
                name: name.into(),
                ..CodeTheme::own(dark)
            });
            assert!(palette.background.is_some(), "{name}: its own ground");
            assert!(
                !Highlighter::new(Some("rs"), &palette)
                    .line("fn main() {}")
                    .is_empty()
            );
        }
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
                name: name.into(),
                ..CodeTheme::own(dark)
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
