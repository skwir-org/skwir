//! Code colored by a tree-sitter grammar (ADR-110), in the theme bat's
//! grammars color it in: which engine colors a text (`engine`, as the
//! Reading page's Grammars choose: each language's own choice, else
//! Automatic's rule, `reading.grammars`), the scopes bat's themes
//! color for tree-sitter's capture names (`scope_of`), and a text parsed
//! in the helper and colored a few lines at a time (`Tree`).
//!
//! Spans come from the helper by line, in the line's bytes; a row of a
//! text read where it is shown is a line, or a part of one, and takes the
//! spans of its line from where it starts in it (`Captured::runs`). Every
//! byte of a row is in a run, those no capture colors in the theme's own
//! color, as bat's runs are.

use std::collections::BTreeMap;
use std::ops::Range;
use std::str::FromStr;
use std::sync::Arc;

use skwir_core::CancelToken;
use skwir_core::inspection::Run;
use skwir_core::syntax::{MAX_LINES, Span, Symbol};
use syntect::highlighting::{FontStyle, Style};
use syntect::parsing::ScopeStack;

use super::Palette;
use crate::grammars::{self, Found, Source};
use crate::tree_sitter::{self, Helper};

/// Which grammars color a language that both bat's grammars and a
/// tree-sitter grammar know (`First`).
pub const ENGINE_KEY: &str = "reading.grammars";

/// The longest line a tree-sitter grammar colors, and the longest code
/// block: the helper answers a line's spans whole, and a minified file's
/// one line could hold more of them than a reply does
/// (`syntax::MAX_REPLY`), at a span every two bytes. bat's grammar colors
/// a longer one.
pub const LONGEST_LINE: usize = 1 << 20;

/// Which grammars color a language both know: bat's (the default) or a
/// tree-sitter grammar found for it. A language only one knows is colored
/// by that one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum First {
    #[default]
    Bat,
    TreeSitter,
}

impl First {
    /// The setting's choices, `(value, label)`, the default first.
    pub const CHOICES: [(&'static str, &'static str); 2] =
        [("bat", "bat first"), ("tree_sitter", "Tree-sitter first")];

    /// The choice a setting's value says; bat's for any other.
    pub fn of(value: Option<&str>) -> First {
        match value {
            Some("tree_sitter") => First::TreeSitter,
            _ => First::Bat,
        }
    }

    /// The choice the settings make (`reading.grammars`).
    pub fn chosen(app: &skwir_core::App) -> First {
        First::of(app.settings.get::<String>(ENGINE_KEY).as_deref())
    }
}

/// What each language is colored by, as chosen on the Reading page: a
/// table of languages (in lower case) to choices (`Choice::key`). A
/// language it does not name is Automatic.
pub const CHOSEN_KEY: &str = "reading.grammar_for";

/// What colors a language, chosen rather than Automatic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Choice {
    /// bat's grammars (with the ones added to bat and Sublime Text).
    Bat,
    /// The tree-sitter grammar of the language this source keeps.
    TreeSitter(Source),
    /// No colors.
    Plain,
}

impl Choice {
    /// How the settings name it.
    pub fn key(self) -> &'static str {
        match self {
            Choice::Bat => "bat",
            Choice::Plain => "plain",
            Choice::TreeSitter(source) => source.key(),
        }
    }

    /// The choice the settings name `key`; none for any other.
    pub fn of_key(key: &str) -> Option<Choice> {
        match key {
            "bat" => Some(Choice::Bat),
            "plain" => Some(Choice::Plain),
            other => Source::of_key(other).map(Choice::TreeSitter),
        }
    }
}

/// What the settings choose of grammars: Automatic's rule, and the
/// languages chosen otherwise.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GrammarChoices {
    pub first: First,
    pub chosen: Arc<BTreeMap<String, Choice>>,
}

impl GrammarChoices {
    /// Automatic for every language, by `first`.
    pub fn automatic(first: First) -> Self {
        GrammarChoices {
            first,
            chosen: Arc::default(),
        }
    }

    /// As the settings say (`reading.grammars`, `reading.grammar_for`); a
    /// choice no longer known is Automatic.
    pub fn chosen(app: &skwir_core::App) -> Self {
        GrammarChoices {
            first: First::chosen(app),
            chosen: Arc::new(chosen_in(
                &app.settings
                    .get::<BTreeMap<String, String>>(CHOSEN_KEY)
                    .unwrap_or_default(),
            )),
        }
    }

