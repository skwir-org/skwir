//! What a search takes, said whole (ADR-137): its terms and options, each
//! with what it means and examples of its values; how terms go together;
//! whole terms for the values sizes, times and ratings take; what a name's
//! wildcards match; and what `content:` reads, the signs of a regex among
//! them. `query.reference` answers it as data; the address field lists it
//! under `?` and F1 (`Typed::Help`), each row writing its part where the
//! caret is, in place of a lone `?`.

use skwir_core::reference::{Group, Item, Reference, floor_boundary};
use skwir_core::suggest::{Ask, Section, Suggestion};
use skwir_core::words;
use skwir_core::words::{List, Words};

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
    let mut terms = Group::new(words!(
        query_ref_terms = "Terms",
        note =
            "A heading of what a search takes (F1 in the address field): the keys that test files."
    ));
    let mut options = Group::new(words!(
        query_ref_options = "Where and how it looks",
        note = "A heading of what a search takes: the options (in:, depth:, hidden:…)."
    ));
    for (written, means, example, option) in assist::keys_said() {
        let item = Item::new(
            written,
            words!(
                query_ref_key = "{ $means }: { $example }",
                means = means,
                example = example,
                note = "What a key of a search means, then examples of its values (Of a kind: image, video, document…)."
            ),
        );
        if option {
            options = options.item(item);
        } else {
            terms = terms.item(item);
        }
    }
    let together = Group::new(words!(
        query_ref_together = "Together (side by side: both)",
        note = "A heading of what a search takes: how terms go together; two terms side by side both hold."
    ))
    .item(
        Item::new(
            "OR",
            words!(
                query_ref_or = "Either: a OR b",
                note = "What OR means in a search; a OR b is an example as written: keep OR."
            ),
        )
        .inserting("OR "),
    )
    .item(
        Item::new(
            "NOT",
            words!(
                query_ref_not = "Not: NOT a, also written -a",
                note = "What NOT means in a search; NOT a and -a are examples as written."
            ),
        )
        .inserting("NOT "),
    )
    .item(Item::new(
        "-",
        words!(
            query_ref_minus = "Not this: -draft, -#tag, -kind:image",
            note = "What a - before a term means in a search; the terms are examples as written."
        ),
    ))
    .item(
        Item::new(
            "( )",
            words!(
                query_ref_parens = "Together: (a OR b) c",
                note = "What parentheses mean in a search; (a OR b) c is an example as written."
            ),
        )
        .inserting("("),
    )
    .item(
        Item::new(
            "\"…\"",
            words!(
                query_ref_quotes = "As written: \"two words\", \"OR\", \"#3\"",
                note = "What quotes mean in a search: what they hold is a name as written; the examples are written as the search language writes them."
            ),
        )
        .inserting("\""),
    );
    // What the parser reads, said from its own tables; its examples in
    // the units sizes are shown in (ADR-139, 9).
    let units_where = |keep: fn(&str) -> bool| {
        List::and(
            lang::SIZE_UNITS
                .iter()
                .map(|(name, _)| *name)
                .filter(|name| keep(name)),
        )
    };
    let decimal = units_where(|name| name.len() == 2);
    let binary = units_where(|name| name.ends_with("iB"));
    let alone = units_where(|name| name.len() == 1 && name != "B");
    let mb = skwir_core::fs::size_units().names()[2];
    let periods: Vec<&str> = lang::NAMED_PERIODS
        .iter()
        .map(|(name, ..)| *name)
        .filter(|name| *name != "today")
        .collect();
    let [minutes, hours, days, weeks, months, years] = lang::DURATION_UNITS;
    let [(i, _), (c, _)] = lang::REGEX_FLAGS;
    let values = Group::new(words!(
        query_ref_values = "Values",
        note = "A heading of what a search takes: the values sizes, times and ratings take."
    ))
    .item(Item::new(
        format!("size>10{mb}"),
        words!(
            query_ref_size = "More than 10 { $mb }. Each of { $decimal } is 1000 times the one before; each of { $binary } is 1024 times, and so is each of { $alone } written alone.",
            mb = mb,
            decimal = decimal,
            binary = binary,
            alone = alone,
            note = "What size>10MB means in a search, and the units of size it reads. mb: a unit as the size setting writes it; decimal (kB, MB…), binary (KiB, MiB…) and alone (k, M…): lists of units as written."
        ),
    ))
    .item(Item::new(
        format!("size:1{mb}..100{mb}"),
        words!(
            query_ref_size_range = "From one size to the other, both included",
            note = "What a range of sizes means in a search (size:1MB..100MB): both ends included."
        ),
    ))
    .item(Item::new(
        "rating>=3",
        words!(
            query_ref_rating = "At least (> < >= <=); rating:0 finds files with no rating",
            note = "What rating>=3 means in a search; the signs and rating:0 are written as the search language writes them."
        ),
    ))
    .item(Item::new(
        "modified:today",
        words!(
            query_ref_period = "A period: { $periods }",
            periods = List::or(periods),
            note = "What modified:today means in a search: a period. periods: the other periods it reads, as written, a list."
        ),
    ))
    .item(Item::new(
        "modified:2026-05",
        words!(
            query_ref_date = "A year, a month or a day: 2026, 2026-05-14",
            note = "What modified:2026-05 means in a search; the dates are written as the search language writes them."
        ),
    ))
    .item(Item::new(
        "modified:2026-01..2026-03",
        words!(
            query_ref_date_range = "From the start of the first to the end of the second",
            note = "What a range of dates means in a search (modified:2026-01..2026-03): both ends included, whole (January to the end of March)."
        ),
    ))
    .item(Item::new(
        "modified<7d",
        words!(
            query_ref_duration = "Within the last 7 days: { $m } minutes, { $h } hours, { $d } days, { $w } weeks, { $mo } months, { $y } years",
            m = minutes,
            h = hours,
            d = days,
            w = weeks,
            mo = months,
            y = years,
            note = "What modified<7d means in a search, and the units of time it reads: each unit as written (m, h, d…), then what it counts."
        ),
    ))
    .item(Item::new(
        "modified>1y",
        words!(
            query_ref_older = "Longer ago than a year",
            note = "What modified>1y means in a search."
        ),
    ));
    let names = Group::new(words!(
        query_ref_names = "Names",
        note = "A heading of what a search takes: words and wildcards matched against names."
    ))
    .item(Item::new(
        "report",
        words!(
            query_ref_word = "Names holding it, capitals or not",
            note = "What a word means in a search (report): the names holding it."
        ),
    ))
    .item(Item::new(
        "*.pdf",
        words!(
            query_ref_star = "* any characters",
            note = "What * means in a name searched: keep * first."
        ),
    ))
    .item(Item::new(
        "report-??",
        words!(
            query_ref_question = "? one character",
            note = "What ? means in a name searched: keep ? first."
        ),
    ))
    .item(Item::new(
        "[ab]*",
        words!(
            query_ref_brackets = "[…] one of these",
            note = "What brackets mean in a name searched: keep […] first."
        ),
    ));
    let contents = Group::new(words!(
        query_ref_contents = "Contents",
        note = "A heading of what a search takes: what the files hold (content:)."
    ))
    .item(Item::new(
        "content:\"two words\"",
        words!(
            query_ref_content = "Files holding these words",
            note = "What content:\"two words\" means in a search."
        ),
    ))
    .item(Item::new(
        "content:/to.?do/",
        words!(
            query_ref_content_regex = "Files holding what a pattern matches",
            note = "What content:/to.?do/ means in a search: a regular expression."
        ),
    ))
    .item(Item::new(
        "content:/todo/i",
        words!(
            query_ref_flags = "{ $i }: capitals or not; { $c }: capitals told apart",
            i = i.to_string(),
            c = c.to_string(),
            note = "What the flags after a pattern mean in a search (content:/todo/i); i and c are the flags, as written."
        ),
    ));
    let pattern = skwir_core::reference::regex_pattern()
        .item(Item::new(
            r"\/",
            words!(
                query_ref_slash = "A slash itself: one alone ends the pattern",
                note = "What \\/ means in a pattern between slashes."
            ),
        ))
        .note(words!(
            query_ref_line = "^ and $ are the start and the end of a line.",
            note = "A note under the signs of a pattern; ^ and $ are signs as written."
        ));
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
    groups().into_iter().fold(
        Reference::new(words!(
            query_ref_title = "Search",
            note = "The heading of what a search takes (F1 in the address field)."
        )),
        |reference, (group, _)| reference.group(group),
    )
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
            let mut row = Suggestion::new(Section::Syntax, Words::as_written(item.written.clone()))
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
            if written == Written::Part || group.title.english().starts_with("Together") {
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
                .any(|r| r.title.english().contains("start and the end of a line"))
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
        for unit in lang::DURATION_UNITS {
            parses(&format!("modified<3{unit}"));
        }
        for (flag, _) in lang::REGEX_FLAGS {
            parses(&format!("content:/a/{flag}"));
        }
        parses(r"content:/a\/b/");
        let said = reference();
        let says = |written: &str| {
            said.items()
                .find(|(_, _, item)| item.written == written)
                .map(|(_, _, item)| item.says.english())
                .unwrap_or_default()
        };
        assert!(says("modified:today").contains("last-year"));
        assert!(says("size>10MB").contains("TiB"));
        assert!(says("modified<7d").contains("mo months"));
        assert!(says("content:/todo/i").contains("c: capitals told apart"));
    }
}
