//! Text sizes (ADR-117): each part of the window is written at a size of
//! its own, and what holds text is as tall as that text needs.
//!
//! - **Parts.** The interface (bars, menus, the sidebar, dialogs), the
//!   files (the views of a pane: names and details in the list, the grid,
//!   the columns and the board), reading (the running text of previews)
//!   and code. Each has a setting in steps (`STEPS`), a percentage of its
//!   100 %; the files' 100 % is the interface's size.
//! - **A size is a part's body text**, in pixels: its 100 % (the
//!   desktop's size, `hundreds`; Skwir's own, `Part::base`, where none is
//!   said) times the setting.
//! - **A part is drawn in a rem of its own** (`Sizes::rem`, the
//!   `elements::rem` element): gpui's `text_sm` (0.875 rem) is its body
//!   text there, and every length written in rems (the spacing scale,
//!   `p_1`, `gap_2`) follows it. gpui's other sizes are never written
//!   (`TextSizes` are).
//! - **Named sizes** (`TextSizes`): every place writes at one of them,
//!   never `text_xs`, a literal or a length. Body is the default: all
//!   text you read or act on, names, values and sentences (a tooltip, a
//!   setting's description, a message), and every control (a button, a
//!   menu's row, a choice, a field, the toolbar's labels, the footer).
//!   Small, nine tenths of the body, is only a note read after the Body
//!   text it stands beside: a shortcut or a key cap, a count or a unit,
//!   what is said under a name (a size, a date, a speed), a column's
//!   header, a menu's or the sidebar's group heading (a view's groups are
//!   headed in Body, as the names under them), a short hint beside a
//!   control, a badge
//!   (a state, a tag over a picture). Nothing is written smaller. Title is
//!   the title of a dialog or a panel. Text an
//!   element paints itself takes the same sizes from the rem in force
//!   (`painted`).
//! - **Lengths written at 100 %** (a row's 26 px, a column's width) grow
//!   with the text of where they are drawn: in rems (`sized`, laid out
//!   in the rem in force), or scaled in whole pixels (`Scale::px`) where
//!   they are counted: by the theme's scale for the part in `render`
//!   (`Sizes::scale`), by the rem in force in an element's layout
//!   (`Scale::of`), the two one scale, the part's rem over gpui's.
//!   Pictures and icons keep their size: only what holds text grows with
//!   it.

use gpui::{Pixels, Rems, Styled, Window, px, rems};
use skwir_core::App;

/// The steps a part's size is chosen in, as percentages of its 100 %.
pub const STEPS: [u32; 8] = [80, 90, 100, 110, 125, 150, 175, 200];
/// The steps of the preview's own size buttons, which go further, for the
/// file shown (`inspector.rs`).
pub const PREVIEW_STEPS: [u32; 11] = [60, 70, 80, 90, 100, 110, 125, 150, 175, 200, 250];
/// A part at its own size.
pub const DEFAULT_STEP: u32 = 100;

/// gpui's rem at 100 %, against which the spacing scale is written: its
/// `text_sm` (0.875 rem) is Skwir's 14 px body text there.
pub const REM: f32 = 16.;
/// The share of a rem that is a part's body text (gpui's `text_sm`).
const BODY_REMS: f32 = 0.875;

/// The named text sizes, as shares of the rem in force: 14, 12.6 and
/// 18 px where the body is 14 px. Small is nine tenths of the body, a
/// step the eye sees that never falls far below it (12 px at a desktop's
/// 10 pt). A smaller one, four fifths, was the badges' until 2026-11-05:
/// the user found it too small, and every badge moved to Small.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Text {
    /// All text you read or act on, controls included: the default.
    Body,
    /// A note read after the Body text it stands beside, or a badge.
    Small,
    /// A dialog's or a panel's title.
    Title,
}

impl Text {
    pub fn rems(self) -> f32 {
        match self {
            Text::Body => BODY_REMS,
            Text::Small => BODY_REMS * 0.9,
            Text::Title => 1.125,
        }
    }
}

