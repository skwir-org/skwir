//! A GTK3 theme's colors (ADR-071, points 3 and 4): the `@define-color`s
//! of its style sheet, after the `@import`s it makes within the theme,
//! with GTK's color functions (`shade`, `mix`, `alpha`, `lighter`,
//! `darker`) evaluated. GTK is never loaded and no widget's style is read:
//! only the colors a theme names. A theme whose sheet lives only in a
//! compiled `gtk.gresource` (Arc's), or GTK's built-in Adwaita, names none
//! here. Read on the desktop style's thread, never while drawing.

// Colors are written as CSS writes them, `0xrrggbb`.
#![allow(clippy::unreadable_literal)]

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use crate::desktop_style::{Rgb, Scheme, Schemes, over};

/// The most one sheet may hold: a theme's run to a few hundred kilobytes.
const SHEET: u64 = 8 << 20;
/// The most sheets one theme reads, its imports included.
const SHEETS: usize = 64;
/// How many colors deep a definition may name others before it is taken
/// for a loop.
const DEPTH: usize = 64;

/// A color with its opacity, each channel from 0 to 1.
type Rgba = [f32; 4];

/// Where GTK looks for a theme, in its order: the user's data folder,
/// `~/.themes`, then the system's data folders.
pub(crate) fn folders() -> Vec<PathBuf> {
    crate::xdg::data_home()
        .map(|data| data.join("themes"))
        .into_iter()
        .chain(skwir_core::os_path::home().map(|home| home.join(".themes")))
        .chain(
            crate::xdg::data_dirs()
                .into_iter()
                .map(|data| data.join("themes")),
        )
        .collect()
}

/// The schemes of the theme `theme` names (`Name`, or `Name:dark` as
/// `GTK_THEME` asks for its dark variant), the first of `folders` that
/// holds it: its sheet's, and its dark variant's (`gtk-dark.css`) beside
/// it unless the dark one alone was asked for.
pub(crate) fn schemes(theme: &str, folders: &[PathBuf]) -> Schemes {
    let (name, variant) = theme.trim().split_once(':').unwrap_or((theme.trim(), ""));
    let dark_only = variant.eq_ignore_ascii_case("dark");
    let mut schemes = Schemes::default();
    let Some((root, gtk)) = find(name, folders) else {
        return schemes;
    };
    let (light, dark) = (gtk.join("gtk.css"), gtk.join("gtk-dark.css"));
    let sheets: &[&Path] = if dark_only {
        // GTK takes the theme's own sheet where it has no dark one.
        if dark.is_file() { &[&dark] } else { &[&light] }
    } else {
        &[&light, &dark]
    };
    for sheet in sheets.iter().filter(|sheet| sheet.is_file()) {
        if let Some(scheme) = Sheet::read(sheet, &root).scheme() {
            schemes.add(scheme);
        }
    }
    schemes
}

/// The folder of the theme `name` and its GTK3 folder, in the first of
/// `folders` that holds it: the newest `gtk-3.N` GTK 3 would read
/// (`gtk-3.24` down to `gtk-3.0`), the one holding a `gtk.css`.
fn find(name: &str, folders: &[PathBuf]) -> Option<(PathBuf, PathBuf)> {
    if name.is_empty() || matches!(name, "." | "..") || name.contains(['/', '\\']) {
        return None;
    }
    folders.iter().find_map(|folder| {
        let root = folder.join(name);
        (0..=24)
            .rev()
            .step_by(2)
            .map(|minor| root.join(format!("gtk-3.{minor}")))
            .find(|gtk| gtk.join("gtk.css").is_file())
            .map(|gtk| (root.clone(), gtk))
    })
}

/// The colors a theme's sheet names: each name with every definition it
/// was given, in order. The last is the one used; one that names itself
/// takes the one before (adw-gtk3 writes `@define-color warning_color
/// @warning_color;` after defining it).
#[derive(Debug, Default)]
pub(crate) struct Sheet {
    defined: HashMap<String, Vec<String>>,
}

impl Sheet {
    /// What `file`, and the sheets it imports from within `root`, define.
    pub(crate) fn read(file: &Path, root: &Path) -> Self {
        let mut sheet = Sheet::default();
        let mut seen = HashSet::new();
        sheet.load(file, &lexical(root), &mut seen);
        sheet
    }

