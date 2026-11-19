//! A plugin's own words (protocol 2, ADR-139, 14). Its catalogs are
//! `l10n/<language>.ftl` beside its manifest, `en.ftl` its English, written
//! by its author; Fluent, in the part Skwir speaks (`skwir_core::words`).
//! They are read when the plugin is found, as its manifest is, held to
//! bounds and parsed defensively, since they are anyone's; registered while
//! the plugin is on (`words::register`, its id the owner, withdrawn with its
//! other contributions, `App::unregister_owner`); and kept, from when it is
//! found, as what says the words describing it while it is off
//! (`words::describe_with`): its title and what it does, on the Plugins
//! page, nothing else. What the plugin says, a
//! string or a message of them, becomes Skwir's [`Words`], said where it is
//! drawn or sent: in the window's language, English for agents. A message
//! the reader's language lacks is said in English, one its English lacks as
//! its id: never half a sentence, never a crash.
//!
//! A plugin on protocol 1 has none: what it writes is shown as it writes it.

use std::path::Path;

use skwir_core::words::{self, Arg, Catalog, Words};
use skwir_plugin_api::manifest::CATALOGS;
use skwir_plugin_api::protocol::{self, RpcError};

/// The largest catalog read, in bytes: a plugin's messages in one language
/// are a few kilobytes; Skwir's largest crate says some 6,000 in 600 KiB.
pub(crate) const CATALOG_LIMIT: u64 = 256 * 1024;

/// The most languages read of one plugin: past them, the rest are left
/// out, said in English.
pub(crate) const LANGUAGES: usize = 64;

/// How deep a catalog's placeables and calls may nest: Skwir's own go three
/// deep (a selector in a selector). Fluent's parser recurses as they nest,
/// so a catalog nesting thousands deep would take the window down with it.
pub(crate) const DEPTH: usize = 16;

/// What a plugin from outside says, its catalogs read.
pub(crate) struct Catalogs {
    /// What its catalogs are registered under: `outside:<id>`, a name no
    /// crate of Skwir's can have, so that no plugin's catalog stands in for
    /// Skwir's own words.
    domain: &'static str,
    /// Its English (`en.ftl`): never registered (English is no catalog the
    /// registry says), each message read from it the first time it is said.
    english: Option<Catalog>,
    /// Its other languages: each tag and its catalog's text, kept for the
    /// program's life as the plugin is.
    translations: Vec<(&'static str, &'static str)>,
}

impl Catalogs {
    /// The catalogs of the plugin `id` in `folder`, speaking protocol `api`:
    /// none below protocol 2. One that is no catalog, over its bound, not
    /// UTF-8, nesting too deep or that Fluent's parser cannot read is left
    /// out, and the log says why.
    pub fn load(folder: &Path, id: &str, api: u32) -> Catalogs {
        let mut catalogs = Catalogs {
            domain: crate::intern(&format!("outside:{id}")),
            english: None,
            translations: Vec::new(),
        };
        if api < 2 {
            return catalogs;
        }
        let Ok(entries) = std::fs::read_dir(folder.join(CATALOGS)) else {
            return catalogs;
        };
        let mut paths: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
        paths.sort();
        let mut languages = 0;
        for path in paths {
            let Some(language) = language_of(&path) else {
                continue;
            };
            if languages == LANGUAGES {
                tracing::warn!(
                    plugin = id,
                    "catalogs past the first {LANGUAGES} languages are left out"
                );
                break;
            }
            languages += 1;
            let source = match read(&path) {
                Ok(source) => source,
                Err(why) => {
                    tracing::warn!(plugin = id, "{}: left out: {why}", path.display());
                    continue;
                }
            };
            // Kept as long as the plugin is, the program's life: each
            // message without arguments is said from it as it lies.
            let source: &'static str = Box::leak(source.into_boxed_str());
            if language == "en" {
                catalogs.english = Some(Catalog::new("en", source));
            } else {
                catalogs
                    .translations
                    .push((crate::intern(&language), source));
            }
        }
        catalogs
    }

    /// Whether its English says message `id`: one it does not is said as
    /// its id.
    pub fn says(&self, id: &str) -> bool {
        self.english
            .as_ref()
            .is_some_and(|english| english.message(id).is_some())
    }

    /// Its catalogs registered as `owner`'s (the plugin's id): withdrawn
    /// with its other contributions when it is turned off.
    pub fn register(&self, owner: &str) {
        for (language, source) in &self.translations {
            words::register(owner, self.domain, Catalog::new(language, *source));
        }
    }

