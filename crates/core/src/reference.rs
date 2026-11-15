//! What a field takes, said beside it (ADR-137): the placeholders, the
//! terms or the signs it reads, each as it is written, what it means, what
//! it gives for what the field is about, and what choosing it puts in the
//! field.
//!
//! The owner of a field's language describes it as data (a plugin's
//! command answers one, `rename_bulk.reference`; a question passes one,
//! `ui.ask`'s `reference`), and the window draws it: a dialog's field from
//! its [?] or F1, the address field in its card. A command's schema points
//! agents to the same data, so what the window shows and what an agent
//! reads are one text.
//!
//! A field also offers, as it is typed, the items that go on with the part
//! typed since the last of the signs that begin one (`starts`: `{` in a
//! template): [`Reference::offered`], which the window lists under the
//! field.
//!
//! The syntax of regular expressions, which two plugins read with the same
//! crate (`regex`: a bulk rename's pattern, a search's `content:/…/`), is
//! said once here ([`regex_pattern`]).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What a field takes: its groups, in the order they are read.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Reference {
    /// What it is about, as its heading says it: "Template".
    pub title: String,
    pub groups: Vec<Group>,
    /// What begins a part the field offers to go on with as it is typed
    /// (`{` in a template, `\` in a pattern); none, nothing is offered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub starts: Vec<String>,
    /// What writes a sign of `starts` itself and begins no part (`{{` in
    /// a template, `\\` in a pattern, `$$` in a replacement).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub escapes: Vec<String>,
}

/// What a field offers as it is typed ([`Reference::offered`]): the part
/// begun (byte offsets into the text: from its sign, over what follows
/// the caret and goes on with it), which an item chosen takes the place
/// of, and the items that go on with it, by their group's place and
/// their own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offered {
    pub range: std::ops::Range<usize>,
    pub items: Vec<(usize, usize)>,
}

/// Items about one thing ("Parts of the name", "Repeats"), and what holds
/// for them all.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Group {
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Item>,
    /// Lines said under its items.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// One thing the field reads.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Item {
    /// As it is written: `{n:03}`, `\d`, `kind:`.
    pub written: String,
    /// What it means, in words.
    pub says: String,
    /// What it gives for what the field is about (the first file's), when
    /// that is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gives: Option<String>,
    /// What choosing it puts where the field's caret is (over what is
    /// selected there); `None`, with no `fills`: an item only read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inserts: Option<String>,
    /// What choosing it writes whole in fields named by their key (a
    /// recipe: a pattern and its replacement), in place of `inserts`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fills: BTreeMap<String, String>,
}

impl Reference {
    pub fn new(title: impl Into<String>) -> Self {
        Reference {
            title: title.into(),
            groups: Vec::new(),
            starts: Vec::new(),
            escapes: Vec::new(),
        }
    }

    /// Offer, as the field is typed, what goes on with a part begun by one
    /// of `starts`.
    #[must_use]
    pub fn starts(mut self, starts: &[&str]) -> Self {
        self.starts = starts.iter().map(|s| (*s).to_owned()).collect();
        self
    }

    /// What writes a sign itself, beginning no part: `{{`.
    #[must_use]
    pub fn escapes(mut self, escapes: &[&str]) -> Self {
        self.escapes = escapes.iter().map(|s| (*s).to_owned()).collect();
        self
    }

    /// What `text` offers with its caret at `caret`: the part begun by the
    /// last sign of `starts` before the caret that no escape writes, and
    /// the items whose `inserts` go on with what is typed of it (longer
    /// than it, starting with it). The part runs over what follows the
    /// caret as far as one of them goes on so (`{na|m` takes `{nam`
    /// whole). `None` where nothing does: no such sign before the caret,
    /// or a part already whole there (an item written from the sign over
    /// the caret: `{n|ame}`), which the caret only passes through.
    pub fn offered(&self, text: &str, caret: usize) -> Option<Offered> {
        let caret = floor_boundary(text, caret);
        let start = self.part_begun(&text[..caret])?;
        let (typed, after) = (&text[start..caret], &text[caret..]);
        let parts = || {
            self.items()
                .filter_map(|(group, at, item)| Some((group, at, item.inserts.as_deref()?)))
        };
        let whole = parts()
            .any(|(_, _, part)| text[start..].starts_with(part) && part.len() >= typed.len());
        if whole {
            return None;
        }
        let mut end = caret;
        let items: Vec<(usize, usize)> = parts()
            .filter(|(_, _, part)| part.len() > typed.len() && part.starts_with(typed))
            .map(|(group, at, part)| {
                end = end.max(caret + common_prefix(&part[typed.len()..], after));
                (group, at)
            })
            .collect();
        (!items.is_empty()).then_some(Offered {
            range: start..end,
            items,
        })
    }

