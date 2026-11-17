//! skwir plugin: rename-bulk. Turn a selection plus a rename spec into a
//! plan of renames that is safe to apply in order, even when new names
//! collide with old ones (swaps and chains).
//!
//! Per item, in this order: the regex replacement on the full name, then
//! the template (its placeholders: [`PLACEHOLDERS`], said in
//! [`RenameSpec::template`] and the reference), then the case rule
//! (`lower`, `upper`, `title`, `sentence`), which applies to the stem and
//! leaves the extension alone. A name's extension is the one a sort by
//! extension reads (`location::extension_start`): a folder has none.
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
use skwir_core::words;
use skwir_core::words::{Name, Words};
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

/// The keys the template reads (`{name}`, `{n:03}`: the key before any
/// `:`); `render` refuses any other. What `rename_bulk.reference` offers
/// and what the schema says of the template are held to this list by a
/// test, so the three never drift apart.
pub const PLACEHOLDERS: [&str; 7] = ["name", "ext", ".ext", "full", "parent", "n", "date"];

/// What to do to each name: the regex first, then the template, then the
/// case. `rename_bulk.reference` says what each field takes, with what it
/// gives for the first item.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct RenameSpec {
    /// The new name, text and placeholders: `{name}` (without its
    /// extension; a folder has none), `{ext}`, `{.ext}` (with its dot,
    /// nothing without one), `{full}`, `{parent}`, `{n}` and `{n:03}` (the
    /// counter, 255 digits at most), `{date}` and `{date:%Y%m%d}` (when it
    /// was modified, any strftime format); `{{` and `}}` for braces. The
    /// name the regex left when there is one.
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
    /// The first item's name, renamed or not: what the recipes of the
    /// reference work on (`recipes`), the same at every key while it is.
    pub first: Option<OsString>,
}

/// Where the regex matched in an old name, and what it put in the new
/// one: byte ranges into each, as a preview marks them. `put` only while
/// the template and the case left the name as the regex made it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Marks {
    pub matched: Vec<Range<usize>>,
    pub put: Vec<Range<usize>>,
}

/// Why a bulk rename cannot be planned: data, said in the reader's
/// language as its words (`BulkRenameError::words`), English for `Display`
/// outside a chain said.
#[derive(Debug, PartialEq, Eq)]
pub enum BulkRenameError {
    /// What the `regex` crate said of the pattern.
    Regex(String),
    /// What is wrong with the template.
    Template(Words),
    /// `reason` is what `order_renames` said.
    InvalidName {
        item: String,
        name: String,
        reason: Words,
    },
    NotText {
        item: String,
    },
    Duplicate {
        name: String,
        items: Vec<String>,
    },
    Conflict {
        item: String,
        name: String,
    },
    MissingDate {
        item: String,
    },
    Empty,
    /// The job working it out was cancelled (`scheduler::job_cancelled`):
    /// a preview the user typed past.
    Cancelled,
}

impl BulkRenameError {
    /// What it says: under a field of the dialog, or in a toast. A clash
    /// of many names says a few and how many more, never all of a large
    /// selection.
    pub fn words(&self) -> Words {
        match self {
            BulkRenameError::Regex(why) => words!(
                rename_bulk_invalid_regex = "invalid regex: { $why }",
                why = why.clone(),
                note = "Said under the regex's field; what follows is the regex library's own explanation, as written."
            ),
            BulkRenameError::Template(why) => words!(
                rename_bulk_invalid_template = "invalid template: { $why }",
                why = why.clone(),
                note = "Said under the template's field; what follows says what is wrong."
            ),
            BulkRenameError::InvalidName { item, name, reason } => words!(
                rename_bulk_invalid_name =
                    "{ $item } would become an invalid name { $name }: { $reason }",
                item = Name(item.clone()),
                name = Name(name.clone()),
                reason = reason.clone(),
                note = "A file's name, the name it would get, and why that name is not allowed."
            ),
            BulkRenameError::NotText { item } => words!(
                rename_bulk_not_text = "{ $item } is not a name made of text; rename it on its own",
                item = Name(item.clone()),
                note = "A name whose bytes are not text: the rules work on text, so it can only be renamed alone."
            ),
            BulkRenameError::Duplicate { name, items } => {
                let named = |at: usize| Name(items.get(at).cloned().unwrap_or_default());
                words!(
                    rename_bulk_duplicate = "{ $count ->
                        [2] several items would become { $name }: { $first }, { $second }
                        [3] several items would become { $name }: { $first }, { $second }, { $third }
                       *[other] several items would become { $name }: { $first }, { $second }, { $third } and { $more } more
                    }",
                    count = items.len(),
                    name = Name(name.clone()),
                    first = named(0),
                    second = named(1),
                    third = named(2),
                    more = items.len().saturating_sub(3),
                    note = "The new name several files would take, then the first three of them, and how many more."
                )
            }
            BulkRenameError::Conflict { item, name } => words!(
                rename_bulk_conflict = "{ $item } would become { $name }, which already exists",
                item = Name(item.clone()),
                name = Name(name.clone())
            ),
            BulkRenameError::MissingDate { item } => words!(
                rename_bulk_missing_date =
                    "{ $item } has no modification time yet (needed for { $placeholder })",
                item = Name(item.clone()),
                placeholder = "{date}",
                note = "The placeholder is the template's, as written: its modification date."
            ),
            BulkRenameError::Empty => words!(rename_bulk_nothing = "nothing to rename"),
            BulkRenameError::Cancelled => words!(
                rename_bulk_cancelled = "cancelled",
                note = "Working out a bulk rename's names was stopped: what was typed since asks again."
            ),
        }
    }
}