    /// The languages whose tree-sitter grammar is chosen from a source:
    /// what `grammars::choose` takes.
    pub fn sources(&self) -> BTreeMap<String, Source> {
        self.chosen
            .iter()
            .filter_map(|(language, choice)| match choice {
                Choice::TreeSitter(source) => Some((language.clone(), *source)),
                _ => None,
            })
            .collect()
    }
}

/// The choices a table says, by language in lower case.
pub fn chosen_in(table: &BTreeMap<String, String>) -> BTreeMap<String, Choice> {
    table
        .iter()
        .filter_map(|(language, key)| Some((language.to_lowercase(), Choice::of_key(key)?)))
        .collect()
}

/// What colors a text: bat's grammars, the tree-sitter grammar named, or
/// nothing (its language chosen Plain text).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Engine {
    Bat,
    TreeSitter(String),
    Plain,
}

/// What colors a text named `name` (a file's name; its language when it
/// has none, as a document's code block), colored as `language` (what the
/// code view colors by: an extension or a name), `first_line` its first
/// line. Its language's own choice first (`grammars.chosen`, by the
/// tree-sitter grammar found for it, else by bat's grammar's name): bat's,
/// the tree-sitter grammar (the source chosen is the one `found` uses, or
/// Automatic when it is gone), or nothing. Automatic: the tree-sitter
/// grammar found for it where `grammars.first` chooses it, or where bat's
/// grammars know no `language`; else bat's. Before the grammars are
/// found, or with none, bat's unless chosen Plain. It may read bat's
/// grammars: a worker's work.
pub fn engine(
    name: &str,
    language: Option<&str>,
    first_line: Option<&str>,
    grammars: &GrammarChoices,
) -> Engine {
    let found = grammars::found().unwrap_or_default();
    engine_in(
        &found,
        (name, language, first_line),
        grammars,
        |token| {
            super::syntaxes()
                .find_syntax_by_token(token)
                .is_some_and(|syntax| syntax.name != "Plain Text")
        },
        |token| {
            super::syntaxes()
                .find_syntax_by_token(token)
                .filter(|syntax| syntax.name != "Plain Text")
                .map(|syntax| syntax.name.to_lowercase())
        },
    )
}

/// `engine` among the grammars `found`, `bat_knows` saying whether bat's
/// grammars color a language token, `bat_name` the name of bat's grammar
/// that does (in lower case).
fn engine_in(
    found: &[Found],
    (name, language, first_line): (&str, Option<&str>, Option<&str>),
    grammars: &GrammarChoices,
    bat_knows: impl FnOnce(&str) -> bool,
    bat_name: impl FnOnce(&str) -> Option<String>,
) -> Engine {
    // By the file's name, else by the language a code block names
    // (`rust`, where its grammar colors `.rs`).
    let tree = grammars::for_file(found, name, first_line).or_else(|| {
        language
            .and_then(|language| grammars::named(found, language))
            .filter(|found| found.problem.is_none())
    });
    if !grammars.chosen.is_empty() {
        let language_of = tree
            .map(|tree| tree.grammar.name.to_lowercase())
            .or_else(|| language.and_then(bat_name));
        match language_of.and_then(|of| grammars.chosen.get(&of)) {
            Some(Choice::Plain) => return Engine::Plain,
            Some(Choice::Bat) => return Engine::Bat,
            Some(Choice::TreeSitter(source)) => {
                if let Some(tree) = tree.filter(|tree| tree.source == *source) {
                    return Engine::TreeSitter(tree.grammar.name.clone());
                }
            }
            None => {}
        }
    }
    let Some(tree) = tree else {
        return Engine::Bat;
    };
    if grammars.first == First::Bat && language.is_some_and(bat_knows) {
        return Engine::Bat;
    }
    Engine::TreeSitter(tree.grammar.name.clone())
}

/// The scope bat's themes color for a capture name of tree-sitter's
/// queries (`@keyword`, `function.method`): the table's, else that of its
/// longest dotted prefix the table knows (Helix's `keyword.control.import`,
/// Neovim's `function.method.call`). `None` for one that colors nothing
/// (Neovim's `@spell`, `@none`). A scope may be a stack, outermost first
/// (`meta.mapping.key string`), as bat's grammars nest them.
pub fn scope_of(capture: &str) -> Option<&'static str> {
    let mut name = capture.strip_prefix('@').unwrap_or(capture);
    loop {
        if let Some(scope) = scope(name) {
            return Some(scope);
        }
        name = name.rsplit_once('.')?.0;
    }
}