/// The named sizes on anything styled, in the rem in force: the window's
/// (the interface's), a pane's view's (the files'), ADR-117.
pub trait TextSizes: Styled + Sized {
    /// A name or a value you read or choose (a file, a place, a tab, a
    /// menu item, a property's value), and anything written as a
    /// sentence (a tooltip, a description, a message, a hint).
    fn text_body(self) -> Self {
        self.text_size(rems(Text::Body.rems()))
    }

    /// A note beside Body text: a shortcut or a key cap, a count or a
    /// unit, what is said under a name, a column's header, a menu's or
    /// the sidebar's group heading; a badge. Never a sentence, nor a
    /// control.
    fn text_small(self) -> Self {
        self.text_size(rems(Text::Small.rems()))
    }

    /// A dialog's or a panel's title.
    fn text_title(self) -> Self {
        self.text_size(rems(Text::Title.rems()))
    }
}

impl<T: Styled> TextSizes for T {}

/// A named size in pixels, for text an element paints itself, in the rem
/// in force where it is laid out.
pub fn painted(window: &Window, text: Text) -> Pixels {
    window.rem_size() * text.rems()
}

/// A length written at 100 %, in rems: laid out, it grows with the text of
/// where it is drawn (a bar's height, a row's, the padding around text).
pub fn sized(at_100: f32) -> Rems {
    rems(at_100 / REM)
}

/// A part of the window with a text size of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// Bars, menus, the sidebar, dialogs, tooltips, footers.
    Interface,
    /// The views of a pane: names and details in the list, the grid, the
    /// columns and the board.
    Files,
    /// The running text of previews: Markdown, HTML, documents, books,
    /// mail.
    Reading,
    /// Code previews, the byte view, the terminal.
    Code,
}

impl Part {
    pub const ALL: [Part; 4] = [Part::Interface, Part::Files, Part::Reading, Part::Code];

    fn ix(self) -> usize {
        self as usize
    }

    /// The setting choosing its size.
    pub fn key(self) -> &'static str {
        match self {
            Part::Interface => crate::settings::TEXT_SIZE_KEY,
            Part::Files => crate::settings::FILES_TEXT_SIZE_KEY,
            Part::Reading => crate::settings::READING_TEXT_SIZE_KEY,
            Part::Code => crate::settings::CODE_TEXT_SIZE_KEY,
        }
    }

    /// Skwir's own body text at 100 %, in pixels, where nothing says
    /// another (`Sizes::default`, code's without a monospace size).
    /// Lengths are written beside a 14 px body whatever the part
    /// (`Sizes::scale`).
    pub fn base(self) -> f32 {
        match self {
            Part::Interface | Part::Files | Part::Code => 14.,
            Part::Reading => 16.,
        }
    }

    /// How the user knows it: what a change of its size says.
    pub fn title(self) -> &'static str {
        match self {
            Part::Interface => "Text size",
            Part::Files => "Files text",
            Part::Reading => "Reading text",
            Part::Code => "Code text",
        }
    }

    /// The part a command names (`ui.text_size`'s `part`).
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "interface" => Part::Interface,
            "files" => Part::Files,
            "reading" => Part::Reading,
            "code" => Part::Code,
            _ => return None,
        })
    }
}

/// The step a setting says, `DEFAULT_STEP` when it says none of them.
pub fn step_of(value: Option<&str>) -> u32 {
    value
        .and_then(|value| value.trim().trim_end_matches('%').trim().parse().ok())
        .filter(|step| STEPS.contains(step))
        .unwrap_or(DEFAULT_STEP)
}

/// The step after `step` going `dir` (`in` larger, `out` smaller, else
/// back to 100 %), staying at the ends.
pub fn stepped(step: u32, dir: &str) -> u32 {
    let at = STEPS.iter().position(|s| *s == step);
    match (dir, at) {
        ("in", Some(at)) => STEPS[(at + 1).min(STEPS.len() - 1)],
        ("out", Some(at)) => STEPS[at.saturating_sub(1)],
        // A size not among the steps, or `reset`: back to 100 %.
        _ => DEFAULT_STEP,
    }
}

