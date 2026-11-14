//! skwir plugin: rename-bulk. Turn a selection plus a rename spec into a
//! plan of renames that is safe to apply in order, even when new names
//! collide with old ones (swaps and chains).
//!
//! Per item, in this order: the regex replacement on the full name, then
//! the template, then the case rule. Template placeholders:
//!
//! - `{name}` stem, `{ext}` extension without the dot, `{.ext}` with its dot
//!   (nothing without one), `{full}` the whole name
//! - `{n}` counter from `start` by `step`; `{n:03}` zero-padded to 3
//! - `{date}` the entry's modification time as `%Y-%m-%d`; `{date:%Y-%m-%d_%H%M}`
//!   with any `strftime` format (local time zone)
//! - `{parent}` the parent directory's name
//! - `{{` and `}}` for literal braces
//!
//! Case rules (`lower`, `upper`, `title`, `sentence`) apply to the stem and
//! leave the extension alone.
//!
//! What the template, the regex and its replacement take is said as data
//! (`rename_bulk.reference`, ADR-137): each placeholder with what it gives
//! for the first item, the signs of a pattern and of a replacement, and
//! recipes that fill both. The dialog draws it from its fields' [?] and
//! F1; a test holds it to what `render` reads. A preview says where the
//! regex matched in each old name, and what it put in the new one
//! (`Marks`).

use std::ffi::OsString;
use std::fmt::Write as _;
use std::ops::Range;
use std::path::Path;
use std::time::SystemTime;

use skwir_core::app::App;
use skwir_core::command::{Invocation, Outcome, Topic};
use skwir_core::entry::{Value, keys};
use skwir_core::op::{
    Change, EntryRef, Operation, Plan, RenameOrderError, RenameRequest, order_renames,
};
use skwir_core::plugin::Plugin;
use skwir_core::reference::{Group, Item as Said, Reference};
use skwir_core::registry::{KeyBinding, MenuItem};
use skwir_core::workspace::{Pane, PaneId};

const ID: &str = "rename-bulk";

/// Case rule applied to the stem.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Case {
    Lower,
    Upper,
    Title,
    Sentence,
}

/// What to do to each name: the regex first, then the template, then the
/// case. `rename_bulk.reference` says what each field takes, with what it
/// gives for the first item.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct RenameSpec {
    /// The new name, text and placeholders: `{name}` (without its
    /// extension), `{ext}`, `{.ext}` (with its dot, nothing without one),
    /// `{full}`, `{parent}`, `{n}` and `{n:03}` (the counter), `{date}`
    /// and `{date:%Y%m%d}` (when it was modified, any strftime format);
    /// `{{` and `}}` for braces. The name the regex left when there is one.
    #[serde(default)]
    pub template: Option<String>,
    /// A pattern (the `regex` crate's syntax) over the whole name,
    /// extension included, each match replaced by `replacement`.
    #[serde(default)]
    pub regex: Option<String>,
    /// What each match becomes: `$1` or `${1}` for a group (`${1}` when a
    /// letter, digit or `_` follows), `${name}` for a named one, `$$` for
    /// a dollar sign; empty takes the match away.
    #[serde(default)]
    pub replacement: Option<String>,
    /// The case of the name without its extension, last.
    #[serde(default)]
    pub case: Option<Case>,
    /// What the counter starts at.
    #[serde(default = "one")]
    pub start: u64,
    /// What the counter adds for each item, in the order given.
    #[serde(default = "one")]
    pub step: u64,
}

fn one() -> u64 {
    1
}

impl Default for RenameSpec {
    fn default() -> Self {
        RenameSpec {
            template: None,
            regex: None,
            replacement: None,
            case: None,
            start: 1,
            step: 1,
        }
    }
}

/// The result: a plan safe to apply in order, the logical old → new list,
/// and each rename's marks.
#[derive(Debug)]
pub struct BulkRename {
    pub plan: Plan,
    pub preview: Vec<Change>,
    /// Beside each of `preview`'s renames.
    pub marks: Vec<Marks>,
}

/// Where the regex matched in an old name, and what it put in the new
/// one: byte ranges into each, as a preview marks them. `put` only while
/// the template and the case left the name as the regex made it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Marks {
    pub matched: Vec<Range<usize>>,
    pub put: Vec<Range<usize>>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BulkRenameError {
    #[error("invalid regex: {0}")]
    Regex(String),
    #[error("invalid template: {0}")]
    Template(String),
    #[error("`{item}` would become an invalid name `{name}`: {reason}")]
    InvalidName {
        item: String,
        name: String,
        reason: String,
    },
    #[error("`{item}` is not a name made of text; rename it on its own")]
    NotText { item: String },
    #[error("several items would become `{name}`: {}", items.join(", "))]
    Duplicate { name: String, items: Vec<String> },
    #[error("`{item}` would become `{name}`, which already exists")]
    Conflict { item: String, name: String },
    #[error("`{item}` has no modification time yet (needed for {{date}})")]
    MissingDate { item: String },
    #[error("nothing to rename")]
    Empty,
    /// The job working it out was cancelled (`scheduler::job_cancelled`):
    /// a preview the user typed past.
    #[error("cancelled")]
    Cancelled,
}

/// How many items are worked out between two looks at whether the job
/// was cancelled.
const LOOK_EVERY: usize = 256;

/// One input item: where it is, what it is called, and when it changed.
pub type Item = (EntryRef, OsString, Option<SystemTime>);

fn split_ext(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, ext),
        _ => (name, ""),
    }
}

