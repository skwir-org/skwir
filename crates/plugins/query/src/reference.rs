//! What a search takes, said whole (ADR-137): its terms and options, each
//! with what it means and examples of its values; how terms go together;
//! whole terms for the values sizes, times and ratings take; what a name's
//! wildcards match; and what `content:` reads, the signs of a regex among
//! them. `query.reference` answers it as data; the address field lists it
//! under `?` and F1 (`Typed::Help`), each row writing its part where the
//! caret is, in place of a lone `?`.

use skwir_core::reference::{Group, Item, Reference, floor_boundary};
use skwir_core::suggest::{Ask, Section, Suggestion};

use crate::{assist, lang};

/// How a group's part is written where the caret is: a term, apart from
/// the words beside it; or a part of a term (a sign inside `/…/`), as it
/// is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Written {
    Term,
    Part,
}

/// The groups, each with how its parts are written, in the order shown.
fn groups() -> Vec<(Group, Written)> {
    let mut terms = Group::new("Terms");
    let mut options = Group::new("Where and how it looks");
    for (written, means, example, option) in assist::keys_said() {
        let item = Item::new(written, format!("{means}: {example}"));
        if option {
            options = options.item(item);
        } else {
            terms = terms.item(item);
        }
    }
    let together = Group::new("Together (side by side: both)")
        .item(Item::new("OR", "Either: a OR b").inserting("OR "))
        .item(Item::new("NOT", "Not: NOT a, as -a").inserting("NOT "))
        .item(Item::new("-", "Not this: -draft, -#tag, -kind:image"))
        .item(Item::new("( )", "Together: (a OR b) c").inserting("("))
        .item(Item::new("\"…\"", "As written: \"two words\", \"OR\", \"#3\"").inserting("\""));
    // What the parser reads, said from its own tables.
    let units: Vec<String> = lang::SIZE_UNITS
        .iter()
        .filter(|(_, bytes)| *bytes > 1)
        .map(|(unit, _)| unit.to_string())
        .collect();
    let periods: Vec<&str> = lang::NAMED_PERIODS
        .iter()
        .map(|(name, ..)| *name)
        .filter(|name| *name != "today")
        .collect();
    let durations: Vec<String> = lang::DURATION_UNITS
        .iter()
        .map(|(unit, counts)| format!("{unit} {counts}"))
        .collect();
    let flags: Vec<String> = lang::REGEX_FLAGS
        .iter()
        .map(|(flag, _, means)| format!("{flag}: {means}"))
        .collect();
    let values = Group::new("Values")
        .item(Item::new(
            "size>10M",
            format!(
                "More than 10 MiB: {}, each 1024 times the one before",
                units.join(", ")
            ),
        ))
        .item(Item::new("size:1M..100M", "From one to the other, both in"))
        .item(Item::new(
            "rating>=3",
            "At least (> < >= <=); rating:0 unrated",
        ))
        .item(Item::new(
            "modified:today",
            format!("A period: {}", periods.join(", ")),
        ))
        .item(Item::new(
            "modified:2026-05",
            "A year, a month or a day: 2026, 2026-05-14",
        ))
        .item(Item::new(
            "modified:2026-01..2026-03",
            "From one to the other, both whole",
        ))
        .item(Item::new(
            "modified<7d",
            format!("Within the last 7 days: {}", durations.join(", ")),
        ))
        .item(Item::new("modified>1y", "Longer ago than a year"));
    let names = Group::new("Names")
        .item(Item::new("report", "Names holding it, capitals or not"))
        .item(Item::new("*.pdf", "* any characters"))
        .item(Item::new("report-??", "? one character"))
        .item(Item::new("[ab]*", "[…] one of these"));
    let contents = Group::new("Contents")
        .item(Item::new(
            "content:\"two words\"",
            "Files holding these words",
        ))
        .item(Item::new(
            "content:/to.?do/",
            "Files holding what a pattern matches",
        ))
        .item(Item::new("content:/todo/i", flags.join("; ")));
    let pattern = skwir_core::reference::regex_pattern()
        .item(Item::new(
            r"\/",
            "A slash itself: one alone ends the pattern",
        ))
        .note("^ and $ are the start and the end of a line.");
    vec![
        (terms, Written::Term),
        (together, Written::Term),
        (values, Written::Term),
        (names, Written::Term),
        (contents, Written::Term),
        (pattern, Written::Part),
        (options, Written::Term),
    ]
}

/// What a search takes, as `query.reference` answers it.
pub fn reference() -> Reference {
    groups()
        .into_iter()
        .fold(Reference::new("Search"), |reference, (group, _)| {
            reference.group(group)
        })
}