    fn load(&mut self, file: &Path, root: &Path, seen: &mut HashSet<PathBuf>) {
        let file = lexical(file);
        if !file.starts_with(root) || seen.len() >= SHEETS || !seen.insert(file.clone()) {
            return;
        }
        let Ok(text) = crate::file::read_regular_text(&file, SHEET) else {
            return;
        };
        let text = without_comments(&text);
        let mut rest = text.as_str();
        while let Some(at) = rest.find('@') {
            rest = &rest[at + 1..];
            let keyword = |word: &str| {
                rest.strip_prefix(word)
                    .filter(|after| after.starts_with(char::is_whitespace))
            };
            if let Some(after) = keyword("define-color") {
                let (statement, next) = statement(after);
                rest = next;
                let statement = statement.trim_start();
                let end = statement
                    .find(char::is_whitespace)
                    .unwrap_or(statement.len());
                let (name, value) = statement.split_at(end);
                if !name.is_empty() && !value.trim().is_empty() {
                    self.defined
                        .entry(name.to_owned())
                        .or_default()
                        .push(value.trim().to_owned());
                }
            } else if let Some(after) = keyword("import") {
                let (statement, next) = statement(after);
                rest = next;
                if let Some(target) = imported(statement, &file) {
                    self.load(&target, root, seen);
                }
            }
        }
    }

    /// The color `name` names, as the theme defines it last.
    pub(crate) fn color(&self, name: &str) -> Option<Rgba> {
        Eval {
            sheet: self,
            open: Vec::new(),
        }
        .named(name)
    }

    /// The scheme of point 4's GTK3 column, when the theme names what is
    /// read on, in what, and its chrome.
    pub(crate) fn scheme(&self) -> Option<Scheme> {
        let opaque = |color: Rgba| [color[0], color[1], color[2]];
        let bg = opaque(self.color("theme_base_color")?);
        let surface = opaque(self.color("theme_bg_color")?);
        let on_bg = |name: &str| self.color(name).map(|color| over(color, bg));
        let selection_bg = on_bg("theme_selected_bg_color");
        let selection = selection_bg.zip(
            self.color("theme_selected_fg_color")
                .zip(selection_bg)
                .map(|(fg, ground)| over(fg, ground)),
        );
        Some(Scheme {
            bg,
            surface,
            text: on_bg("theme_text_color")?,
            text_muted: None,
            zebra: None,
            border: on_bg("borders"),
            accent: selection_bg,
            selection,
            error: on_bg("error_color"),
            warning: on_bg("warning_color"),
            success: on_bg("success_color"),
        })
    }
}

/// `text` without its `/* … */` comments.
fn without_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = rest[start + 2..]
            .find("*/")
            .map_or("", |end| &rest[start + 2 + end + 2..]);
        out.push(' ');
    }
    out.push_str(rest);
    out
}

/// A statement up to its `;` (outside brackets and quotes), and what
/// follows it.
fn statement(text: &str) -> (&str, &str) {
    let mut depth = 0_usize;
    let mut quote = None;
    for (i, c) in text.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ';' | '{' | '}' if depth == 0 => return (&text[..i], &text[i + 1..]),
            _ => {}
        }
    }
    (text, "")
}

/// The sheet an `@import` names from `file`: `url("…")`, `url(…)` or a
/// string, a relative one beside `file`; `None` for another scheme than
/// `file://` (a `resource://` of a compiled theme).
fn imported(statement: &str, file: &Path) -> Option<PathBuf> {
    let statement = statement.trim();
    let inner = match statement.strip_prefix("url(") {
        Some(url) => url.split(')').next()?.trim(),
        None => statement.split_whitespace().next()?,
    };
    let target = inner
        .strip_prefix(['"', '\''])
        .and_then(|t| t.strip_suffix(['"', '\'']))
        .unwrap_or(inner);
    if target.contains("://") {
        return crate::uri::path_of(target);
    }
    Some(file.parent()?.join(target))
}