/// The preview's step after `percent` going larger or smaller: the next
/// of `PREVIEW_STEPS` past it, staying at the ends. A size between two
/// steps goes to the one beside it.
pub fn preview_stepped(percent: u32, larger: bool) -> u32 {
    if larger {
        PREVIEW_STEPS
            .iter()
            .copied()
            .find(|step| *step > percent)
            .unwrap_or(PREVIEW_STEPS[PREVIEW_STEPS.len() - 1])
    } else {
        PREVIEW_STEPS
            .iter()
            .rev()
            .copied()
            .find(|step| *step < percent)
            .unwrap_or(PREVIEW_STEPS[0])
    }
}

/// How a size is written, in the settings and the preview: `110%`.
pub fn step_label(step: u32) -> String {
    format!("{step}%")
}

/// The parts' sizes: each one's 100 %, in pixels, and the step its
/// setting chose. The files' 100 % is the interface's size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sizes {
    hundred: [f32; 4],
    step: [u32; 4],
}

impl Default for Sizes {
    /// Every part at its own 100 %.
    fn default() -> Self {
        Sizes {
            hundred: Part::ALL.map(Part::base),
            step: [DEFAULT_STEP; 4],
        }
    }
}

/// The interface's body text where no desktop names a size (ADR-117):
/// GTK's own 10 pt on Linux and the other free systems, 13 px on macOS,
/// `WinUI`'s 14 px on Windows.
pub const SYSTEM_BODY: f32 = if cfg!(target_os = "macos") {
    13.
} else if cfg!(windows) {
    14.
} else {
    10. * 96. / 72.
};

/// Each part's 100 % from what the desktop says (ADR-117), whatever fonts
/// are chosen: the interface its font's size, else the system's own;
/// reading 16 px, as a browser keeps a page's text; code its monospace
/// font's size, else 14 px; each times its text scaling factor. Sizes no
/// one would read at are left.
fn hundreds(desktop: &crate::desktop_style::Sizes) -> [f32; 4] {
    let sane = |px: Option<f32>| px.filter(|px| (6.0..=48.).contains(px));
    let scaling = desktop
        .scaling
        .filter(|factor| (0.5..=4.).contains(factor))
        .unwrap_or(1.);
    let interface = sane(desktop.interface).unwrap_or(SYSTEM_BODY) * scaling;
    let code = sane(desktop.monospace).unwrap_or(Part::Code.base()) * scaling;
    // The files' 100 % is the interface's size (`Sizes::hundred`).
    [interface, interface, Part::Reading.base() * scaling, code]
}

impl Sizes {
    /// The sizes the settings of `app` choose, from what the desktop says.
    pub fn resolve(app: &App) -> Self {
        Sizes {
            hundred: hundreds(&crate::desktop_style::sizes(&app.settings)),
            step: Part::ALL.map(|part| step_of(app.settings.get::<String>(part.key()).as_deref())),
        }
    }

    /// The sizes with `part` at `step`: what a test or a sample draws.
    #[must_use]
    pub fn with_step(mut self, part: Part, step: u32) -> Self {
        self.step[part.ix()] = step;
        self
    }

    /// A part's body text.
    pub fn body(&self, part: Part) -> Pixels {
        px(self.of(part))
    }

    /// A part's body text, in pixels.
    pub fn of(&self, part: Part) -> f32 {
        self.hundred(part) * self.step(part) as f32 / 100.
    }

    /// A part's body text at 100 %, in pixels.
    pub fn hundred(&self, part: Part) -> f32 {
        match part {
            Part::Files => self.of(Part::Interface),
            _ => self.hundred[part.ix()],
        }
    }

    /// The step a part's setting chose.
    pub fn step(&self, part: Part) -> u32 {
        self.step[part.ix()]
    }

    /// The rem a part is drawn in: its body text is `text_sm` there.
    pub fn rem(&self, part: Part) -> Pixels {
        px(self.of(part) / BODY_REMS)
    }

    /// How much larger than written a length is drawn in a part: its rem
    /// over gpui's (`REM`), where lengths are written beside a 14 px body,
    /// so the same as `Scale::of` reads in the part's layout and as `sized`
    /// lengths come to there. Reading's body is 16 px at 100 %: its lengths
    /// are 16/14 of what is written.
    pub fn scale(&self, part: Part) -> Scale {
        Scale(f32::from(self.rem(part)) / REM)
    }
}