impl std::fmt::Display for BulkRenameError {
    /// Its words: English, but where a chain of errors is said, in the
    /// language it is said in.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.words(), f)
    }
}

impl std::error::Error for BulkRenameError {}

/// How many items are worked out between two looks at whether the job
/// was cancelled.
const LOOK_EVERY: usize = 256;

/// The widest a counter is written (`{n:03}`): a name holds 255 bytes at
/// most on the file systems of every system, so a wider one names
/// nothing, and written for each of a selection it would hold that much
/// for every item before the names were refused.
const WIDEST_COUNTER: usize = 255;

/// One input item: where it is, what it is called, when it changed, and
/// whether it is a folder (whose name has no extension).
#[derive(Clone, Debug)]
pub struct Item {
    pub entry: EntryRef,
    pub name: OsString,
    pub modified: Option<SystemTime>,
    pub is_dir: bool,
}

impl Item {
    /// A file at `entry`, called `name`, changed at `modified`.
    pub fn new(entry: EntryRef, name: impl Into<OsString>, modified: Option<SystemTime>) -> Self {
        Item {
            entry,
            name: name.into(),
            modified,
            is_dir: false,
        }
    }

    /// The same, a folder when `is_dir`.
    #[must_use]
    pub fn folder(mut self, is_dir: bool) -> Self {
        self.is_dir = is_dir;
        self
    }
}

/// A name's stem and its extension without the dot, as a sort and a group
/// by extension read it (`location::extension_start`): a folder has none,
/// nor `.bashrc`, and `notes.` keeps its dot in its stem.
fn split_ext(name: &str, is_dir: bool) -> (&str, &str) {
    match skwir_core::location::extension_start(name.as_bytes(), is_dir) {
        Some(at) => (&name[..at - 1], &name[at..]),
        None => (name, ""),
    }
}