/// `ask`'s text with `part` written where its caret is: a term apart from
/// the words around it, past the word (the quoted value, the regex) the
/// caret stands in, joined to a `-` or a `(` that begins it; a part of
/// one as it is; a lone `?` is the text replaced.
fn put(ask: &Ask, part: &str, written: Written) -> String {
    let text = ask.text.as_str();
    if text.trim() == "?" {
        return part.to_owned();
    }
    let (caret, joins) = match written {
        Written::Term => assist::term_place(text, ask.caret),
        Written::Part => (floor_boundary(text, ask.caret), true),
    };
    let (before, mut after) = text.split_at(caret);
    // A part ending in its space takes the place of the one after it.
    if part.ends_with(' ') {
        after = after.trim_start_matches(' ');
    }
    let mut out = String::with_capacity(text.len() + part.len() + 2);
    out.push_str(before);
    if !joins && !before.is_empty() && !before.ends_with(char::is_whitespace) {
        out.push(' ');
    }
    out.push_str(part);
    // Apart from the word after it too: a key written before a word
    // waits for its own value, and never takes that word for it
    // (`kind:report`); only what begins a term (`-`, `(`, a quote) joins
    // what follows.
    if written == Written::Term
        && !after.is_empty()
        && !after.starts_with(char::is_whitespace)
        && !after.starts_with(')')
        && !part.ends_with(['(', '"', ' ', '-'])
    {
        out.push(' ');
    }
    out.push_str(after);
    out
}