fn apply_case(name: &str, case: Case) -> String {
    let (stem, ext) = split_ext(name);
    let new_stem = match case {
        Case::Lower => stem.to_lowercase(),
        Case::Upper => stem.to_uppercase(),
        Case::Title => {
            let mut out = String::with_capacity(stem.len());
            let mut start = true;
            for ch in stem.chars() {
                if ch.is_whitespace() || ch == '_' || ch == '-' || ch == '.' {
                    out.push(ch);
                    start = true;
                } else if start {
                    out.extend(ch.to_uppercase());
                    start = false;
                } else {
                    out.extend(ch.to_lowercase());
                }
            }
            out
        }
        Case::Sentence => {
            let mut chars = stem.chars();
            match chars.next() {
                Some(first) => first
                    .to_uppercase()
                    .chain(chars.flat_map(char::to_lowercase))
                    .collect(),
                None => String::new(),
            }
        }
    };
    if ext.is_empty() {
        new_stem
    } else {
        format!("{new_stem}.{ext}")
    }
}

/// Expand the template for one item. `name` is the (regex-transformed) name.
fn render(
    template: &str,
    name: &str,
    parent: &str,
    n: u64,
    mtime: Option<SystemTime>,
    item: &str,
) -> Result<String, BulkRenameError> {
    let (stem, ext) = split_ext(name);
    let mut out = String::with_capacity(template.len() + 16);
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let mut token = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(ch) => token.push(ch),
                        None => {
                            return Err(BulkRenameError::Template(format!(
                                "unclosed `{{` in `{template}`"
                            )));
                        }
                    }
                }
                let (key, arg) = token
                    .split_once(':')
                    .map_or((token.as_str(), None), |(k, a)| (k, Some(a)));
                match key {
                    "name" => out.push_str(stem),
                    "ext" => out.push_str(ext),
                    ".ext" if !ext.is_empty() => {
                        out.push('.');
                        out.push_str(ext);
                    }
                    ".ext" => {}
                    "full" => out.push_str(name),
                    "parent" => out.push_str(parent),
                    "n" => {
                        let width: usize = match arg {
                            None => 0,
                            Some(a) => a
                                .trim_start_matches('0')
                                .parse()
                                .or_else(|_| {
                                    if a.chars().all(|c| c == '0') {
                                        Ok(a.len())
                                    } else {
                                        Err(())
                                    }
                                })
                                .map_err(|()| {
                                    BulkRenameError::Template(format!("bad counter width `{a}`"))
                                })?,
                        };
                        let _ = write!(out, "{n:0width$}");
                    }
                    "date" => {
                        let t = mtime.ok_or_else(|| BulkRenameError::MissingDate {
                            item: item.to_owned(),
                        })?;
                        let fmt = arg.unwrap_or("%Y-%m-%d");
                        let ts = jiff::Timestamp::try_from(t)
                            .map_err(|e| BulkRenameError::Template(e.to_string()))?;
                        let zoned = ts.to_zoned(jiff::tz::TimeZone::system());
                        // `strftime(..).to_string()` would panic on a
                        // format it cannot write; this says why instead.
                        let s = jiff::fmt::strtime::format(fmt, &zoned).map_err(|e| {
                            BulkRenameError::Template(format!("bad date format `{fmt}`: {e}"))
                        })?;
                        out.push_str(&s);
                    }
                    other => {
                        return Err(BulkRenameError::Template(format!(
                            "unknown placeholder `{{{other}}}`"
                        )));
                    }
                }
            }
            c => out.push(c),
        }
    }
    Ok(out)
}

/// Compute the new name of every item (unchanged names included). The
/// rules work on text: a name that is not valid UTF-8 may only come out
/// unchanged, never with its bytes replaced.
pub fn new_names(items: &[Item], spec: &RenameSpec) -> Result<Vec<OsString>, BulkRenameError> {
    Ok(worked_out(items, spec)?
        .into_iter()
        .map(|(name, _)| name)
        .collect())
}

/// The spec's regex, read.
fn pattern(spec: &RenameSpec) -> Result<Option<regex::Regex>, BulkRenameError> {
    spec.regex
        .as_deref()
        .map(|r| regex::Regex::new(r).map_err(|e| BulkRenameError::Regex(e.to_string())))
        .transpose()
}

/// `name` with each match of `re` replaced as `replace_all` replaces it
/// (`Captures::expand`), and where it matched and what it put.
fn replaced(re: &regex::Regex, name: &str, replacement: &str) -> (String, Marks) {
    let mut out = String::with_capacity(name.len());
    let mut marks = Marks::default();
    let mut last = 0;
    for caps in re.captures_iter(name) {
        let Some(whole) = caps.get(0) else { continue };
        out.push_str(&name[last..whole.start()]);
        let start = out.len();
        caps.expand(replacement, &mut out);
        marks.matched.push(whole.range());
        marks.put.push(start..out.len());
        last = whole.end();
    }
    out.push_str(&name[last..]);
    (out, marks)
}