    /// Its catalogs kept as what says the words describing it (its title,
    /// what it does) while it is off, on the Plugins page where a reader
    /// decides to turn it on (`words::describe_with`): the host keeps them
    /// with its list of plugins. Nothing else it says is said from them.
    pub fn describe(&self) {
        words::describe_with(
            self.domain,
            self.translations
                .iter()
                .map(|(language, source)| Catalog::new(language, *source))
                .collect(),
        );
    }

    /// What `said` says, as Skwir's words: text as written, or a message of
    /// the plugin's catalogs.
    pub fn said(&self, said: &protocol::Words) -> Words {
        match said {
            protocol::Words::Written(text) => Words::as_written(text.clone()),
            protocol::Words::Message { id, args } => {
                let english = self
                    .english
                    .as_ref()
                    .and_then(|english| english.message(id));
                let args = args
                    .iter()
                    .map(|(name, arg)| (name.clone(), arg_of(arg)))
                    .collect();
                Words::of_catalog(self.domain, id.clone(), english.as_deref(), args)
            }
        }
    }

    /// What an error the plugin answered says: its words on protocol 2
    /// (`RpcError::words`), its message as written on protocol 1.
    pub fn error(&self, api: u32, error: &RpcError) -> Words {
        if api < 2 {
            return Words::as_written(error.message.clone());
        }
        self.said(&error.words())
    }
}

/// A message's argument, as Skwir says one of its type.
fn arg_of(arg: &protocol::Arg) -> Arg {
    match arg {
        protocol::Arg::Count(n) => Arg::Count(*n),
        protocol::Arg::Text(text) => Arg::Text(text.clone().into()),
        protocol::Arg::Name(name) => Arg::Name(name.clone()),
        protocol::Arg::Size(bytes) => Arg::Size(*bytes),
        protocol::Arg::Folder(name) => Arg::Folder(name.clone()),
        protocol::Arg::Decimal { value, digits } => Arg::Decimal(*value, *digits),
        protocol::Arg::Moment { at, shape } => Arg::Moment(skwir_core::words::Moment {
            at: *at,
            shape: match shape {
                protocol::Shape::Date => skwir_core::words::Shape::Date,
                protocol::Shape::Time => skwir_core::words::Shape::Time,
                protocol::Shape::DateTime => skwir_core::words::Shape::DateTime,
            },
        }),
    }
}

/// The language a catalog at `path` is in: its name's stem, `fr` of
/// `fr.ftl`, where that is a language's tag (letters and digits in subtags
/// of one to eight, parted by `-`, beginning with a language's two or three
/// letters); `None` for any other file.
fn language_of(path: &Path) -> Option<String> {
    if path.extension().is_none_or(|e| e != "ftl") {
        return None;
    }
    let tag = path.file_stem()?.to_str()?;
    let mut subtags = tag.split('-');
    let first = subtags.next()?;
    let ok = (2..=3).contains(&first.len())
        && first.chars().all(|c| c.is_ascii_alphabetic())
        && subtags.all(|subtag| {
            (1..=8).contains(&subtag.len()) && subtag.chars().all(|c| c.is_ascii_alphanumeric())
        })
        && tag.len() <= 35;
    ok.then(|| tag.to_owned())
}

/// Why a catalog is left out.
#[derive(Debug)]
pub(crate) enum Refused {
    Unread(std::io::Error),
    NotText,
    TooDeep,
    Unparsed,
}

impl std::fmt::Display for Refused {
    /// For the log, in English.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::Unread(error) => write!(f, "{error} (at most {CATALOG_LIMIT} bytes)"),
            Refused::NotText => f.write_str("not UTF-8"),
            Refused::TooDeep => write!(f, "placeables nested more than {DEPTH} deep"),
            Refused::Unparsed => f.write_str("Fluent's parser could not read it"),
        }
    }
}

/// The catalog at `path`, read whole if it is a regular file within
/// [`CATALOG_LIMIT`], in UTF-8 (a byte-order mark dropped), nesting no
/// deeper than [`DEPTH`], and read by Fluent's parser without failing.
pub(crate) fn read(path: &Path) -> Result<String, Refused> {
    let bytes = skwir_core::fs::file::read_regular(path, CATALOG_LIMIT).map_err(Refused::Unread)?;
    let text = String::from_utf8(bytes).map_err(|_| Refused::NotText)?;
    let text = match text.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_owned(),
        None => text,
    };
    checked(text)
}

