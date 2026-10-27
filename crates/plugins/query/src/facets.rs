//! A query as the designer shows it: chips.
//!
//! A query's top-level terms (the ones it requires together) are sorted into
//! facets: its words, the kinds of file, when they were modified, created
//! or last opened and when a picture was taken (one term each), their
//! size, the tags every match has and those none has, what they contain,
//! and where the search looks.
//!
//! The tags are a set, each a category holding what is within it
//! (`metadata::tag_within`): a tag within one already there narrows it
//! (`Places/France` takes the place of `Places`), one holding one there
//! adds nothing. Each is a chip of its own whose segments go up to its
//! categories (`Places › France`: `Places` goes back to `Places`), so a
//! search of tags reads, and walks, like a path. What no
//! facet holds (an OR across terms, a NOT, another field, a second period
//! of one time) stays as text in a chip of its own, so nothing a person
//! typed is lost. Modified and Taken are offered while not set; Created
//! and Accessed are chips once a search has them (the timeline's).
//!
//! [`describe`] says what a search is, with the invocation every choice
//! runs, so the UI draws chips without knowing the language; [`refine`]
//! applies one choice and writes the query back. The text stays the one
//! truth: chips are a view of it, and the words and presets they write are
//! the language's own (`kind:image`, `modified:this-week`, `size>1G`).

use serde::Serialize;
use skwir_core::command::Invocation;
use skwir_core::metadata::{TAG_SEPARATOR, TagRef, clean_tag, same_tag, tag_leaf, tag_within};
use skwir_platform::mime::Kind;

use crate::lang::{self, ContentPat, Expr, Query, Term, quote};

/// The times a search can be narrowed to: when files were modified,
/// created, last opened, and when a picture or a video was taken. Each is
/// a facet named as its term's key, with its chip's title and whether the
/// chip is offered while not set.
const WHEN: [(&str, &str, bool); 4] = [
    ("modified", "Modified", true),
    ("created", "Created", false),
    ("accessed", "Accessed", false),
    ("taken", "Taken", true),
];

/// When, as the chips of [`WHEN`] offer it: a label and what follows the
/// key in the query (`:` is implied before a word).
pub const PERIODS: [(&str, &str); 9] = [
    ("Today", "today"),
    ("Yesterday", "yesterday"),
    ("This week", "this-week"),
    ("Last week", "last-week"),
    ("This month", "this-month"),
    ("Last month", "last-month"),
    ("This year", "this-year"),
    ("Last year", "last-year"),
    ("Older than a year", ">1y"),
];

/// How big, as the Size chip offers it.
pub const SIZE: [(&str, &str); 6] = [
    ("Empty", "0"),
    ("Under 100 KB", "<100k"),
    ("Under 1 MB", "<1M"),
    ("1 to 100 MB", "1M..100M"),
    ("Over 100 MB", ">100M"),
    ("Over 1 GB", ">1G"),
];

/// A chip: a facet of the search, or a part of it no facet holds.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Chip {
    /// `words`, `kind`, `modified`, `created`, `accessed`, `taken`,
    /// `size`, `tag`, `content`, `where`, or `advanced` for a part no
    /// facet holds.
    pub id: String,
    pub title: String,
    /// What the facet is set to, in words; `None` while it is not.
    pub value: Option<String>,
    /// A bundled symbolic icon.
    pub icon: String,
    pub choices: Vec<Choice>,
    /// Takes the facet out of the search, when it is set.
    pub clear: Option<Invocation>,
    /// A tag's chip: its categories, each going up to it, then itself.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<Segment>,
}

/// A part of a tag's chip.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Segment {
    pub label: String,
    /// Up to this category; `None` for the tag itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Invocation>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Choice {
    pub label: String,
    pub command: Invocation,
    /// This is what the facet is set to (or one of the kinds it holds).
    pub active: bool,
}

/// A search's chips, in the order they are shown, and the search cut in
/// two for whoever edits it: where it looks (`root`) and the rest
/// (`text`, the query without its `in:`), which `query.search` puts
/// together again (`expr` and `root`).
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Description {
    pub chips: Vec<Chip>,
    #[serde(with = "skwir_core::os_path")]
    pub root: std::path::PathBuf,
    pub text: String,
    /// The place it looks within instead of a folder (`in:recent:`): the
    /// scope of whoever edits it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub within: Option<skwir_core::location::Location>,
}

/// A query cut into facets. Each keeps the text it prints as, so a
/// query written back reads as its author wrote the parts not changed.
#[derive(Clone, Debug, Default)]
struct Parts {
    words: Vec<String>,
    kinds: Vec<Kind>,
    /// A term for each of [`WHEN`].
    when: [Option<String>; 4],
    size: Option<String>,
    /// Every match has each, or one within it.
    tags: Vec<String>,
    /// No match has any.
    not_tags: Vec<String>,
    content: Option<String>,
    advanced: Vec<String>,
}

/// The terms a query ANDs together.
fn items(q: &Query) -> Vec<&Expr> {
    match &q.expr {
        None => Vec::new(),
        Some(Expr::And(items)) => items.iter().collect(),
        Some(e) => vec![e],
    }
}