    /// Where the last part begun in `before` starts: a sign of `starts`
    /// that no escape writes, read from the start (`{{{` begins one at
    /// its third brace).
    fn part_begun(&self, before: &str) -> Option<usize> {
        let at_sign = |rest: &str, of: &[String]| {
            of.iter()
                .find(|sign| !sign.is_empty() && rest.starts_with(sign.as_str()))
                .map(String::len)
        };
        let mut begun = None;
        let mut at = 0;
        while at < before.len() {
            let rest = &before[at..];
            if let Some(len) = at_sign(rest, &self.escapes) {
                at += len;
                continue;
            }
            if at_sign(rest, &self.starts).is_some() {
                begun = Some(at);
            }
            at += rest.chars().next().map_or(1, char::len_utf8);
        }
        begun
    }

    #[must_use]
    pub fn group(mut self, group: Group) -> Self {
        self.groups.push(group);
        self
    }

    /// The `item`th item of its `group`th group.
    pub fn item(&self, group: usize, item: usize) -> Option<&Item> {
        self.groups.get(group)?.items.get(item)
    }

    /// Every item, with its group's and its own place.
    pub fn items(&self) -> impl Iterator<Item = (usize, usize, &Item)> {
        self.groups.iter().enumerate().flat_map(|(g, group)| {
            group
                .items
                .iter()
                .enumerate()
                .map(move |(i, item)| (g, i, item))
        })
    }

    /// Whether it says nothing.
    pub fn is_empty(&self) -> bool {
        self.groups
            .iter()
            .all(|g| g.items.is_empty() && g.notes.is_empty())
    }
}

/// `at` moved back to the start of the character it falls in (a caret is
/// a byte offset; one inside a character stands before it).
pub fn floor_boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// The length in bytes of what `a` and `b` begin with alike, whole
/// characters.
fn common_prefix(a: &str, b: &str) -> usize {
    a.chars()
        .zip(b.chars())
        .take_while(|(x, y)| x == y)
        .map(|(c, _)| c.len_utf8())
        .sum()
}

impl Group {
    pub fn new(title: impl Into<String>) -> Self {
        Group {
            title: title.into(),
            items: Vec::new(),
            notes: Vec::new(),
        }
    }

    #[must_use]
    pub fn item(mut self, item: Item) -> Self {
        self.items.push(item);
        self
    }

    #[must_use]
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

impl Item {
    /// An item put at the caret as it is written.
    pub fn new(written: impl Into<String>, says: impl Into<String>) -> Self {
        let written = written.into();
        Item {
            inserts: Some(written.clone()),
            written,
            says: says.into(),
            gives: None,
            fills: BTreeMap::new(),
        }
    }

    /// An item put at the caret as `text`, written otherwise (`(…)`,
    /// putting `()`).
    #[must_use]
    pub fn inserting(mut self, text: impl Into<String>) -> Self {
        self.inserts = Some(text.into());
        self
    }

    /// An item only read: choosing it puts nothing.
    #[must_use]
    pub fn read_only(mut self) -> Self {
        self.inserts = None;
        self.fills.clear();
        self
    }

    /// What it gives here.
    #[must_use]
    pub fn gives(mut self, gives: impl Into<String>) -> Self {
        self.gives = Some(gives.into());
        self
    }

    /// Choosing it writes `text` whole in the field `field`, and puts
    /// nothing at the caret.
    #[must_use]
    pub fn fills(mut self, field: impl Into<String>, text: impl Into<String>) -> Self {
        self.inserts = None;
        self.fills.insert(field.into(), text.into());
        self
    }