/// `path` with its `.` and `..` resolved by name, never through the
/// file system: what decides whether an import stays within the theme.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// A color expression evaluated against a sheet's names.
struct Eval<'a> {
    sheet: &'a Sheet,
    /// The names whose definitions are being read, each with the one
    /// read: a name met again takes the one before it.
    open: Vec<(&'a str, usize)>,
}

impl Eval<'_> {
    fn named(&mut self, name: &str) -> Option<Rgba> {
        let (name, definitions) = self.sheet.defined.get_key_value(name)?;
        let index = match self.open.iter().rev().find(|(open, _)| *open == name) {
            Some((_, index)) => index.checked_sub(1)?,
            None => definitions.len() - 1,
        };
        if self.open.len() >= DEPTH {
            return None;
        }
        self.open.push((name, index));
        let mut cursor = Cursor(definitions[index].as_str());
        let color = self.value(&mut cursor).filter(|_| cursor.done());
        self.open.pop();
        color
    }

    fn value(&mut self, p: &mut Cursor<'_>) -> Option<Rgba> {
        p.skip_space();
        if p.eat('@') {
            let name = p.ident()?;
            return self.named(name);
        }
        if p.eat('#') {
            return hex(p.take_while(|c| c.is_ascii_hexdigit()));
        }
        let word = p.ident()?.to_ascii_lowercase();
        p.skip_space();
        if !p.eat('(') {
            return keyword(&word);
        }
        let color = match word.as_str() {
            "rgb" | "rgba" => {
                let r = p.channel(255.)?;
                p.comma().then_some(())?;
                let g = p.channel(255.)?;
                p.comma().then_some(())?;
                let b = p.channel(255.)?;
                let a = if p.comma() { p.channel(1.)? } else { 1. };
                Some([r, g, b, a])
            }
            "hsl" | "hsla" => {
                let h = p.number()?;
                p.comma().then_some(())?;
                let s = p.channel(1.)?;
                p.comma().then_some(())?;
                let l = p.channel(1.)?;
                let a = if p.comma() { p.channel(1.)? } else { 1. };
                let [r, g, b] = from_hsl([h.rem_euclid(360.) / 360., s, l]);
                Some([r, g, b, a])
            }
            "shade" => {
                let color = self.value(p)?;
                p.comma().then_some(())?;
                Some(shade(color, p.number()?))
            }
            "lighter" => self.value(p).map(|color| shade(color, 1.3)),
            "darker" => self.value(p).map(|color| shade(color, 0.7)),
            "alpha" => {
                let mut color = self.value(p)?;
                p.comma().then_some(())?;
                color[3] = (color[3] * p.number()?).clamp(0., 1.);
                Some(color)
            }
            "mix" => {
                let from = self.value(p)?;
                p.comma().then_some(())?;
                let to = self.value(p)?;
                p.comma().then_some(())?;
                let along = p.number()?;
                Some([0, 1, 2, 3].map(|i| (from[i] + (to[i] - from[i]) * along).clamp(0., 1.)))
            }
            _ => None,
        }?;
        p.skip_space();
        p.eat(')').then_some(color)
    }
}

/// A color expression being read.
struct Cursor<'a>(&'a str);

impl<'a> Cursor<'a> {
    fn skip_space(&mut self) {
        self.0 = self.0.trim_start();
    }

    fn done(&mut self) -> bool {
        self.skip_space();
        self.0.is_empty()
    }

    fn eat(&mut self, c: char) -> bool {
        match self.0.strip_prefix(c) {
            Some(rest) => {
                self.0 = rest;
                true
            }
            None => false,
        }
    }

    fn comma(&mut self) -> bool {
        self.skip_space();
        let ate = self.eat(',');
        self.skip_space();
        ate
    }

    fn take_while(&mut self, keep: impl Fn(char) -> bool) -> &'a str {
        let end = self.0.find(|c| !keep(c)).unwrap_or(self.0.len());
        let (taken, rest) = self.0.split_at(end);
        self.0 = rest;
        taken
    }

    fn ident(&mut self) -> Option<&'a str> {
        let ident = self.take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'));
        (!ident.is_empty()).then_some(ident)
    }

    fn number(&mut self) -> Option<f32> {
        self.skip_space();
        let number =
            self.take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'));
        number.parse().ok()
    }

    /// A channel of `rgb()` or `hsl()`: a number out of `full`, or a
    /// percentage.
    fn channel(&mut self, full: f32) -> Option<f32> {
        let number = self.number()?;
        let value = if self.eat('%') {
            number / 100.
        } else {
            number / full
        };
        Some(value.clamp(0., 1.))
    }
}