/// Which of [`WHEN`] a time term is.
fn when_of(t: &Term) -> Option<usize> {
    let key = match t {
        Term::Time { field, .. } => field.name(),
        Term::Taken { .. } => "taken",
        _ => return None,
    };
    WHEN.iter().position(|(w, ..)| *w == key)
}

/// A term on its own, as a part of an AND prints it.
fn print(e: &Expr) -> String {
    match e {
        Expr::Or(_) => format!("({e})"),
        _ => e.to_string(),
    }
}

impl Parts {
    fn of(q: &Query) -> Parts {
        let mut p = Parts::default();
        for e in items(q) {
            // The first term of each time; a second is text.
            if let Expr::Term(t) = e
                && let Some(i) = when_of(t)
                && p.when[i].is_none()
            {
                p.when[i] = Some(t.to_string());
                continue;
            }
            match e {
                // A name prints as typed: `invoice`, not `name:invoice`.
                Expr::Term(Term::Name(n)) => p.words.push(quote(&n.text)),
                Expr::Term(Term::Kind(k)) if p.kinds.is_empty() => p.kinds.clone_from(k),
                Expr::Term(Term::Size { text, .. }) if p.size.is_none() => {
                    p.size = Some(text.clone());
                }
                Expr::Term(Term::Tag(t)) => p.add_tag(t),
                Expr::Not(inner) if matches!(&**inner, Expr::Term(Term::Tag(_))) => {
                    if let Expr::Term(Term::Tag(t)) = &**inner {
                        p.exclude_tag(t);
                    }
                }
                Expr::Term(t @ Term::Content(_)) if p.content.is_none() => {
                    p.content = Some(t.to_string());
                }
                other => p.advanced.push(print(other)),
            }
        }
        p
    }

    /// Everything a fragment the user typed holds, added to these parts:
    /// words to the words, a facet where it is free, the rest as text.
    fn merge(&mut self, other: Parts) {
        self.words.extend(other.words);
        if self.kinds.is_empty() {
            self.kinds = other.kinds;
        } else if !other.kinds.is_empty() {
            self.advanced.push(kind_term(&other.kinds));
        }
        for (mine, theirs) in self.when.iter_mut().zip(other.when).chain([
            (&mut self.size, other.size),
            (&mut self.content, other.content),
        ]) {
            match (mine.is_none(), theirs) {
                (true, theirs) if theirs.is_some() => *mine = theirs,
                (_, Some(theirs)) => self.advanced.push(theirs),
                _ => {}
            }
        }
        for tag in other.tags {
            self.add_tag(&tag);
        }
        for tag in other.not_tags {
            self.exclude_tag(&tag);
        }
        self.advanced.extend(other.advanced);
    }

    /// One more tag every match has: one within it narrows a tag there
    /// (`Places/France` replaces `Places`); one holding a tag there adds
    /// nothing. No longer one none has.
    fn add_tag(&mut self, tag: &str) {
        let tag = clean_tag(tag);
        if tag.is_empty() || self.tags.iter().any(|t| tag_within(t, &tag)) {
            return;
        }
        self.tags.retain(|t| !tag_within(&tag, t));
        self.not_tags.retain(|t| !same_tag(t, &tag));
        self.tags.push(tag);
    }

    /// One more tag no match has; no longer one every match has.
    fn exclude_tag(&mut self, tag: &str) {
        let tag = clean_tag(tag);
        if tag.is_empty() || self.not_tags.iter().any(|t| same_tag(t, &tag)) {
            return;
        }
        self.tags.retain(|t| !same_tag(t, &tag));
        self.not_tags.push(tag);
    }

    /// The query text: these parts, then `options`' `in:`, `depth:`,
    /// `hidden:` and `case:`.
    fn write(&self, options: &Query) -> String {
        let mut out: Vec<String> = self.words.clone();
        if !self.kinds.is_empty() {
            out.push(kind_term(&self.kinds));
        }
        out.extend(self.when.iter().flatten().cloned());
        out.extend(self.size.iter().cloned());
        out.extend(self.tags.iter().map(|t| tag_term(t)));
        out.extend(
            self.not_tags
                .iter()
                .map(|t| Expr::Not(Box::new(Expr::Term(Term::Tag(t.clone())))).to_string()),
        );
        out.extend(self.content.iter().cloned());
        out.extend(self.advanced.iter().cloned());
        let options = Query {
            expr: None,
            ..options.clone()
        }
        .to_string();
        out.push(options);
        out.retain(|s| !s.is_empty());
        out.join(" ")
    }
}

fn kind_term(kinds: &[Kind]) -> String {
    Term::Kind(kinds.to_vec()).to_string()
}

fn tag_term(tag: &str) -> String {
    Term::Tag(tag.to_owned()).to_string()
}

/// A time or `size` with what the chip gives: a word follows a colon
/// (`modified:today`), a comparison stands as it is (`size>1G`).
fn with_key(key: &str, value: &str) -> String {
    let value = value.trim();
    if value.starts_with(['<', '>', '=']) {
        format!("{key}{value}")
    } else {
        format!("{key}:{value}")
    }
}