    /// A whole value (a mode, a path): choosing it writes it as it is
    /// written in place of all the field `field` holds, never beside what
    /// is typed there (`6` and `755` would make `6755`).
    #[must_use]
    pub fn whole_in(self, field: impl Into<String>) -> Self {
        let written = self.written.clone();
        self.fills(field, written)
    }
}

/// The signs of a regular expression, as the `regex` crate reads them
/// (a bulk rename's pattern, a search's `content:/…/`, an exclusion's
/// `regex:`): what it matches, how often, where, and in groups. Its
/// caller says what "the start" is the start of (a name, a line), and
/// what a group's match is for where something reads it after (a
/// replacement's `$1`).
pub fn regex_pattern() -> Group {
    Group::new("Pattern")
        .item(Item::new(".", "Any character"))
        .item(Item::new(r"\d", "A digit"))
        .item(Item::new(r"\w", "A letter, a digit or _"))
        .item(Item::new(
            r"\s",
            "Any space: a space, a tab, a line's end, any other",
        ))
        .item(Item::new("[abc]", "One of these").inserting("[]"))
        .item(Item::new("[^abc]", "One of anything but these").inserting("[^]"))
        .item(Item::new("[a-z]", "One in this range"))
        .item(Item::new(
            "*",
            "What is before it, any number of times, or none",
        ))
        .item(Item::new("+", "What is before it, once or more"))
        .item(Item::new("?", "What is before it, or nothing"))
        .item(Item::new("{2,4}", "What is before it, 2 to 4 times"))
        .item(Item::new("+?", "As few times as it can (*? too)"))
        .item(Item::new("^", "The start"))
        .item(Item::new("$", "The end"))
        .item(Item::new(r"\b", "Where a word starts or ends"))
        .item(Item::new("a|b", "Either"))
        .item(Item::new("(…)", "A group, which a sign after it takes whole: (ab)+").inserting("()"))
        .item(Item::new(r"(?<year>\d{4})", "A group with a name: year"))
        .item(Item::new(
            r"\.",
            r"A sign itself: \. \( \[ \* \+ \? \$ \^ \|",
        ))
        .item(Item::new(
            "(?i)",
            "From here on, capitals and small letters alike",
        ))
        .note("No look-around and no back-references: (?=…) and \\1 are not read.")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every sign the pattern group puts at a caret is one the crate
    /// reads, written alone, as said, or as it is put.
    #[test]
    fn the_signs_of_a_pattern_are_patterns_the_crate_reads() {
        for item in regex_pattern().items {
            let complete = |text: &str| match text {
                // What is before it: a letter.
                "*" | "+" | "?" | "{2,4}" | "+?" => format!("a{text}"),
                "[]" => "[a]".to_owned(),
                "[^]" => "[^a]".to_owned(),
                "(…)" => "(a)".to_owned(),
                other => other.to_owned(),
            };
            for text in [Some(&item.written), item.inserts.as_ref()]
                .into_iter()
                .flatten()
            {
                let pattern = complete(text);
                assert!(
                    regex::Regex::new(&pattern).is_ok(),
                    "{pattern} (from {text}) is no pattern"
                );
            }
        }
    }

    /// What is offered as the field is typed: the items going on with the
    /// part begun by the last sign before the caret, never one already
    /// whole, nothing without such a sign.
    #[test]
    fn a_part_begun_is_offered_what_goes_on_with_it() {
        let reference = Reference::new("Template")
            .group(
                Group::new("Parts")
                    .item(Item::new("{name}", "The name"))
                    .item(Item::new("{n}", "The counter"))
                    .item(Item::new("{n:03}", "Three digits"))
                    .item(Item::new("{{", "A brace"))
                    .item(Item::new("recipe", "Fills").fills("regex", "x")),
            )
            .starts(&["{"])
            .escapes(&["{{", "}}"]);
        let at = |text: &str, caret: usize| reference.offered(text, caret);
        let offered = at("a-{n", 4).unwrap();
        assert_eq!(offered.range, 2..4);
        assert_eq!(offered.items, [(0, 0), (0, 1), (0, 2)]);
        assert_eq!(at("{name}-{", 8).unwrap().items.len(), 4);
        assert_eq!(at("{n:0", 4).unwrap().items, [(0, 2)]);
        assert_eq!(at("{name}", 6), None, "whole already");
        assert_eq!(at("{name}-x", 8), None, "nothing goes on with it");
        assert_eq!(at("name", 4), None, "no sign before the caret");
        // Where the caret is, not the text's end.
        assert_eq!(at("{n x", 2).unwrap().range, 0..2);
        // A caret inside a character is taken back to its start.
        assert_eq!(at("{é", 2).map(|o| o.range), Some(0..1));
        assert_eq!(Reference::new("None").offered("{n", 2), None);
    }

    /// A caret put inside a part already whole (clicked, or moved by the
    /// arrows) offers nothing: taking a row there would write the part
    /// twice (`{full}ll}`). Inside a part begun, the part runs over what
    /// follows the caret and goes on with it, which a row taken replaces.
    #[test]
    fn a_whole_part_offers_nothing_and_a_part_begun_runs_past_the_caret() {
        let reference = Reference::new("Template")
            .group(
                Group::new("Parts")
                    .item(Item::new("{full}", "The whole name"))
                    .item(Item::new("{n}", "The counter"))
                    .item(Item::new("{n:03}", "Three digits")),
            )
            .starts(&["{"]);
        let at = |text: &str, caret: usize| reference.offered(text, caret);
        for caret in 1..=6 {
            assert_eq!(at("{full}", caret), None, "{{full}} at {caret}");
        }
        assert_eq!(at("{n} {full}", 2), None);
        assert_eq!(at("{n:03}-x", 3), None);
        // `{fu|l` begun: the part is `{ful`, past the caret.
        let offered = at("{ful x", 3).unwrap();
        assert_eq!(offered.range, 0..4);
        assert_eq!(offered.items, [(0, 0)]);
        // What follows goes on with none of them: the part ends at the caret.
        assert_eq!(at("{nx}", 2).unwrap().range, 0..2);
        // Over whole characters only.
        assert_eq!(at("{fé", 2).unwrap().range, 0..2);
    }

    /// An escape writes a sign itself and begins no part: a brace typed
    /// twice is a brace, the third begins a placeholder.
    #[test]
    fn an_escape_begins_no_part() {
        let pattern = Reference::new("Pattern")
            .group(
                Group::new("Signs")
                    .item(Item::new(r"\d", "A digit"))
                    .item(Item::new(r"\w", "A letter")),
            )
            .starts(&["\\"])
            .escapes(&["\\\\"]);
        assert_eq!(pattern.offered(r"a\\", 3), None);
        assert_eq!(pattern.offered(r"\\d", 3), None);
        assert_eq!(pattern.offered(r"\\\", 3).map(|o| o.range), Some(2..3));
        let template = Reference::new("Template")
            .group(Group::new("Parts").item(Item::new("{name}", "The name")))
            .starts(&["{"])
            .escapes(&["{{", "}}"]);
        assert_eq!(template.offered("a{{", 3), None);
        assert_eq!(template.offered("{{na", 4), None);
        assert_eq!(template.offered("{{{na", 5).map(|o| o.range), Some(2..5));
        assert_eq!(template.offered("}}{na", 5).map(|o| o.range), Some(2..5));
    }

    #[test]
    fn items_are_found_by_their_places_and_a_recipe_fills_in_place_of_inserting() {
        let recipe = Item::new(r"\s+ → _", "Spaces to _")
            .fills("regex", r"\s+")
            .fills("replacement", "_");
        let reference = Reference::new("Regex")
            .group(Group::new("Recipes").item(recipe.clone()))
            .group(regex_pattern());
        assert_eq!(reference.item(0, 0), Some(&recipe));
        assert_eq!(recipe.inserts, None);
        assert_eq!(recipe.fills.len(), 2);
        assert_eq!(
            reference.item(1, 1).map(|i| i.written.as_str()),
            Some(r"\d")
        );
        assert_eq!(reference.items().count(), 1 + regex_pattern().items.len());
        assert!(!reference.is_empty());
        assert!(Reference::new("Nothing").is_empty());
        // Said as data, as a question passes it.
        let json = serde_json::to_value(&reference).unwrap();
        assert_eq!(
            serde_json::from_value::<Reference>(json).unwrap(),
            reference
        );
    }
}
