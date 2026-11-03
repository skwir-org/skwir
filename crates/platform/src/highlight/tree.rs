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

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::str::FromStr;
use std::sync::Arc;

use parking_lot::Mutex;
use skwir_core::CancelToken;
use skwir_core::inspection::Run;
use skwir_core::syntax::{Span, Symbol};
use syntect::highlighting::{FontStyle, Style};
use syntect::parsing::ScopeStack;

use super::Palette;
use super::added::{Added, Table};
use crate::grammars::{self, Found, Source};
use crate::tree_sitter::{self, Helper, Text};

/// Which grammars color a language that both bat's grammars and a
/// tree-sitter grammar know (`First`).
pub const ENGINE_KEY: &str = "reading.grammars";

/// The longest line a tree-sitter grammar colors, and the longest code
/// block: the longest the helper keeps spans of (`syntax::MAX_LINE`), one
/// bound for both. bat's grammar colors a longer one.
pub const LONGEST_LINE: usize = skwir_core::syntax::MAX_LINE;

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
    /// what `grammars::sync` takes.
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

/// A language as the Reading page lists it, and as its own choice is kept
/// (`reading.grammar_for`): the tree-sitter grammar each source keeps of
/// it, usable or not, and the grammars added to bat and Sublime Text that
/// color its files.
#[derive(Clone, Debug)]
pub struct Language {
    /// Its name in lower case: what its choice is kept by.
    pub key: String,
    pub title: String,
    pub file_types: Vec<String>,
    pub tree: Vec<Found>,
    pub added: Vec<Added>,
}

/// The languages a tree-sitter grammar was found or a grammar added for,
/// by key: each source's grammar of a language beside the others (`all`
/// keeps them by language), an added grammar joining the tree-sitter
/// language of its name, else the first whose files it colors too, else
/// standing alone.
pub fn languages(all: &[Found], added: &[Added]) -> Vec<Language> {
    let mut languages: Vec<Language> = Vec::new();
    for found in all {
        let key = found.grammar.name.to_lowercase();
        match languages.last_mut() {
            Some(language) if language.key == key => language.tree.push(found.clone()),
            _ => languages.push(Language {
                key,
                title: found.grammar.name.clone(),
                file_types: found.file_types.clone(),
                tree: vec![found.clone()],
                added: Vec::new(),
            }),
        }
    }
    // Where each name and each file type is first found, so an added
    // grammar finds its language at once, however many there are.
    let mut by_key: HashMap<String, usize> = HashMap::new();
    let mut by_type: HashMap<String, usize> = HashMap::new();
    for (at, language) in languages.iter().enumerate() {
        by_key.entry(language.key.clone()).or_insert(at);
        for file_type in types_of(&language.file_types) {
            by_type.entry(file_type).or_insert(at);
        }
    }
    let mut alone: Vec<Language> = Vec::new();
    let mut alone_by_key: HashMap<String, usize> = HashMap::new();
    for added in added {
        let key = added.name.to_lowercase();
        let home = by_key.get(&key).copied().or_else(|| {
            types_of(&added.file_types)
                .iter()
                .filter_map(|file_type| by_type.get(file_type).copied())
                .min()
        });
        if let Some(at) = home {
            languages[at].added.push(added.clone());
        } else if let Some(&at) = alone_by_key.get(&key) {
            alone[at].added.push(added.clone());
        } else {
            alone_by_key.insert(key.clone(), alone.len());
            alone.push(Language {
                key,
                title: added.name.clone(),
                file_types: added.file_types.clone(),
                tree: Vec::new(),
                added: vec![added.clone()],
            });
        }
    }
    languages.extend(alone);
    languages.sort_by(|a, b| a.key.cmp(&b.key));
    languages
}

/// File types as they are matched: without a leading dot, in lower case.
fn types_of(types: &[String]) -> HashSet<String> {
    types
        .iter()
        .map(|t| t.trim_start_matches('.').to_lowercase())
        .collect()
}

/// The languages as last found and read (`languages` of `grammars::all`
/// and `added::listed`), worked out again only when either was.
pub fn listed_languages() -> Arc<[Language]> {
    type Listed = (Arc<[Found]>, Arc<[Added]>, Arc<[Language]>);
    static LISTED: Mutex<Option<Listed>> = Mutex::new(None);
    let all = grammars::all().unwrap_or_default();
    let added = super::added::listed().unwrap_or_default();
    let mut listed = LISTED.lock();
    if let Some((was_all, was_added, languages)) = listed.as_ref()
        && Arc::ptr_eq(was_all, &all)
        && Arc::ptr_eq(was_added, &added)
    {
        return Arc::clone(languages);
    }
    let languages: Arc<[Language]> = languages(&all, &added).into();
    *listed = Some((all, added, Arc::clone(&languages)));
    languages
}