/// How much larger than at 100 % something is drawn: lengths written at
/// 100 % are multiplied by it, in whole pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale(pub f32);

impl Default for Scale {
    fn default() -> Self {
        Scale::ONE
    }
}

impl Scale {
    pub const ONE: Scale = Scale(1.);

    /// The scale of what is laid out now, from the rem in force (a part's,
    /// under `elements::rem`).
    pub fn of(window: &Window) -> Self {
        Scale(f32::from(window.rem_size()) / REM)
    }

    /// `at_100` pixels, at this scale, in whole pixels.
    pub fn px(self, at_100: f32) -> Pixels {
        px(self.f(at_100))
    }

    /// `px`, as a number.
    pub fn f(self, at_100: f32) -> f32 {
        (at_100 * self.0).round()
    }

    /// A length measured at this scale, back at 100 % (a column's width
    /// dragged, remembered whatever the size).
    pub fn at_100(self, measured: f32) -> f32 {
        if self.0 > 0. {
            measured / self.0
        } else {
            measured
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Why a preview's content is sized as written: at the size its reader
    /// chose (`inspector::State::font_size`, ADR-117).
    const READ: &str = "a preview's content, at the size its reader chose";
    /// Why a size held in a variable is a named one.
    const NAMED_ABOVE: &str = "a named size (`painted`, `Sizes::body`) taken where it is held";

    /// Text sized other than by a name, each where it must be: its file
    /// under `src/`, the size as written (spaces left out), how many times
    /// it is there, and why. One more, or one fewer, fails.
    const SIZED_AS_WRITTEN: &[(&str, &str, usize, &str)] = &[
        ("code_preview.rs", "px(font_size-2.)", 1, READ),
        ("inspector.rs", "px(table_size(font_size))", 1, READ),
        (
            "inspector.rs",
            "px(table_size(font_size)*12./TABLE_TEXT)",
            1,
            READ,
        ),
        ("inspector.rs", "px(bytes_size(font_size))", 1, READ),
        ("inspector.rs", "px(size)", 1, READ),
        (
            "inspector.rs",
            "std::cell::Cell::new(TEXT_SIZE)",
            1,
            "what a preview is estimated at before its first draw reads its size",
        ),
        ("markdown.rs", "px(style.body_size)", 2, READ),
        ("markdown.rs", "px(size)", 2, READ),
        ("markdown.rs", "px(style.code_size)", 1, READ),
        ("markdown.rs", "px((style.body_size-4.).max(8.))", 1, READ),
        ("note_popover.rs", "px(size)", 1, READ),
        (
            "form.rs",
            "gpui::px(10.)",
            1,
            "a choice's picture (`form::drawn`), drawn at its own size as a picture is",
        ),
        (
            "form.rs",
            "gpui::px(12.)",
            1,
            "a choice's picture (`form::drawn`), drawn at its own size as a picture is",
        ),
        (
            "elements/terminal.rs",
            "self.font_size",
            1,
            "the terminal's, given by `terminal.rs`",
        ),
        (
            "input.rs",
            "font_size",
            1,
            "the field's text style, which the place it stands in names",
        ),
        ("elements/bars.rs", "label_size(window)", 1, NAMED_ABOVE),
        ("elements/treemap.rs", "px(self.font())", 1, NAMED_ABOVE),
        ("elements/treemap.rs", "px(self.size)", 1, NAMED_ABOVE),
        ("elements/treemap.rs", "px(size)", 1, NAMED_ABOVE),
        ("nav_menu.rs", "size", 1, NAMED_ABOVE),
    ];

    /// `text`'s code without its comments, what its strings hold and its
    /// white space, and the line of each of its bytes: a call written over
    /// several lines reads as one.
    fn code_of(text: &str) -> (String, Vec<usize>) {
        let (mut code, mut lines) = (String::new(), Vec::new());
        for (n, line) in text.lines().enumerate() {
            let (mut quoted, mut escaped) = (false, false);
            for (at, c) in line.char_indices() {
                if !quoted && line[at..].starts_with("//") {
                    break;
                }
                let quote = c == '"' && !escaped;
                if quote {
                    quoted = !quoted;
                }
                escaped = c == '\\' && !escaped;
                if !c.is_whitespace() && (quote || !quoted) {
                    code.push(c);
                    lines.extend(std::iter::repeat_n(n + 1, c.len_utf8()));
                }
            }
        }
        (code, lines)
    }

    /// What stands between the parenthesis opened before `from` and the
    /// one that closes it.
    fn inside(code: &str, from: usize) -> &str {
        let mut depth = 1;
        for (at, c) in code[from..].char_indices() {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &code[from..from + at];
                    }
                }
                _ => {}
            }
        }
        &code[from..]
    }

    /// `list` cut at its commas outside parentheses.
    fn arguments(list: &str) -> Vec<&str> {
        let (mut parts, mut depth, mut start) = (Vec::new(), 0, 0);
        for (at, c) in list.char_indices() {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                ',' if depth == 0 => {
                    parts.push(&list[start..at]);
                    start = at + 1;
                }
                _ => {}
            }
        }
        parts.push(&list[start..]);
        parts
    }

    /// What a field is given, from `from` up to the comma or the brace
    /// that ends it.
    fn given(code: &str, from: usize) -> &str {
        let mut depth = 0;
        for (at, c) in code[from..].char_indices() {
            match c {
                ')' | ']' | '}' | ',' | ';' if depth == 0 => return &code[from..from + at],
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                _ => {}
            }
        }
        &code[from..]
    }

    /// `expr` without the paths before it (`gpui::`, `crate::sizes::`).
    fn unpathed(mut expr: &str) -> &str {
        loop {
            let name = expr
                .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
                .unwrap_or(expr.len());
            match expr[name..].strip_prefix("::") {
                Some(rest) if name > 0 => expr = rest,
                _ => return expr,
            }
        }
    }

    /// Whether `expr` is a number: as pixels, as rems, or bare.
    fn number(expr: &str) -> bool {
        let expr = unpathed(expr);
        let digit = |rest: &str| rest.starts_with(|c: char| c.is_ascii_digit());
        digit(expr)
            || ["px(", "rems("]
                .iter()
                .any(|unit| expr.strip_prefix(unit).is_some_and(digit))
    }

    /// Whether what follows `font_size:` is a type (a field declared, an
    /// argument), not a size given.
    fn a_type(expr: &str) -> bool {
        let expr = unpathed(expr);
        let name = expr
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(expr.len());
        let after = &expr[name..];
        let primitive = ["f32", "f64", "u16", "u32", "usize"].contains(&&expr[..name]);
        let declared = expr.starts_with(|c: char| c.is_ascii_uppercase())
            && (after.is_empty() || after.starts_with(['<', ',', '>', ')', '}']));
        primitive || declared
    }

    /// Whether `expr` names a size (`TextSizes`' share, `painted`, a
    /// part's body text).
    fn named(expr: &str) -> bool {
        !number(expr)
            && ["painted(", "Text::", ".body(Part::"]
                .iter()
                .any(|n| expr.contains(n))
    }

    /// Every place names one of the named sizes (`TextSizes`, `painted`,
    /// `Sizes::body`), never gpui's scale, a number or a length (ADR-117):
    /// a size written beside them would not follow the text size, and a
    /// length (`sized`) follows the rem without the names' floor (the
    /// grid's tags once fell to 9.5 px so). Read in the code without its
    /// comments and white space, so a call over several lines or behind a
    /// path (`gpui::px`, `sizes::sized`) is read as one: every text size
    /// given (`text_size`, the size `shape_line` and `line_wrapper` take, a
    /// `font_size:` given) and every line height, which may be any length
    /// but a number. What is sized as written stands in
    /// `SIZED_AS_WRITTEN`, with why.
    #[test]
    fn every_place_names_a_text_size() {
        const GPUI: [&str; 7] = [
            ".text_xs()",
            ".text_sm()",
            ".text_base()",
            ".text_lg()",
            ".text_xl()",
            ".text_2xl()",
            ".text_3xl()",
        ];
        let src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut found = Vec::new();
        let mut written: std::collections::HashMap<(String, String), usize> =
            std::collections::HashMap::new();
        let mut folders = vec![src.clone()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).expect("the sources") {
                let path = entry.expect("a source").path();
                if path.is_dir() {
                    folders.push(path);
                    continue;
                }
                let file = path
                    .strip_prefix(&src)
                    .expect("under src")
                    .to_string_lossy()
                    .replace('\\', "/");
                if path.extension().is_none_or(|ext| ext != "rs") || file == "sizes.rs" {
                    continue;
                }
                let (code, lines) = code_of(&std::fs::read_to_string(&path).expect("a source"));
                let mut sized = |at: usize, what: &str, expr: &str, may: bool| {
                    let allowed = SIZED_AS_WRITTEN
                        .iter()
                        .any(|(f, e, _, _)| *f == file && *e == expr);
                    if allowed {
                        *written.entry((file.clone(), expr.to_owned())).or_default() += 1;
                    } else if !may {
                        found.push(format!("{file}:{}: {what} {expr}", lines[at]));
                    }
                };
                for gpui in GPUI {
                    for (at, _) in code.match_indices(gpui) {
                        sized(at, "gpui's", gpui, false);
                    }
                }
                for call in [
                    ".text_size(",
                    "shape_line(",
                    "line_wrapper(",
                    ".line_height(",
                ] {
                    for (at, _) in code.match_indices(call) {
                        let list = arguments(inside(&code, at + call.len()));
                        let expr = match call {
                            "shape_line(" | "line_wrapper(" => list.get(1).copied(),
                            _ => list.first().copied(),
                        };
                        let Some(expr) = expr.filter(|expr| !expr.is_empty()) else {
                            continue;
                        };
                        let may = if call == ".line_height(" {
                            !number(expr)
                        } else {
                            named(expr)
                        };
                        sized(at, call, expr, may);
                    }
                }
                for (at, _) in code.match_indices("font_size:") {
                    let field = code[..at]
                        .chars()
                        .next_back()
                        .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'));
                    let expr = given(&code, at + "font_size:".len());
                    if field && !expr.starts_with(':') && !a_type(expr) {
                        sized(at, "font_size:", expr, named(expr));
                    }
                }
            }
        }
        for (file, expr, times, why) in SIZED_AS_WRITTEN {
            assert!(!why.is_empty(), "{file}: {expr} says why");
            let seen = written
                .get(&((*file).to_owned(), (*expr).to_owned()))
                .copied()
                .unwrap_or(0);
            if seen != *times {
                found.push(format!(
                    "{file}: {expr} is written {seen} times, `SIZED_AS_WRITTEN` says {times}"
                ));
            }
        }
        assert!(found.is_empty(), "sizes not named:\n{}", found.join("\n"));
    }

    /// The reading of the code the test above relies on.
    #[test]
    fn a_size_is_read_over_lines_behind_paths_and_out_of_comments() {
        let (code, lines) = code_of(
            "a.text_size(\n    gpui::px(12.), // .text_xs()\n)\nlet s = \"// not a comment\";",
        );
        assert_eq!(code, "a.text_size(gpui::px(12.),)lets=\"\";");
        assert_eq!(lines[code.find("px").unwrap()], 2);
        let list = inside(&code, "a.text_size(".len());
        assert_eq!(arguments(list), ["gpui::px(12.)", ""]);
        assert!(number("gpui::px(12.)") && number("crate::sizes::rems(1.)") && number("14."));
        assert!(!number("px(size)") && !named("sizes::sized(14.)"));
        assert!(named(
            "crate::sizes::painted(window,crate::sizes::Text::Small)"
        ));
        assert!(!named("px(10.)"));
        assert!(a_type("f32") && a_type("std::cell::Cell<f32>") && a_type("Pixels"));
        assert!(!a_type("std::cell::Cell::new(TEXT_SIZE)") && !a_type("px(14.)"));
        assert_eq!(
            given("font_size:px(f(1,2)),x", "font_size:".len()),
            "px(f(1,2))"
        );
    }

    #[test]
    fn a_step_is_read_whatever_is_written_around_it() {
        assert_eq!(step_of(Some("125")), 125);
        assert_eq!(step_of(Some(" 125 % ")), 125);
        assert_eq!(step_of(Some("120")), DEFAULT_STEP);
        assert_eq!(step_of(Some("large")), DEFAULT_STEP);
        assert_eq!(step_of(None), DEFAULT_STEP);
    }

    #[test]
    fn steps_stop_at_the_ends_and_go_back_to_100() {
        assert_eq!(stepped(100, "in"), 110);
        assert_eq!(stepped(100, "out"), 90);
        assert_eq!(stepped(200, "in"), 200);
        assert_eq!(stepped(80, "out"), 80);
        assert_eq!(stepped(150, "reset"), 100);
    }

    #[test]
    fn at_100_the_rem_is_gpui_s_and_lengths_are_as_written() {
        let sizes = Sizes::default();
        assert_eq!(sizes.rem(Part::Files), px(REM));
        assert_eq!(sizes.scale(Part::Files).px(26.), px(26.));
        // Reading's body is 16 px: what holds it grows as its rem does.
        assert_eq!(sizes.scale(Part::Reading).px(14.), px(16.));
    }

    /// One scale per part: the theme's in `render` is what the part's rem
    /// gives a length in its layout (`Scale::of`, `sized`), every part and
    /// every step alike.
    #[test]
    fn a_part_s_scale_is_its_rem_s() {
        for part in Part::ALL {
            for step in STEPS {
                let sizes = Sizes::default().with_step(part, step);
                let of_rem = f32::from(sizes.rem(part)) / REM;
                assert!(
                    (sizes.scale(part).0 - of_rem).abs() < 1e-6,
                    "{part:?} {step}"
                );
                // A length in rems laid out there is the scaled length.
                let laid_out = f32::from(sizes.rem(part)) * sized(26.).0;
                let scaled = f32::from(sizes.scale(part).px(26.));
                assert!((scaled - laid_out.round()).abs() < 1e-3, "{part:?} {step}");
            }
        }
    }

    #[test]
    fn a_hundred_is_the_desktop_s_size_else_the_system_s() {
        use crate::desktop_style::Sizes as Desktop;
        let close = |a: f32, b: f32| (a - b).abs() < 1e-3;
        // Nothing said: the system's own, reading and code Skwir's.
        let none = hundreds(&Desktop::default());
        assert!(close(none[0], SYSTEM_BODY));
        assert!(close(none[2], 16.) && close(none[3], 14.));
        // GNOME's 11 pt with Large Text: every part scaled.
        let gnome = hundreds(&Desktop {
            interface: Some(11. * 96. / 72.),
            monospace: Some(11. * 96. / 72.),
            from: Some("GNOME".into()),
            scaling: Some(1.25),
        });
        assert!(close(gnome[0], 18.333));
        assert!(close(gnome[2], 20.) && close(gnome[3], 18.333));
        // A size no one reads at is left; a factor out of reason too.
        let odd = hundreds(&Desktop {
            interface: Some(2.),
            scaling: Some(40.),
            ..Desktop::default()
        });
        assert!(close(odd[0], SYSTEM_BODY));
    }

    #[test]
    fn the_preview_steps_past_the_size_it_is_at() {
        assert_eq!(preview_stepped(100, true), 110);
        assert_eq!(preview_stepped(100, false), 90);
        assert_eq!(preview_stepped(117, true), 125);
        assert_eq!(preview_stepped(117, false), 110);
        assert_eq!(preview_stepped(250, true), 250);
        assert_eq!(preview_stepped(60, false), 60);
    }

    #[test]
    fn the_files_100_is_the_interface_s_size() {
        let sizes = Sizes::default()
            .with_step(Part::Interface, 125)
            .with_step(Part::Files, 80);
        assert!((sizes.hundred(Part::Files) - 17.5).abs() < 1e-3);
        assert!((sizes.of(Part::Files) - 14.).abs() < 1e-3);
        assert!((sizes.of(Part::Reading) - 16.).abs() < 1e-3);
    }

    #[test]
    fn a_length_scales_in_whole_pixels() {
        let sizes = Sizes::default().with_step(Part::Files, 125);
        let scale = sizes.scale(Part::Files);
        assert_eq!(scale.px(26.), px(33.));
        assert_eq!(sizes.rem(Part::Files), px(20.));
        assert!((scale.at_100(225.) - 180.).abs() < 1e-3);
    }
}