/// `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`.
fn hex(digits: &str) -> Option<Rgba> {
    let digit = |i: usize, width: usize| {
        u8::from_str_radix(digits.get(i * width..(i + 1) * width)?, 16)
            .ok()
            .map(|v| f32::from(v) / if width == 1 { 15. } else { 255. })
    };
    let (width, count) = match digits.len() {
        3 => (1, 3),
        4 => (1, 4),
        6 => (2, 3),
        8 => (2, 4),
        _ => return None,
    };
    let mut color = [1.; 4];
    for (i, channel) in color.iter_mut().enumerate().take(count) {
        *channel = digit(i, width)?;
    }
    Some(color)
}

/// The CSS color keywords themes write; `currentColor` is a widget's, so
/// none here.
fn keyword(word: &str) -> Option<Rgba> {
    let hex = match word {
        "transparent" => return Some([0., 0., 0., 0.]),
        "white" => 0xffffff,
        "black" => 0x000000,
        "red" => 0xff0000,
        "lime" => 0x00ff00,
        "green" => 0x008000,
        "blue" => 0x0000ff,
        "yellow" => 0xffff00,
        "cyan" | "aqua" => 0x00ffff,
        "magenta" | "fuchsia" => 0xff00ff,
        "gray" | "grey" => 0x808080,
        "silver" => 0xc0c0c0,
        "maroon" => 0x800000,
        "olive" => 0x808000,
        "navy" => 0x000080,
        "purple" => 0x800080,
        "teal" => 0x008080,
        "orange" => 0xffa500,
        _ => return None,
    };
    let channel = |shift: u32| f32::from(((hex >> shift) & 0xff) as u8) / 255.;
    Some([channel(16), channel(8), channel(0), 1.])
}

/// GTK's `shade`: lightness and saturation multiplied by `factor`.
fn shade(color: Rgba, factor: f32) -> Rgba {
    let [h, s, l] = to_hsl([color[0], color[1], color[2]]);
    let [r, g, b] = from_hsl([h, (s * factor).clamp(0., 1.), (l * factor).clamp(0., 1.)]);
    [r, g, b, color[3]]
}

/// Hue (0 to 1), saturation and lightness.
fn to_hsl([r, g, b]: Rgb) -> [f32; 3] {
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = f32::midpoint(max, min);
    let delta = max - min;
    if delta <= f32::EPSILON {
        return [0., 0., l];
    }
    let s = if l <= 0.5 {
        delta / (max + min)
    } else {
        delta / (2. - max - min)
    };
    let h = if (max - r).abs() <= f32::EPSILON {
        (g - b) / delta
    } else if (max - g).abs() <= f32::EPSILON {
        2. + (b - r) / delta
    } else {
        4. + (r - g) / delta
    };
    [(h / 6.).rem_euclid(1.), s, l]
}

fn from_hsl([h, s, l]: [f32; 3]) -> Rgb {
    if s <= 0. {
        return [l; 3];
    }
    let q = if l < 0.5 { l * (1. + s) } else { l + s - l * s };
    let p = 2. * l - q;
    let channel = |t: f32| {
        let t = t.rem_euclid(1.);
        if t < 1. / 6. {
            p + (q - p) * 6. * t
        } else if t < 0.5 {
            q
        } else if t < 2. / 3. {
            p + (q - p) * (2. / 3. - t) * 6.
        } else {
            p
        }
    };
    [channel(h + 1. / 3.), channel(h), channel(h - 1. / 3.)]
}