/// The folder an item is in, by its name (`{parent}`).
fn parent_of(entry: &EntryRef) -> String {
    entry
        .path
        .parent()
        .and_then(Path::file_name)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The counter for the `i`th item.
fn counter(i: usize, spec: &RenameSpec) -> Result<u64, BulkRenameError> {
    (i as u64)
        .checked_mul(spec.step)
        .and_then(|k| k.checked_add(spec.start))
        .ok_or_else(|| BulkRenameError::Template("the counter overflows".into()))
}

/// Every item's new name and its marks.
fn worked_out(
    items: &[Item],
    spec: &RenameSpec,
) -> Result<Vec<(OsString, Marks)>, BulkRenameError> {
    let re = pattern(spec)?;
    let replacement = spec.replacement.clone().unwrap_or_default();
    let mut out = Vec::with_capacity(items.len());
    for (i, (entry, original, mtime)) in items.iter().enumerate() {
        if i % LOOK_EVERY == 0 && skwir_core::scheduler::job_cancelled() {
            return Err(BulkRenameError::Cancelled);
        }
        let name = original.to_string_lossy();
        let mut n = name.clone().into_owned();
        let mut marks = Marks::default();
        if let Some(re) = &re {
            (n, marks) = replaced(re, &n, &replacement);
        }
        // What the regex made, to tell whether what follows kept it.
        let made = (!marks.put.is_empty()).then(|| n.clone());
        if let Some(t) = &spec.template {
            n = render(t, &n, &parent_of(entry), counter(i, spec)?, *mtime, &name)?;
        }
        if let Some(case) = spec.case {
            n = apply_case(&n, case);
        }
        if made.as_deref() != Some(n.as_str()) {
            marks.put.clear();
        }
        if original.to_str().is_some() {
            out.push((n.into(), marks));
        } else if n == name {
            out.push((original.clone(), Marks::default()));
        } else {
            return Err(BulkRenameError::NotText {
                item: name.into_owned(),
            });
        }
    }
    Ok(out)
}

/// Build the plan. `exists` says whether a path that none of `items`
/// holds is taken; a target there is a conflict. Ordering (chains, swaps,
/// cycles) is [`order_renames`]'s.
pub fn plan_bulk_rename_in(
    items: &[Item],
    exists: impl Fn(&Path) -> bool,
    spec: &RenameSpec,
) -> Result<BulkRename, BulkRenameError> {
    if items.is_empty() {
        return Err(BulkRenameError::Empty);
    }
    let (new, mut marks): (Vec<OsString>, Vec<Marks>) =
        worked_out(items, spec)?.into_iter().unzip();
    if skwir_core::scheduler::job_cancelled() {
        return Err(BulkRenameError::Cancelled);
    }
    let requests: Vec<RenameRequest> = items
        .iter()
        .zip(new)
        .map(|((entry, _, _), new_name)| RenameRequest {
            entry: entry.clone(),
            new_name,
        })
        .collect();
    let old = |i: usize| items[i].1.to_string_lossy().into_owned();
    let steps = order_renames(&requests, exists).map_err(|e| match e {
        RenameOrderError::InvalidName { item, reason } => BulkRenameError::InvalidName {
            item: old(item),
            name: requests[item].new_name.to_string_lossy().into_owned(),
            reason,
        },
        RenameOrderError::Duplicate { items, target } => BulkRenameError::Duplicate {
            name: file_name(&target),
            items: items.into_iter().map(old).collect(),
        },
        RenameOrderError::Conflict { item, target } => BulkRenameError::Conflict {
            item: old(item),
            name: file_name(&target),
        },
    })?;
    let renamed = |r: &RenameRequest| r.entry.path.file_name() != Some(r.new_name.as_os_str());
    let mut kept = requests.iter().map(renamed);
    marks.retain(|_| kept.next().unwrap_or(false));
    let preview: Vec<Change> = requests
        .iter()
        .filter(|r| renamed(r))
        .map(|r| Change::Rename {
            from: r.entry.path.clone(),
            to: r.entry.path.with_file_name(&r.new_name),
        })
        .collect();
    Ok(BulkRename {
        plan: Plan::with_steps(
            format!("rename {} items", preview.len()),
            steps
                .into_iter()
                .map(|r| Box::new(r) as Box<dyn Operation>)
                .collect(),
        ),
        preview,
        marks,
    })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// [`plan_bulk_rename_in`] with no other names taken.
pub fn plan_bulk_rename(items: &[Item], spec: &RenameSpec) -> Result<BulkRename, BulkRenameError> {
    plan_bulk_rename_in(items, |_| false, spec)
}

// ------------------------------------------------------------------ plugin

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct Args {
    #[serde(default)]
    pane: Option<PaneId>,
    /// The files to rename, by absolute path, in this order; the pane's
    /// selection, in its order, when omitted.
    #[serde(default)]
    entries: Vec<skwir_core::op::EntryArg>,
    #[serde(flatten)]
    spec: RenameSpec,
}

/// What a preview is asked: a bulk rename's arguments, and how many of
/// its renames to say at most (all, by default); how many there are in all
/// is said beside them.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct PreviewArgs {
    #[serde(flatten)]
    args: Args,
    #[serde(default)]
    limit: Option<usize>,
}

/// Plan a bulk rename of `a`'s entries, or of the pane's selection.
fn plan_bulk(app: &App, a: Args) -> anyhow::Result<BulkRename> {
    if a.entries.is_empty() {
        plan_selection(app, a.pane, &a.spec)
    } else {
        plan_named(a.entries, &a.spec)
    }
}

/// Plan a bulk rename of files named by path (a script's, an agent's), in
/// the order given, read on the thread of whoever named them: their times
/// as a listing shows them (a link's, what it leads to), and a target
/// taken when a file not among them is there.
fn plan_named(
    entries: Vec<skwir_core::op::EntryArg>,
    spec: &RenameSpec,
) -> anyhow::Result<BulkRename> {
    let items = named_items(entries, usize::MAX)?;
    let renamed: std::collections::HashSet<&Path> =
        items.iter().map(|(r, _, _)| r.path.as_path()).collect();
    let exists = |path: &Path| !renamed.contains(path) && std::fs::symlink_metadata(path).is_ok();
    Ok(plan_bulk_rename_in(&items, exists, spec)?)
}

/// The first `at_most` of the files named by path, read on the thread of
/// whoever named them: their times as a listing shows them (a link's,
/// what it leads to).
fn named_items(
    entries: Vec<skwir_core::op::EntryArg>,
    at_most: usize,
) -> anyhow::Result<Vec<Item>> {
    Ok(skwir_core::op::EntryArg::refs(entries)?
        .into_iter()
        .take(at_most)
        .map(|r| {
            let name = r.path.file_name().unwrap_or_default().to_owned();
            let mtime = std::fs::metadata(&r.path)
                .or_else(|_| std::fs::symlink_metadata(&r.path))
                .and_then(|m| m.modified())
                .ok();
            (r, name, mtime)
        })
        .collect())
}

/// The first `at_most` of what `p` selects, in the order shown (folders
/// opened in place included), but what an agent's call leaves out.
fn selected_items(
    p: &Pane,
    withheld: &std::collections::HashSet<skwir_core::EntryId>,
    at_most: usize,
) -> Vec<Item> {
    let mut items = Vec::new();
    for id in p.order().iter().copied() {
        if items.len() == at_most {
            break;
        }
        if !p.selection.contains(id) || withheld.contains(&id) {
            continue;
        }
        let Some(e) = p.entry(id) else { continue };
        let Some(path) = p.path_of(id) else {
            continue;
        };
        let mtime = e.metadata.value(keys::MODIFIED).and_then(Value::as_time);
        items.push((
            EntryRef::new(id.file, path).with_only_name(e.is_only_name()),
            e.name.clone(),
            mtime,
        ));
    }
    items
}

/// Plan a bulk rename of the pane's selection, in its sorted order. What
/// else is taken is read from the listing, never from the disk: the
/// preview runs on every keystroke. In a folder a target is taken when an
/// entry not selected has that name; elsewhere (search results, the
/// shelf) the listing cannot say, and applying finds out.
fn plan_selection(
    app: &App,
    pane: Option<PaneId>,
    spec: &RenameSpec,
) -> anyhow::Result<BulkRename> {
    let pane = app.pane_or_active(pane);
    // An agent's call leaves out the private files the user selected.
    let withheld = app.withheld(pane);
    let ws = app.workspace.read();
    let p = ws
        .pane(pane)
        .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
    anyhow::ensure!(p.open.is_some(), "pane has no open location");
    let dir = p.location().and_then(skwir_core::Location::path);
    let items = selected_items(p, &withheld, usize::MAX);
    if items.is_empty() {
        return Err(skwir_core::command::NothingSelected.into());
    }
    let exists = |path: &Path| {
        dir.as_deref().is_some_and(|dir| path.parent() == Some(dir))
            && path
                .file_name()
                .and_then(|name| p.listing.find_by_name(name))
                .is_some_and(|e| !p.selection.contains(e.id))
    };
    Ok(plan_bulk_rename_in(&items, exists, spec)?)
}

// --------------------------------------------------------------- reference

/// A field of a bulk rename, whose reference is asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Template,
    Regex,
    Replacement,
}