/// The key of the language a text is of, as the Reading page keeps its
/// choice (`Language::key`): the text named `name`, colored as `language`
/// (`engine`'s). The language of the tree-sitter grammar that colors it
/// (`grammars::for_file`: a usable one, else any, so a language whose
/// grammars cannot be used is still the one chosen for), else of the one
/// a code block names; else the one bat's grammar for it joins, as an
/// added grammar joins one (`languages`: its own, by its name, else the
/// tree-sitter language whose files it colors too; a ` ```c++ ` block is
/// `cpp`'s); else bat's grammar's name, in lower case.
pub fn language_key(
    languages: &[Language],
    all: &[Found],
    (name, language, first_line): (&str, Option<&str>, Option<&str>),
    bat: &Table,
) -> Option<String> {
    let tree = grammars::for_file(all, name, first_line)
        .or_else(|| grammars::for_file_any(all, name, first_line))
        .or_else(|| language.and_then(|language| grammars::named(all, language)));
    if let Some(tree) = tree {
        return Some(tree.grammar.name.to_lowercase());
    }
    let named = language.and_then(|language| bat.grammar_of(language))?;
    let key = named.name.to_lowercase();
    let types = types_of(&named.file_types);
    let home = languages
        .iter()
        .find(|listed| listed.key == key)
        .or_else(|| {
            languages.iter().find(|listed| {
                listed
                    .added
                    .iter()
                    .any(|added| added.name.eq_ignore_ascii_case(&named.name))
            })
        })
        .or_else(|| {
            languages.iter().find(|listed| {
                !listed.tree.is_empty() && !types_of(&listed.file_types).is_disjoint(&types)
            })
        });
    Some(home.map_or(key, |listed| listed.key.clone()))
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
/// line. Its language's own choice first (`grammars.chosen`, by the key
/// the Reading page keeps it by, `language_key`): bat's, the tree-sitter
/// grammar of the source chosen (Automatic when it cannot be used), or
/// nothing. Automatic: the tree-sitter grammar used for it where
/// `grammars.first` chooses it, or where bat's grammars know no
/// `language`; else bat's. The grammar used for each language is picked
/// under these very choices (`grammars::sync`), whoever made them: a
/// choice made in another window, by hand or by a script is honoured.
/// Before the grammars are found, or with none, bat's unless chosen
/// Plain. It may read what bat's set holds: a worker's work.
pub fn engine(
    name: &str,
    language: Option<&str>,
    first_line: Option<&str>,
    grammars: &GrammarChoices,
) -> Engine {
    let found = grammars::sync(&grammars.sources());
    let all = grammars::all().unwrap_or_default();
    engine_in(
        (&found, &all, &listed_languages()),
        (name, language, first_line),
        grammars,
        &super::added::table_now(),
    )
}

/// `engine` among the grammars `found` (one for each language, as used),
/// `all` (every source's) and the languages they make, `bat` saying what
/// bat's grammars color.
fn engine_in(
    (found, all, languages): (&[Found], &[Found], &[Language]),
    file: (&str, Option<&str>, Option<&str>),
    grammars: &GrammarChoices,
    bat: &Table,
) -> Engine {
    let (name, language, first_line) = file;
    // By the file's name, else by the language a code block names
    // (`rust`, where its grammar colors `.rs`).
    let tree = grammars::for_file(found, name, first_line).or_else(|| {
        language
            .and_then(|language| grammars::named(found, language))
            .filter(|found| found.problem.is_none())
    });
    if !grammars.chosen.is_empty() {
        let key = language_key(languages, all, file, bat);
        match key.and_then(|key| grammars.chosen.get(&key)) {
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
    if grammars.first == First::Bat && language.is_some_and(|token| bat.colors(token).is_some()) {
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
        // The spans that end before the row starts are passed over at once,
        // not one by one: a row deep in a long line starts after thousands.
        let first = spans.partition_point(|span| span.end as usize <= from);
        for span in &spans[first..] {
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
    /// every caller shares (`tree_sitter::shared`). A text a preview holds
    /// is shared with it (`Text::shared`): the tree keeps it, to parse it
    /// again in a helper started later, never a copy of it.
    ///
    /// # Errors
    /// The helper's: not installed, the grammar failing to load, a text
    /// too long, a timeout, a cancel.
    pub fn open(
        grammar: &str,
        text: impl Into<Text>,
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
        text: impl Into<Text>,
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
    /// the text's last line left out. `cancel` stops the wait.
    ///
    /// # Errors
    /// The helper's: one that ended, a timeout, a cancel.
    pub fn spans(&self, lines: Range<u32>, cancel: &CancelToken) -> anyhow::Result<Vec<Vec<Span>>> {
        let end = lines.end.min(self.lines);
        if lines.start >= end {
            return Ok(Vec::new());
        }
        self.helper.colors(&self.doc, lines.start..end, cancel)
    }

    /// The runs of `text`, the bytes from `from` of a line whose spans are
    /// `spans` (`Captured::runs`).
    pub fn runs(&self, spans: &[Span], from: usize, text: &str) -> Vec<Run> {
        self.captured.runs(spans, from, text)
    }

    /// Where it folds, as the code view folds (`Inspection::folds`): lines
    /// `start..end`, the first and the last shown and those between hidden,
    /// one line at least; nested or apart, in order of `start`. `cancel`
    /// stops the wait.
    ///
    /// # Errors
    /// The helper's, a cancel.
    pub fn folds(&self, cancel: &CancelToken) -> anyhow::Result<Vec<Range<usize>>> {
        anyhow::ensure!(!cancel.is_cancelled(), "cancelled");
        Ok(nest(self.helper.folds(&self.doc, cancel)?))
    }

    /// The definitions it holds, in the text's order. `cancel` stops the
    /// wait.
    ///
    /// # Errors
    /// The helper's, a cancel.
    pub fn outline(&self, cancel: &CancelToken) -> anyhow::Result<Vec<Symbol>> {
        anyhow::ensure!(!cancel.is_cancelled(), "cancelled");
        self.helper.outline(&self.doc, cancel)
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
    let spans = tree.spans(0..tree.lines(), cancel)?;
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
            queries_at: None,
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
        let bat = table(&[
            ("JSON", &["json"]),
            ("Rust", &["rs"]),
            ("Plain Text", &["txt"]),
        ]);
        let listed = languages(&found, &[]);
        let engine = |name, language, first| {
            engine_in(
                (&found, &found, &listed),
                (name, language, None),
                &GrammarChoices::automatic(first),
                &bat,
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
                (&[], &[], &[]),
                ("main.zig", Some("zig"), None),
                &GrammarChoices::automatic(First::TreeSitter),
                &bat,
            ),
            Engine::Bat
        );
        assert_eq!(First::of(Some("tree_sitter")), First::TreeSitter);
        assert_eq!(First::of(Some("bat")), First::Bat);
        assert_eq!(First::of(None), First::Bat);
        assert_eq!(First::CHOICES[0].0, "bat", "the default first");
    }

    /// bat's set as a table of grammars and the files they color.
    fn table(grammars: &[(&str, &[&str])]) -> Table {
        Table::new(
            grammars
                .iter()
                .map(|(name, types)| super::super::added::Named {
                    name: (*name).to_owned(),
                    file_types: types.iter().map(|&t| t.to_owned()).collect(),
                })
                .collect(),
        )
    }

    fn added(name: &str, file_types: &[&str]) -> Added {
        Added {
            name: name.into(),
            source: super::super::added::Source::Bat,
            path: std::path::PathBuf::from(format!("{name}.sublime-syntax")),
            member: None,
            file_types: file_types.iter().map(|&t| t.to_owned()).collect(),
            problem: None,
        }
    }

    /// A choice is kept by the key the Reading page lists its language
    /// by, and honoured by that key whatever colors the text: a language
    /// whose tree-sitter grammars cannot be used is still the one chosen
    /// for, not bat's name for it; an added grammar's, by where it is
    /// listed.
    #[test]
    fn a_choice_is_found_by_the_key_its_language_is_listed_by() {
        let all = [
            found("bash", &["sh", "bash"], Some("no highlights query")),
            found("cpp", &["cpp", "cc"], None),
        ];
        let used = all.clone();
        let bat = table(&[
            ("Bourne Again Shell (bash)", &["sh", "bash"]),
            ("C++", &["cpp", "cc", "c++"]),
            ("Skwirlang", &["skwirl"]),
            ("Plain Text", &["txt"]),
        ]);
        let added_ones = [added("Skwirlang", &["skwirl"]), added("Cpp Extra", &["cc"])];
        let listed = languages(&all, &added_ones);
        let keys: Vec<&str> = listed.iter().map(|l| l.key.as_str()).collect();
        assert_eq!(keys, ["bash", "cpp", "skwirlang"]);
        assert_eq!(listed[1].added.len(), 1, "joined by a file type");
        let key = |name, language| language_key(&listed, &all, (name, language, None), &bat);
        assert_eq!(key("x.sh", Some("sh")).as_deref(), Some("bash"));
        assert_eq!(key("sh", Some("sh")).as_deref(), Some("bash"), "a block");
        assert_eq!(
            key("c++", Some("c++")).as_deref(),
            Some("cpp"),
            "bat's joins"
        );
        assert_eq!(
            key("a.skwirl", Some("skwirl")).as_deref(),
            Some("skwirlang")
        );
        assert_eq!(key("notes.txt", Some("txt")), None, "plain text");
        let engine = |name, language, chosen: &[(&str, &str)]| {
            let table = chosen
                .iter()
                .map(|(language, key)| ((*language).to_owned(), (*key).to_owned()))
                .collect();
            let grammars = GrammarChoices {
                first: First::Bat,
                chosen: Arc::new(chosen_in(&table)),
            };
            engine_in(
                (&used, &all, &listed),
                (name, language, None),
                &grammars,
                &bat,
            )
        };
        // Its only grammar cannot be used: bat colors it, unless chosen
        // Plain text by the key it is listed by.
        assert_eq!(engine("x.sh", Some("sh"), &[]), Engine::Bat);
        assert_eq!(
            engine("x.sh", Some("sh"), &[("bash", "plain")]),
            Engine::Plain
        );
        assert_eq!(
            engine("a.skwirl", Some("skwirl"), &[("skwirlang", "plain")]),
            Engine::Plain
        );
        assert_eq!(
            engine("a.cpp", Some("cpp"), &[("cpp", "helix")]),
            Engine::TreeSitter("cpp".into())
        );
    }

    /// A language's own choice comes before Automatic's rule: bat, its
    /// tree-sitter grammar (while the source chosen is the one used), or
    /// no colors, a language bat alone knows found by bat's grammar.
    #[test]
    fn a_language_s_own_choice_comes_first() {
        let found = [found("json", &["json"], None), found("zig", &["zig"], None)];
        let bat = table(&[("JSON", &["json"]), ("Vue Component", &["vue"])]);
        let listed = languages(&found, &[]);
        let engine = |name, language, chosen: &[(&str, &str)]| {
            let table = chosen
                .iter()
                .map(|(language, key)| ((*language).to_owned(), (*key).to_owned()))
                .collect();
            let grammars = GrammarChoices {
                first: First::Bat,
                chosen: Arc::new(chosen_in(&table)),
            };
            engine_in(
                (&found, &found, &listed),
                (name, language, None),
                &grammars,
                &bat,
            )
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
        // A row deep in a long line, a span every two bytes: its own part,
        // from its first byte, whether a span starts there or not.
        let long: Vec<Span> = (0..50_000u32)
            .map(|n| span(4 * n, 4 * n + 2, u16::from(n % 2 == 1)))
            .collect();
        for from in [120_000usize, 120_001, 120_002, 199_998] {
            let text = "x".repeat(10);
            let runs = captured.runs(&long, from, &text);
            assert_eq!(runs.first().map(|run| run.range.start), Some(0), "{from}");
            assert_eq!(runs.last().map(|run| run.range.end), Some(text.len()));
            let expected = |at: usize| match at % 4 {
                0 | 1 if at < 200_000 => {
                    if (at / 4) % 2 == 1 {
                        number.color
                    } else {
                        string.color
                    }
                }
                _ => plain.color,
            };
            for run in &runs {
                for at in run.range.clone() {
                    assert_eq!(run.color, Some(expected(from + at)), "{from} {at}");
                }
            }
        }
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
        let spans = tree.spans(2..3, &cancel).unwrap();
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
        assert!(
            tree.spans(9..12, &cancel).unwrap().is_empty(),
            "past its end"
        );
        assert!(
            tree.folds(&cancel).unwrap().is_empty(),
            "the fixture folds nothing"
        );
    }
}
