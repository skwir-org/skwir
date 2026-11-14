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
}

/// What a field offers as it is typed ([`Reference::offered`]): the part
/// typed (byte offsets into the text), which an item chosen takes the
/// place of, and the items that go on with it, by their group's place and
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
        }
    }

    /// Offer, as the field is typed, what goes on with a part begun by one
    /// of `starts`.
    #[must_use]
    pub fn starts(mut self, starts: &[&str]) -> Self {
        self.starts = starts.iter().map(|s| (*s).to_owned()).collect();
        self
    }

    /// What `text` offers with its caret at `caret`: the part typed since
    /// the last sign that begins one, and the items whose `inserts` go on
    /// with it (longer than it, starting with it). `None` where nothing
    /// does: no such sign before the caret, or the part already whole.
    pub fn offered(&self, text: &str, caret: usize) -> Option<Offered> {
        let caret = (0..=caret.min(text.len()))
            .rev()
            .find(|at| text.is_char_boundary(*at))
            .unwrap_or(0);
        let before = &text[..caret];
        let start = self
            .starts
            .iter()
            .filter(|start| !start.is_empty())
            .filter_map(|start| before.rfind(start.as_str()))
            .max()?;
        let typed = &before[start..];
        let items: Vec<(usize, usize)> = self
            .items()
            .filter(|(_, _, item)| {
                item.inserts
                    .as_deref()
                    .is_some_and(|part| part.len() > typed.len() && part.starts_with(typed))
            })
            .map(|(group, at, _)| (group, at))
            .collect();
        (!items.is_empty()).then_some(Offered {
            range: start..caret,
            items,
        })
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
}

/// The signs of a regular expression, as the `regex` crate reads them
/// (a bulk rename's pattern, a search's `content:/…/`): what it matches,
/// how often, where, and in groups. Its caller says what "the start" is
/// the start of (a name, a line).
pub fn regex_pattern() -> Group {
    Group::new("Pattern")
        .item(Item::new(".", "Any character"))
        .item(Item::new(r"\d", "A digit"))
        .item(Item::new(r"\w", "A letter, a digit or _"))
        .item(Item::new(r"\s", "A space or a tab"))
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
        .item(Item::new("(…)", "A group: what it held is $1, $2… after").inserting("()"))
        .item(Item::new(
            r"(?<year>\d{4})",
            "A group with a name: ${year} after",
        ))
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
                    .item(Item::new("recipe", "Fills").fills("regex", "x")),
            )
            .starts(&["{"]);
        let at = |text: &str, caret: usize| reference.offered(text, caret);
        let offered = at("a-{n", 4).unwrap();
        assert_eq!(offered.range, 2..4);
        assert_eq!(offered.items, [(0, 0), (0, 1), (0, 2)]);
        assert_eq!(at("{name}-{", 8).unwrap().items.len(), 3);
        assert_eq!(at("{n:0", 4).unwrap().items, [(0, 2)]);
        assert_eq!(at("{name}", 6), None, "whole already");
        assert_eq!(at("{name}-x", 8), None, "nothing goes on with it");
        assert_eq!(at("name", 4), None, "no sign before the caret");
        // Where the caret is, not the text's end.
        assert_eq!(at("{n} {name}", 2).unwrap().range, 0..2);
        // A caret inside a character is taken back to its start.
        assert_eq!(at("{é", 2).map(|o| o.range), Some(0..1));
        assert_eq!(Reference::new("None").offered("{n", 2), None);
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