/// Parse `term` alone and check it is the facet's kind of term.
fn check(term: &str, is: impl Fn(&Term) -> bool) -> Result<(), String> {
    let q = lang::parse(term).map_err(|e| e.to_string())?;
    match q.expr {
        Some(Expr::Term(t)) if is(&t) => Ok(()),
        _ => Err(format!("`{term}` is not one condition")),
    }
}

/// `expr` with one facet changed: `value` sets it (a kind toggles in and
/// out), `None` clears it. `facet` is a chip's id; for `advanced`, `value`
/// is the index of the part to remove.
pub fn refine(expr: &str, facet: &str, value: Option<&str>) -> Result<String, String> {
    let mut q = lang::parse(expr).map_err(|e| e.to_string())?;
    let mut p = Parts::of(&q);
    // A time: set to a term of its own key, or cleared.
    if let Some(i) = WHEN.iter().position(|(w, ..)| *w == facet) {
        p.when[i] = match value {
            None => None,
            Some(v) => {
                let term = with_key(facet, v);
                check(&term, |t| when_of(t) == Some(i))?;
                Some(term)
            }
        };
        return Ok(p.write(&q));
    }
    match (facet, value) {
        ("words", value) => {
            p.words.clear();
            let text = value.unwrap_or_default().trim();
            if !text.is_empty() {
                let typed = lang::parse(text).map_err(|e| e.to_string())?;
                p.merge(Parts::of(&typed));
            }
        }
        ("kind", None) => p.kinds.clear(),
        ("kind", Some(word)) => {
            let kind = Kind::from_name(word).ok_or_else(|| format!("no kind `{word}`"))?;
            if let Some(at) = p.kinds.iter().position(|k| *k == kind) {
                p.kinds.remove(at);
            } else {
                p.kinds.push(kind);
            }
        }
        ("size", None) => p.size = None,
        ("size", Some(v)) => {
            let term = with_key("size", v);
            check(&term, |t| matches!(t, Term::Size { .. }))?;
            p.size = Some(term);
        }
        // One more tag (the Tag chip's choice), or none.
        ("tag", Some(v)) => p.add_tag(v),
        ("tag", None) => p.tags.clear(),
        // One more tag every match has, or one less: the sidebar's
        // Shift+click on a tag, a tag the search can be narrowed by.
        ("also_tag", Some(v)) => {
            let v = clean_tag(v);
            if p.tags.iter().any(|t| same_tag(t, &v)) {
                p.tags.retain(|t| !same_tag(t, &v));
            } else {
                p.add_tag(&v);
            }
        }
        ("drop_tag", Some(v)) => p.tags.retain(|t| !same_tag(t, v.trim())),
        // A tag's chip back up to one of its categories.
        ("tag_up", Some(v)) => {
            let v = clean_tag(v);
            if let Some(at) = p.tags.iter().position(|t| tag_within(t, &v)) {
                p.tags[at].clone_from(&v);
                let mut kept = 0;
                p.tags.retain(|t| {
                    kept += 1;
                    kept - 1 == at || !tag_within(t, &v)
                });
            }
        }
        // A tag no match has, or one less.
        ("not_tag", Some(v)) => {
            let v = clean_tag(v);
            if p.not_tags.iter().any(|t| same_tag(t, &v)) {
                p.not_tags.retain(|t| !same_tag(t, &v));
            } else {
                p.exclude_tag(&v);
            }
        }
        ("not_tag", None) => p.not_tags.clear(),
        ("content", None) => p.content = None,
        ("content", Some(text)) => {
            p.content = Some(
                Term::Content(ContentPat::text(text, q.case_sensitive).map_err(|e| e.to_string())?)
                    .to_string(),
            );
        }
        ("where", Some("subtree")) => q.depth = None,
        ("where", Some("here")) => q.depth = Some(1),
        ("where", Some("hidden")) => q.hidden = !q.hidden,
        ("advanced", Some(index)) => {
            let at: usize = index.parse().map_err(|_| format!("no part `{index}`"))?;
            if at < p.advanced.len() {
                p.advanced.remove(at);
            }
        }
        (facet, _) => return Err(format!("no facet `{facet}`")),
    }
    Ok(p.write(&q))
}

fn refine_invocation(facet: &str, value: Option<&str>) -> Invocation {
    Invocation::with_args(
        "query.refine",
        &serde_json::json!({ "facet": facet, "value": value }),
    )
}

/// Ask for a line of text and refine `facet` with it.
fn ask(facet: &str, title: &str, placeholder: &str, text: &str) -> Invocation {
    Invocation::with_args(
        "ui.ask",
        &serde_json::json!({
            "title": title,
            "accept": "Search",
            "placeholder": placeholder,
            "text": text,
            "command": "query.refine",
            "args": { "facet": facet },
            "field": "value",
        }),
    )
}

/// Ask for a tag to type.
pub fn ask_tag(current: &str) -> Invocation {
    ask("tag", "Tagged", "A tag", current)
}