fn apply_case(name: &str, case: Case, is_dir: bool) -> String {
    let (stem, ext) = split_ext(name, is_dir);
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

/// Expand the template for one item. `name` is the (regex-transformed)
/// name of `item`, a folder when `is_dir`.
fn render(
    template: &str,
    name: &str,
    is_dir: bool,
    parent: &str,
    n: u64,
    mtime: Option<SystemTime>,
    item: &str,
) -> Result<String, BulkRenameError> {
    let (stem, ext) = split_ext(name, is_dir);
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
                            return Err(BulkRenameError::Template(words!(
                                rename_bulk_unclosed = "unclosed `{ $brace }` in `{ $template }`",
                                brace = "{",
                                template = template.to_owned(),
                                note = "The template as it was typed: a brace opens a placeholder that never closes."
                            )));
                        }
                    }
                }
                let (key, arg) = token
                    .split_once(':')
                    .map_or((token.as_str(), None), |(k, a)| (k, Some(a)));
                if !PLACEHOLDERS.contains(&key) {
                    return Err(BulkRenameError::Template(words!(
                        rename_bulk_unknown_placeholder = "unknown placeholder `{ $placeholder }`",
                        placeholder = format!("{{{key}}}"),
                        note = "The placeholder as it was typed in the template."
                    )));
                }
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
                                    BulkRenameError::Template(words!(
                                        rename_bulk_bad_width = "bad counter width `{ $width }`",
                                        width = a.to_owned(),
                                        note = "What was typed after {n: in the template, which is no number of digits."
                                    ))
                                })?,
                        };
                        if width > WIDEST_COUNTER {
                            return Err(BulkRenameError::Template(words!(
                                rename_bulk_counter_too_wide = "{ $width ->
                                    [one] a counter { $width } digit wide is wider than a name can be ({ $most } at most)
                                   *[other] a counter { $width } digits wide is wider than a name can be ({ $most } at most)
                                }",
                                width = width,
                                most = WIDEST_COUNTER,
                                note = "The counter's width typed in the template, and the most a name holds, in bytes."
                            )));
                        }
                        let _ = write!(out, "{n:0width$}");
                    }
                    "date" => {
                        let t = mtime.ok_or_else(|| BulkRenameError::MissingDate {
                            item: item.to_owned(),
                        })?;
                        let fmt = arg.unwrap_or("%Y-%m-%d");
                        let ts = jiff::Timestamp::try_from(t).map_err(|e| {
                            BulkRenameError::Template(Words::as_written(e.to_string()))
                        })?;
                        let zoned = ts.to_zoned(jiff::tz::TimeZone::system());
                        // `strftime(..).to_string()` would panic on a
                        // format it cannot write; this says why instead.
                        let s = jiff::fmt::strtime::format(fmt, &zoned).map_err(|e| {
                            BulkRenameError::Template(words!(
                                rename_bulk_bad_date_format = "bad date format `{ $format }`: { $why }",
                                format = fmt.to_owned(),
                                why = e.to_string(),
                                note = "The date's format as typed after {date: in the template, then the date library's explanation, as written."
                            ))
                        })?;
                        out.push_str(&s);
                    }
                    // Listed, never read: the test of `PLACEHOLDERS` fails.
                    other => {
                        return Err(BulkRenameError::Template(words!(
                            rename_bulk_not_read_yet =
                                "placeholder `{ $placeholder }` not read yet",
                            placeholder = format!("{{{other}}}")
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
        .ok_or_else(|| {
            BulkRenameError::Template(words!(
                rename_bulk_counter_overflows = "the counter overflows",
                note = "The counter's start and step make a number too large for the items."
            ))
        })
}

/// Every item's new name and its marks.
fn worked_out(
    items: &[Item],
    spec: &RenameSpec,
) -> Result<Vec<(OsString, Marks)>, BulkRenameError> {
    let re = pattern(spec)?;
    let replacement = spec.replacement.clone().unwrap_or_default();
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let Item {
            entry,
            name: original,
            modified: mtime,
            is_dir,
        } = item;
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
            n = render(
                t,
                &n,
                *is_dir,
                &parent_of(entry),
                counter(i, spec)?,
                *mtime,
                &name,
            )?;
        }
        if let Some(case) = spec.case {
            n = apply_case(&n, case, *is_dir);
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
        .map(|(item, new_name)| RenameRequest {
            entry: item.entry.clone(),
            new_name,
        })
        .collect();
    let old = |i: usize| items[i].name.to_string_lossy().into_owned();
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
        first: items.first().map(|item| item.name.clone()),
        plan: Plan::with_steps(
            words!(
                rename_bulk_plan = "{ $count ->
                    [one] rename { $count } item
                   *[other] rename { $count } items
                }",
                count = preview.len(),
                note = "A bulk rename's title, in the review, Activity and the History. Begins in lower case: said inside other words too (undo rename 3 items)."
            ),
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
    /// The pane whose selection is renamed when no entries are given; the
    /// active pane when omitted.
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
    /// How many of the renames to say at most, the first; all when
    /// omitted. `count` says how many there are in all.
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
        items.iter().map(|item| item.entry.path.as_path()).collect();
    let exists = |path: &Path| !renamed.contains(path) && std::fs::symlink_metadata(path).is_ok();
    Ok(plan_bulk_rename_in(&items, exists, spec)?)
}

/// The first `at_most` of the files named by path, read on the thread of
/// whoever named them, those alone: their times as a listing shows them
/// (a link's, what it leads to), and whether each is a folder (a link to
/// one too).
fn named_items(
    entries: Vec<skwir_core::op::EntryArg>,
    at_most: usize,
) -> anyhow::Result<Vec<Item>> {
    entries
        .into_iter()
        .take(at_most)
        .map(|entry| {
            let r = entry.into_ref()?;
            let name = r.path.file_name().unwrap_or_default().to_owned();
            let read = std::fs::metadata(&r.path).or_else(|_| std::fs::symlink_metadata(&r.path));
            let modified = read.as_ref().ok().and_then(|m| m.modified().ok());
            let is_dir = read.is_ok_and(|m| m.is_dir());
            Ok(Item::new(r, name, modified).folder(is_dir))
        })
        .collect()
}

/// The first `at_most` of what `p` selects, in the order shown (folders
/// opened in place included), but what an agent's call leaves out.
fn selected_items(
    p: &Pane,
    withheld: &std::collections::HashSet<skwir_core::EntryId>,
    at_most: usize,
) -> Vec<Item> {
    let mut items = Vec::new();
    // As it stood when asked, where the window made it a plan on a worker.
    let chosen = skwir_core::app::chosen(p);
    for id in p.order().iter().copied() {
        if items.len() == at_most {
            break;
        }
        if !chosen.contains(id) || withheld.contains(&id) {
            continue;
        }
        let Some(e) = p.entry(id) else { continue };
        let Some(path) = p.path_of(id) else {
            continue;
        };
        let mtime = e.metadata.value(keys::MODIFIED).and_then(Value::as_time);
        items.push(
            Item::new(
                EntryRef::new(id.file, path).with_only_name(e.is_only_name()),
                e.name.clone(),
                mtime,
            )
            .folder(e.is_dir()),
        );
    }
    items
}

/// Plan a bulk rename of the pane's selection, in its sorted order. What
/// else is taken is read from the listing, never from the disk: the
/// preview runs on every keystroke. In a folder a target is taken when an
/// entry not selected has that name; elsewhere (search results, the
/// shelf) the listing cannot say, and applying finds out. The workspace
/// is held only to take the items and to look up each target: working
/// out 100,000 names never keeps the window's thread waiting on it.
fn plan_selection(
    app: &App,
    pane: Option<PaneId>,
    spec: &RenameSpec,
) -> anyhow::Result<BulkRename> {
    let pane = app.pane_or_active(pane);
    // An agent's call leaves out the private files the user selected.
    let withheld = app.withheld(pane);
    let (dir, items) = {
        let ws = app.workspace.read();
        let p = ws.pane(pane).ok_or_else(|| {
            anyhow::Error::from(words!(
                rename_bulk_no_such_pane = "no such pane",
                note = "An error an agent or a script reads: the pane it named is not there."
            ))
        })?;
        anyhow::ensure!(
            p.open.is_some(),
            words!(
                rename_bulk_no_location = "pane has no open location",
                note = "An error an agent or a script reads: the pane shows no folder yet."
            )
        );
        let dir = p.location().and_then(skwir_core::Location::path);
        (dir, selected_items(p, &withheld, usize::MAX))
    };
    if items.is_empty() {
        return Err(skwir_core::command::NothingSelected.into());
    }
    let exists = |path: &Path| {
        dir.as_deref().is_some_and(|dir| path.parent() == Some(dir))
            && path.file_name().is_some_and(|name| {
                let ws = app.workspace.read();
                ws.pane(pane).is_some_and(|p| {
                    p.listing
                        .find_by_name(name)
                        .is_some_and(|e| !skwir_core::app::chosen(p).contains(e.id))
                })
            })
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
    /// The field whose reference is asked.
    field: Field,
    #[serde(flatten)]
    args: Args,
}

/// What `field` takes (ADR-137): its placeholders or signs, each with
/// what it gives `first` (the first item renamed) under `spec` as it
/// stands, and the recipes that fill the regex and its replacement.
/// Each field offers, as it is typed, what goes on with a part begun by
/// these: a placeholder, a sign, a group's. The template's reads the
/// rename as it stands; the regex's and the replacement's, `first`'s name
/// alone, so the dialog asks them again only when it changes (the
/// preview's `first`).
pub fn reference(field: Field, first: Option<&Item>, spec: &RenameSpec) -> Reference {
    match field {
        Field::Template => template_reference(first, spec)
            .starts(&["{"])
            .escapes(&["{{", "}}"]),
        Field::Regex => Reference::new(
            words!(
                rename_bulk_regex = "Regex",
                note = "The title of what the regex field takes: a regular expression."
            ),
        )
        .group(recipes(first))
        .group(
            skwir_core::reference::regex_pattern()
                .note(
                    words!(
                        rename_bulk_regex_note = "^ and $ are the start and the end of the name. It runs on the whole name, extension included, every match replaced; the template comes after it.",
                        note = "^ and $ are signs of the pattern, as written."
                    ),
                )
                .note(
                    words!(
                        rename_bulk_regex_groups_note = "What a group held is $1, $2… in the replacement; what a group named year held, { $named }.",
                        named = "${year}",
                        note = "$1, $2 and the argument are signs of the replacement, as written; year is the group's name in it."
                    ),
                ),
        )
            .starts(&["\\", "(?"])
            .escapes(&["\\\\"]),
        Field::Replacement => Reference::new(
            words!(
                rename_bulk_replacement = "Replacement",
                note = "The title of what the replacement field takes: what each match of the regex becomes."
            ),
        )
        .group(
            Group::new(
                words!(
                    rename_bulk_what_a_match_becomes = "What a match becomes",
                    note = "A heading of the replacement's reference."
                ),
            )
            .item(Said::new(
                "$1",
                words!(
                    rename_bulk_first_group = "What the first group held ($2, $3…)",
                    note = "Said beside $1 in the replacement's reference; $2 and $3 are signs, as written."
                ),
            ))
            .item(Said::new(
                "${1}",
                words!(
                    rename_bulk_group_before_letter = "The same, before a letter, a digit or _",
                    note = "Said beside ${1}: the first group's, written so a letter, a digit or an underscore may follow."
                ),
            ))
            .item(Said::new(
                "${year}",
                words!(
                    rename_bulk_named_group = "What the group named year held",
                    note = "Said beside ${year}: year is the group's name, as written in the pattern."
                ),
            ))
            .item(Said::new(
                "$0",
                words!(rename_bulk_whole_match = "The whole match"),
            ))
            .item(Said::new(
                "$$",
                words!(rename_bulk_dollar = "A dollar sign"),
            ))
            .note(
                words!(
                    rename_bulk_group_name_note = "{ $written } reads as a group named { $read }, which holds nothing: write { $meant }.",
                    written = "$1_x",
                    read = "1_x",
                    meant = "${1}_x",
                    note = "The arguments are what is written in the replacement, as written."
                ),
            )
            .note(
                words!(
                    rename_bulk_empty_replacement_note = "Left empty, what the pattern matched is taken away.",
                    note = "Of the replacement field."
                ),
            ),
        )
            .group(recipes(first))
            .starts(&["$"])
            .escapes(&["$$"]),
    }
}

/// The template's placeholders, each giving what it does for `first`.
fn template_reference(first: Option<&Item>, spec: &RenameSpec) -> Reference {
    // What the regex leaves of the first item's name: what the template
    // reads.
    let read = first.and_then(|first| {
        let name = first.name.to_str()?;
        let name = match pattern(spec).ok()? {
            Some(re) => replaced(&re, name, spec.replacement.as_deref().unwrap_or("")).0,
            None => name.to_owned(),
        };
        Some((
            name,
            parent_of(&first.entry),
            first.modified,
            first.name.to_string_lossy().into_owned(),
            first.is_dir,
        ))
    });
    let start = spec.start;
    let item = |written: &str, says: Words| {
        let said = Said::new(written, says);
        match read
            .as_ref()
            .and_then(|(name, parent, mtime, item, is_dir)| {
                render(written, name, *is_dir, parent, start, *mtime, item).ok()
            }) {
            Some(gives) => said.gives(gives),
            None => said,
        }
    };
    // The placeholders are the template's, as written.
    Reference::new(
        words!(
            rename_bulk_template = "Template",
            note = "The title of what the template field takes: the new name, with placeholders."
        ),
    )
    .group(
        Group::new(
            words!(
                rename_bulk_parts_of_the_name = "Parts of the name",
                note = "A heading of the template's reference."
            ),
        )
        .item(item(
            "{name}",
            words!(rename_bulk_name_without_extension = "The name without its extension"),
        ))
        .item(item(
            "{ext}",
            words!(rename_bulk_extension = "The extension, without its dot"),
        ))
        .item(item(
            "{.ext}",
            words!(
                rename_bulk_extension_with_dot = "The extension with its dot; nothing when there is none"
            ),
        ))
        .item(item(
            "{full}",
            words!(rename_bulk_whole_name = "The whole name"),
        ))
        .item(item(
            "{parent}",
            words!(rename_bulk_parent = "The folder it is in"),
        ))
        .note(
            words!(
                rename_bulk_parts_note = "The regex runs first: these are the name it leaves. The case comes last, and leaves the extension as it is. A folder has no extension."
            ),
        ),
    )
    .group(
        Group::new(
            words!(
                rename_bulk_counter = "Counter",
                note = "A heading of the template's reference: the number counted for each item."
            ),
        )
        .item(item(
            "{n}",
            words!(
                rename_bulk_counts = "Counts from Start, one more for each, in the order shown",
                note = "Start is the name of the dialog's field the counter starts at."
            ),
        ))
        .item(item(
            "{n:02}",
            words!(rename_bulk_two_digits = "The same, two digits at least"),
        ))
        .item(item(
            "{n:03}",
            words!(rename_bulk_three_digits = "The same, three digits at least"),
        )),
    )
    .group(
        Group::new(
            words!(
                rename_bulk_when_modified = "When it was modified",
                note = "A heading of the template's reference: the placeholders of the item's modification date."
            ),
        )
        .item(item("{date}", words!(rename_bulk_day = "Its day")))
        .item(item(
            "{date:%Y%m%d}",
            words!(rename_bulk_day_without_dashes = "Its day, without dashes"),
        ))
        .item(item(
            "{date:%Y-%m-%d_%H%M}",
            words!(rename_bulk_day_and_time = "Its day and time"),
        ))
        .item(item(
            "{date:%d.%m.%Y}",
            words!(rename_bulk_day_first = "Its day, the day first"),
        ))
        .item(item(
            "{date:%B %Y}",
            words!(rename_bulk_month_and_year = "Its month and year"),
        ))
        .note(
            words!(
                rename_bulk_date_note = "Inside { $placeholder }: %Y the year, %m the month, %d the day, %H the hour, %M the minute, %S the second, %B the month's name (%b short), %A the day's, %j the day of the year. In this system's time zone.",
                placeholder = "{date:…}",
                note = "The placeholder and the signs after % are the template's, as written."
            ),
        ),
    )
    .group(
        Group::new(
            words!(
                rename_bulk_signs = "Signs",
                note = "A heading of the template's reference: what is written for a sign itself."
            ),
        )
        .item(Said::new(
            "{{",
            words!(
                rename_bulk_brace = "A brace itself ({ $open } and { $close })",
                open = "{{",
                close = "}}",
                note = "The arguments are what is written in the template for each brace, as written."
            ),
        )),
    )
}

/// Patterns and their replacements for what names often need, each
/// giving what it makes of `first` when it changes it.
fn recipes(first: Option<&Item>) -> Group {
    let name = first.and_then(|first| first.name.to_str());
    // Written as its pattern; what it is for, and what replaces the
    // pattern where it says, said.
    let recipe = |says: Words, pattern: &str, replacement: &str| {
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
    // What follows an arrow is what replaces the pattern, as written where
    // it is a sign.
    Group::new(
        words!(
            rename_bulk_recipes = "Recipes",
            note = "A heading of the regex's and the replacement's reference: patterns that do what names often need."
        ),
    )
    .item(recipe(
        words!(
            rename_bulk_recipe_spaces = "Spaces to underscores → { $shown }",
            shown = "_"
        ),
        r"\s+",
        "_",
    ))
    .item(recipe(
        words!(rename_bulk_recipe_underscores = "Underscores to spaces → a space"),
        "_",
        " ",
    ))
    .item(recipe(
        words!(rename_bulk_recipe_one_space = "Several spaces to one → a space"),
        r"\s{2,}",
        " ",
    ))
    .item(recipe(
        words!(
            rename_bulk_recipe_start = "Take away a start (IMG_)",
            note = "IMG_ is the start of a name a camera gives, as written."
        ),
        "^IMG_",
        "",
    ))
    // A pair of one kind, holding no other: never a bracket left
    // alone (`Song).mp3`), nor `(a]` taken.
    .item(recipe(
        words!(rename_bulk_recipe_brackets = "Take away what is in brackets"),
        r"\s*(?:\([^()]*\)|\[[^\[\]]*\])",
        "",
    ))
    // A dot only with a separator after it: `001.jpg` keeps its
    // extension.
    .item(recipe(
        words!(rename_bulk_recipe_numbers = "Take away the numbers at the start"),
        r"^\d+(?:[\s_-]+|\.[\s_-]+)",
        "",
    ))
    .item(recipe(
        words!(
            rename_bulk_recipe_swap = "Swap two parts around “ - ” → { $shown }",
            shown = "$2 - $1$3",
            note = "Two parts of a name on either side of a dash between spaces; what follows the arrow is the replacement, as written."
        ),
        r"^(.+) - (.+?)(\.[^.]+)?$",
        "$2 - $1$3",
    ))
    .item(recipe(
        words!(
            rename_bulk_recipe_jpeg = "One extension for JPEG pictures → { $shown }",
            shown = ".jpg"
        ),
        r"(?i)\.jpe?g$",
        ".jpg",
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

    fn title(&self) -> skwir_core::words::Words {
        skwir_core::words!(rename_bulk_plugin_title = "Bulk rename")
    }

    fn description(&self) -> skwir_core::words::Words {
        skwir_core::words!(
            rename_bulk_plugin_description =
                "Rename the selection from a template, a regex, and case rules"
        )
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
            words!(rename_bulk_command = "Rename several from a template"),
            words!(
                rename_bulk_command_description = "Rename the selection, or the entries given, from a template, regex, and case rule (plan)",
                note = "Read by agents and in F1's list of actions."
            ),
            |a, app| Ok(Outcome::Plan(plan_bulk(app, a)?.plan)),
        )
        .plans();
        app.commands.register::<PreviewArgs, _>(ID, Topic::Files, "rename_bulk.preview", words!(rename_bulk_preview = "Preview bulk rename"), words!(rename_bulk_preview_description = "Return the old → new names a bulk rename would produce (`renames`, the first `limit` of them when given; each with where the regex `matched` in the old name and what it `put` in the new one, byte ranges), how many there are (`count`), and the first item's name (`first`), which the regex's and the replacement's references work on alone", note = "Read by agents: the words in backticks are the answer's fields and the command's arguments, as written."), |a, app| {
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
            let first = r.first.as_ref().map(|name| name.to_string_lossy());
            Ok(Outcome::Value(serde_json::json!({ "renames": shown, "count": renames.len(), "first": first })))
        })
        .data();
        app.commands.register::<ReferenceArgs, _>(ID, Topic::Files, "rename_bulk.reference", words!(rename_bulk_reference = "Say what a bulk rename's field takes"), words!(rename_bulk_reference_description = "Return what `field` (template, regex, replacement) takes: its placeholders or signs, each with what it gives for the first of the entries given or selected, the rename's other arguments being as given, and recipes filling the regex and its replacement", note = "Read by agents: `field` is the argument, and template, regex and replacement its values, as written."), |a, app| {
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
                words!(
                    rename_bulk_menu = "Rename Several…",
                    note = "A context menu item: renames the files selected together, from a template."
                ),
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
        Item::new(
            EntryRef::new(
                FileId::new(
                    1,
                    name.len() as u64 * 7919 + name.bytes().map(u64::from).sum::<u64>(),
                ),
                format!("/d/{name}"),
            ),
            name,
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

    /// A name's extension is the one a sort and a group by extension read
    /// (`location::extension_start`): a folder has none, `notes.` keeps its
    /// dot, `archive.tar.gz` has `gz`; the case leaves it as it is.
    #[test]
    fn an_extension_is_read_as_the_views_read_it() {
        let items = [
            item("v1.2").folder(true),
            item("Notes."),
            item("archive.tar.gz"),
            item(".bashrc"),
        ];
        let spec = RenameSpec {
            template: Some("{name}-{n}{.ext}".into()),
            ..Default::default()
        };
        assert_eq!(
            names(&items, &spec),
            ["v1.2-1", "Notes.-2", "archive.tar-3.gz", ".bashrc-4"]
        );
        let spec = RenameSpec {
            case: Some(Case::Upper),
            ..Default::default()
        };
        assert_eq!(
            names(&items, &spec),
            ["V1.2", "NOTES.", "ARCHIVE.TAR.gz", ".BASHRC"]
        );
        let spec = RenameSpec {
            case: Some(Case::Lower),
            ..Default::default()
        };
        assert_eq!(names(&[item("Notes.")], &spec), ["notes."], "the dot stays");
        // What the reference gives a folder first is what renaming it does.
        let said = reference(Field::Template, Some(&items[0]), &RenameSpec::default());
        let gives = |written: &str| {
            said.items()
                .find(|(_, _, item)| item.written == written)
                .and_then(|(_, _, item)| item.gives.clone())
        };
        assert_eq!(gives("{name}").as_deref(), Some("v1.2"));
        assert_eq!(gives("{.ext}").as_deref(), Some(""));
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
        let first = Item::new(
            EntryRef::new(FileId::new(1, 1), "/Holidays/IMG_0042.jpg"),
            "IMG_0042.jpg",
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
                false,
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
        // Typed on, a placeholder begun is offered what goes on with it.
        let offered = said.offered("{name}-{n", 9).unwrap();
        assert_eq!(offered.range, 7..9);
        let offered: Vec<&str> = offered
            .items
            .iter()
            .filter_map(|&(g, i)| said.item(g, i)?.inserts.as_deref())
            .collect();
        assert_eq!(offered, ["{name}", "{n}", "{n:02}", "{n:03}"]);
        let regex = reference(Field::Regex, None, &RenameSpec::default());
        assert!(regex.offered(r"\d+\", 4).is_some_and(|o| o.range == (3..4)));
        assert!(regex.offered("(?", 2).is_some());
        let replacement = reference(Field::Replacement, None, &RenameSpec::default());
        assert_eq!(replacement.offered("$", 1).map(|o| o.items.len()), Some(5));
        // A sign written twice is the sign itself, and begins nothing.
        assert_eq!(said.offered("a{{", 3), None);
        assert_eq!(regex.offered(r"\\", 2), None);
        assert_eq!(replacement.offered("$$", 2), None);
        // Without an item, nothing is given, and all is still said.
        let said = reference(Field::Template, None, &RenameSpec::default());
        assert!(said.items().all(|(_, _, item)| item.gives.is_none()));
        assert!(said.items().count() > 10);
    }

    /// One list says what the template reads: `render` reads each of
    /// `PLACEHOLDERS` and refuses any other, the reference offers each and
    /// no other, and the schema says each.
    #[test]
    fn one_list_of_placeholders() {
        let t = SystemTime::UNIX_EPOCH;
        for key in PLACEHOLDERS {
            let rendered = render(
                &format!("{{{key}}}"),
                "a.txt",
                false,
                "d",
                1,
                Some(t),
                "a.txt",
            );
            assert!(rendered.is_ok(), "{key}: {rendered:?}");
        }
        assert!(matches!(
            render("{size}", "a.txt", false, "d", 1, Some(t), "a.txt"),
            Err(BulkRenameError::Template(_))
        ));
        let said = reference(Field::Template, None, &RenameSpec::default());
        let offered: std::collections::BTreeSet<&str> = said
            .items()
            .filter_map(|(_, _, item)| item.inserts.as_deref()?.strip_prefix('{'))
            .filter(|rest| !rest.starts_with('{'))
            .filter_map(|rest| rest.split([':', '}']).next())
            .collect();
        assert_eq!(offered, PLACEHOLDERS.into_iter().collect());
        let schema = serde_json::to_value(schemars::schema_for!(RenameSpec)).unwrap();
        let described = schema["properties"]["template"]["description"]
            .as_str()
            .unwrap();
        for key in PLACEHOLDERS {
            assert!(
                described.contains(&format!("{{{key}")),
                "{key}: {described}"
            );
        }
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
                .find(|item| item.says.english().starts_with(says))
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
        // Nested, the innermost pair goes, and no bracket is left alone.
        assert_eq!(
            made("Take away what is in brackets", "Song (live (2019)).mp3"),
            "Song (live).mp3"
        );
        assert_eq!(
            made("Take away what is in brackets", "Song (a].mp3"),
            "Song (a].mp3"
        );
        assert_eq!(
            made("Take away the numbers at the start", "03 - Song.mp3"),
            "Song.mp3"
        );
        for (name, made_of) in [
            ("01. Song.mp3", "Song.mp3"),
            ("01_Song.mp3", "Song.mp3"),
            ("2019 report.pdf", "report.pdf"),
            // Numbers alone keep their extension.
            ("001.jpg", "001.jpg"),
        ] {
            assert_eq!(
                made("Take away the numbers at the start", name),
                made_of,
                "{name}"
            );
        }
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
                .find(|item| item.says.english().starts_with(says))
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
        let items = [Item::new(
            EntryRef::new(FileId::new(1, 1), "/d/a.txt"),
            "a.txt",
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
        // A clash of many names says a few and how many more, never all.
        let many: Vec<Item> = (0..1000).map(|i| item(&format!("f{i}"))).collect();
        let said = plan_bulk_rename(
            &many,
            &RenameSpec {
                template: Some("same".into()),
                ..Default::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(said.ends_with(" and 997 more"), "{said}");
        assert!(said.len() < 100, "{said}");
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
        // As wide as a name, and no wider: never a string that long for
        // each item before the names are refused.
        let wide = |width: &str| {
            new_names(
                &items,
                &RenameSpec {
                    template: Some(format!("{{n:{width}}}")),
                    ..Default::default()
                },
            )
        };
        assert_eq!(wide("255").unwrap()[0].len(), 255);
        for wider in ["256", "65536", "99999999999999999999999"] {
            assert!(
                matches!(wide(wider), Err(BulkRenameError::Template(_))),
                "{wider}"
            );
        }
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
        let dated = [Item::new(
            EntryRef::new(FileId::new(1, 1), "/d/a"),
            "a",
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
        let items = [Item::new(
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
            items.push(Item::new(EntryRef::from_path(&p).unwrap(), *n, None));
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
        // The first name, which the regex's and the replacement's
        // references work on alone: kept while it is (the dialog).
        assert_eq!(v["first"], "x1.txt");
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