/// The table `scope_of` reads: the names tree-sitter's own queries,
/// Helix's, Neovim's (and its older ones) and Zed's use, each with the
/// scope Sublime Text's grammars give the same thing.
fn scope(capture: &str) -> Option<&'static str> {
    Some(match capture {
        "attribute" | "tag.attribute" => "entity.other.attribute-name",
        "boolean" | "constant.builtin" | "constant.builtin.boolean" => "constant.language",
        "character" | "constant.character" => "constant.character",
        "character.special"
        | "constant.character.escape"
        | "escape"
        | "string.escape"
        | "string.special.escape" => "constant.character.escape",
        "comment" => "comment",
        "comment.documentation" | "comment.block.documentation" => "comment.block.documentation",
        "constant" => "constant",
        "constant.macro" => "constant.other",
        "constant.numeric" | "number" | "float" | "number.float" => "constant.numeric",
        "constructor" | "type" | "type.definition" => "entity.name.type",
        "diff.plus" => "markup.inserted",
        "diff.minus" => "markup.deleted",
        "diff.delta" => "markup.changed",
        "embedded" => "source",
        "field" | "property" | "variable.member" | "variable.other.member" => {
            "variable.other.member"
        }
        "function" | "function.method" | "method" => "entity.name.function",
        "function.builtin" => "support.function",
        "function.macro" => "entity.name.function.macro",
        "include" | "keyword.import" | "keyword.control.import" => "keyword.control.import",
        "keyword" => "keyword",
        "keyword.control" | "conditional" | "repeat" | "exception" | "keyword.return" => {
            "keyword.control"
        }
        "keyword.function" => "storage.type.function",
        "keyword.operator" | "operator" => "keyword.operator",
        "keyword.storage" => "storage",
        "keyword.storage.type" | "keyword.type" | "type.builtin" => "storage.type",
        "keyword.storage.modifier" | "keyword.modifier" | "type.qualifier" => "storage.modifier",
        "label" => "entity.name.label",
        "markup.bold" | "markup.strong" | "text.strong" => "markup.bold",
        "markup.heading" | "text.title" => "markup.heading",
        "markup.italic" | "markup.emphasis" | "text.emphasis" => "markup.italic",
        "markup.link" | "markup.link.url" | "text.uri" | "text.reference" => {
            "markup.underline.link"
        }
        "markup.list" => "markup.list",
        "markup.quote" => "markup.quote",
        "markup.raw" | "text.literal" => "markup.raw",
        "markup.strikethrough" => "markup.strikethrough",
        "module" | "namespace" => "entity.name.namespace",
        "parameter" | "variable.parameter" => "variable.parameter",
        "punctuation" => "punctuation",
        "punctuation.bracket" => "punctuation.section",
        "punctuation.delimiter" => "punctuation.separator",
        "punctuation.special" => "punctuation.section.embedded",
        "string" => "string",
        "string.regex" | "string.regexp" => "string.regexp",
        "string.special" => "string.other",
        "string.special.key" => "meta.mapping.key string",
        "string.special.symbol" => "constant.other.symbol",
        "tag" | "tag.builtin" => "entity.name.tag",
        "tag.delimiter" => "punctuation.definition.tag",
        "variable" => "variable",
        "variable.builtin" => "variable.language",
        _ => return None,
    })
}

/// How a capture is drawn: a color, and bold or italic where the theme's
/// font styles are drawn (`Palette::styled`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub color: [u8; 3],
    pub bold: bool,
    pub italic: bool,
}

impl Look {
    fn of(style: Style, styled: bool) -> Look {
        let has = |font: FontStyle| styled && style.font_style.contains(font);
        Look {
            color: [style.foreground.r, style.foreground.g, style.foreground.b],
            bold: has(FontStyle::BOLD),
            italic: has(FontStyle::ITALIC),
        }
    }

    fn run(self, range: Range<usize>) -> Run {
        Run {
            range,
            color: Some(self.color),
            bold: self.bold,
            italic: self.italic,
            ..Run::default()
        }
    }
}

/// The looks a palette gives the captures of one text (`Opened::captures`),
/// each looked up once through the theme: its lines are colored a chunk
/// at a time.
#[derive(Clone, Debug)]
pub struct Captured {
    looks: Vec<Look>,
    /// The look of what no capture colors: the theme's text.
    plain: Look,
}

impl Palette {
    /// How the palette draws capture `capture`: the theme's style for the
    /// scope it maps to (`scope_of`), through syntect's own lookup, else
    /// its text's.
    pub fn look_of(&self, capture: &str) -> Look {
        self.captured(&[capture.to_owned()]).look(0)
    }

