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

/// How deep `text`'s placeables and calls nest: each `{` and, inside one,
/// each `(` opens a level, outside its string literals.
fn depth(text: &str) -> usize {
    let (mut braces, mut parens, mut deepest) = (0usize, 0usize, 0usize);
    let (mut in_string, mut escaped) = (false, false);
    for c in text.chars() {
        if in_string {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '{' => braces += 1,
            '}' => {
                braces = braces.saturating_sub(1);
                if braces == 0 {
                    parens = 0;
                }
            }
            '(' if braces > 0 => parens += 1,
            ')' if braces > 0 => parens = parens.saturating_sub(1),
            '"' if braces > 0 => in_string = true,
            _ => {}
        }
        deepest = deepest.max(braces + parens);
    }
    deepest
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