/// `text`, a catalog, where it nests no deeper than [`DEPTH`] and Fluent's
/// parser reads it whole without failing: a message it cannot read is said
/// in English, but a parser that stopped would stop the window.
pub(crate) fn checked(text: String) -> Result<String, Refused> {
    if depth(&text) > DEPTH {
        return Err(Refused::TooDeep);
    }
    // Errors are messages it cannot read: they are said in English. Only a
    // parser that gives up on the whole is refused.
    let parsed = std::panic::catch_unwind(|| {
        let _ = fluent_syntax::parser::parse_runtime(text.as_str());
    });
    match parsed {
        Ok(()) => Ok(text),
        Err(_) => Err(Refused::Unparsed),
    }
}

/// How deep `text`'s placeables and calls may nest as Fluent's parser reads
/// it, at most: never less than it, so that a catalog nesting deeper than
/// [`DEPTH`] is refused before that parser, which recurses as they nest,
/// overflows the window's stack. Counting more is safe (a catalog refused
/// is said in English); counting less would let one `.ftl` take Skwir down
/// at every start.
///
/// Every `{` opens a level, and every `(` inside one, wherever it stands,
/// unless it is surely inside a string literal: one begun by a `"` right
/// after a `{` (only an expression follows it). A closer closes a level only
/// where no literal can hide it: a `"` elsewhere may begin one (in an
/// expression) or be text (in a variant's text), so after it a `)` or `}` is
/// not counted, until the next `"` closes either (text and literal alike, no
/// `{`, `}` or `]` between them that could have changed which), or the line
/// ends, which ends any literal. A `)` closes only a `(` (text holds `)` as
/// it is), a `}` the last `{` and the `(` opened inside it. A line beginning
/// with `#` or with an id and `=` is where Fluent's parser stands outside
/// every placeable, whatever came before: an id followed by `=` is no
/// expression, nor `#`, nor text at a line's start.
fn depth(text: &str) -> usize {
    /// Where a line is read, as literals go.
    #[derive(Clone, Copy, PartialEq)]
    enum At {
        /// Surely in no string literal.
        Outside,
        /// After a `"` that began a literal or was text: closers may be the
        /// literal's.
        Quoted { escaped: bool },
        /// Where which `"` began a literal cannot be told: no closer counts
        /// until the line ends.
        Unknown,
        /// Surely in a string literal (begun right after a `{`).
        Literal { escaped: bool },
    }
    // The levels open: `true` a placeable's `{`, `false` a call's `(`.
    let mut open: Vec<bool> = Vec::new();
    let mut deepest = 0;
    for line in text.split_inclusive('\n') {
        if line.starts_with('#') || begins_message(line) {
            open.clear();
        }
        let mut at = At::Outside;
        // Whether the last character read, spaces apart, was a `{` read
        // outside any literal.
        let mut after_brace = false;
        for b in line.bytes() {
            match at {
                At::Literal { escaped: true } => at = At::Literal { escaped: false },
                At::Literal { escaped: false } => match b {
                    b'\\' => at = At::Literal { escaped: true },
                    b'"' => {
                        at = At::Outside;
                        after_brace = false;
                    }
                    _ => {}
                },
                At::Quoted { escaped: true } if !matches!(b, b'{' | b'}' | b']') => {
                    at = At::Quoted { escaped: false };
                }
                At::Quoted { .. } | At::Unknown | At::Outside => {
                    let outside = at == At::Outside;
                    match b {
                        b'{' => {
                            open.push(true);
                            if matches!(at, At::Quoted { .. }) {
                                at = At::Unknown;
                            }
                        }
                        b'(' if !open.is_empty() => open.push(false),
                        b')' if outside && open.last() == Some(&false) => {
                            open.pop();
                        }
                        b'}' if outside => {
                            while open.last() == Some(&false) {
                                open.pop();
                            }
                            open.pop();
                        }
                        b'}' | b']' if matches!(at, At::Quoted { .. }) => at = At::Unknown,
                        b'"' if outside && after_brace => at = At::Literal { escaped: false },
                        b'"' if outside && !open.is_empty() => at = At::Quoted { escaped: false },
                        b'"' if matches!(at, At::Quoted { .. }) => at = At::Outside,
                        b'\\' if matches!(at, At::Quoted { .. }) => {
                            at = At::Quoted { escaped: true };
                        }
                        _ => {}
                    }
                    if b != b' ' {
                        after_brace = outside && b == b'{';
                    }
                }
            }
            deepest = deepest.max(open.len());
            if deepest > DEPTH {
                return deepest;
            }
        }
    }
    deepest
}