    /// The looks of `captures`, in their order.
    pub fn captured(&self, captures: &[String]) -> Captured {
        let lookup = syntect::highlighting::Highlighter::new(self.colors());
        let plain = Look::of(lookup.style_for_stack(&[]), self.styled);
        let looks = captures
            .iter()
            .map(|capture| {
                scope_of(capture)
                    .and_then(|scope| ScopeStack::from_str(scope).ok())
                    .map_or(plain, |stack| {
                        Look::of(lookup.style_for_stack(stack.as_slice()), self.styled)
                    })
            })
            .collect();
        Captured { looks, plain }
    }
}

impl Captured {
    /// The look of capture `capture` (`Span::capture`).
    pub fn look(&self, capture: u16) -> Look {
        self.looks
            .get(usize::from(capture))
            .copied()
            .unwrap_or(self.plain)
    }

    /// The runs of `text`, the bytes from `from` of a line whose spans are
    /// `spans` (in the line's own bytes, in order, apart): in `text`'s own
    /// coordinates, each of its bytes in one, a span cut where `text`
    /// starts or ends. A row of a long line is such a part of it.
    pub fn runs(&self, spans: &[Span], from: usize, text: &str) -> Vec<Run> {
        let end = from + text.len();
        let mut runs: Vec<Run> = Vec::new();
        let mut push = |look: Look, range: Range<usize>| {
            // A span ends at a character's boundary in the text parsed;
            // one that would not in `text` is moved back to one.
            let (start, stop) = (
                boundary(text, range.start - from),
                boundary(text, range.end - from),
            );
            if start >= stop {
                return;
            }
            match runs.last_mut() {
                Some(last)
                    if last.range.end == start
                        && last.color == Some(look.color)
                        && (last.bold, last.italic) == (look.bold, look.italic) =>
                {
                    last.range.end = stop;
                }
                _ => runs.push(look.run(start..stop)),
            }
        };
        let mut at = from;
        for span in spans {
            let (start, stop) = (span.start as usize, span.end as usize);
            if start >= end {
                break;
            }
            let (start, stop) = (start.max(at), stop.min(end));
            if start >= stop {
                continue;
            }
            if at < start {
                push(self.plain, at..start);
            }
            push(self.look(span.capture), start..stop);
            at = stop;
        }
        if at < end {
            push(self.plain, at..end);
        }
        runs
    }
}

/// `at` in `text`, moved back to a character's boundary.
fn boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// A text parsed by a tree-sitter grammar in a helper, and the looks a
/// palette gives its captures: colored a few lines at a time where it is
/// shown, folded and outlined as its grammar's queries say. Dropping it
/// forgets the text in the helper without waiting. Its calls block: a
/// worker's work.
pub struct Tree<'h> {
    helper: &'h Helper,
    doc: tree_sitter::Doc,
    lines: u32,
    captured: Captured,
}

impl Tree<'static> {
    /// `text`, UTF-8, parsed by the grammar named `grammar` in the helper
    /// every caller shares (`tree_sitter::shared`).
    ///
    /// # Errors
    /// The helper's: not installed, the grammar failing to load, a text
    /// too long, a timeout, a cancel.
    pub fn open(
        grammar: &str,
        text: &[u8],
        palette: &Palette,
        cancel: &CancelToken,
    ) -> anyhow::Result<Tree<'static>> {
        Tree::open_in(tree_sitter::shared(), grammar, text, palette, cancel)
    }
}

impl<'h> Tree<'h> {
    /// `Tree::open` in `helper`.
    ///
    /// # Errors
    /// As `Tree::open`.
    pub fn open_in(
        helper: &'h Helper,
        grammar: &str,
        text: &[u8],
        palette: &Palette,
        cancel: &CancelToken,
    ) -> anyhow::Result<Tree<'h>> {
        let opened = helper.open(grammar, text, cancel)?;
        Ok(Tree {
            helper,
            captured: palette.captured(&opened.captures),
            lines: opened.lines,
            doc: opened.doc,
        })
    }

    /// How many lines the text holds.
    pub fn lines(&self) -> u32 {
        self.lines
    }

    /// The spans of lines `lines`, each line's in its own bytes; those past
    /// the text's last line left out.
    ///
    /// # Errors
    /// The helper's: one that ended, a timeout.
    pub fn spans(&self, lines: Range<u32>) -> anyhow::Result<Vec<Vec<Span>>> {
        let asked = lines.end.min(self.lines).saturating_sub(lines.start);
        let mut spans = Vec::with_capacity(asked as usize);
        let mut from = lines.start;
        while from < lines.end.min(self.lines) {
            let to = from.saturating_add(MAX_LINES).min(lines.end);
            spans.extend(self.helper.colors(&self.doc, from..to, &CancelToken::new())?);
            from = to;
        }
        Ok(spans)
    }

    /// The runs of `text`, the bytes from `from` of a line whose spans are
    /// `spans` (`Captured::runs`).
    pub fn runs(&self, spans: &[Span], from: usize, text: &str) -> Vec<Run> {
        self.captured.runs(spans, from, text)
    }

    /// Where it folds, as the code view folds (`Inspection::folds`): lines
    /// `start..end`, the first and the last shown and those between hidden,
    /// one line at least; nested or apart, in order of `start`.
    ///
    /// # Errors
    /// The helper's.
    pub fn folds(&self) -> anyhow::Result<Vec<Range<usize>>> {
        Ok(nest(self.helper.folds(&self.doc, &CancelToken::new())?))
    }

    /// The definitions it holds, in the text's order.
    ///
    /// # Errors
    /// The helper's.
    pub fn outline(&self) -> anyhow::Result<Vec<Symbol>> {
        self.helper.outline(&self.doc, &CancelToken::new())
    }
}