/// The picker's answer for the Tag chip (`ui.choose`'s source): the tags
/// among the files, most used first, each refining the search, then one
/// to type.
pub fn tag_choices(counts: &std::collections::BTreeMap<String, usize>) -> serde_json::Value {
    let mut tags: Vec<(&String, &usize)> = counts.iter().collect();
    tags.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    let mut choices: Vec<serde_json::Value> = tags
        .into_iter()
        .map(|(tag, n)| {
            serde_json::json!({
                "title": tag,
                "detail": if *n == 1 { "1 file".to_owned() } else { format!("{n} files") },
                "icon": "tag",
                "command": refine_invocation("tag", Some(tag)),
            })
        })
        .collect();
    choices.push(serde_json::json!({
        "title": "Another tag…",
        "section": "Other",
        "command": ask_tag(""),
    }));
    serde_json::json!({
        "title": "Tagged",
        "empty": "No tags among these files",
        "choices": choices,
    })
}

/// What a preset list says a term is: its label when it is one of them,
/// else what follows the key.
fn label_of(term: &str, key: &str, presets: &[(&str, &str)]) -> String {
    let rest = term.strip_prefix(key).unwrap_or(term);
    let bare = rest.strip_prefix(':').unwrap_or(rest);
    presets
        .iter()
        .find(|(_, v)| *v == bare || *v == rest)
        .map_or_else(|| bare.to_owned(), |(label, _)| (*label).to_owned())
}

/// A chip of presets, one active when the term is it, and Custom….
fn preset_chip(
    id: &str,
    title: &str,
    icon: &str,
    term: Option<&String>,
    presets: &[(&str, &str)],
    custom: (&str, &str),
) -> Chip {
    let current = term.map(|t| t.strip_prefix(id).unwrap_or(t).to_owned());
    let bare = current
        .as_deref()
        .map(|c| c.strip_prefix(':').unwrap_or(c).to_owned());
    let mut choices: Vec<Choice> = presets
        .iter()
        .map(|(label, value)| Choice {
            label: (*label).to_owned(),
            command: refine_invocation(id, Some(value)),
            active: bare.as_deref() == Some(*value) || current.as_deref() == Some(*value),
        })
        .collect();
    choices.push(Choice {
        label: "Custom…".to_owned(),
        command: ask(id, custom.0, custom.1, bare.as_deref().unwrap_or_default()),
        active: false,
    });
    Chip {
        id: id.to_owned(),
        title: title.to_owned(),
        value: term.map(|t| label_of(t, id, presets)),
        icon: icon.to_owned(),
        choices,
        clear: term.map(|_| refine_invocation(id, None)),
        segments: Vec::new(),
    }
}

/// A tag's chip: its categories, each going back up to it, then itself;
/// its menu goes up too, and takes it out.
fn tag_chip(tag: &str) -> Chip {
    let parts: Vec<&str> = tag.split(TAG_SEPARATOR).collect();
    let mut segments = Vec::new();
    let mut choices = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let up = parts[..=i].join("/");
        let last = i + 1 == parts.len();
        let command = (!last).then(|| refine_invocation("tag_up", Some(&up)));
        if let Some(command) = &command {
            choices.push(Choice {
                label: format!("All of “{part}”"),
                command: command.clone(),
                active: false,
            });
        }
        segments.push(Segment {
            label: (*part).to_owned(),
            command,
        });
    }
    choices.push(Choice {
        label: "Not tagged so instead".into(),
        command: refine_invocation("not_tag", Some(tag)),
        active: false,
    });
    Chip {
        id: "tag".into(),
        title: "Tag".into(),
        value: Some(tag.to_owned()),
        icon: "tag".into(),
        choices,
        clear: Some(refine_invocation("drop_tag", Some(tag))),
        segments,
    }
}

/// A part of a folder's narrowing (`query.narrow`) as a pill: its words,
/// and the facet and value that take it off, as [`refine`] takes them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub label: String,
    pub facet: &'static str,
    pub value: Option<String>,
    /// The tag it narrows by, whose color its pill wears.
    pub tag: Option<TagRef>,
}

/// What a narrowing reads as (`kind:image modified:2024` is “Images” and
/// “modified in 2024”), and the facets it sets, which the Narrow by bar
/// does not offer again.
pub fn parts(expr: &str) -> Result<(Vec<Part>, Vec<String>), String> {
    let q = lang::parse(expr).map_err(|e| e.to_string())?;
    let p = Parts::of(&q);
    let part = |label: String, facet, value: Option<&str>| Part {
        label,
        facet,
        value: value.map(str::to_owned),
        tag: None,
    };
    let mut parts = Vec::new();
    let mut set = Vec::new();
    if !p.words.is_empty() {
        parts.push(part(format!("“{}”", p.words.join(" ")), "words", None));
    }
    for kind in &p.kinds {
        parts.push(part(kind.title().to_owned(), "kind", Some(kind.name())));
        set.push("kind".to_owned());
    }
    for ((facet, ..), term) in WHEN.iter().zip(&p.when) {
        let Some(term) = term else { continue };
        let when = label_of(term, facet, &PERIODS);
        let label = if when.chars().all(|c| c.is_ascii_digit()) {
            format!("{facet} in {when}")
        } else {
            format!("{facet} {}", when.to_lowercase())
        };
        parts.push(part(label, facet, None));
        set.push((*facet).to_owned());
    }
    if let Some(term) = &p.size {
        let size = label_of(term, "size", &SIZE).to_lowercase();
        parts.push(part(format!("size {size}"), "size", None));
    }
    for tag in &p.tags {
        parts.push(Part {
            tag: Some(TagRef::of(tag)),
            ..part(format!("with {}", tag_leaf(tag)), "drop_tag", Some(tag))
        });
    }
    for tag in &p.not_tags {
        parts.push(Part {
            tag: Some(TagRef::lacking(tag)),
            ..part(format!("not {}", tag_leaf(tag)), "not_tag", Some(tag))
        });
    }
    for (i, text) in p.advanced.iter().enumerate() {
        parts.push(part(text.clone(), "advanced", Some(&i.to_string())));
    }
    set.dedup();
    Ok((parts, set))
}