/// Whether `line` begins a message or a term, as Fluent writes one: at its
/// start, an id (a letter, then letters, digits, `_` and `-`), `-` before
/// it for a term, and `=` after it and spaces.
fn begins_message(line: &str) -> bool {
    let line = line.strip_prefix('-').unwrap_or(line);
    if !line.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return false;
    }
    let rest = line.trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    rest.trim_start_matches([' ', '\t']).starts_with('=')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plugin's folder, decimal and moment reach Skwir as arguments of
    /// their kind: said as the reader's language writes each.
    #[test]
    fn folders_decimals_and_moments_are_said_as_their_kind() {
        let english = skwir_core::words::Catalog::new(
            "en",
            "made = { $ratio }× smaller in { $folder } on { $when }\n".to_owned(),
        );
        let said = english.message("made").expect("the English says it");
        let args = [
            ("ratio", protocol::Arg::decimal(1.5, 1)),
            ("folder", protocol::Arg::folder("Docs")),
            // Noon: the same day in every time zone a test runs in.
            ("when", protocol::Arg::moment(43_200, protocol::Shape::Date)),
        ]
        .into_iter()
        .map(|(name, arg)| (name.to_owned(), arg_of(&arg)))
        .collect();
        let words = Words::of_catalog("outside:test", "made", Some(&said), args);
        // As an agent reads it: English's own date, whatever the window's.
        assert_eq!(words.english(), "1.5× smaller in Docs on Jan 1, 1970");
        assert_eq!(
            arg_of(&protocol::Arg::moment(5, protocol::Shape::DateTime)),
            Arg::Moment(skwir_core::words::Moment {
                at: 5,
                shape: skwir_core::words::Shape::DateTime,
            })
        );
    }

    #[test]
    fn a_catalog_is_named_by_its_language() {
        let named = |name: &str| language_of(Path::new(name));
        assert_eq!(named("fr.ftl").as_deref(), Some("fr"));
        assert_eq!(named("zh-Hans.ftl").as_deref(), Some("zh-Hans"));
        assert_eq!(named("sr-Latn-RS.ftl").as_deref(), Some("sr-Latn-RS"));
        assert_eq!(named("en.ftl").as_deref(), Some("en"));
        for not in [
            "fr.txt",
            "fr",
            "f.ftl",
            "french.ftl",
            "fr--x.ftl",
            "fr-.ftl",
            "../fr.ftl.bak",
            "fr-toolongsubtag.ftl",
        ] {
            assert_eq!(named(not), None, "{not}");
        }
    }

    /// Nesting is bounded before Fluent's parser, which recurses, reads a
    /// catalog: deep braces or calls are refused, text and string literals
    /// holding them are not.
    #[test]
    fn a_catalog_nesting_too_deep_is_refused() {
        let ok = "a = { $n ->\n    [one] { $kind ->\n        [x] y\n       *[other] z\n    }\n   *[other] (w)\n}\nb = (((((((((((((((((((( text\nc = { \"{{{{{{{{{{{{{{{{{{{{\" }\n";
        assert!(checked(ok.to_owned()).is_ok());
        assert_eq!(depth(ok), 2);
        let deep = format!(
            "a = {}x{}\n",
            "{ ".repeat(DEPTH + 1),
            " }".repeat(DEPTH + 1)
        );
        assert!(matches!(checked(deep), Err(Refused::TooDeep)));
        let calls = format!("a = {{ {}$x{} }}\n", "F(".repeat(DEPTH), ")".repeat(DEPTH));
        assert!(matches!(checked(calls), Err(Refused::TooDeep)));
        // What Fluent cannot read is no refusal: its messages are said in
        // English.
        assert!(checked("a = { $n -> }\nb = ok\n".to_owned()).is_ok());
    }

    /// What a string literal or a line's end could hide from the count
    /// hides nothing: each catalog here nests far deeper than Fluent's
    /// parser can recurse, and each is refused, never parsed.
    #[test]
    fn literals_and_line_ends_hide_no_nesting() {
        const DEEP: usize = 100_000;
        let braces = |n: usize| format!("{}{}", "{ ".repeat(n), " }".repeat(n));
        let refused = [
            // A `"` in a variant's text is text, not a literal.
            format!("a = {{ $n ->\n   *[other] said \"{}\n}}\n", braces(DEEP)),
            // A literal left open at a line's end ends there.
            format!("a = {{ \"x\nb = {}\n", braces(DEEP)),
            // Text and a literal alternating on each line, a selector in
            // each variant.
            format!(
                "a = {{ $n ->\n{}   *[z] z\n{}\n",
                "   *[a] \"{\"}\" ->\n".repeat(DEEP / 10),
                "}".repeat(DEEP / 10 + 1)
            ),
            // Closers inside the literals of calls nested in calls.
            format!("a = {{ {}$x }}\n", "F(\")\", ".repeat(DEEP)),
            // Text's `)` closing no call.
            format!("a = {{ {} }}\n", "F({ $x ->\n *[a] ) ) {".repeat(DEEP / 10)),
            // An escaped quote, then braces.
            format!("a = {{ F(\"\\\"{}\") }}\n", braces(DEEP)),
        ];
        for (at, catalog) in refused.into_iter().enumerate() {
            assert!(depth(&catalog) > DEPTH, "catalog {at}");
            assert!(
                matches!(checked(catalog), Err(Refused::TooDeep)),
                "catalog {at}"
            );
        }
    }

    /// What plugins write is not refused: calls with named literals, empty
    /// and braced literals, notes for translators holding braces, many
    /// messages one after another.
    #[test]
    fn catalogs_as_written_are_read() {
        let mut catalog = String::from(
            "# { $place } is where it went: { or ( unclosed\n\
             moved = { GENDER($place) ->\n\
             \x20   [feminine] Déplacée dans { FORM($place, case: \"definite\") } (\"vite\")\n\
             \x20  *[other] Déplacé dans { FORM($place, case: \"definite\") }\n\
             }\n\
             braces = {\"{\"}{ \"\" }{ \"}\" } and { \"a\\\"b\" }\n\
             -term = { \"x\" }\n",
        );
        for n in 0..500 {
            catalog.push_str(&["m", &n.to_string()].concat());
            catalog.push_str(" = { FORM($p, case: \"de\") } { NUMBER($n, style: \"x\") }\n");
        }
        assert!(depth(&catalog) <= 3, "{}", depth(&catalog));
        assert!(checked(catalog).is_ok());
    }

    /// The count is never less than how deep Fluent's own parser nests,
    /// read from any line on (the window parses a message from its line):
    /// texts made at random of what nests, closes and quotes.
    #[test]
    fn nesting_is_never_counted_short() {
        use fluent_syntax::ast;

        fn of_pattern(pattern: &ast::Pattern<&str>) -> usize {
            pattern
                .elements
                .iter()
                .map(|element| match element {
                    ast::PatternElement::Placeable { expression } => 1 + of_expression(expression),
                    ast::PatternElement::TextElement { .. } => 0,
                })
                .max()
                .unwrap_or(0)
        }
        fn of_expression(expression: &ast::Expression<&str>) -> usize {
            match expression {
                ast::Expression::Inline(inline) => of_inline(inline),
                ast::Expression::Select { selector, variants } => variants
                    .iter()
                    .map(|variant| of_pattern(&variant.value))
                    .chain([of_inline(selector)])
                    .max()
                    .unwrap_or(0),
            }
        }
        fn of_arguments(arguments: &ast::CallArguments<&str>) -> usize {
            1 + arguments
                .positional
                .iter()
                .chain(arguments.named.iter().map(|named| &named.value))
                .map(of_inline)
                .max()
                .unwrap_or(0)
        }
        fn of_inline(inline: &ast::InlineExpression<&str>) -> usize {
            match inline {
                ast::InlineExpression::Placeable { expression } => 1 + of_expression(expression),
                ast::InlineExpression::FunctionReference { arguments, .. } => {
                    of_arguments(arguments)
                }
                ast::InlineExpression::TermReference {
                    arguments: Some(arguments),
                    ..
                } => of_arguments(arguments),
                _ => 0,
            }
        }
        fn of_text(text: &str) -> usize {
            let resource = match fluent_syntax::parser::parse_runtime(text) {
                Ok(resource) | Err((resource, _)) => resource,
            };
            resource
                .body
                .iter()
                .map(|entry| match entry {
                    ast::Entry::Message(message) => message
                        .value
                        .iter()
                        .chain(message.attributes.iter().map(|attribute| &attribute.value))
                        .map(of_pattern)
                        .max()
                        .unwrap_or(0),
                    ast::Entry::Term(term) => term
                        .attributes
                        .iter()
                        .map(|attribute| &attribute.value)
                        .chain([&term.value])
                        .map(of_pattern)
                        .max()
                        .unwrap_or(0),
                    _ => 0,
                })
                .max()
                .unwrap_or(0)
        }

        const PIECES: [&str; 24] = [
            "a = ", "b=", "-t = ", "{ ", "{", "}", "\n}", "$n", " ->\n", "\n *[x] ", "\n [y] ",
            "F(", "-t(", ")", ", ", "c: ", "\"", "\\", "]", "x", " ", "\n", "\n# ", "\n  ",
        ];
        let mut random = Random(0x5eed_cafe);
        let counted_short = |text: &str| {
            let counted = depth(text);
            let mut from = 0;
            loop {
                let nested = of_text(&text[from..]);
                assert!(
                    nested <= counted,
                    "{nested} deep, counted {counted}, from {from}: {text:?}"
                );
                match text[from..].find('\n') {
                    Some(at) => from += at + 1,
                    None => break,
                }
            }
        };
        // Pieces at random: mostly what Fluent cannot read whole.
        for _ in 0..20_000 {
            let mut text = String::from("a = ");
            for _ in 0..random.below(60) {
                text.push_str(PIECES[random.below(PIECES.len())]);
            }
            if depth(&text) <= DEPTH {
                counted_short(&text);
            }
        }
        // Messages Fluent reads whole, nesting as deep as Fluent's grammar
        // lets them: literals and text holding every closer and quote.
        for _ in 0..5_000 {
            let mut text = String::from("a = ");
            random.pattern(&mut text, 4);
            text.push('\n');
            assert!(
                fluent_syntax::parser::parse_runtime(text.as_str()).is_ok(),
                "{text:?}"
            );
            counted_short(&text);
        }
    }

    /// A test's random choices, the same at every run.
    struct Random(u64);

    impl Random {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            usize::try_from(self.0 % n as u64).unwrap()
        }

        /// A pattern on one line (a select's variants on lines of their
        /// own), placeables nested at most `deep`.
        fn pattern(&mut self, out: &mut String, deep: usize) {
            for _ in 0..=self.below(2) {
                if deep == 0 || self.below(2) == 0 {
                    const TEXT: [&str; 8] = ["x", " ", "\"", "(", ")", "]", "\\", "*["];
                    out.push('x');
                    for _ in 0..self.below(4) {
                        out.push_str(TEXT[self.below(TEXT.len())]);
                    }
                } else {
                    out.push_str("{ ");
                    self.expression(out, deep - 1);
                }
            }
        }

        /// A placeable's expression, and the `}` that closes it.
        fn expression(&mut self, out: &mut String, deep: usize) {
            if deep > 0 && self.below(3) == 0 {
                match self.below(3) {
                    0 => out.push_str("$n"),
                    1 => self.literal(out),
                    _ => out.push_str("F($n)"),
                }
                out.push_str(" ->\n");
                for key in ["    [one] ", "    [two] "].iter().take(self.below(3)) {
                    out.push_str(key);
                    self.pattern(out, deep - 1);
                    out.push('\n');
                }
                out.push_str("   *[other] ");
                self.pattern(out, deep - 1);
                out.push_str("\n}");
            } else {
                self.inline(out, deep);
                out.push_str(" }");
            }
        }

        fn inline(&mut self, out: &mut String, deep: usize) {
            match if deep == 0 {
                self.below(2)
            } else {
                self.below(4)
            } {
                0 => out.push_str("$x"),
                1 => self.literal(out),
                2 => {
                    out.push_str("F(");
                    let positional = self.below(3);
                    for at in 0..positional {
                        if at > 0 {
                            out.push_str(", ");
                        }
                        self.inline(out, deep - 1);
                    }
                    if self.below(2) == 0 {
                        if positional > 0 {
                            out.push_str(", ");
                        }
                        out.push_str("k: ");
                        self.literal(out);
                    }
                    out.push(')');
                }
                _ => {
                    out.push_str("{ ");
                    self.expression(out, deep - 1);
                }
            }
        }

        fn literal(&mut self, out: &mut String) {
            const HELD: [&str; 9] = ["x", "{", "}", "(", ")", "]", "\\\\", "\\\"", "\\{"];
            out.push('"');
            for _ in 0..self.below(4) {
                out.push_str(HELD[self.below(HELD.len())]);
            }
            out.push('"');
        }
    }
}