/// The helper's folds as the code view takes them: those that hide a line
/// at least, nested or apart (one that crosses one before it left out).
fn nest(folds: Vec<(u32, u32)>) -> Vec<Range<usize>> {
    let mut folds: Vec<Range<usize>> = folds
        .into_iter()
        .map(|(start, end)| start as usize..end as usize)
        .filter(|fold| fold.end > fold.start + 1)
        .collect();
    folds.sort_by_key(|fold| (fold.start, std::cmp::Reverse(fold.end)));
    folds.dedup_by_key(|fold| fold.start);
    let mut ends: Vec<usize> = Vec::new();
    folds.retain(|fold| {
        while ends.last().is_some_and(|&end| end <= fold.start) {
            ends.pop();
        }
        if ends.last().is_some_and(|&end| fold.end > end) {
            return false;
        }
        ends.push(fold.end);
        true
    });
    folds
}

/// `text` colored whole by the grammar named `grammar` in the shared
/// helper, its runs in its own coordinates: opened, colored and let go
/// of, for a short text (a document's code block).
///
/// # Errors
/// As `Tree::open` and `Tree::spans`.
pub fn color_text(
    grammar: &str,
    text: &str,
    palette: &Palette,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<Run>> {
    color_text_in(tree_sitter::shared(), grammar, text, palette, cancel)
}

/// `color_text` in `helper`.
fn color_text_in(
    helper: &Helper,
    grammar: &str,
    text: &str,
    palette: &Palette,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<Run>> {
    anyhow::ensure!(
        text.len() <= LONGEST_LINE,
        "a block longer than {} KiB",
        LONGEST_LINE >> 10
    );
    let tree = Tree::open_in(helper, grammar, text.as_bytes(), palette, cancel)?;
    let spans = tree.spans(0..tree.lines())?;
    let mut runs = Vec::new();
    let mut offset = 0;
    for (n, line) in text.split('\n').enumerate() {
        // The helper's lines leave their `\r` out, as the code view's do.
        let line = line.strip_suffix('\r').unwrap_or(line);
        let spans = spans.get(n).map_or(&[][..], Vec::as_slice);
        runs.extend(tree.runs(spans, 0, line).into_iter().map(|mut run| {
            run.range = run.range.start + offset..run.range.end + offset;
            run
        }));
        offset += line.len() + 1;
        if text.as_bytes().get(offset - 1) == Some(&b'\r') {
            offset += 1;
        }
    }
    Ok(runs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammars::Source;
    use skwir_core::syntax::Grammar;

    #[test]
    fn captures_map_to_the_scopes_bat_s_themes_color() {
        assert_eq!(scope_of("keyword"), Some("keyword"));
        assert_eq!(scope_of("@keyword"), Some("keyword"), "its @ ignored");
        assert_eq!(scope_of("keyword.function"), Some("storage.type.function"));
        assert_eq!(scope_of("string.escape"), Some("constant.character.escape"));
        assert_eq!(scope_of("constant.builtin"), Some("constant.language"));
        assert_eq!(scope_of("number"), Some("constant.numeric"));
        assert_eq!(scope_of("type.builtin"), Some("storage.type"));
        assert_eq!(scope_of("punctuation.bracket"), Some("punctuation.section"));
        assert_eq!(scope_of("markup.link"), Some("markup.underline.link"));
        assert_eq!(scope_of("namespace"), scope_of("module"));
        // A dotted name the table does not know takes its longest prefix
        // that it does: Neovim's, Helix's.
        assert_eq!(
            scope_of("@function.method.call"),
            Some("entity.name.function")
        );
        assert_eq!(scope_of("keyword.control.repeat"), Some("keyword.control"));
        assert_eq!(
            scope_of("constant.numeric.integer"),
            Some("constant.numeric")
        );
        assert_eq!(scope_of("comment.line.double-slash"), Some("comment"));
        assert_eq!(scope_of("markup.heading.1"), Some("markup.heading"));
        // What colors nothing.
        for nothing in ["spell", "@nospell", "none", "conceal", "", "@"] {
            assert_eq!(scope_of(nothing), None, "{nothing}");
        }
        // Every scope the table gives is one syntect reads.
        for capture in [
            "attribute",
            "string.special.key",
            "comment.documentation",
            "tag.delimiter",
            "punctuation.special",
        ] {
            let scope = scope_of(capture).unwrap();
            assert!(ScopeStack::from_str(scope).is_ok(), "{scope}");
        }
    }

    fn found(name: &str, file_types: &[&str], problem: Option<&str>) -> Found {
        Found {
            grammar: Grammar {
                name: name.into(),
                ..Grammar::default()
            },
            source: Source::Helix,
            queries_from: None,
            file_types: file_types.iter().map(|&t| t.to_owned()).collect(),
            first_line: None,
            problem: problem.map(str::to_owned),
        }
    }

    #[test]
    fn the_engine_is_tree_sitter_where_chosen_or_where_bat_knows_nothing() {
        let found = [
            found("json", &["json"], None),
            found("rust", &["rs"], None),
            found("zig", &["zig"], None),
            found("broken", &["brk"], Some("no library")),
        ];
        // bat's grammars know JSON and Rust, not Zig.
        let bat = |token: &str| matches!(token, "json" | "rs" | "rust");
        let engine = |name, language, first| {
            engine_in(
                &found,
                (name, language, None),
                &GrammarChoices::automatic(first),
                bat,
                |_| None,
            )
        };
        let tree = |name: &str| Engine::TreeSitter(name.to_owned());
        // bat first: bat colors what both know, tree-sitter the rest.
        assert_eq!(engine("data.json", Some("json"), First::Bat), Engine::Bat);
        assert_eq!(engine("main.zig", Some("zig"), First::Bat), tree("zig"));
        // Tree-sitter first: tree-sitter where it has a grammar.
        assert_eq!(
            engine("data.json", Some("json"), First::TreeSitter),
            tree("json")
        );
        assert_eq!(
            engine("main.zig", Some("zig"), First::TreeSitter),
            tree("zig")
        );
        // Neither knows it, or only a grammar that cannot be used does.
        for first in [First::Bat, First::TreeSitter] {
            assert_eq!(engine("notes.txt", Some("txt"), first), Engine::Bat);
            assert_eq!(engine("x.brk", Some("brk"), first), Engine::Bat);
        }
        // A code block names its language: by the files its grammar
        // colors, else by the grammar's name.
        assert_eq!(
            engine("rust", Some("rust"), First::TreeSitter),
            tree("rust")
        );
        assert_eq!(engine("rust", Some("rust"), First::Bat), Engine::Bat);
        assert_eq!(engine("zig", Some("zig"), First::Bat), tree("zig"));
        assert_eq!(
            engine("broken", Some("broken"), First::TreeSitter),
            Engine::Bat
        );
        // No grammars found: bat's.
        assert_eq!(
            engine_in(
                &[],
                ("main.zig", Some("zig"), None),
                &GrammarChoices::automatic(First::TreeSitter),
                bat,
                |_| None
            ),
            Engine::Bat
        );
        assert_eq!(First::of(Some("tree_sitter")), First::TreeSitter);
        assert_eq!(First::of(Some("bat")), First::Bat);
        assert_eq!(First::of(None), First::Bat);
        assert_eq!(First::CHOICES[0].0, "bat", "the default first");
    }

    /// A language's own choice comes before Automatic's rule: bat, its
    /// tree-sitter grammar (while the source chosen is the one used), or
    /// no colors, a language bat alone knows found by bat's grammar.
    #[test]
    fn a_language_s_own_choice_comes_first() {
        let found = [found("json", &["json"], None), found("zig", &["zig"], None)];
        let bat = |token: &str| matches!(token, "json" | "vue");
        let bat_name = |token: &str| (token == "vue").then(|| "vue component".to_owned());
        let engine = |name, language, chosen: &[(&str, &str)]| {
            let table = chosen
                .iter()
                .map(|(language, key)| ((*language).to_owned(), (*key).to_owned()))
                .collect();
            let grammars = GrammarChoices {
                first: First::Bat,
                chosen: Arc::new(chosen_in(&table)),
            };
            engine_in(&found, (name, language, None), &grammars, bat, bat_name)
        };
        let tree = |name: &str| Engine::TreeSitter(name.to_owned());
        // bat first, but JSON chosen from the source that keeps its grammar.
        assert_eq!(engine("a.json", Some("json"), &[]), Engine::Bat);
        assert_eq!(
            engine("a.json", Some("json"), &[("JSON", "helix")]),
            tree("json")
        );
        // A source that does not keep it: Automatic.
        assert_eq!(
            engine("a.json", Some("json"), &[("json", "zed")]),
            Engine::Bat
        );
        // bat for one tree-sitter colors otherwise; no colors at all.
        assert_eq!(engine("b.zig", Some("zig"), &[("zig", "bat")]), Engine::Bat);
        assert_eq!(
            engine("b.zig", Some("zig"), &[("zig", "plain")]),
            Engine::Plain
        );
        // A language only bat knows, by its grammar's name.
        assert_eq!(
            engine("c.vue", Some("vue"), &[("vue component", "plain")]),
            Engine::Plain
        );
        // A choice no longer known is Automatic.
        assert_eq!(
            engine("b.zig", Some("zig"), &[("zig", "gone")]),
            tree("zig")
        );
        assert_eq!(
            Choice::of_key("neovim"),
            Some(Choice::TreeSitter(Source::Neovim))
        );
        assert_eq!(Choice::TreeSitter(Source::TreeSitter).key(), "tree-sitter");
    }

    #[test]
    fn captures_wear_the_theme_s_colors() {
        let palette = Palette::of(&super::super::CodeTheme::own(false));
        let (keyword, comment) = (palette.look_of("keyword"), palette.look_of("comment"));
        assert_ne!(keyword.color, comment.color, "base16 Ocean light");
        // The same color bat gives its own keywords and comments.
        let bat = |language, line| {
            super::super::Highlighter::new(Some(language), &palette)
                .line(line)
                .first()
                .and_then(|run| run.color)
        };
        assert_eq!(Some(comment.color), bat("sh", "# a comment"));
        assert_eq!(Some(keyword.color), bat("rs", "if"));
        // What no scope colors is the theme's text, a dotted name its
        // prefix's.
        let plain = palette.look_of("nospell");
        assert_eq!(palette.look_of("@spell"), plain);
        assert_ne!(plain.color, keyword.color);
        assert_eq!(palette.look_of("keyword.coroutine"), keyword);
        // skwir's own colors draw no font styles; a theme chosen does.
        assert!(!palette.look_of("markup.bold").bold);
        let latte = Palette::of(&super::super::CodeTheme {
            name: "Catppuccin Latte".into(),
            ..super::super::CodeTheme::own(false)
        });
        assert!(latte.look_of("markup.bold").bold);
        assert!(latte.look_of("markup.italic").italic);
    }

    #[test]
    fn a_row_takes_the_spans_of_its_line_from_where_it_starts() {
        let palette = Palette::of(&super::super::CodeTheme::own(false));
        let captured = palette.captured(&["string".into(), "number".into()]);
        let (string, number, plain) = (captured.look(0), captured.look(1), captured.plain);
        assert_eq!(captured.look(7), plain, "a capture out of the list");
        let span = |start, end, capture| Span {
            start,
            end,
            capture,
        };
        // `"ab": 12` — the key a string, the number a number.
        let line = "\"ab\": 12";
        let spans = [span(0, 4, 0), span(6, 8, 1)];
        let runs = captured.runs(&spans, 0, line);
        let ranges: Vec<_> = runs.iter().map(|run| run.range.clone()).collect();
        assert_eq!(ranges, [0..4, 4..6, 6..8], "every byte in a run");
        assert_eq!(
            runs.iter()
                .map(|run| run.color.unwrap())
                .collect::<Vec<_>>(),
            [string.color, plain.color, number.color]
        );
        // The same line cut into rows of 3 bytes: each row takes its part.
        let rows: Vec<Vec<(Range<usize>, [u8; 3])>> = [(0, "\"ab"), (3, "\": "), (6, "12")]
            .into_iter()
            .map(|(from, text)| {
                captured
                    .runs(&spans, from, text)
                    .into_iter()
                    .map(|run| (run.range, run.color.unwrap()))
                    .collect()
            })
            .collect();
        assert_eq!(
            rows,
            [
                vec![(0..3, string.color)],
                vec![(0..1, string.color), (1..3, plain.color)],
                vec![(0..2, number.color)],
            ]
        );
        // A span that ends inside a character of the row's text is moved
        // back to its start; an empty row has no run.
        let runs = captured.runs(&[span(0, 1, 1)], 0, "é!");
        assert_eq!(
            runs.iter()
                .map(|run| (run.range.clone(), run.color.unwrap()))
                .collect::<Vec<_>>(),
            [(0..3, plain.color)]
        );
        assert!(captured.runs(&spans, 0, "").is_empty());
    }

    #[test]
    fn the_helper_s_folds_are_the_code_view_s() {
        // A fold hiding nothing goes; one crossing another goes; the
        // widest of a line stays.
        assert_eq!(
            nest(vec![(0, 9), (1, 2), (2, 6), (2, 4), (5, 11), (6, 8)]),
            [0..9, 2..6, 6..8]
        );
        assert!(nest(Vec::new()).is_empty());
    }

    /// The helper built beside the tests (`cargo nextest run -p
    /// skwir-tree-sitter-worker -p skwir-platform` builds it); without it
    /// the test says so and passes, as the helper's own do.
    #[cfg(target_os = "linux")]
    fn built_helper() -> Option<std::path::PathBuf> {
        let helper = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.parent()?.join("skwir-tree-sitter-worker")))
            .filter(|path| path.is_file());
        if helper.is_none() {
            eprintln!("skipped: needs skwir-tree-sitter-worker built beside the tests");
        }
        helper
    }

    /// tree-sitter-json as WebAssembly, the helper's own fixture.
    #[cfg(target_os = "linux")]
    fn json() -> Grammar {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tree-sitter-worker/tests/fixtures/json");
        Grammar {
            name: "json".into(),
            library: fixtures.join("tree-sitter-json.wasm"),
            symbol: "tree_sitter_json".into(),
            highlights: vec![fixtures.join("queries/highlights.scm")],
            ..Grammar::default()
        }
    }

    /// A JSON text colored through the helper: each line's runs cover it,
    /// in several of the theme's colors, its numbers and its constants as
    /// bat's themes color them; a long line read as rows takes its spans
    /// row by row.
    #[cfg(target_os = "linux")]
    #[test]
    fn json_is_colored_by_its_tree_sitter_grammar() {
        let Some(executable) = built_helper() else {
            return;
        };
        let helper = tree_sitter::with_program(executable);
        helper.set_grammars(vec![json()]);
        let palette = Palette::of(&super::super::CodeTheme::own(false));
        let cancel = CancelToken::new();
        let text = "{\r\n  \"count\": 42,\n  \"ok\": [true, null, \"a\\n\"]\n}\n";
        let runs = color_text_in(&helper, "json", text, &palette, &cancel).unwrap();
        let colors: std::collections::BTreeSet<_> =
            runs.iter().filter_map(|run| run.color).collect();
        assert!(colors.len() > 2, "{runs:?}");
        let at = |part: &str| {
            let start = text.find(part).unwrap();
            runs.iter()
                .find(|run| run.range.contains(&start))
                .and_then(|run| run.color)
                .unwrap()
        };
        assert_eq!(at("42"), palette.look_of("number").color);
        assert_eq!(at("true"), palette.look_of("constant.builtin").color);
        assert_eq!(at("\\n"), palette.look_of("escape").color);
        // Every byte but the line endings is in a run, none crosses one.
        let covered: usize = runs.iter().map(|run| run.range.len()).sum();
        assert_eq!(covered, text.len() - text.matches('\n').count() - 1);
        assert!(
            runs.iter()
                .all(|run| !text[run.range.clone()].contains('\n'))
        );
        // A tree kept: a line cut into rows is colored a row at a time.
        let tree = Tree::open_in(&helper, "json", text.as_bytes(), &palette, &cancel).unwrap();
        assert_eq!(tree.lines(), 4);
        let spans = tree.spans(2..3).unwrap();
        let line = "  \"ok\": [true, null, \"a\\n\"]";
        let whole = tree.runs(&spans[0], 0, line);
        let (head, tail) = line.split_at(12);
        let mut rows = tree.runs(&spans[0], 0, head);
        rows.extend(tree.runs(&spans[0], 12, tail).into_iter().map(|mut run| {
            run.range = run.range.start + 12..run.range.end + 12;
            run
        }));
        let colors_at = |runs: &[Run]| {
            (0..line.len())
                .map(|at| {
                    runs.iter()
                        .find(|run| run.range.contains(&at))
                        .unwrap()
                        .color
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(colors_at(&rows), colors_at(&whole));
        assert!(tree.spans(9..12).unwrap().is_empty(), "past its end");
        assert!(
            tree.folds().unwrap().is_empty(),
            "the fixture folds nothing"
        );
    }
}