/// The search without where it looks (`Description::text`): what it asks
/// for, as typed.
pub fn text_of(expr: &str) -> Option<String> {
    let q = lang::parse(expr).ok()?;
    Some(without_root(&Parts::of(&q), &q))
}

/// Written as `refine` writes a query back: names as typed.
fn without_root(p: &Parts, q: &Query) -> String {
    p.write(&Query {
        root_given: false,
        within: None,
        ..q.clone()
    })
}

/// The chips of `expr`: every facet, set or not, then the parts no facet
/// holds. With `tags_known`, the Tag chip picks from the tags the search
/// found (`query.tag_choices`), else it asks for one.
pub fn describe(
    expr: &str,
    tags_known: bool,
    app: Option<&skwir_core::App>,
) -> Result<Description, String> {
    let q = lang::parse(expr).map_err(|e| e.to_string())?;
    let p = Parts::of(&q);
    let mut chips = Vec::new();

    let words = p.words.join(" ");
    chips.push(Chip {
        id: "words".into(),
        title: "Name".into(),
        value: (!words.is_empty()).then(|| words.clone()),
        icon: "search".into(),
        choices: vec![Choice {
            label: if words.is_empty() {
                "Name contains…"
            } else {
                "Change…"
            }
            .into(),
            command: ask(
                "words",
                "Name contains",
                "Words or patterns: invoice *.pdf",
                &words,
            ),
            active: false,
        }],
        clear: (!words.is_empty()).then(|| refine_invocation("words", None)),
        segments: Vec::new(),
    });

    chips.push(Chip {
        id: "kind".into(),
        title: "Kind".into(),
        value: (!p.kinds.is_empty()).then(|| {
            p.kinds
                .iter()
                .map(|k| k.title())
                .collect::<Vec<_>>()
                .join(", ")
        }),
        icon: "grid".into(),
        choices: Kind::ALL
            .iter()
            .map(|k| Choice {
                label: k.title().to_owned(),
                command: refine_invocation("kind", Some(k.name())),
                active: p.kinds.contains(k),
            })
            .collect(),
        clear: (!p.kinds.is_empty()).then(|| refine_invocation("kind", None)),
        segments: Vec::new(),
    });

    for ((facet, title, offered), term) in WHEN.iter().zip(&p.when) {
        if term.is_none() && !offered {
            continue;
        }
        chips.push(preset_chip(
            facet,
            title,
            "calendar",
            term.as_ref(),
            &PERIODS,
            (
                title,
                "A date, a range or a period: 2026-05, 2026-01-01..2026-03-31, <30d",
            ),
        ));
    }
    chips.push(preset_chip(
        "size",
        "Size",
        "sliders",
        p.size.as_ref(),
        &SIZE,
        ("Size", "A size or a range: >10M, 1M..5M, <500k"),
    ));

    // Each tag a chip of its own, walking up to its categories; then the
    // facet unset, for one more.
    for tag in &p.tags {
        chips.push(tag_chip(tag));
    }
    for tag in &p.not_tags {
        chips.push(Chip {
            id: "not_tag".into(),
            title: "Not tagged".into(),
            value: Some(tag.clone()),
            icon: "tag".into(),
            choices: vec![Choice {
                label: "Tagged instead".into(),
                command: refine_invocation("tag", Some(tag)),
                active: false,
            }],
            clear: Some(refine_invocation("not_tag", Some(tag))),
            segments: Vec::new(),
        });
    }
    chips.push(Chip {
        id: "tag".into(),
        title: "Tag".into(),
        value: None,
        icon: "tag".into(),
        choices: vec![Choice {
            label: "Tagged…".into(),
            command: if tags_known {
                Invocation::with_args(
                    "ui.choose",
                    &serde_json::json!({
                        "title": "Tagged",
                        "placeholder": "Filter tags",
                        "source": Invocation::new("query.tag_choices"),
                    }),
                )
            } else {
                ask_tag("")
            },
            active: false,
        }],
        clear: None,
        segments: Vec::new(),
    });

    let content = p
        .content
        .as_deref()
        .map(|c| c.strip_prefix("content:").unwrap_or(c).to_owned());
    chips.push(Chip {
        id: "content".into(),
        title: "Contains".into(),
        value: content.clone(),
        icon: "contents".into(),
        choices: vec![Choice {
            label: "Text in the file…".into(),
            command: ask(
                "content",
                "Contains",
                "Text the files contain",
                content.as_deref().unwrap_or_default(),
            ),
            active: false,
        }],
        clear: content.map(|_| refine_invocation("content", None)),
        segments: Vec::new(),
    });

    let folder = match (&q.within, app) {
        (Some(place), Some(app)) => app.location_title(place),
        (Some(place), None) => place.to_string(),
        (None, _) => crate::place_name(app, &q.root),
    };
    let mut where_value = Vec::new();
    if q.depth == Some(1) {
        where_value.push("this folder only");
    }
    if q.hidden {
        where_value.push("hidden files");
    }
    chips.push(Chip {
        id: "where".into(),
        title: format!("In {folder}"),
        value: (!where_value.is_empty()).then(|| where_value.join(", ")),
        icon: "list".into(),
        // A place is searched as it lists, never below it.
        choices: if q.within.is_some() {
            Vec::new()
        } else {
            vec![
                Choice {
                    label: "This folder and its subfolders".into(),
                    command: refine_invocation("where", Some("subtree")),
                    active: q.depth.is_none(),
                },
                Choice {
                    label: "This folder only".into(),
                    command: refine_invocation("where", Some("here")),
                    active: q.depth == Some(1),
                },
                Choice {
                    label: "Hidden files".into(),
                    command: refine_invocation("where", Some("hidden")),
                    active: q.hidden,
                },
            ]
        },
        clear: None,
        segments: Vec::new(),
    });

    for (i, text) in p.advanced.iter().enumerate() {
        let index = i.to_string();
        chips.push(Chip {
            id: "advanced".into(),
            title: String::new(),
            value: Some(text.clone()),
            icon: "code".into(),
            choices: vec![Choice {
                label: "Remove".into(),
                command: refine_invocation("advanced", Some(&index)),
                active: false,
            }],
            clear: Some(refine_invocation("advanced", Some(&index))),
            segments: Vec::new(),
        });
    }
    let text = without_root(&p, &q);
    Ok(Description {
        chips,
        root: q.root,
        text,
        within: q.within,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chip<'a>(d: &'a Description, id: &str) -> &'a Chip {
        d.chips.iter().find(|c| c.id == id).unwrap()
    }

    /// A folder narrowed by a tag reads as a pill of that tag, in its
    /// color, hollow for the files without it; the others are no tag's.
    #[test]
    fn a_narrowing_by_a_tag_names_its_tag() {
        let (parts, _) = parts("kind:image tag:Places/France NOT tag:draft").unwrap();
        let read: Vec<(&str, Option<TagRef>)> = parts
            .iter()
            .map(|p| (p.label.as_str(), p.tag.clone()))
            .collect();
        assert_eq!(
            read,
            [
                ("Images", None),
                ("with France", Some(TagRef::of("Places/France"))),
                ("not draft", Some(TagRef::lacking("draft"))),
            ]
        );
    }

    #[test]
    fn a_query_is_cut_into_facets_and_the_rest() {
        let d = describe(
            r#"invoice "two words" kind:document modified:this-week size>1M tag:work content:total (a OR b) -draft in:/data depth:1"#,
            false,
            None,
        ).unwrap();
        assert_eq!(
            chip(&d, "words").value.as_deref(),
            Some(r#"invoice "two words""#)
        );
        assert_eq!(chip(&d, "kind").value.as_deref(), Some("Documents"));
        assert_eq!(chip(&d, "modified").value.as_deref(), Some("This week"));
        assert!(
            chip(&d, "modified").choices[2].active,
            "This week is ticked"
        );
        assert_eq!(chip(&d, "size").value.as_deref(), Some(">1M"));
        assert_eq!(chip(&d, "tag").value.as_deref(), Some("work"));
        assert_eq!(chip(&d, "content").value.as_deref(), Some("total"));
        assert_eq!(chip(&d, "where").title, "In data");
        assert_eq!(chip(&d, "where").value.as_deref(), Some("this folder only"));
        let advanced: Vec<_> = d
            .chips
            .iter()
            .filter(|c| c.id == "advanced")
            .map(|c| c.value.clone().unwrap())
            .collect();
        assert_eq!(advanced, ["(name:a OR name:b)", "NOT name:draft"]);
        // Each tag is a chip of its own, which takes it out.
        let two = describe("tag:a tag:b", false, None).unwrap();
        let tags: Vec<_> = two
            .chips
            .iter()
            .filter(|c| c.id == "tag" && c.value.is_some())
            .map(|c| {
                (
                    c.value.clone().unwrap(),
                    c.clear.clone().unwrap().args["facet"].clone(),
                )
            })
            .collect();
        assert_eq!(
            tags,
            [
                ("a".to_owned(), serde_json::json!("drop_tag")),
                ("b".to_owned(), serde_json::json!("drop_tag"))
            ]
        );
        assert!(
            two.chips.iter().any(|c| c.id == "tag" && c.value.is_none()),
            "and the facet, for one more"
        );
        // The search cut in two: where it looks, and the rest.
        assert_eq!(d.root, std::path::Path::new("/data"));
        let again = lang::parse(&format!("{} in:/data", d.text)).unwrap();
        let q = lang::parse(r#"invoice "two words" kind:document modified:this-week size>1M tag:work content:total (a OR b) -draft in:/data depth:1"#).unwrap();
        assert_eq!(again.to_string(), q.to_string());
        assert!(!d.text.contains("in:"), "{}", d.text);
        // Nothing is set on an empty search but where it looks.
        let d = describe("in:/data", false, None).unwrap();
        assert_eq!(d.text, "");
        assert!(d.chips.iter().all(|c| c.value.is_none()));
        assert!(d.chips.iter().all(|c| c.id == "where" || c.clear.is_none()));
    }

    /// Each choice, applied, writes the query it says; the parts it does
    /// not touch keep their text.
    #[test]
    fn a_choice_writes_the_query_back() {
        let base = "invoice size<1M in:/data";
        let r = |facet, value| refine(base, facet, value).unwrap();
        assert_eq!(
            r("kind", Some("image")),
            "invoice kind:image size<1M in:/data"
        );
        assert_eq!(
            r("modified", Some("this-week")),
            "invoice modified:this-week size<1M in:/data"
        );
        assert_eq!(
            r("modified", Some(">1y")),
            "invoice modified>1y size<1M in:/data"
        );
        assert_eq!(r("size", Some(">1G")), "invoice size>1G in:/data");
        assert_eq!(r("size", None), "invoice in:/data");
        assert_eq!(
            r("words", Some("report *.pdf")),
            "report *.pdf size<1M in:/data"
        );
        assert_eq!(r("words", None), "size<1M in:/data");
        assert_eq!(
            r("tag", Some("to do")),
            r#"invoice size<1M tag:"to do" in:/data"#
        );
        assert_eq!(
            r("content", Some("total")),
            "invoice size<1M content:total in:/data"
        );
        assert_eq!(r("where", Some("here")), "invoice size<1M in:/data depth:1");
        assert_eq!(
            r("where", Some("hidden")),
            "invoice size<1M in:/data hidden:true"
        );
        // Tags add up, and toggle out.
        let two = refine("tag:a in:/d", "also_tag", Some("to do")).unwrap();
        assert_eq!(two, r#"tag:a tag:"to do" in:/d"#);
        assert_eq!(
            refine(&two, "also_tag", Some("A")).unwrap(),
            r#"tag:"to do" in:/d"#
        );
        assert_eq!(
            refine(&two, "also_tag", Some("TO DO")).unwrap(),
            "tag:a in:/d"
        );
        assert_eq!(
            refine("in:/d", "also_tag", Some("a")).unwrap(),
            "tag:a in:/d"
        );
        // Kinds toggle in and out.
        let two = refine("kind:image in:/d", "kind", Some("video")).unwrap();
        assert_eq!(two, "kind:image,video in:/d");
        assert_eq!(
            refine(&two, "kind", Some("images")).unwrap(),
            "kind:video in:/d"
        );
        assert_eq!(refine(&two, "kind", None).unwrap(), "in:/d");
        // An advanced part goes by its index.
        let q = "a OR b -draft in:/d";
        assert_eq!(refine(q, "advanced", Some("0")).unwrap(), "in:/d");
        let q = "x (a OR b) -draft in:/d";
        assert_eq!(
            refine(q, "advanced", Some("1")).unwrap(),
            "x (name:a OR name:b) in:/d"
        );
        // What a person types in the words box lands where it belongs.
        assert_eq!(
            refine("in:/d", "words", Some("cat kind:image")).unwrap(),
            "cat kind:image in:/d"
        );
        // A choice that is not its facet's kind of term is refused.
        assert!(refine(base, "modified", Some("kind:image")).is_err());
        assert!(refine(base, "size", Some("huge")).is_err());
        assert!(refine(base, "kind", Some("pdf")).is_err());
        assert!(refine(base, "nope", None).is_err());
    }

    /// Each time is a facet of its own, as the timeline refines them: a
    /// day takes the place of a month, and the others keep theirs.
    #[test]
    fn each_time_is_a_facet_of_its_own() {
        let r = |q: &str, facet, value| refine(q, facet, value).unwrap();
        let april = r("kind:image in:/d", "taken", Some("2023-04"));
        assert_eq!(april, "kind:image taken:2023-04 in:/d");
        let day = r(&april, "taken", Some("2023-04-15"));
        assert_eq!(day, "kind:image taken:2023-04-15 in:/d");
        assert_eq!(
            r(&day, "taken", Some("2023-04-14")),
            "kind:image taken:2023-04-14 in:/d"
        );
        assert_eq!(r(&day, "taken", None), "kind:image in:/d");
        assert_eq!(
            r("modified:2026 in:/d", "created", Some("2026-07")),
            "modified:2026 created:2026-07 in:/d"
        );
        assert_eq!(
            r(
                "created:2026-07 modified:2026 in:/d",
                "created",
                Some("2026-07-02")
            ),
            "modified:2026 created:2026-07-02 in:/d"
        );
        assert_eq!(
            r("accessed<7d in:/d", "accessed", Some(">=last-week")),
            "accessed>=last-week in:/d"
        );
        assert_eq!(r("accessed<7d in:/d", "accessed", None), "in:/d");
        // A value is a time of the facet's own key, nothing else.
        assert!(refine("in:/d", "taken", Some("kind:image")).is_err());
        assert!(refine("in:/d", "created", Some("2026-02-30")).is_err());
        // Their pills: “Taken: 2023-04” in the address field.
        let d = describe(&april, false, None).unwrap();
        let taken = chip(&d, "taken");
        assert_eq!(
            (taken.title.as_str(), taken.value.as_deref()),
            ("Taken", Some("2023-04"))
        );
        assert_eq!(taken.clear, Some(refine_invocation("taken", None)));
        assert!(d.chips.iter().all(|c| c.id != "advanced"));
        let d = describe("created:this-week accessed:2026 in:/d", false, None).unwrap();
        assert_eq!(chip(&d, "created").value.as_deref(), Some("This week"));
        assert_eq!(chip(&d, "accessed").value.as_deref(), Some("2026"));
        // Offered while not set: Modified and Taken.
        let d = describe("in:/d", false, None).unwrap();
        let offered: Vec<&str> = d.chips.iter().map(|c| c.id.as_str()).collect();
        assert!(offered.contains(&"modified") && offered.contains(&"taken"));
        assert!(!offered.contains(&"created") && !offered.contains(&"accessed"));
        // A second period of one time is text; so is a time ORed.
        let d = describe("taken:2023 taken:2024 in:/d", false, None).unwrap();
        assert_eq!(chip(&d, "advanced").value.as_deref(), Some("taken:2024"));
        // A folder narrowed by when its files were created reads so.
        let (parts, set) = parts("created:2026 accessed:this-week").unwrap();
        let labels: Vec<&str> = parts.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(labels, ["created in 2026", "accessed this week"]);
        assert_eq!(set, ["created", "accessed"]);
    }

    #[test]
    fn tags_narrow_into_their_categories_and_walk_back_up() {
        let r = |q: &str, facet, value| refine(q, facet, Some(value)).unwrap();
        // A tag within one there narrows it; one holding one adds nothing.
        assert_eq!(
            r("tag:Places in:/d", "also_tag", "places/France"),
            "tag:places/France in:/d"
        );
        assert_eq!(
            r("tag:Places/France in:/d", "tag", "Places"),
            "tag:Places/France in:/d"
        );
        assert_eq!(
            r("tag:Places/France in:/d", "also_tag", "2024"),
            "tag:Places/France tag:2024 in:/d"
        );
        // Back up to a category, from the chip's segment.
        assert_eq!(
            r("tag:Places/France/Paris tag:2024 in:/d", "tag_up", "Places"),
            "tag:Places tag:2024 in:/d",
            "in its place"
        );
        assert_eq!(r("tag:a tag:b in:/d", "drop_tag", "A"), "tag:b in:/d");
        // None has it: a tag to leave out, which a tag every match has
        // stops being.
        assert_eq!(
            r("tag:Places in:/d", "not_tag", "2024"),
            "tag:Places NOT tag:2024 in:/d"
        );
        assert_eq!(
            r("tag:Places NOT tag:2024 in:/d", "not_tag", "2024"),
            "tag:Places in:/d"
        );
        assert_eq!(r("tag:2024 in:/d", "not_tag", "2024"), "NOT tag:2024 in:/d");
        assert_eq!(r("NOT tag:2024 in:/d", "tag", "2024"), "tag:2024 in:/d");
        // The chip of a tag in categories: each goes up to its own.
        let d = describe("tag:Places/France NOT tag:x in:/d", false, None).unwrap();
        let chip = d
            .chips
            .iter()
            .find(|c| c.value.as_deref() == Some("Places/France"))
            .unwrap();
        let segments: Vec<_> = chip
            .segments
            .iter()
            .map(|s| {
                (
                    s.label.as_str(),
                    s.command.as_ref().map(|c| c.args["value"].clone()),
                )
            })
            .collect();
        assert_eq!(
            segments,
            [
                ("Places", Some(serde_json::json!("Places"))),
                ("France", None)
            ]
        );
        let not = d.chips.iter().find(|c| c.id == "not_tag").unwrap();
        assert_eq!(not.value.as_deref(), Some("x"));
    }

    #[test]
    fn written_back_a_query_means_the_same() {
        for q in [
            "invoice kind:document modified:this-week in:/data",
            r#""it's" size:1M..5M tag:"a b" content:"/x" hidden:true in:"/two words""#,
            "(a OR b) -draft modified:2026-05 in:/d case:sensitive",
        ] {
            let again = refine(q, "kind", Some("image")).unwrap();
            let back = refine(&again, "kind", Some("image")).unwrap();
            // The same parts, ANDed in another order, and the same options.
            let meaning = |text: &str| {
                let q = lang::parse(text).unwrap();
                let mut parts: Vec<String> = items(&q).into_iter().map(print).collect();
                parts.sort();
                (parts, Query { expr: None, ..q }.to_string())
            };
            assert_eq!(meaning(&back), meaning(q), "{q} -> {again} -> {back}");
        }
    }
}