/// What `rename_bulk.reference` is asked: a field, and the bulk rename as
/// it stands (its items, its regex and its start), for what each
/// placeholder gives the first item.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ReferenceArgs {
    field: Field,
    #[serde(flatten)]
    args: Args,
}

/// What `field` takes (ADR-137): its placeholders or signs, each with
/// what it gives `first` (the first item renamed) under `spec` as it
/// stands, and the recipes that fill the regex and its replacement.
pub fn reference(field: Field, first: Option<&Item>, spec: &RenameSpec) -> Reference {
    match field {
        Field::Template => template_reference(first, spec),
        Field::Regex => Reference::new("Regex").group(recipes(first)).group(
            skwir_core::reference::regex_pattern().note(
                "^ and $ are the start and the end of the name. It runs on the whole name, \
                 extension included, every match replaced; the template comes after it.",
            ),
        ),
        Field::Replacement => Reference::new("Replacement")
            .group(
                Group::new("What a match becomes")
                    .item(Said::new("$1", "What the first group held ($2, $3…)"))
                    .item(Said::new("${1}", "The same, before a letter, a digit or _"))
                    .item(Said::new("${year}", "What the group named year held"))
                    .item(Said::new("$0", "The whole match"))
                    .item(Said::new("$$", "A dollar sign"))
                    .note("$1_x reads as a group named 1_x, which holds nothing: write ${1}_x.")
                    .note("Left empty, what the pattern matched is taken away."),
            )
            .group(recipes(first)),
    }
}

/// The template's placeholders, each giving what it does for `first`.
fn template_reference(first: Option<&Item>, spec: &RenameSpec) -> Reference {
    // What the regex leaves of the first item's name: what the template
    // reads.
    let read = first.and_then(|(entry, original, mtime)| {
        let name = original.to_str()?;
        let name = match pattern(spec).ok()? {
            Some(re) => replaced(&re, name, spec.replacement.as_deref().unwrap_or("")).0,
            None => name.to_owned(),
        };
        Some((
            name,
            parent_of(entry),
            *mtime,
            original.to_string_lossy().into_owned(),
        ))
    });
    let start = spec.start;
    let item = |written: &str, says: &str| {
        let said = Said::new(written, says);
        match read.as_ref().and_then(|(name, parent, mtime, item)| {
            render(written, name, parent, start, *mtime, item).ok()
        }) {
            Some(gives) => said.gives(gives),
            None => said,
        }
    };
    Reference::new("Template")
        .group(
            Group::new("Parts of the name")
                .item(item("{name}", "The name without its extension"))
                .item(item("{ext}", "The extension, without its dot"))
                .item(item("{.ext}", "The extension with its dot; nothing when there is none"))
                .item(item("{full}", "The whole name"))
                .item(item("{parent}", "The folder it is in"))
                .note("The regex runs first: these are the name it leaves. The case comes last, and leaves the extension as it is."),
        )
        .group(
            Group::new("Counter")
                .item(item("{n}", "Counts from Start, one more for each, in the order shown"))
                .item(item("{n:02}", "The same, two digits at least"))
                .item(item("{n:03}", "The same, three digits at least")),
        )
        .group(
            Group::new("When it was modified")
                .item(item("{date}", "Its day"))
                .item(item("{date:%Y%m%d}", "Its day, without dashes"))
                .item(item("{date:%Y-%m-%d_%H%M}", "Its day and time"))
                .item(item("{date:%d.%m.%Y}", "Its day, the day first"))
                .item(item("{date:%B %Y}", "Its month and year"))
                .note("Inside {date:…}: %Y the year, %m the month, %d the day, %H the hour, %M the minute, %S the second, %B the month's name (%b short), %A the day's, %j the day of the year. In this system's time zone."),
        )
        .group(
            Group::new("Signs")
                .item(Said::new("{{", "A brace itself ({{ and }})")),
        )
}

