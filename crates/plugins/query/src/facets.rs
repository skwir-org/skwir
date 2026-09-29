//! A query as the designer shows it: chips.
//!
//! A query's top-level terms (the ones it requires together) are sorted into
//! facets: its words, the kinds of file, when they were modified, their
//! size, a tag, what they contain, and where the search looks. What no
//! facet holds (an OR across terms, a NOT, another field, a second period)
//! stays as text in a chip of its own, so nothing a person typed is lost.
//!
//! [`describe`] says what a search is, with the invocation every choice
//! runs, so the UI draws chips without knowing the language; [`refine`]
//! applies one choice and writes the query back. The text stays the one
//! truth: chips are a view of it, and the words and presets they write are
//! the language's own (`kind:image`, `modified:this-week`, `size>1G`).

use bibli_core::command::Invocation;
use bibli_platform::mime::Kind;
use serde::Serialize;

use crate::lang::{self, ContentPat, Expr, Query, Term, TimeField, quote};

/// When, as the Modified chip offers it: a label and what follows
/// `modified` in the query (`:` is implied before a word).
pub const MODIFIED: [(&str, &str); 9] = [
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
    /// `words`, `kind`, `modified`, `size`, `tag`, `content`, `where`, or
    /// `advanced` for a part no facet holds.
    pub id: String,
    pub title: String,
    /// What the facet is set to, in words; `None` while it is not.
    pub value: Option<String>,
    /// A bundled symbolic icon.
    pub icon: String,
    pub choices: Vec<Choice>,
    /// Takes the facet out of the search, when it is set.
    pub clear: Option<Invocation>,
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
    #[serde(with = "bibli_core::os_path")]
    pub root: std::path::PathBuf,
    pub text: String,
}

/// A query cut into facets. Each keeps the text it prints as, so a
/// query written back reads as its author wrote the parts not changed.
#[derive(Clone, Debug, Default)]
struct Parts {
    words: Vec<String>,
    kinds: Vec<Kind>,
    modified: Option<String>,
    size: Option<String>,
    tag: Option<String>,
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
            match e {
                // A name prints as typed: `invoice`, not `name:invoice`.
                Expr::Term(Term::Name(n)) => p.words.push(quote(&n.text)),
                Expr::Term(Term::Kind(k)) if p.kinds.is_empty() => p.kinds.clone_from(k),
                Expr::Term(Term::Time {
                    field: TimeField::Modified,
                    text,
                    ..
                }) if p.modified.is_none() => p.modified = Some(text.clone()),
                Expr::Term(Term::Size { text, .. }) if p.size.is_none() => {
                    p.size = Some(text.clone());
                }
                Expr::Term(Term::Tag(t)) if p.tag.is_none() => p.tag = Some(t.clone()),
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
        for (mine, theirs) in [
            (&mut self.modified, other.modified),
            (&mut self.size, other.size),
            (&mut self.content, other.content),
        ] {
            match (mine.is_none(), theirs) {
                (true, theirs) if theirs.is_some() => *mine = theirs,
                (_, Some(theirs)) => self.advanced.push(theirs),
                _ => {}
            }
        }
        match (&self.tag, other.tag) {
            (None, tag) => self.tag = tag,
            (Some(_), Some(tag)) => self.advanced.push(tag_term(&tag)),
            (Some(_), None) => {}
        }
        self.advanced.extend(other.advanced);
    }

    /// The query text: these parts, then `options`' `in:`, `depth:`,
    /// `hidden:` and `case:`.
    fn write(&self, options: &Query) -> String {
        let mut out: Vec<String> = self.words.clone();
        if !self.kinds.is_empty() {
            out.push(kind_term(&self.kinds));
        }
        out.extend(self.modified.iter().cloned());
        out.extend(self.size.iter().cloned());
        out.extend(self.tag.iter().map(|t| tag_term(t)));
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

/// `modified` or `size` with what the chip gives: a word follows a colon
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
        ("modified", None) => p.modified = None,
        ("modified", Some(v)) => {
            let term = with_key("modified", v);
            check(&term, |t| {
                matches!(
                    t,
                    Term::Time {
                        field: TimeField::Modified,
                        ..
                    }
                )
            })?;
            p.modified = Some(term);
        }
        ("size", None) => p.size = None,
        ("size", Some(v)) => {
            let term = with_key("size", v);
            check(&term, |t| matches!(t, Term::Size { .. }))?;
            p.size = Some(term);
        }
        ("tag", value) => {
            p.tag = value
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_owned);
        }
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
    }
}

/// The chips of `expr`: every facet, set or not, then the parts no facet
/// holds. With `tags_known`, the Tag chip picks from the tags the search
/// found (`query.tag_choices`), else it asks for one.
pub fn describe(expr: &str, tags_known: bool) -> Result<Description, String> {
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
    });

    chips.push(preset_chip(
        "modified",
        "Modified",
        "calendar",
        p.modified.as_ref(),
        &MODIFIED,
        (
            "Modified",
            "A date, a range or a period: 2026-05, 2026-01-01..2026-03-31, <30d",
        ),
    ));
    chips.push(preset_chip(
        "size",
        "Size",
        "sliders",
        p.size.as_ref(),
        &SIZE,
        ("Size", "A size or a range: >10M, 1M..5M, <500k"),
    ));

    let tag = p.tag.clone().unwrap_or_default();
    chips.push(Chip {
        id: "tag".into(),
        title: "Tag".into(),
        value: p.tag.clone(),
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
                ask_tag(&tag)
            },
            active: false,
        }],
        clear: p.tag.as_ref().map(|_| refine_invocation("tag", None)),
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
    });

    let folder = crate::place_name(&q.root);
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
        choices: vec![
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
        ],
        clear: None,
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
        });
    }
    // Written as `refine` writes a query back: names as typed.
    let text = p.write(&Query {
        root_given: false,
        ..q.clone()
    });
    Ok(Description {
        chips,
        root: q.root,
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chip<'a>(d: &'a Description, id: &str) -> &'a Chip {
        d.chips.iter().find(|c| c.id == id).unwrap()
    }

    #[test]
    fn a_query_is_cut_into_facets_and_the_rest() {
        let d = describe(
            r#"invoice "two words" kind:document modified:this-week size>1M tag:work content:total (a OR b) -draft in:/data depth:1"#,
            false,
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
        // The search cut in two: where it looks, and the rest.
        assert_eq!(d.root, std::path::Path::new("/data"));
        let again = lang::parse(&format!("{} in:/data", d.text)).unwrap();
        let q = lang::parse(r#"invoice "two words" kind:document modified:this-week size>1M tag:work content:total (a OR b) -draft in:/data depth:1"#).unwrap();
        assert_eq!(again.to_string(), q.to_string());
        assert!(!d.text.contains("in:"), "{}", d.text);
        // Nothing is set on an empty search but where it looks.
        let d = describe("in:/data", false).unwrap();
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