#[cfg(test)]
// The channels compared are the same numbers divided alike: equal exactly.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn near(a: Rgb, b: u32) -> bool {
        let b = [16, 8, 0].map(|shift| f32::from(((b >> shift) & 0xff) as u8) / 255.);
        a.iter().zip(b).all(|(a, b)| (a - b).abs() * 255. <= 1.)
    }

    fn rgb(color: Rgba) -> Rgb {
        [color[0], color[1], color[2]]
    }

    /// A theme of the test's own, its files by name under `gtk-3.0`.
    fn theme(folder: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = folder.join(name);
        for (file, text) in files {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        root
    }

    #[test]
    fn colors_are_read_by_reference_through_gtk_s_functions() {
        let dir = tempfile::tempdir().unwrap();
        let root = theme(
            dir.path(),
            "Synthetic",
            &[(
                "gtk-3.0/gtk.css",
                "/* @define-color commented #000; */\n\
                 @define-color base #336699;\n\
                 @define-color short #fff;\n\
                 @define-color quarter #ff000040;\n\
                 @define-color by_name @base;\n\
                 @define-color shaded shade(@base, 1.2);\n\
                 @define-color light lighter(@base);\n\
                 @define-color dark darker(@base);\n\
                 @define-color half alpha(@base, 0.5);\n\
                 @define-color mixed mix(@base, white, 0.5);\n\
                 @define-color nested mix(shade(@base, 0.8), rgba(255, 255, 255, 0.5), 0.25);\n\
                 @define-color percent rgb(100%, 50%, 0%);\n\
                 @define-color hue hsl(120, 100%, 25%);\n\
                 @define-color keyword white;\n\
                 @define-color current mix(currentColor, white, 0.5);\n\
                 @define-color missing @nowhere;\n\
                 @define-color looped @loops;\n\
                 @define-color loops @looped;\n\
                 @define-color again #111111;\n\
                 @define-color again shade(@again, 2);\n\
                 @define-color trailing #fff junk;\n\
                 .widget { color: @base; background: @define-color; }\n",
            )],
        );
        let sheet = Sheet::read(&root.join("gtk-3.0/gtk.css"), &root);
        let color = |name: &str| sheet.color(name);
        assert_eq!(color("commented"), None);
        assert!(near(rgb(color("base").unwrap()), 0x336699));
        assert_eq!(color("short"), Some([1.; 4]));
        assert!((color("quarter").unwrap()[3] - 64. / 255.).abs() < 1e-6);
        assert_eq!(color("by_name"), color("base"));
        // GTK's shade, in its HSL: lightness and saturation scaled.
        assert!(near(rgb(color("shaded").unwrap()), 0x317ac4));
        assert_eq!(color("light"), Some(shade(color("base").unwrap(), 1.3)));
        assert_eq!(color("dark"), Some(shade(color("base").unwrap(), 0.7)));
        assert_eq!(color("half").unwrap()[3], 0.5);
        assert!(near(rgb(color("mixed").unwrap()), 0x99b2cc));
        assert!(color("nested").is_some());
        assert!(near(rgb(color("percent").unwrap()), 0xff8000));
        assert!(near(rgb(color("hue").unwrap()), 0x008000));
        assert_eq!(color("keyword"), Some([1.; 4]));
        // What a widget's own color or an undefined name stand for, a
        // loop and what is left over: none.
        assert_eq!(color("current"), None);
        assert_eq!(color("missing"), None);
        assert_eq!(color("looped"), None);
        assert_eq!(color("trailing"), None);
        // A color defined by its own name keeps the definition before.
        assert!(near(rgb(color("again").unwrap()), 0x222222));
    }

    #[test]
    fn imports_are_read_within_the_theme_once_each() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("outside.css"),
            "@define-color outside #ff0000;",
        )
        .unwrap();
        let root = theme(
            dir.path(),
            "Imports",
            &[
                (
                    "gtk-3.0/gtk.css",
                    "@import url(\"colors.css\");\n\
                     @import url(sub/more.css);\n\
                     @import '../../outside.css';\n\
                     @import url(\"resource:///org/example/theme/gtk.css\");\n\
                     @define-color later @early;\n",
                ),
                (
                    "gtk-3.0/colors.css",
                    "@import url(\"gtk.css\");\n@define-color early #00ff00; @define-color replaced #000;",
                ),
                (
                    "gtk-3.0/sub/more.css",
                    "@import url(\"../colors.css\"); @define-color replaced #fff;",
                ),
            ],
        );
        let sheet = Sheet::read(&root.join("gtk-3.0/gtk.css"), &root);
        assert_eq!(sheet.color("later"), Some([0., 1., 0., 1.]));
        assert_eq!(sheet.color("replaced"), Some([1.; 4]), "the last one read");
        assert_eq!(sheet.color("outside"), None, "never outside the theme");
    }

    fn full(bg: &str, fg: &str) -> String {
        format!(
            "@define-color theme_base_color {bg};\n\
             @define-color theme_bg_color shade(@theme_base_color, 0.95);\n\
             @define-color theme_text_color {fg};\n\
             @define-color theme_selected_bg_color #3584e4;\n\
             @define-color theme_selected_fg_color alpha(white, 0.5);\n\
             @define-color borders alpha({fg}, 0.15);\n\
             @define-color error_color #c01c28;\n\
             @define-color warning_color #9c6e03;\n\
             @define-color success_color #1b8553;\n"
        )
    }

    #[test]
    fn a_theme_gives_its_look_and_its_dark_variant() {
        let dir = tempfile::tempdir().unwrap();
        let folders = [dir.path().join("user"), dir.path().join("system")];
        theme(
            &folders[1],
            "Both",
            &[
                ("gtk-3.0/gtk.css", &full("#ffffff", "rgba(0, 0, 0, 0.8)")),
                ("gtk-3.0/gtk-dark.css", &full("#1e1e1e", "white")),
            ],
        );
        let both = schemes("Both", &folders);
        let light = both.light.clone().expect("its own sheet");
        assert!(!light.is_dark());
        assert_eq!(light.bg, [1.; 3]);
        assert!(near(light.text, 0x333333), "laid over what it is read on");
        assert!(near(light.accent.unwrap(), 0x3584e4));
        let (selection_bg, selection_text) = light.selection.unwrap();
        assert_eq!(Some(selection_bg), light.accent);
        assert!(
            near(selection_text, 0x9ac2f2),
            "white at half over the selection"
        );
        assert!(light.border.is_some() && light.error.is_some());
        assert_eq!((light.text_muted, light.zebra), (None, None), "derived");
        assert!(both.dark.as_ref().is_some_and(Scheme::is_dark));

        // Its dark variant alone, as GTK_THEME asks for it.
        let dark = schemes("Both:dark", &folders);
        assert_eq!((dark.light, dark.dark), (None, both.dark));

        // The user's own copy, in a newer GTK folder, is the one read.
        theme(
            &folders[0],
            "Both",
            &[("gtk-3.20/gtk.css", &full("#fafafa", "#101010"))],
        );
        let user = schemes("Both", &folders);
        assert!(near(user.light.unwrap().bg, 0xfafafa));
        assert_eq!(user.dark, None);

        // A dark theme without a variant is the dark look's alone.
        theme(
            &folders[1],
            "Night",
            &[("gtk-3.0/gtk.css", &full("#202020", "#eeeeee"))],
        );
        let night = schemes("Night", &folders);
        assert!(night.light.is_none() && night.dark.is_some());
    }

    #[test]
    fn a_compiled_theme_a_partial_one_or_none_gives_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let folders = [dir.path().to_path_buf()];
        theme(
            dir.path(),
            "Compiled",
            &[
                (
                    "gtk-3.0/gtk.css",
                    "@import url(\"resource:///org/example/compiled/gtk-main.css\");",
                ),
                ("gtk-3.0/gtk.gresource", "GVariant"),
            ],
        );
        theme(
            dir.path(),
            "Partial",
            &[("gtk-3.0/gtk.css", "@define-color theme_base_color #fff;")],
        );
        for name in ["Compiled", "Partial", "Adwaita", "", "..", "a/b"] {
            assert!(schemes(name, &folders).is_empty(), "{name:?}");
        }
    }

    #[test]
    fn hsl_goes_there_and_back() {
        for color in [
            [0.2, 0.4, 0.6],
            [1., 0., 0.],
            [0.5, 0.5, 0.5],
            [0.9, 0.8, 0.1],
        ] {
            let back = from_hsl(to_hsl(color));
            assert!(
                color.iter().zip(back).all(|(a, b)| (a - b).abs() < 1e-5),
                "{color:?}"
            );
        }
    }
}