/// Patterns and their replacements for what names often need, each
/// giving what it makes of `first` when it changes it.
fn recipes(first: Option<&Item>) -> Group {
    let name = first.and_then(|(_, original, _)| original.to_str());
    // Written as its pattern; what it is for, then what replaces the
    // pattern (`shown`, words where the replacement is a space), said.
    let recipe = |task: &str, pattern: &str, replacement: &str, shown: Option<&str>| {
        let says = match shown {
            Some(shown) => format!("{task} → {shown}"),
            None => task.to_owned(),
        };
        let said = Said::new(pattern, says)
            .fills("regex", pattern)
            .fills("replacement", replacement);
        let made = name.and_then(|name| {
            let re = regex::Regex::new(pattern).ok()?;
            let made = replaced(&re, name, replacement).0;
            (made != name).then_some(made)
        });
        match made {
            Some(made) => said.gives(made),
            None => said,
        }
    };
    Group::new("Recipes")
        .item(recipe("Spaces to underscores", r"\s+", "_", Some("_")))
        .item(recipe("Underscores to spaces", "_", " ", Some("a space")))
        .item(recipe(
            "Several spaces to one",
            r"\s{2,}",
            " ",
            Some("a space"),
        ))
        .item(recipe("Take away a start (IMG_)", "^IMG_", "", None))
        .item(recipe(
            "Take away what is in brackets",
            r"\s*[\[(][^\])]*[\])]",
            "",
            None,
        ))
        .item(recipe(
            "Take away the numbers at the start",
            r"^\d+[\s._-]*",
            "",
            None,
        ))
        .item(recipe(
            "Swap two parts around “ - ”",
            r"^(.+) - (.+?)(\.[^.]+)?$",
            "$2 - $1$3",
            Some("$2 - $1$3"),
        ))
        .item(recipe(
            "One extension for JPEG pictures",
            r"(?i)\.jpe?g$",
            ".jpg",
            Some(".jpg"),
        ))
}

/// The first of the items `a` names, or of the pane's selection.
fn first_item(app: &App, a: Args) -> anyhow::Result<Option<Item>> {
    if !a.entries.is_empty() {
        return Ok(named_items(a.entries, 1)?.into_iter().next());
    }
    let pane = app.pane_or_active(a.pane);
    let withheld = app.withheld(pane);
    let ws = app.workspace.read();
    Ok(ws
        .pane(pane)
        .and_then(|p| selected_items(p, &withheld, 1).into_iter().next()))
}

/// The rename-bulk plugin.
#[derive(Debug, Default)]
pub struct RenameBulkPlugin;