/// The address field's rows for `?` and F1: every part a search takes,
/// under its group's heading, each writing itself where the caret is,
/// then the group's notes, rows that write and run nothing.
pub fn rows(ask: &Ask) -> Vec<Suggestion> {
    let mut rows = Vec::new();
    let rank = |rows: &Vec<Suggestion>| i32::try_from(rows.len()).unwrap_or(i32::MAX);
    for (group, written) in groups() {
        for item in group.items {
            let Some(part) = item.inserts.as_deref() else {
                continue;
            };
            let mut row = Suggestion::new(Section::Syntax, item.written.clone())
                .detail(item.says.clone())
                .heading(group.title.clone())
                .rank(rank(&rows));
            row.typed = true;
            row.completion = Some(put(ask, part, written));
            rows.push(row);
        }
        for note in group.notes {
            let row = Suggestion::new(Section::Syntax, note)
                .heading(group.title.clone())
                .rank(rank(&rows));
            rows.push(row);
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::suggest::Typed;

    fn ask(text: &str, caret: usize) -> Ask {
        Ask {
            text: text.into(),
            caret,
            typed: Typed::Help,
            scope: None,
            shown: None,
            dir: None,
            pane: None,
            lens: None,
        }
    }

    /// A term goes apart from the words beside it, a sign of a pattern
    /// right where the caret is, and a lone `?` is replaced.
    #[test]
    fn a_part_is_written_where_the_caret_is() {
        assert_eq!(put(&ask("?", 1), "kind:", Written::Term), "kind:");
        assert_eq!(
            put(&ask("report", 6), "kind:", Written::Term),
            "report kind:"
        );
        assert_eq!(
            put(&ask("report ", 7), "kind:", Written::Term),
            "report kind:"
        );
        assert_eq!(put(&ask("a b", 1), "OR ", Written::Term), "a OR b");
        assert_eq!(put(&ask("(a", 1), "kind:", Written::Term), "(kind: a");
        // A key written before a word stands apart from it, never taking
        // it for its value.
        assert_eq!(
            put(&ask("report", 0), "kind:", Written::Term),
            "kind: report"
        );
        assert_eq!(
            put(&ask("a report", 2), "size>10M", Written::Term),
            "a size>10M report"
        );
        assert_eq!(put(&ask("report", 0), "-", Written::Term), "-report");
        assert_eq!(put(&ask("a b", 2), "-", Written::Term), "a -b");
        assert_eq!(
            put(&ask("content:/ab/", 10), r"\d", Written::Part),
            r"content:/a\db/"
        );
        assert_eq!(put(&ask("", 0), "size>10M", Written::Term), "size>10M");
        // A caret inside a character is taken back to its start.
        assert_eq!(put(&ask("é", 1), "x", Written::Part), "xé");
        // A term goes past the word, the quoted value or the regex the
        // caret stands in, never within it.
        assert_eq!(
            put(&ask(r#"name:"a b""#, 9), "kind:", Written::Term),
            r#"name:"a b" kind:"#
        );
        assert_eq!(
            put(&ask("report", 3), "size>10M", Written::Term),
            "report size>10M"
        );
        assert_eq!(
            put(&ask("content:/a b/ x", 10), "kind:", Written::Term),
            "content:/a b/ kind: x"
        );
        assert_eq!(
            put(&ask("a OR b", 3), "kind:", Written::Term),
            "a OR kind: b"
        );
        // A `-` or a `(` begun takes the term it begins.
        assert_eq!(put(&ask("-", 1), "kind:", Written::Term), "-kind:");
        assert_eq!(put(&ask("a -", 3), "#", Written::Term), "a -#");
        assert_eq!(put(&ask("a-", 2), "kind:", Written::Term), "a- kind:");
        // Each result reads: a term written whole is one the parser takes.
        let read = put(&ask(r#"name:"a b""#, 9), "size>10M", Written::Term);
        assert!(crate::lang::parse(&read).is_ok(), "{read}");
        let read = put(&ask("-", 1), "kind:image", Written::Term);
        assert!(crate::lang::parse(&read).is_ok(), "{read}");
    }

    /// Every term the reference offers whole is one the parser reads, and
    /// every key the parser knows is in it.
    #[test]
    fn the_reference_says_what_the_parser_reads() {
        let said = reference();
        for (g, (group, written)) in groups().into_iter().enumerate() {
            assert_eq!(said.groups[g].title, group.title);
            if written == Written::Part || group.title.starts_with("Together") {
                continue;
            }
            for item in &group.items {
                let text = item.inserts.as_deref().unwrap();
                // A key alone waits for its value: given one, it reads.
                let text = match text {
                    "kind:" => "kind:image".to_owned(),
                    "modified:" | "created:" | "accessed:" | "taken:" => format!("{text}today"),
                    "size:" => "size:1M".to_owned(),
                    "ext:" => "ext:pdf".to_owned(),
                    "type:" => "type:file".to_owned(),
                    "rating:" => "rating:3".to_owned(),
                    "mime:" => "mime:image/png".to_owned(),
                    "near:" => "near:45.0,6.0".to_owned(),
                    "inside:" => "inside:1,2,3,4".to_owned(),
                    "in:" => "in:/tmp".to_owned(),
                    "depth:" => "depth:2".to_owned(),
                    "hidden:" | "excluded:" => format!("{text}true"),
                    "case:" => "case:sensitive".to_owned(),
                    key if key.ends_with(':') => format!("{key}x"),
                    other => other.to_owned(),
                };
                assert!(
                    crate::lang::parse(&text).is_ok(),
                    "{text} ({}) does not read",
                    item.says
                );
            }
        }
        let keys: Vec<String> = assist::keys_said().map(|(key, ..)| key).collect();
        for key in keys {
            assert!(
                said.items().any(|(_, _, item)| item.written == key),
                "{key} is not said"
            );
        }
        // Each item's row writes something, each note's nothing, each
        // under its group.
        let rows = rows(&ask("?", 1));
        let notes: usize = said.groups.iter().map(|g| g.notes.len()).sum();
        assert!(notes >= 2);
        assert_eq!(rows.len(), said.items().count() + notes);
        assert!(rows.iter().all(|r| r.heading.is_some()));
        assert_eq!(
            rows.iter().filter(|r| r.completion.is_some()).count(),
            said.items().count()
        );
        let inert: Vec<&Suggestion> = rows.iter().filter(|r| r.completion.is_none()).collect();
        assert!(inert.iter().all(|r| r.run.is_none() && !r.typed));
        assert!(
            inert
                .iter()
                .any(|r| r.title.contains("start and the end of a line"))
        );
        assert_eq!(rows[0].completion.as_deref(), Some("kind:"));
    }

    /// What the reference says of values is the parser's own tables, each
    /// read back: every period named, size unit, duration unit and regex
    /// flag it lists is one a search reads, and a slash written itself
    /// stays in the pattern.
    #[test]
    fn the_values_said_are_the_parser_s() {
        let parses = |text: &str| assert!(crate::lang::parse(text).is_ok(), "{text}");
        for (name, ..) in lang::NAMED_PERIODS {
            parses(&format!("modified:{name}"));
        }
        for (unit, _) in lang::SIZE_UNITS {
            parses(&format!("size>1{unit}"));
            parses(&format!("size>1{}", unit.to_ascii_lowercase()));
        }
        for (unit, _) in lang::DURATION_UNITS {
            parses(&format!("modified<3{unit}"));
        }
        for (flag, ..) in lang::REGEX_FLAGS {
            parses(&format!("content:/a/{flag}"));
        }
        parses(r"content:/a\/b/");
        let said = reference();
        let says = |written: &str| {
            said.items()
                .find(|(_, _, item)| item.written == written)
                .map(|(_, _, item)| item.says.clone())
                .unwrap_or_default()
        };
        assert!(says("modified:today").contains("last-year"));
        assert!(says("size>10M").contains('T'));
        assert!(says("modified<7d").contains("mo months"));
        assert!(says("content:/todo/i").contains("c: capitals told apart"));
    }
}