impl Plugin for RenameBulkPlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Bulk rename"
    }

    fn description(&self) -> &'static str {
        "Rename the selection from a template, a regex, and case rules"
    }

    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Files
    }

    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Write])
    }
    fn register(&self, app: &App) {
        app.commands.register::<Args, _>(
            ID, Topic::Files,
            "rename_bulk",
            "Rename several from a template",
            "Rename the selection, or the entries given, from a template, regex, and case rule (plan)",
            |a, app| Ok(Outcome::Plan(plan_bulk(app, a)?.plan)),
        )
        .plans();
        app.commands.register::<PreviewArgs, _>(ID, Topic::Files, "rename_bulk.preview", "Preview bulk rename", "Return the old → new names a bulk rename would produce (`renames`, the first `limit` of them when given; each with where the regex `matched` in the old name and what it `put` in the new one, byte ranges) and how many there are (`count`)", |a, app| {
            let r = plan_bulk(app, a.args)?;
            // Display names for the dialog; the plan itself is lossless.
            let renames: Vec<(&Path, &Path)> = r
                .preview
                .iter()
                .filter_map(|c| match c {
                    Change::Rename { from, to } => Some((from.as_path(), to.as_path())),
                    _ => None,
                })
                .collect();
            let ranges = |ranges: &[Range<usize>]| -> Vec<[usize; 2]> {
                ranges.iter().map(|r| [r.start, r.end]).collect()
            };
            let shown: Vec<serde_json::Value> = renames
                .iter()
                .zip(&r.marks)
                .take(a.limit.unwrap_or(usize::MAX))
                .filter_map(|((from, to), marks)| {
                    let mut said = serde_json::json!({ "from": from.file_name()?.to_string_lossy(), "to": to.file_name()?.to_string_lossy() });
                    if !marks.matched.is_empty() {
                        said["matched"] = serde_json::json!(ranges(&marks.matched));
                    }
                    if !marks.put.is_empty() {
                        said["put"] = serde_json::json!(ranges(&marks.put));
                    }
                    Some(said)
                })
                .collect();
            Ok(Outcome::Value(serde_json::json!({ "renames": shown, "count": renames.len() })))
        })
        .data();
        app.commands.register::<ReferenceArgs, _>(ID, Topic::Files, "rename_bulk.reference", "Say what a bulk rename's field takes", "Return what `field` (template, regex, replacement) takes: its placeholders or signs, each with what it gives the first of the entries given or selected under the rename as given, and recipes filling the regex and its replacement", |a, app| {
            let spec = a.args.spec.clone();
            let first = first_item(app, a.args)?;
            Ok(Outcome::Value(serde_json::to_value(reference(a.field, first.as_ref(), &spec))?))
        })
        .data();
        app.registry.keymap.add(
            ID,
            KeyBinding {
                keys: "ctrl-f2".into(),
                command: Invocation::new("ui.rename_bulk_prompt"),
                when: Some("pane selection !place".into()),
            },
        );
        // F2 on several items renames them together, as in Nautilus. On one
        // item the shell's own F2 (`pane single`, bound after the plugins'
        // keys, so it wins where both hold) renames it in place.
        app.registry.keymap.add(
            ID,
            KeyBinding {
                keys: "f2".into(),
                command: Invocation::new("ui.rename_bulk_prompt"),
                when: Some("pane selection !place".into()),
            },
        );
        app.registry.menus.add(
            ID,
            MenuItem::new(
                "context",
                "Rename Several…",
                "rename",
                201,
                Invocation::new("ui.rename_bulk_prompt"),
            )
            // Rename's place when several are selected (Rename is for
            // one); Ctrl+F2 opens it for one too.
            .when("selection !single !place"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::entry::FileId;
    use std::sync::Arc;

    fn item(name: &str) -> Item {
        (
            EntryRef::new(
                FileId::new(
                    1,
                    name.len() as u64 * 7919 + name.bytes().map(u64::from).sum::<u64>(),
                ),
                format!("/d/{name}"),
            ),
            name.into(),
            None,
        )
    }

    fn names(items: &[Item], spec: &RenameSpec) -> Vec<OsString> {
        new_names(items, spec).unwrap()
    }

    #[test]
    fn templates_counters_and_placeholders() {
        let items = [item("photo.JPG"), item("notes")];
        let spec = RenameSpec {
            template: Some("{parent}-{name}-{n:03}.{ext}".into()),
            start: 5,
            step: 5,
            ..Default::default()
        };
        assert_eq!(names(&items, &spec), ["d-photo-005.JPG", "d-notes-010."]);
        let spec = RenameSpec {
            template: Some("{{{full}}}".into()),
            ..Default::default()
        };
        assert_eq!(names(&items, &spec)[0], "{photo.JPG}");
        assert!(matches!(
            new_names(
                &items,
                &RenameSpec {
                    template: Some("{nope}".into()),
                    ..Default::default()
                }
            ),
            Err(BulkRenameError::Template(_))
        ));
        assert!(matches!(
            new_names(
                &items,
                &RenameSpec {
                    template: Some("{date}".into()),
                    ..Default::default()
                }
            ),
            Err(BulkRenameError::MissingDate { .. })
        ));
    }

    /// `{.ext}` has its dot only where there is an extension; `{full}`,
    /// the dialog's first template, leaves every name as it is.
    #[test]
    fn an_extension_with_its_dot_and_the_whole_name() {
        let items = [item("photo.jpg"), item("notes"), item(".hidden")];
        let spec = RenameSpec {
            template: Some("{name}-1{.ext}".into()),
            ..Default::default()
        };
        assert_eq!(
            names(&items, &spec),
            ["photo-1.jpg", "notes-1", ".hidden-1"]
        );
        let spec = RenameSpec {
            template: Some("{full}".into()),
            ..Default::default()
        };
        assert_eq!(names(&items, &spec), ["photo.jpg", "notes", ".hidden"]);
    }

    /// The preview marks where the regex matched in the old name, and what
    /// it put in the new one while nothing after it changed the name.
    #[test]
    fn a_preview_marks_what_the_regex_matched_and_put() {
        let items = [item("a b  c.txt"), item("plain.txt")];
        let spec = RenameSpec {
            regex: Some(r"\s+".into()),
            replacement: Some("__".into()),
            ..Default::default()
        };
        let r = plan_bulk_rename(&items, &spec).unwrap();
        assert_eq!(r.preview.len(), 1, "plain.txt is unchanged");
        assert_eq!(r.marks.len(), 1, "one mark beside each rename");
        assert_eq!(r.marks[0].matched, [1..2, 3..5]);
        // a__b__c.txt
        assert_eq!(r.marks[0].put, [1..3, 4..6]);
        // The case changes what it put: only the matches are marked.
        let spec = RenameSpec {
            case: Some(Case::Upper),
            ..spec
        };
        let r = plan_bulk_rename(&items, &spec).unwrap();
        assert_eq!(r.marks[0].matched, [1..2, 3..5]);
        assert!(r.marks[0].put.is_empty());
        // Groups expand as `replace_all` expands them.
        let spec = RenameSpec {
            regex: Some(r"(\w+) (\w+)".into()),
            replacement: Some("${2}_$1".into()),
            ..Default::default()
        };
        assert_eq!(names(&[item("ab cd.txt")], &spec), ["cd_ab.txt"]);
    }

    /// Every placeholder the template's reference offers is one `render`
    /// reads, and gives the first item what renaming it would.
    #[test]
    fn the_template_reference_says_what_render_reads() {
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let first: Item = (
            EntryRef::new(FileId::new(1, 1), "/Holidays/IMG_0042.jpg"),
            "IMG_0042.jpg".into(),
            Some(t),
        );
        let spec = RenameSpec {
            start: 7,
            ..Default::default()
        };
        let said = reference(Field::Template, Some(&first), &spec);
        let gives = |written: &str| {
            said.items()
                .find(|(_, _, item)| item.written == written)
                .and_then(|(_, _, item)| item.gives.clone())
        };
        assert_eq!(gives("{name}").as_deref(), Some("IMG_0042"));
        assert_eq!(gives("{.ext}").as_deref(), Some(".jpg"));
        assert_eq!(gives("{parent}").as_deref(), Some("Holidays"));
        assert_eq!(gives("{n:03}").as_deref(), Some("007"));
        assert!(gives("{date}").is_some_and(|d| d.starts_with("2023-11-1")));
        for (_, _, item) in said.items() {
            let inserts = item.inserts.as_deref().unwrap();
            let rendered = render(
                inserts,
                "IMG_0042.jpg",
                "Holidays",
                7,
                Some(t),
                "IMG_0042.jpg",
            );
            assert!(rendered.is_ok(), "{inserts}: {rendered:?}");
        }
        // The regex runs first: the template reads the name it left.
        let spec = RenameSpec {
            regex: Some("^IMG_".into()),
            ..Default::default()
        };
        let said = reference(Field::Template, Some(&first), &spec);
        assert_eq!(
            said.items()
                .find(|(_, _, item)| item.written == "{name}")
                .and_then(|(_, _, item)| item.gives.clone())
                .as_deref(),
            Some("0042")
        );
        // Without an item, nothing is given, and all is still said.
        let said = reference(Field::Template, None, &RenameSpec::default());
        assert!(said.items().all(|(_, _, item)| item.gives.is_none()));
        assert!(said.items().count() > 10);
    }

    /// Each recipe is a pattern that reads, and does what it says on a
    /// name that needs it; the first item shows what it makes of it.
    #[test]
    fn recipes_do_what_they_say() {
        let group = recipes(None);
        let made = |says: &str, name: &str| {
            let item = group
                .items
                .iter()
                .find(|item| item.says.starts_with(says))
                .unwrap_or_else(|| panic!("no recipe {says}"));
            let spec = RenameSpec {
                regex: item.fills.get("regex").cloned(),
                replacement: item.fills.get("replacement").cloned(),
                ..Default::default()
            };
            names(&[self::item(name)], &spec)[0]
                .to_string_lossy()
                .into_owned()
        };
        assert_eq!(made("Spaces to underscores", "a  b c.txt"), "a_b_c.txt");
        assert_eq!(made("Underscores to spaces", "a_b.txt"), "a b.txt");
        assert_eq!(made("Several spaces to one", "a   b c.txt"), "a b c.txt");
        assert_eq!(made("Take away a start (IMG_)", "IMG_0042.jpg"), "0042.jpg");
        assert_eq!(
            made("Take away what is in brackets", "Song (live) [2019].mp3"),
            "Song.mp3"
        );
        assert_eq!(
            made("Take away the numbers at the start", "03 - Song.mp3"),
            "Song.mp3"
        );
        assert_eq!(
            made("Swap two parts around “ - ”", "Artist - Title.mp3"),
            "Title - Artist.mp3"
        );
        assert_eq!(
            made("Swap two parts around “ - ”", "Artist - Title"),
            "Title - Artist"
        );
        assert_eq!(made("One extension for JPEG pictures", "a.JPEG"), "a.jpg");
        for item in &group.items {
            assert!(
                item.inserts.is_none() && item.fills.len() == 2,
                "{}",
                item.says
            );
        }
        let first = item("IMG_0042.jpg");
        let group = recipes(Some(&first));
        let gives = |says: &str| {
            group
                .items
                .iter()
                .find(|item| item.says.starts_with(says))
                .and_then(|item| item.gives.clone())
        };
        assert_eq!(
            gives("Take away a start (IMG_)").as_deref(),
            Some("0042.jpg")
        );
        assert_eq!(
            gives("Spaces to underscores"),
            None,
            "unchanged: nothing said"
        );
    }

    #[test]
    fn regex_groups_then_case() {
        let items = [item("IMG_1234.jpg"), item("img_77.PNG")];
        let spec = RenameSpec {
            regex: Some(r"(?i)img_(\d+)".into()),
            replacement: Some("shot-$1".into()),
            case: Some(Case::Upper),
            ..Default::default()
        };
        assert_eq!(names(&items, &spec), ["SHOT-1234.jpg", "SHOT-77.PNG"]);
        let spec = RenameSpec {
            case: Some(Case::Title),
            ..Default::default()
        };
        assert_eq!(
            names(&[item("my_holiday photos.txt")], &spec),
            ["My_Holiday Photos.txt"]
        );
        let spec = RenameSpec {
            case: Some(Case::Sentence),
            ..Default::default()
        };
        assert_eq!(names(&[item("HELLO wORLD.md")], &spec), ["Hello world.md"]);
        assert!(matches!(
            new_names(
                &items,
                &RenameSpec {
                    regex: Some("(".into()),
                    ..Default::default()
                }
            ),
            Err(BulkRenameError::Regex(_))
        ));
    }

    #[test]
    fn date_uses_mtime() {
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let items = [(
            EntryRef::new(FileId::new(1, 1), "/d/a.txt"),
            "a.txt".into(),
            Some(t),
        )];
        let spec = RenameSpec {
            template: Some("{date:%Y}-{name}.{ext}".into()),
            ..Default::default()
        };
        assert_eq!(names(&items, &spec), ["2023-a.txt"]);
    }

    #[test]
    fn validation_and_conflicts() {
        let items = [item("a"), item("b")];
        let dup = plan_bulk_rename(
            &items,
            &RenameSpec {
                template: Some("same".into()),
                ..Default::default()
            },
        );
        assert!(matches!(dup, Err(BulkRenameError::Duplicate { .. })));
        let bad = plan_bulk_rename(
            &items,
            &RenameSpec {
                template: Some("x/{name}".into()),
                ..Default::default()
            },
        );
        assert!(matches!(bad, Err(BulkRenameError::InvalidName { .. })));
        let conflict = plan_bulk_rename_in(
            &items,
            |p| p == Path::new("/d/a-1"),
            &RenameSpec {
                template: Some("{name}-{n}".into()),
                ..Default::default()
            },
        );
        assert!(matches!(conflict, Err(BulkRenameError::Conflict { .. })));
        let unchanged = plan_bulk_rename(&items, &RenameSpec::default()).unwrap();
        assert!(unchanged.plan.is_empty() && unchanged.preview.is_empty());
    }

    #[test]
    fn large_counters_are_refused_not_wrapped() {
        let items = [item("a"), item("b")];
        let spec = RenameSpec {
            template: Some("{n}".into()),
            start: u64::MAX,
            ..Default::default()
        };
        assert!(matches!(
            new_names(&items, &spec),
            Err(BulkRenameError::Template(_))
        ));
        let spec = RenameSpec {
            template: Some("{date:x%}".into()),
            ..Default::default()
        };
        let dated = [(
            EntryRef::new(FileId::new(1, 1), "/d/a"),
            "a".into(),
            Some(SystemTime::UNIX_EPOCH),
        )];
        assert!(matches!(
            new_names(&dated, &spec),
            Err(BulkRenameError::Template(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn names_that_are_not_text_are_never_mangled() {
        use std::os::unix::ffi::OsStringExt;
        let raw = OsString::from_vec(b"caf\xe9".to_vec());
        let items = [(
            EntryRef::new(FileId::new(1, 1), Path::new("/d").join(&raw)),
            raw.clone(),
            None,
        )];
        assert_eq!(names(&items, &RenameSpec::default()), [raw]);
        assert!(matches!(
            new_names(
                &items,
                &RenameSpec {
                    case: Some(Case::Upper),
                    ..Default::default()
                }
            ),
            Err(BulkRenameError::NotText { .. })
        ));
    }

    fn setup(names: &[&str]) -> (tempfile::TempDir, Vec<Item>, Arc<App>) {
        let tmp = tempfile::tempdir().unwrap();
        let mut items = Vec::new();
        for n in names {
            let p = tmp.path().join(n);
            std::fs::write(&p, n.as_bytes()).unwrap();
            items.push((EntryRef::from_path(&p).unwrap(), (*n).into(), None));
        }
        let app = App::new(skwir_core::app::AppConfig::ephemeral(
            tmp.path().join("app"),
        ))
        .unwrap();
        (tmp, items, app)
    }

    fn contents(dir: &Path) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter(|e| e.path().is_file())
            .map(|e| {
                (
                    e.file_name().to_string_lossy().into_owned(),
                    std::fs::read_to_string(e.path()).unwrap(),
                )
            })
            .collect();
        v.sort();
        v
    }

    fn apply(app: &App, plan: &Plan) {
        let r = app.executor.execute_blocking(plan, app.lifetime());
        assert!(r.error.is_none(), "{:?}", r.error);
    }

    #[test]
    fn swaps_and_shifts_apply_for_real_and_undo() {
        // Listed as 2, 1: numbering them 1, 2 swaps the two names.
        let (tmp, items, app) = setup(&["2", "1"]);
        let spec = RenameSpec {
            template: Some("{n}".into()),
            ..Default::default()
        };
        let r = plan_bulk_rename(&items, &spec).unwrap();
        assert_eq!(r.plan.len(), 3, "one parked under a free name");
        apply(&app, &r.plan);
        assert_eq!(
            contents(tmp.path()),
            [("1".into(), "2".into()), ("2".into(), "1".into())]
        );
        let u = app.executor.undo_blocking(app.lifetime()).unwrap();
        assert!(u.error.is_none(), "{:?}", u.error);
        assert_eq!(
            contents(tmp.path()),
            [("1".into(), "1".into()), ("2".into(), "2".into())]
        );

        // 1, 2, 3 -> 2, 3, 4: each waits for the next to move.
        let (tmp, items, app) = setup(&["1", "2", "3"]);
        let spec = RenameSpec {
            template: Some("{n}".into()),
            start: 2,
            ..Default::default()
        };
        let r = plan_bulk_rename(&items, &spec).unwrap();
        assert_eq!(r.plan.len(), 3, "a chain needs no parking");
        apply(&app, &r.plan);
        assert_eq!(
            contents(tmp.path()),
            [
                ("2".into(), "1".into()),
                ("3".into(), "2".into()),
                ("4".into(), "3".into())
            ]
        );
    }

    #[test]
    fn plugin_command_end_to_end() {
        let (tmp, _items, app) = setup(&["x1.txt", "x2.txt"]);
        app.plugins.add(Arc::new(RenameBulkPlugin), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &skwir_core::Location::from_path(tmp.path()))
            .unwrap();
        assert!(app.pump_until_complete(pane, std::time::Duration::from_secs(5)));
        app.dispatch(&Invocation::new("select_all")).unwrap();
        let preview = app
            .dispatch(&Invocation::with_args(
                "rename_bulk.preview",
                &serde_json::json!({ "template": "file-{n:02}.{ext}" }),
            ))
            .unwrap();
        let Outcome::Value(v) = preview else { panic!() };
        assert_eq!(v["renames"][0]["to"], "file-01.txt");
        assert_eq!(v["count"], 2);
        // At most `limit` said, all counted.
        let Outcome::Value(v) = app
            .dispatch(&Invocation::with_args(
                "rename_bulk.preview",
                &serde_json::json!({ "template": "file-{n:02}.{ext}", "limit": 1 }),
            ))
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(v["renames"].as_array().map(Vec::len), Some(1));
        assert_eq!(v["count"], 2);
        // Where the regex matched, and what it put.
        let Outcome::Value(v) = app
            .dispatch(&Invocation::with_args(
                "rename_bulk.preview",
                &serde_json::json!({ "regex": "^x", "replacement": "file-" }),
            ))
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(v["renames"][0]["matched"], serde_json::json!([[0, 1]]));
        assert_eq!(v["renames"][0]["put"], serde_json::json!([[0, 5]]));
        // What the template takes, with what it gives the first selected.
        let Outcome::Value(v) = app
            .dispatch(&Invocation::with_args(
                "rename_bulk.reference",
                &serde_json::json!({ "field": "template", "start": 3 }),
            ))
            .unwrap()
        else {
            panic!()
        };
        let said: Reference = serde_json::from_value(v).unwrap();
        let given: Vec<(&str, Option<&str>)> = said
            .items()
            .map(|(_, _, item)| (item.written.as_str(), item.gives.as_deref()))
            .collect();
        assert!(given.contains(&("{name}", Some("x1"))), "{given:?}");
        assert!(given.contains(&("{n:02}", Some("03"))), "{given:?}");
        let Outcome::Plan(plan) = app
            .dispatch(&Invocation::with_args(
                "rename_bulk",
                &serde_json::json!({ "template": "file-{n:02}.{ext}" }),
            ))
            .unwrap()
        else {
            panic!()
        };
        let r = app.executor.execute_blocking(&plan, app.lifetime());
        assert!(r.error.is_none());
        assert!(tmp.path().join("file-01.txt").exists() && tmp.path().join("file-02.txt").exists());
        app.plugins.set_enabled("rename-bulk", false, &app);
        assert!(app.commands.get("rename_bulk").is_none());
    }
}
