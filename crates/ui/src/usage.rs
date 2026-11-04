//! Plugin-controlled Usage pane view. Filesystem work remains in the plugin.
//!
//! The map draws the folder's own entries, and inside each folder what it
//! holds, as many levels as the zoom asks and the blocks have room for
//! (`elements::treemap`). The folder's own entries are the pane's; the
//! entries drawn inside them are lent to the pane while they are drawn
//! (`Pane::lend`, ADR-116), so a block at any depth is selected by a click
//! and acted on by the menu as a row is, and a double click on one inside
//! goes there. The arrows move among the blocks beside the cursor's in its
//! folder, out to the folder's own neighbours at its edge. Every
//! level draws its whole folder: the entries too small to draw and, while
//! hidden files are not shown, the hidden ones each take a block of their
//! own, so no block looks larger than it is.
//!
//! What a block's size and color say is the map's mode (`Mode`, the
//! window's): the room it takes in its folder's hue, the files it holds,
//! when it last changed, or the kind of files that fill it. A block whose
//! room can be had back as a whole (a cache, a trash) is lined, in every
//! mode.
use crate::sizes::TextSizes;
use crate::sizes::sized;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use gpui::{AnyElement, Context, MouseButton, Window, div, prelude::*, px};
use serde::{Deserialize, Serialize};
use skwir_core::entry::{Entry, EntryKind};
use skwir_core::{EntryId, command::Invocation, os_path::PathArg};
use skwir_platform::mime::Kind;

use crate::elements::treemap::{Laidout, TreemapItem, TreemapStyle, treemap};
use crate::elements::zoom_wheel::{self, Zoomed};
use crate::treemap::{Toward, neighbour};
use crate::{
    format,
    shell::Shell,
    theme::{AGES, BLOCK_HUES, BlockFill, Theme},
    toolbar::{self, Button},
    view::{Nav, PaneView, Weighs, Zooms},
};

/// Levels drawn (`Shell::levels`): the folder's own entries, and inside
/// them as many more as the plugin sends (its `NESTED_LEVELS`).
pub const LEVELS: [u32; 4] = [1, 2, 3, 4];
pub const MAX_LEVELS: u32 = LEVELS[LEVELS.len() - 1];
pub const DEFAULT_LEVELS: u32 = 3;

/// What the zoom's steps say here (`ui.zoom` in, out and reset), each
/// with its tooltip: in the View menu and on the toolbar.
pub const STEPS: [(&str, &str); 3] = [
    ("More levels", "More levels (or Ctrl and the wheel)"),
    ("Fewer levels", "Fewer levels (or Ctrl and the wheel)"),
    ("Default levels", "Default levels"),
];

/// What the map draws of each block (`Shell::usage_mode`): the window's,
/// remembered with its shape.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// As large as the room it takes, in its folder's hue.
    #[default]
    Size,
    /// As large as the files it holds.
    Files,
    /// As large as the room it takes, warmer the newer its last change.
    Age,
    /// As large as the room it takes, in the color of the kind of files
    /// that take most of it.
    Types,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Size, Mode::Files, Mode::Age, Mode::Types];

    /// Its word in `ui.usage_mode`.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Size => "size",
            Mode::Files => "files",
            Mode::Age => "age",
            Mode::Types => "types",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Mode::Size => "Size",
            Mode::Files => "Files",
            Mode::Age => "Age",
            Mode::Types => "Types",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Mode::Size => "Blocks as large as the room they take",
            Mode::Files => "Blocks as large as the files they hold",
            Mode::Age => "Blocks colored by when they last changed",
            Mode::Types => "Blocks colored by the kind of files that fill them",
        }
    }
}

/// Room that can be had back as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reclaim {
    /// Its program makes it again when it needs it.
    Cache,
    /// Freed when the Trash is emptied.
    Trash,
}
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Node {
    pub id: Option<EntryId>,
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub incomplete: bool,
    pub size: u64,
    pub alloc: u64,
    #[serde(default)]
    pub files: u64,
    /// Its newest change, in seconds since 1970; 0 when unknown.
    #[serde(default)]
    pub modified: i64,
    /// A file's kind.
    #[serde(default)]
    pub kind: Option<Kind>,
    /// What an entry that is no folder is itself (a file, a link, a
    /// pipe): what it is lent to the pane as. `None` for a folder.
    #[serde(default)]
    pub entry_kind: Option<EntryKind>,
    /// A folder's bytes by the kinds' names (`other`: of no kind).
    #[serde(default)]
    pub kinds: BTreeMap<String, u64>,
    #[serde(default)]
    pub reclaim: Option<Reclaim>,
    /// Its bytes that caches and trashes hold.
    #[serde(default)]
    pub reclaimable: u64,
    /// A file's number of names (hard links); 0 for a folder.
    #[serde(default)]
    pub nlink: u64,
    /// Of a folder: what its files with names outside it take on disk,
    /// which deleting it would not free (ADR-049).
    #[serde(default)]
    pub shared: u64,
    pub children: Vec<Node>,
    pub other_alloc: u64,
    #[serde(default)]
    pub other_size: u64,
    pub other_count: u64,
    pub other_incomplete: u64,
    #[serde(default)]
    pub other_files: u64,
    #[serde(default)]
    pub other_modified: i64,
}

impl Node {
    /// The entry it is, as it is lent to the pane: `None` for what the
    /// scan could not say the kind of, which is not lent rather than
    /// taken for a file to read.
    fn entry(&self, path: PathBuf) -> Option<(Entry, PathBuf)> {
        let id = self.id?;
        let kind = if self.is_dir {
            EntryKind::Directory
        } else {
            self.entry_kind.filter(|kind| *kind != EntryKind::Unknown)?
        };
        let name = path.file_name()?.to_os_string();
        Some((Entry::new(id, name, kind), path))
    }

    /// What its block weighs in `mode`.
    fn measure(&self, mode: Mode) -> u64 {
        match mode {
            Mode::Files => self.files,
            Mode::Size | Mode::Age | Mode::Types => self.alloc,
        }
    }

    /// Its bytes by kind, the heaviest first: a file's are all its kind's.
    fn kinds(&self) -> Vec<(Option<Kind>, u64)> {
        if !self.is_dir {
            return vec![(self.kind, self.alloc)];
        }
        let mut kinds: Vec<_> = self
            .kinds
            .iter()
            .map(|(name, bytes)| (Kind::from_name(name), *bytes))
            .collect();
        kinds.sort_by_key(|(_, bytes)| std::cmp::Reverse(*bytes));
        kinds
    }
}
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Status {
    pub root: Option<PathBuf>,
    pub updating: bool,
    pub scanning: bool,
    pub files: u64,
    pub dirs: u64,
    pub alloc: u64,
    pub size: u64,
    pub tree: Option<Node>,
    pub error: Option<String>,
    pub scanned_dirs: u64,
}
#[derive(Default)]
pub struct UsageState {
    pub status: Option<Status>,
    /// What the block under the pointer stands for, so the pointer keeps
    /// it as the map reflows.
    hover: Option<Part>,
    root: Option<PathBuf>,
    location_lifetime: Option<skwir_core::CancelToken>,
    /// The plugin's descriptor revision, captured once per registration so
    /// frames read one atomic instead of querying the registry.
    source: Option<Arc<AtomicU64>>,
    revision: Option<u64>,
}

impl Shell {
    pub fn sync_usage(&mut self, cx: &mut Context<Self>) {
        if self.ui().view_id != crate::view::USAGE {
            if self.ui().usage.root.is_some() {
                let _ = self.app.dispatch(&Invocation::with_args(
                    "usage.release",
                    &serde_json::json!({"pane":self.pane}),
                ));
                self.ui_mut().usage = UsageState::default();
            }
            // What the map drew is drawn no more: what was selected of it
            // leaves the selection (ADR-116).
            let lent = self
                .app
                .workspace
                .read()
                .pane(self.pane)
                .is_some_and(skwir_core::workspace::Pane::has_lent);
            if lent && let Some(p) = self.app.workspace.write().pane_mut(self.pane) {
                p.lend(Vec::new());
            }
            return;
        }
        let dir = self
            .app
            .workspace
            .read()
            .pane(self.pane)
            .and_then(|p| p.location().and_then(skwir_core::Location::path));
        if self.ui().usage.root != dir
            || self
                .ui()
                .usage
                .location_lifetime
                .as_ref()
                .is_some_and(skwir_core::CancelToken::is_cancelled)
        {
            let location_lifetime = self
                .app
                .workspace
                .read()
                .pane(self.pane)
                .map(skwir_core::workspace::Pane::location_lifetime);
            self.ui_mut().usage = UsageState {
                root: dir.clone(),
                location_lifetime,
                ..UsageState::default()
            };
            let command = if dir.is_some() {
                "usage.scan"
            } else {
                "usage.release"
            };
            if let Err(error) = self.app.dispatch(&Invocation::with_args(
                command,
                &serde_json::json!({"pane":self.pane}),
            )) {
                self.set_status(error.to_string(), true, cx);
            }
        }
        if dir.is_none() {
            return;
        }
        if self.ui().usage.source.is_none() {
            // Once per registration: `sync_views` resets this state when the
            // plugin's view is replaced, which starts a new capture.
            let source = self
                .app
                .registry
                .views
                .find(|s| &*s.id == crate::view::USAGE)
                .map(|s| Arc::clone(&s.revision));
            self.ui_mut().usage.source = source;
        }
        let revision = self
            .ui()
            .usage
            .source
            .as_ref()
            .map(|source| source.load(Ordering::Acquire));
        if revision != self.ui().usage.revision {
            self.ui_mut().usage.revision = revision;
            if let Some(value) =
                self.query_value("usage.status", serde_json::json!({"pane":self.pane}), cx)
                && let Ok(status) = serde_json::from_value::<Status>(value)
                && status.root == dir
            {
                self.ui_mut().usage.status = Some(status);
            }
        }
    }

    pub fn refresh_usage(&mut self, cx: &mut Context<Self>) {
        if self.ui().view_id == crate::view::USAGE {
            if let Err(error) = self.app.dispatch(&Invocation::with_args(
                "usage.scan",
                &serde_json::json!({"pane":self.pane,"force":true}),
            )) {
                self.set_status(error.to_string(), true, cx);
            }
            cx.notify();
        }
    }
}

/// What a block of the map stands for, by the plugin's paths.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    Entry(PathBuf),
    /// A folder's entries too small to draw, together.
    Rest(PathBuf),
    /// A folder's hidden entries, while hidden files are not shown.
    Hidden(PathBuf),
}

/// A block, by its key in the map.
struct Block {
    part: Part,
    /// Folders inside the one shown: 0 for its own entries.
    depth: usize,
    /// The entry it is: one of the folder's own, or one lent to the pane
    /// (ADR-116). What a click selects; `None` for a block standing for
    /// several, or one the scan could not say the kind of.
    entry: Option<EntryId>,
    /// The block it is drawn in, by key.
    parent: Option<usize>,
    /// Its bytes, and whether it is a folder: what the footer weighs.
    size: u64,
    is_dir: bool,
    /// Where a double click goes, as the pane names it: the entry, or the
    /// folder holding the entries a block stands for.
    path: PathBuf,
    /// What the line under the map says while the pointer is on it.
    hint: String,
}

/// `480 MB`, or `480 MB · 120 MB on disk` when the two differ as said.
fn sizes(size: u64, alloc: u64) -> String {
    let (size, alloc) = (format::size(size), format::size(alloc));
    if size == alloc {
        size
    } else {
        format!("{size} · {alloc} on disk")
    }
}

/// What deleting `node` would free, where its names say it is less than
/// it takes (ADR-049): a folder holding files with names outside it, a
/// file with other names.
fn names_said(node: &Node) -> Option<String> {
    if node.is_dir {
        (node.shared > 0).then(|| freed(node))
    } else {
        (node.nlink > 1).then(|| format!("one of {} names of its file", node.nlink))
    }
}

/// What deleting a folder frees: what it takes on disk but for its files
/// with names outside it.
fn freed(node: &Node) -> String {
    format!(
        "{} freed if deleted",
        format::size(node.alloc.saturating_sub(node.shared))
    )
}

/// `24%` of `whole`; `<1%` for a speck.
fn share(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "0%".into();
    }
    let percent = part as f64 * 100. / whole as f64;
    if percent > 0. && percent < 1. {
        "<1%".into()
    } else {
        format!("{percent:.0}%")
    }
}

/// When each step of `age` begins, in seconds since 1970: a day, a week,
/// a month and a year before `now`, as the user's calendar counts them
/// (a day across a clock change, a month of its own length), never as
/// fixed numbers of seconds. Far in the past where one cannot be told.
fn age_limits(now: &jiff::Zoned) -> [i64; AGES - 1] {
    use jiff::ToSpan;
    [1.day(), 1.week(), 1.month(), 1.year()].map(|span| {
        now.checked_sub(span)
            .map_or(i64::MIN, |then| then.timestamp().as_second())
    })
}

/// How long ago `modified` was, as `Theme::block_age` steps: within a
/// day, a week, a month, a year (`age_limits`), or longer (or unknown).
fn age(modified: i64, limits: &[i64; AGES - 1]) -> usize {
    if modified <= 0 {
        return AGES - 1;
    }
    limits
        .iter()
        .position(|limit| modified > *limit)
        .unwrap_or(AGES - 1)
}

/// What the legend calls each step of `age`.
const AGE_NAMES: [&str; AGES] = ["a day", "a week", "a month", "a year", "longer"];

/// A kind as the legend and the hints say it.
fn kind_title(kind: Option<Kind>) -> &'static str {
    kind.map_or("Other files", Kind::title)
}

/// `Videos 62% · Images 30%`: the kinds that take a tenth of `node` or
/// more, the heaviest first; `Videos` alone when one takes it all.
fn kinds_said(node: &Node) -> String {
    let kinds = node.kinds();
    let total: u64 = kinds.iter().map(|(_, bytes)| bytes).sum();
    match kinds.as_slice() {
        [] => String::new(),
        [(kind, _)] => kind_title(*kind).to_owned(),
        _ => kinds
            .iter()
            .filter(|(_, bytes)| bytes * 10 >= total)
            .map(|(kind, bytes)| format!("{} {}", kind_title(*kind), share(*bytes, total)))
            .collect::<Vec<_>>()
            .join(" · "),
    }
}

/// What a block whose room can be had back says of it, or of what lies
/// in it (`within`: a cache or a trash it lies in).
fn reclaim_said(node: &Node, within: Option<Reclaim>) -> Option<String> {
    match node.reclaim.or(within) {
        Some(Reclaim::Cache) if node.reclaim.is_some() => {
            Some("a cache, which its program makes again when needed".into())
        }
        Some(Reclaim::Cache) => {
            Some("in a cache, which its program makes again when needed".into())
        }
        Some(Reclaim::Trash) => Some("in the Trash: emptying it frees this".into()),
        None if node.reclaimable > 0 => Some(format!(
            "{} of it in caches and the Trash",
            format::size(node.reclaimable.min(node.alloc))
        )),
        None => None,
    }
}

/// The entries of a folder drawn together: the hidden ones.
#[derive(Default)]
struct Gathered {
    size: u64,
    alloc: u64,
    files: u64,
    modified: i64,
    count: usize,
    incomplete: bool,
}

impl Gathered {
    fn add(&mut self, node: &Node) {
        self.size += node.size;
        self.alloc += node.alloc;
        self.files += if node.is_dir { node.files } else { 1 };
        self.modified = self.modified.max(node.modified);
        self.count += 1;
        self.incomplete |= node.incomplete;
    }
}

/// One of `BLOCK_HUES` for the file numbered `ino`, its bits mixed
/// (splitmix64's finalizer): a filesystem numbers files in steps, and a
/// number taken as it is would give files made one after the other the
/// same few hues.
fn hue_of(ino: u64) -> usize {
    let mut z = ino.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    ((z ^ (z >> 31)) % BLOCK_HUES as u64) as usize
}

/// What an entry of the folder shown wears its hue by: its file, or its
/// name where it was not read.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Wearer {
    File(skwir_core::FileId),
    Name(String),
}

impl Wearer {
    fn of(node: &Node) -> Self {
        node.id.map_or_else(
            || Wearer::Name(node.name.clone()),
            |id| Wearer::File(id.file),
        )
    }
}

/// The hues the folder shown's entries wear, kept while it is shown (the
/// map is built again at each count, sort and selection). An entry is
/// given one once, when it is first drawn: its number's (`hue_of`), or the
/// next one the fewest entries drawn wear, so up to `BLOCK_HUES` entries
/// wear one each. It keeps it while it is drawn, however the others come
/// and go (a count trims the smallest into the rest, a hidden file shown)
/// and however the map reflows. Another folder shown begins again.
#[derive(Default)]
struct Hues {
    root: Option<PathBuf>,
    worn: std::collections::HashMap<Wearer, usize>,
}

impl Hues {
    /// The hue of each of `drawn`, entries of the folder `root` (each its
    /// wearer and number; `None`: an entry not drawn, which gets 0).
    fn wear(&mut self, root: &Path, drawn: &[Option<(Wearer, u64)>]) -> Vec<usize> {
        if self.root.as_deref() != Some(root) {
            self.root = Some(root.to_path_buf());
            self.worn.clear();
        }
        // An entry no longer drawn gives its hue back.
        let present: std::collections::HashSet<&Wearer> =
            drawn.iter().flatten().map(|(wearer, _)| wearer).collect();
        self.worn.retain(|wearer, _| present.contains(wearer));
        let mut count = [0usize; BLOCK_HUES];
        for &hue in self.worn.values() {
            count[hue] += 1;
        }
        // Those first drawn now, by number: the same entries wear the same
        // hues whatever order the map has them in.
        let mut new: Vec<(u64, &Wearer)> = drawn
            .iter()
            .flatten()
            .filter(|(wearer, _)| !self.worn.contains_key(wearer))
            .map(|(wearer, number)| (*number, wearer))
            .collect();
        new.sort_unstable_by_key(|&(number, _)| number);
        for (number, wearer) in new {
            if self.worn.contains_key(wearer) {
                // Two names of one file: one hue.
                continue;
            }
            let least = count.iter().copied().min().unwrap_or(0);
            let mut hue = hue_of(number);
            while count[hue] != least {
                hue = (hue + 1) % BLOCK_HUES;
            }
            count[hue] += 1;
            self.worn.insert(wearer.clone(), hue);
        }
        drawn
            .iter()
            .map(|drawn| {
                drawn
                    .as_ref()
                    .and_then(|(wearer, _)| self.worn.get(wearer).copied())
                    .unwrap_or(0)
            })
            .collect()
    }
}

/// The blocks of a breakdown, and what each stands for.
struct Build<'a> {
    theme: &'a Theme,
    show_hidden: bool,
    levels: usize,
    scanning: bool,
    mode: Mode,
    /// When each step of a block's age begins (`age_limits`).
    ages: [i64; AGES - 1],
    /// Now, as the user's calendar says it.
    zoned: &'a jiff::Zoned,
    /// The folder as the pane names it, and as the plugin does (it counts
    /// by canonical path): a block's path is read from one into the other.
    root: &'a Path,
    tree: &'a Path,
    selected: &'a dyn Fn(EntryId) -> bool,
    /// The hues of the folder shown's entries, kept between builds.
    hues: &'a mut Hues,
    blocks: Vec<Block>,
    /// The entries drawn inside the folder's own, as they are lent to the
    /// pane.
    lent: Vec<(Entry, PathBuf)>,
}

/// What a level inherits from the folder it is drawn in.
struct Outer {
    /// That folder's block.
    key: usize,
    hue: usize,
    trail: String,
    /// The cache or the trash it lies in.
    within: Option<Reclaim>,
}

impl Build<'_> {
    fn shown(&self, path: &Path) -> PathBuf {
        path.strip_prefix(self.tree)
            .map_or_else(|_| path.to_path_buf(), |inside| self.root.join(inside))
    }

    /// `12 Mar 2024`: when `modified` was.
    fn when(&self, modified: i64) -> Option<String> {
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(modified.try_into().ok()?);
        (modified > 0).then(|| format::friendly_time(t, self.zoned))
    }

    /// The fills of a block: its outer entry's hue, its last change, or
    /// the kind of what fills it.
    fn color(&self, node: &Node, hue: usize, depth: usize) -> BlockFill {
        match self.mode {
            Mode::Size | Mode::Files => self.theme.block(hue, node.is_dir, depth),
            Mode::Age => self.theme.block_age(age(node.modified, &self.ages), depth),
            Mode::Types => {
                let kinds = node.kinds();
                let total: u64 = kinds.iter().map(|(_, bytes)| bytes).sum();
                let (kind, bytes) = kinds.first().copied().unwrap_or((None, 0));
                let share = if total == 0 {
                    0.
                } else {
                    bytes as f32 / total as f32
                };
                self.theme.block_kind(kind, share, depth)
            }
        }
    }

    /// What a block says under its name.
    fn detail(&self, node: &Node) -> String {
        let measure = node.measure(self.mode);
        match (node.incomplete, measure) {
            (true, 0) if self.scanning => return "counting…".into(),
            (true, 0) => return "incomplete".into(),
            _ => {}
        }
        let said = match self.mode {
            Mode::Size | Mode::Types => format::size(measure),
            Mode::Files => format::count(usize::try_from(measure).unwrap_or(usize::MAX), "file"),
            Mode::Age => self
                .when(node.modified)
                .unwrap_or_else(|| format::size(measure)),
        };
        let said = if node.incomplete {
            format!("≥ {said}")
        } else {
            said
        };
        match node.reclaim {
            Some(Reclaim::Cache) => format!("{said} · cache"),
            Some(Reclaim::Trash) => format!("{said} · trash"),
            None => said,
        }
    }

    /// What the line under the map says of `node`, in `parent`.
    fn hint(&self, node: &Node, trail: &str, parent: &Node, outer: Option<&Outer>) -> String {
        let counted = |said: String| {
            if node.incomplete {
                format!("counted so far: {said}")
            } else {
                said
            }
        };
        let mut parts = vec![trail.to_owned()];
        match self.mode {
            Mode::Files => {
                parts.push(counted(format::count(
                    usize::try_from(node.files).unwrap_or(usize::MAX),
                    "file",
                )));
                parts.push(format!(
                    "{} of the files in {}",
                    share(node.files, parent.files),
                    parent.name
                ));
            }
            Mode::Size | Mode::Age | Mode::Types => {
                parts.push(counted(sizes(node.size, node.alloc)));
                parts.push(format!(
                    "{} of {}",
                    share(node.alloc, parent.alloc),
                    parent.name
                ));
            }
        }
        match self.mode {
            Mode::Age => parts.extend(self.when(node.modified).map(|w| format!("changed {w}"))),
            Mode::Types => parts.push(kinds_said(node)),
            Mode::Size | Mode::Files => {}
        }
        parts.extend(reclaim_said(node, outer.and_then(|o| o.within)));
        parts.extend(names_said(node));
        if outer.is_some() {
            parts.push("double-click to go there".into());
        }
        parts.retain(|part| !part.is_empty());
        parts.join(" · ")
    }

    /// The blocks of what `node` holds, `depth` folders inside the one shown.
    fn level(&mut self, node: &Node, depth: usize, outer: Option<&Outer>) -> Vec<TreemapItem> {
        // Each block is drawn at least this large, so each can be pointed at.
        let floor = (node.measure(self.mode) as f64 / 200.).max(1.);
        let within = |what: String| match outer {
            Some(outer) => format!("{what} in {}", outer.trail),
            None => what,
        };
        let mut items = Vec::new();
        let mut hidden = Gathered::default();
        let show_hidden = self.show_hidden;
        let shown = |child: &Node| show_hidden || !child.name.starts_with('.');
        // Colored by identity, so a block keeps its color as the map
        // reflows and as other entries come and go (`Hues`).
        let hues = if outer.is_some() {
            Vec::new()
        } else {
            let drawn: Vec<Option<(Wearer, u64)>> = node
                .children
                .iter()
                .enumerate()
                .map(|(index, child)| {
                    let number = child.id.map_or(index as u64, |id| id.file.ino);
                    shown(child).then(|| (Wearer::of(child), number))
                })
                .collect();
            self.hues.wear(self.root, &drawn)
        };
        for (index, child) in node.children.iter().enumerate() {
            if !shown(child) {
                hidden.add(child);
                continue;
            }
            let (hue, trail) = match outer {
                Some(outer) => (outer.hue, format!("{} › {}", outer.trail, child.name)),
                None => (hues[index], child.name.clone()),
            };
            let path = self.shown(&child.path);
            // The folder's own entries are the pane's; one drawn inside
            // them is lent to it while it is drawn.
            let entry = match outer {
                None => child.id,
                Some(_) => child.entry(path.clone()).map(|lent| {
                    let id = lent.0.id;
                    self.lent.push(lent);
                    id
                }),
            };
            let key = self.blocks.len();
            self.blocks.push(Block {
                part: Part::Entry(child.path.clone()),
                depth,
                entry,
                parent: outer.map(|o| o.key),
                size: child.size,
                is_dir: child.is_dir,
                path,
                hint: self.hint(child, &trail, node, outer),
            });
            let children = if child.is_dir && depth + 1 < self.levels {
                let outer = Outer {
                    key,
                    hue,
                    trail,
                    within: child.reclaim.or(outer.and_then(|o| o.within)),
                };
                self.level(child, depth + 1, Some(&outer))
            } else {
                Vec::new()
            };
            let fill = self.color(child, hue, depth);
            items.push(TreemapItem {
                key,
                weight: (child.measure(self.mode) as f64).max(floor),
                label: child.name.clone().into(),
                detail: self.detail(child).into(),
                color: fill.closed,
                tray: fill.open,
                text: self.theme.block_text,
                hatched: child.incomplete,
                dashed: child.reclaim.is_some(),
                selected: entry.is_some_and(|id| (self.selected)(id)),
                children,
            });
        }
        if hidden.count > 0 {
            let what = format::count(hidden.count, "hidden item");
            let key = self.blocks.len();
            self.blocks.push(Block {
                part: Part::Hidden(node.path.clone()),
                depth,
                entry: None,
                parent: outer.map(|o| o.key),
                size: hidden.size,
                is_dir: false,
                path: self.shown(&node.path),
                hint: format!(
                    "{} · {} · double-click to show hidden files",
                    within(what.clone()),
                    self.rest_said(hidden.size, hidden.alloc, hidden.files)
                ),
            });
            let measure = match self.mode {
                Mode::Files => hidden.files,
                Mode::Size | Mode::Age | Mode::Types => hidden.alloc,
            };
            items.push(self.rest(key, what, measure, floor, hidden.incomplete));
        }
        if node.other_count > 0 {
            let count = usize::try_from(node.other_count).unwrap_or(usize::MAX);
            let key = self.blocks.len();
            self.blocks.push(Block {
                part: Part::Rest(node.path.clone()),
                depth,
                entry: None,
                parent: outer.map(|o| o.key),
                size: node.other_size,
                is_dir: false,
                path: self.shown(&node.path),
                hint: format!(
                    "{} · {} · {}",
                    within(format::count(count, "smaller item")),
                    self.rest_said(node.other_size, node.other_alloc, node.other_files),
                    if depth == 0 {
                        "double-click to see all items in List"
                    } else {
                        "double-click to go there"
                    }
                ),
            });
            let measure = match self.mode {
                Mode::Files => node.other_files,
                Mode::Size | Mode::Age | Mode::Types => node.other_alloc,
            };
            items.push(self.rest(
                key,
                format::count(count, "other item"),
                measure,
                floor,
                node.other_incomplete > 0,
            ));
        }
        items
    }

    /// What several entries drawn together hold, as the mode counts.
    fn rest_said(&self, size: u64, alloc: u64, files: u64) -> String {
        match self.mode {
            Mode::Files => format::count(usize::try_from(files).unwrap_or(usize::MAX), "file"),
            Mode::Size | Mode::Age | Mode::Types => sizes(size, alloc),
        }
    }

    /// A block standing for several entries.
    fn rest(
        &self,
        key: usize,
        what: String,
        measure: u64,
        floor: f64,
        incomplete: bool,
    ) -> TreemapItem {
        TreemapItem {
            key,
            weight: (measure as f64).max(floor),
            label: what.into(),
            detail: match self.mode {
                Mode::Files => {
                    format::count(usize::try_from(measure).unwrap_or(usize::MAX), "file")
                }
                Mode::Size | Mode::Age | Mode::Types => format::size(measure),
            }
            .into(),
            color: self.theme.block_rest,
            tray: self.theme.block_rest,
            text: self.theme.text,
            hatched: incomplete,
            dashed: false,
            selected: false,
            children: Vec::new(),
        }
    }
}

/// The blocks of the map as `from` says, each entry drawn inside the
/// folder's own with what it is lent to the pane as.
fn build_map(
    shell: &Shell,
    from: BuiltFrom,
    theme: &Theme,
    zoned: &jiff::Zoned,
    hues: &mut Hues,
) -> Built {
    let state = &shell.ui().usage;
    let status = state.status.as_ref();
    let mut items = Vec::new();
    let mut blocks = Vec::new();
    let mut lent = Vec::new();
    if let (Some(node), Some(root)) = (status.and_then(|s| s.tree.as_ref()), state.root.as_deref())
    {
        let ws = shell.app.workspace.read();
        let p = ws.pane(shell.pane);
        let selected = |id| p.is_some_and(|p| p.selection.contains(id));
        let mut build = Build {
            theme,
            show_hidden: from.show_hidden,
            levels: from.levels as usize,
            scanning: status.is_some_and(|s| s.scanning),
            mode: from.mode,
            ages: age_limits(zoned),
            zoned,
            root,
            tree: &node.path,
            selected: &selected,
            hues,
            blocks: Vec::new(),
            lent: Vec::new(),
        };
        items = build.level(node, 0, None);
        blocks = build.blocks;
        lent = build.lent;
    }
    let keys = keys_of(&blocks);
    Built {
        from,
        items: Rc::new(items),
        blocks: Rc::new(blocks),
        keys,
        lent,
    }
}

#[derive(Default)]
pub struct UsageView {
    /// Where the folder's own entries were drawn, by key, and the entry of
    /// each key: what the arrows move among.
    laid: Laidout,
    entries: Vec<Option<EntryId>>,
    /// The map's blocks as last built, and what from: built again only
    /// when that changes, not at each frame (a pointer moving over the
    /// map draws one).
    built: Option<Built>,
    /// The hues the folder shown's entries wear, kept as it is built again.
    hues: Hues,
    /// What was last lent to the pane, and the version it said
    /// (`Pane::lend`): lent again when either moves.
    lent: Option<(LentFrom, u64)>,
}

/// What the entries of `selection` weigh, each by the block it is drawn
/// as (`Built::keys`), at any depth: a file of several names selected
/// under each counts once; two folders sharing one each count it, as each
/// holds it; what lies in a folder selected counts with it, not again.
/// `None` when one is not drawn (folded into the rest): its weight is not
/// known.
fn weigh(built: &Built, selection: &skwir_core::workspace::Selection) -> Option<u64> {
    let mut files = std::collections::HashSet::new();
    selection.iter().try_fold(0, |sum, id| {
        let block = &built.blocks[*built.keys.get(&id)?];
        let within = std::iter::successors(block.parent, |k| built.blocks[*k].parent)
            .any(|k| built.blocks[k].entry.is_some_and(|e| selection.contains(e)));
        let once = !within && (block.is_dir || files.insert(id.file));
        Some(sum + if once { block.size } else { 0 })
    })
}

/// The block of each entry drawn, by its id.
fn keys_of(blocks: &[Block]) -> std::collections::HashMap<EntryId, usize> {
    blocks
        .iter()
        .enumerate()
        .filter_map(|(key, block)| Some((block.entry?, key)))
        .collect()
}

/// Where an arrow goes from the cursor's block (`UsageView::beside`).
enum Beside {
    /// The cursor is on one of the folder's own entries, or none: the
    /// pane's order says (`nav_delta`).
    Own,
    /// At the map's edge: the cursor stays.
    Edge,
    To(EntryId),
}

impl UsageView {
    /// The entry beside the cursor's block toward `toward`, where it was
    /// last laid out, when the cursor is on a block drawn inside the
    /// folder's own: among the blocks beside it in its folder, else beside
    /// that folder, and so out to the folder's own entries.
    fn beside(&self, cursor: Option<EntryId>, toward: Toward) -> Beside {
        let Some(built) = self.built.as_ref() else {
            return Beside::Own;
        };
        let Some(mut at) = cursor.and_then(|cursor| built.keys.get(&cursor).copied()) else {
            return Beside::Own;
        };
        if built.blocks.get(at).is_none_or(|b| b.depth == 0) {
            return Beside::Own;
        }
        let laid = self.laid.borrow();
        loop {
            let parent = built.blocks[at].parent;
            let (keys, rects): (Vec<usize>, Vec<crate::treemap::Rect>) = laid
                .iter()
                .filter(|(key, _)| {
                    built
                        .blocks
                        .get(*key)
                        .is_some_and(|b| b.parent == parent && b.entry.is_some())
                })
                .copied()
                .unzip();
            let Some(from) = keys.iter().position(|key| *key == at) else {
                return Beside::Edge;
            };
            if let Some(to) = neighbour(&rects, from, toward)
                && let Some(id) = built.blocks[keys[to]].entry
            {
                return Beside::To(id);
            }
            let Some(outer) = parent else {
                return Beside::Edge;
            };
            at = outer;
        }
    }
}

/// The blocks of the map, built from `from`.
struct Built {
    from: BuiltFrom,
    items: Rc<Vec<TreemapItem>>,
    blocks: Rc<Vec<Block>>,
    /// The block of each entry drawn, by its id.
    keys: std::collections::HashMap<EntryId, usize>,
    /// The entries drawn inside the folder's own, as they are lent.
    lent: Vec<(Entry, PathBuf)>,
}

/// What decides which entries are drawn inside the folder's own: the
/// plugin's answer, the levels and whether hidden files are shown. Not
/// the mode, the selection nor the minute, which change only how.
#[derive(Clone, PartialEq)]
struct LentFrom {
    root: Option<PathBuf>,
    revision: Option<u64>,
    levels: u32,
    show_hidden: bool,
}

/// What the blocks depend on: the plugin's answer, how it is shown, what
/// is selected, the minute (a file's age) and the colors.
#[derive(PartialEq)]
struct BuiltFrom {
    root: Option<PathBuf>,
    revision: Option<u64>,
    mode: Mode,
    levels: u32,
    show_hidden: bool,
    selection: Option<u64>,
    minute: i64,
    theme: Theme,
}

impl PaneView for UsageView {
    fn id(&self) -> &'static str {
        crate::view::USAGE
    }
    fn refresh(&mut self, shell: &mut Shell, cx: &mut Context<Shell>) {
        shell.refresh_usage(cx);
    }
    /// Its scanning and empty states are its own.
    fn draws_empty(&self) -> bool {
        true
    }
    fn scroll_to(&mut self, _: usize) {}
    /// The arrows go to the block beside the cursor's on the map, the
    /// largest first; the pane's order is only where they land.
    fn nav_delta(&self, nav: Nav, shell: &Shell) -> Option<isize> {
        let toward = match nav {
            Nav::Up => Toward::Up,
            Nav::Down => Toward::Down,
            Nav::Left => Toward::Left,
            Nav::Right => Toward::Right,
            _ => return None,
        };
        let laid = self.laid.borrow();
        let (ids, rects): (Vec<EntryId>, Vec<crate::treemap::Rect>) = laid
            .iter()
            .filter_map(|(key, rect)| Some((self.entries.get(*key).copied().flatten()?, *rect)))
            .unzip();
        let cursor = shell.cursor_id();
        let to = match cursor.and_then(|c| ids.iter().position(|id| *id == c)) {
            Some(from) => ids[neighbour(&rects, from, toward)?],
            None => *ids.first()?,
        };
        let target = shell.ui().list.index_of(to)? as isize;
        let current = shell.cursor_index().map_or(-1, |i| i as isize);
        Some(target - current)
    }
    /// On a block drawn inside the folder's own, the arrows go to the one
    /// beside it in its folder, else beside that folder, out to the
    /// folder's own entries (`UsageView::beside`), with the key's step: a
    /// lent entry is in no order for `nav_delta` to count in.
    fn nav(
        &mut self,
        nav: Nav,
        step: crate::list::Step,
        shell: &mut Shell,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) -> bool {
        let toward = match nav {
            Nav::Up => Toward::Up,
            Nav::Down => Toward::Down,
            Nav::Left => Toward::Left,
            Nav::Right => Toward::Right,
            _ => return false,
        };
        match self.beside(shell.cursor_id(), toward) {
            Beside::Own => false,
            Beside::Edge => true,
            Beside::To(id) => {
                shell.select_entry(id, step, window, cx);
                true
            }
        }
    }
    fn zooms(&self) -> Zooms {
        Zooms::Levels
    }
    /// Its folders measured: the footer weighs them too, once counted.
    fn weighs(&self, shell: &Shell) -> Option<Weighs> {
        let status = shell.ui().usage.status.as_ref()?;
        if status.tree.as_ref().is_none_or(|tree| tree.incomplete) {
            return Some(Weighs::default());
        }
        let built = self.built.as_ref();
        let selected = shell
            .app
            .workspace
            .read()
            .pane(shell.pane)
            .and_then(|p| weigh(built?, &p.selection));
        Some(Weighs {
            shown: Some(status.size),
            selected,
        })
    }
    fn render(&mut self, shell: &mut Shell, _: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
        render(self, shell, cx)
    }
}

fn render(view: &mut UsageView, shell: &mut Shell, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let pane = shell.pane;
    let summary = summary(&shell.ui().usage);
    let mode = shell.usage_mode;
    // The richest toolbar that fits: the count of levels between the
    // zoom's steps where there is room (`elements::adaptive`).
    let header = crate::elements::adaptive::AdaptiveRow {
        variants: [true, false]
            .into_iter()
            .map(|count| tools(shell, &theme, &summary, count, cx))
            .collect(),
        height: toolbar::HEIGHT,
        chosen: None,
    };
    // Read before the lock below: a second read waits behind a writer.
    let show_hidden = shell.sort_spec().show_hidden;
    let zoned = jiff::Zoned::now();
    let built_from = |shell: &Shell| BuiltFrom {
        root: shell.ui().usage.root.clone(),
        revision: shell.ui().usage.revision,
        mode,
        levels: shell.levels,
        show_hidden,
        selection: shell
            .app
            .workspace
            .read()
            .pane(pane)
            .map(|p| p.selection.revision()),
        minute: zoned.timestamp().as_second().div_euclid(60),
        theme: theme.clone(),
    };
    let from = built_from(shell);
    if view.built.as_ref().is_none_or(|built| built.from != from) {
        view.built = Some(build_map(shell, from, &theme, &zoned, &mut view.hues));
    }
    // What is drawn inside the folder's own entries is lent to the pane
    // (ADR-116) when it is not what the pane holds: drawn otherwise
    // (another folder, scan, level), or let go of by the pane.
    let lent_from = LentFrom {
        root: shell.ui().usage.root.clone(),
        revision: shell.ui().usage.revision,
        levels: shell.levels,
        show_hidden,
    };
    let holds = shell
        .app
        .workspace
        .read()
        .pane(pane)
        .map(skwir_core::workspace::Pane::lent_version);
    if let (Some(holds), Some(built)) = (holds, view.built.as_ref())
        && view.lent.as_ref() != Some(&(lent_from.clone(), holds))
    {
        let lent = built.lent.clone();
        let version = shell
            .app
            .workspace
            .write()
            .pane_mut(pane)
            .map(|p| p.lend(lent));
        view.lent = version.map(|version| (lent_from, version));
        // What is no longer drawn left the selection: rings drawn again.
        let from = built_from(shell);
        if view.built.as_ref().is_none_or(|built| built.from != from) {
            view.built = Some(build_map(shell, from, &theme, &zoned, &mut view.hues));
        }
    }
    let state = &shell.ui().usage;
    let (items, blocks) = view.built.as_ref().map_or_else(
        || (Rc::default(), Rc::default()),
        |built| (Rc::clone(&built.items), Rc::clone(&built.blocks)),
    );
    view.entries = blocks
        .iter()
        .map(|b| {
            b.entry
                .filter(|_| b.depth == 0 && matches!(b.part, Part::Entry(_)))
        })
        .collect();
    let hover = state
        .hover
        .as_ref()
        .and_then(|part| blocks.iter().position(|b| b.part == *part));
    let deleting = deleting_frees(shell);
    let legend = legend(shell, &theme, mode);
    let state = &shell.ui().usage;
    let status = state.status.as_ref();
    let hint = hover.and_then(|key| blocks.get(key)).map_or_else(
        || {
            status.and_then(|s| s.error.clone()).unwrap_or_else(|| {
                if status.is_some_and(|s| s.scanning && s.tree.as_ref().is_some_and(|n| n.incomplete)) {
                    "Striped blocks are still being counted · proportions will change as sizes arrive".into()
                } else if status.is_some_and(|s| s.updating) {
                    format!("Scanning… {} folders read", status.map_or(0, |s| s.scanned_dirs))
                } else if let Some(note) = deleting {
                    note
                } else {
                    "Click to select · double-click to open · Ctrl and the wheel for more or fewer levels".into()
                }
            })
        },
        |b| b.hint.clone(),
    );
    let hovered = Rc::clone(&blocks);
    let entity = cx.entity();
    let entity2 = entity.clone();
    let root = state.root.clone();
    let style = TreemapStyle {
        gap: theme.bg,
        hover: theme.block_hover,
        hover_edge: theme.block_hover_edge,
        selected: theme.block_selected,
        dashed: theme.block_reclaim,
    };
    let map = treemap(
        ("usage-map", pane.0 as usize),
        items,
        style,
        move |key, _, cx| {
            entity.update(cx, |shell, cx| {
                shell.with_pane(pane, |shell| {
                    shell.ui_mut().usage.hover =
                        key.and_then(|key| hovered.get(key).map(|b| b.part.clone()));
                });
                cx.notify();
            });
        },
        move |key, event, window, cx| {
            entity2.update(cx, |shell, cx| {
                shell.with_pane(pane, |shell| {
                    if shell.ui().view_id != crate::view::USAGE || shell.ui().usage.root != root {
                        return;
                    }
                    if let Some(block) = blocks.get(key) {
                        click(shell, block, event, window, cx);
                    }
                });
            });
        },
    )
    .hovered(hover)
    .record(Rc::clone(&view.laid));
    div()
        .size_full()
        .flex()
        .flex_col()
        .text_color(theme.block_text)
        .child(header)
        .child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(map)
                .child(zoom_wheel::overlay(Zoomed::View(pane), cx)),
        )
        .children(legend)
        .child(crate::elements::clip::clipped(
            div()
                .id("usage-hint")
                .h(sized(24.))
                .px_2()
                .text_small()
                .text_color(theme.text_muted)
                .truncate(),
            hint,
        ))
        .into_any_element()
}

/// What the toolbar says of the folder: its size and counts, or why there
/// are none yet.
fn summary(state: &UsageState) -> String {
    let status = state.status.as_ref();
    status.map_or_else(
        || {
            if state.root.is_some() {
                "Scanning…".into()
            } else {
                "Usage is available for filesystem folders".into()
            }
        },
        |s| {
            if s.tree.is_none() && s.scanning {
                return "Calculating sizes…".into();
            }
            let reclaimable = s.tree.as_ref().map_or(0, |n| n.reclaimable.min(n.alloc));
            let freed = s
                .tree
                .as_ref()
                .filter(|n| n.shared > 0)
                .map_or(String::new(), |n| format!(" · {}", freed(n)));
            format!(
                "{}{} · {} · {}{}{}{}",
                if s.tree.as_ref().is_some_and(|n| n.incomplete) {
                    "Counted so far: "
                } else {
                    ""
                },
                sizes(s.size, s.alloc),
                format::count(usize::try_from(s.files).unwrap_or(usize::MAX), "file"),
                format::count(usize::try_from(s.dirs).unwrap_or(usize::MAX), "folder"),
                if reclaimable > 0 {
                    format!(" · {} in caches and the Trash", format::size(reclaimable))
                } else {
                    String::new()
                },
                freed,
                if s.updating { " · updating…" } else { "" }
            )
        },
    )
}

/// The toolbar: what the folder holds, then the modes, the zoom's steps
/// (with the count of levels, when `count`) and Recalculate. The summary
/// is drawn over a box of its own, which asks for no room: it takes what
/// the tools leave, so the tools alone choose the version that fits.
fn tools(
    shell: &Shell,
    theme: &Theme,
    summary: &str,
    count: bool,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let pane = shell.pane;
    let mut header = toolbar::row(theme)
        .text_color(theme.text)
        .id(("usage-tools", pane.0 as usize))
        .child(
            div().flex_1().min_w_0().h_full().relative().child(
                div().absolute().inset_0().flex().items_center().child(
                    crate::elements::clip::clipped(
                        div()
                            .id("usage-summary")
                            .flex_1()
                            .min_w_0()
                            .text_small()
                            .truncate(),
                        summary.to_owned(),
                    ),
                ),
            ),
        );
    let mode = shell.usage_mode;
    for each in Mode::ALL {
        header = header.child(
            Button::new(
                format!("usage-mode-{}-{}", each.name(), pane.0),
                "",
                each.title(),
                "ui.usage_mode",
            )
            .args(serde_json::json!({ "mode": each.name() }))
            .label(each.label())
            .active(each == mode)
            .render(shell, cx),
        );
    }
    header
        .child(toolbar::separator(theme))
        .child(levels(shell, theme, count, cx))
        .child(toolbar::separator(theme))
        .child(
            Button::new(
                format!("usage-refresh-{}", pane.0),
                "refresh",
                "Recalculate sizes",
                "ui.usage_refresh",
            )
            .enabled(shell.ui().usage.root.is_some())
            .render(shell, cx),
        )
        .into_any_element()
}

/// The zoom's steps, for whoever never tries Ctrl and the wheel: the View
/// menu's (`STEPS`), each greyed at its end, around how many levels the map
/// draws when `count`. The count keeps the room of the widest, so neither
/// step moves under the pointer as it changes.
fn levels(shell: &Shell, theme: &Theme, count: bool, cx: &mut Context<Shell>) -> gpui::Div {
    let levels = shell.levels;
    let said = |n: u32| format::count(n as usize, "level");
    let mut step = |dir: &str, icon: &'static str, (_, title): (&str, &'static str), enabled| {
        Button::new(
            format!("usage-levels-{dir}-{}", shell.pane.0),
            icon,
            title,
            "ui.zoom",
        )
        .args(serde_json::json!({ "dir": dir }))
        .enabled(enabled)
        .render(shell, cx)
    };
    let fewer = step("out", "minus", STEPS[1], levels > LEVELS[0]);
    let more = step("in", "plus", STEPS[0], levels < MAX_LEVELS);
    div()
        .flex()
        .flex_none()
        .items_center()
        .child(fewer)
        .when(count, |d| {
            d.child(
                div()
                    .relative()
                    .text_small()
                    .text_color(theme.text_muted)
                    .child(div().invisible().child(said(MAX_LEVELS)))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .justify_center()
                            .child(said(levels)),
                    ),
            )
        })
        .child(more)
}

/// What Delete does to the room the selection takes, while there is one:
/// the Trash keeps it on the disk until it is emptied.
fn deleting_frees(shell: &Shell) -> Option<String> {
    let selected = shell
        .app
        .workspace
        .read()
        .pane(shell.pane)
        .is_some_and(|p| !p.selection.is_empty());
    if !selected {
        return None;
    }
    let args = serde_json::json!({});
    let trash = shell.keys_here("trash", &args).map_or_else(
        || "Moving to the Trash".to_owned(),
        |keys| format!("{keys} moves to the Trash, which"),
    );
    let delete = shell
        .keys_here("delete", &args)
        .map(|keys| format!(" · {keys} deletes for good"));
    Some(format!(
        "{trash} frees no room until it is emptied{}",
        delete.unwrap_or_default()
    ))
}

/// What the colors say, a row above the line under the map, where the
/// mode colors by something of its own: when blocks changed, or the
/// kinds that fill the folder, the heaviest first. One line tall: what
/// does not fit wraps out of sight, the smallest kinds first (layout
/// alone).
fn legend(shell: &Shell, theme: &Theme, mode: Mode) -> Option<gpui::Div> {
    let entries: Vec<(gpui::Hsla, String)> = match mode {
        Mode::Age => (0..AGES)
            .map(|step| (theme.block_age(step, 0).closed, AGE_NAMES[step].to_owned()))
            .collect(),
        Mode::Types => {
            let tree = shell.ui().usage.status.as_ref()?.tree.as_ref()?;
            tree.kinds()
                .into_iter()
                .map(|(kind, bytes)| {
                    (
                        theme.block_kind(kind, 1., 0).closed,
                        format!("{} {}", kind_title(kind), format::size(bytes)),
                    )
                })
                .collect()
        }
        Mode::Size | Mode::Files => return None,
    };
    let item = |color: gpui::Hsla, label: String| {
        div()
            .flex_none()
            .h(sized(24.))
            .flex()
            .items_center()
            .gap(sized(4.))
            .child(div().size(px(10.)).rounded_sm().bg(color))
            .child(label)
    };
    let lead = (mode == Mode::Age).then(|| {
        div()
            .flex_none()
            .h(sized(24.))
            .flex()
            .items_center()
            .child("Changed within")
    });
    Some(
        div()
            .w_full()
            .h(sized(24.))
            .px_2()
            .overflow_hidden()
            .flex()
            .flex_wrap()
            .gap_x(sized(12.))
            .text_small()
            .text_color(theme.text_muted)
            .children(lead)
            .children(entries.into_iter().map(|(color, label)| item(color, label))),
    )
}

/// A press on a block. An entry, the folder's own or one drawn inside
/// them (lent to the pane, ADR-116), is pressed as a row is: selected,
/// Ctrl adding or taking it out, its menu opened; a double click or the
/// middle button on one drawn inside goes there. A block standing for
/// several entries shows them.
fn click(
    shell: &mut Shell,
    block: &Block,
    event: &gpui::MouseDownEvent,
    window: &mut Window,
    cx: &mut Context<Shell>,
) {
    let double = event.button == MouseButton::Left && event.click_count >= 2;
    let pane = shell.pane;
    let reveal = |shell: &mut Shell, tab: bool, window: &mut Window, cx: &mut Context<Shell>| {
        shell.run(
            "ui.reveal",
            serde_json::json!({
                "path": PathArg::from(block.path.clone()),
                "pane": pane,
                "tab": tab,
            }),
            window,
            cx,
        );
    };
    match &block.part {
        Part::Entry(_) if block.depth > 0 && (double || event.button == MouseButton::Middle) => {
            reveal(shell, event.button == MouseButton::Middle, window, cx);
        }
        Part::Entry(_) => {
            if let Some(id) = block.entry {
                shell.on_entry_mouse_down(pane, id, event, window, cx);
                // Nothing on the map is dragged: a press on an entry of a
                // selection of several is its click, which makes the
                // selection that entry (`on_entry_mouse_up`).
                shell.on_entry_mouse_up(pane, id, window, cx);
            }
        }
        Part::Hidden(_) if double => {
            shell.run(
                "toggle_hidden",
                serde_json::json!({ "pane": pane }),
                window,
                cx,
            );
        }
        Part::Rest(_) if double && block.depth == 0 => {
            shell.run("ui.view", serde_json::json!({"id": "list"}), window, cx);
        }
        Part::Rest(_) if double => reveal(shell, false, window, cx),
        Part::Hidden(_) | Part::Rest(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(path: &str, alloc: u64, is_dir: bool, children: Vec<Node>) -> Node {
        Node {
            id: Some(EntryId {
                file: skwir_core::FileId::new(1, alloc),
                at: 0,
            }),
            name: path.rsplit('/').next().unwrap().to_owned(),
            path: PathBuf::from(path),
            is_dir,
            size: alloc,
            alloc,
            children,
            ..Node::default()
        }
    }

    /// The wearers and numbers of files numbered `numbers` (`None`: not
    /// drawn).
    fn wearers(numbers: &[Option<u64>]) -> Vec<Option<(Wearer, u64)>> {
        numbers
            .iter()
            .map(|number| {
                number.map(|ino| {
                    let file = skwir_core::FileId::new(1, ino);
                    (Wearer::File(file), ino)
                })
            })
            .collect()
    }

    #[test]
    fn entries_wear_a_hue_each_whatever_their_order() {
        // Eight files made one after the other, numbered one or eight
        // apart as a filesystem numbers them, a hidden one among them.
        for step in [1, 8] {
            let mut numbers: Vec<Option<u64>> = (0..8).map(|k| Some(1000 + k * step)).collect();
            numbers.insert(3, None);
            let given = Hues::default().wear(Path::new("/r"), &wearers(&numbers));
            let mut worn: Vec<usize> = numbers
                .iter()
                .zip(&given)
                .filter_map(|(number, hue)| number.map(|_| *hue))
                .collect();
            worn.sort_unstable();
            assert_eq!(worn, (0..BLOCK_HUES).collect::<Vec<_>>(), "step {step}");
            // Sized otherwise, the map reorders them: each keeps its hue,
            // drawn first that way or drawn again.
            numbers.reverse();
            let mut fresh = Hues::default().wear(Path::new("/r"), &wearers(&numbers));
            fresh.reverse();
            assert_eq!(fresh, given, "step {step}");
        }
    }

    /// An entry keeps its hue while it is drawn: another drawn or gone,
    /// whatever its number, changes no one else's. Another folder shown
    /// begins again.
    #[test]
    fn a_hue_stays_with_its_entry_as_others_come_and_go() {
        let mut hues = Hues::default();
        let root = Path::new("/r");
        let numbers: Vec<Option<u64>> = (0..12).map(|k| Some(1000 + k)).collect();
        let first = hues.wear(root, &wearers(&numbers));
        // One numbered before them all comes (a count brought it back
        // from the rest), and one goes.
        let mut more = numbers.clone();
        more.insert(0, Some(1));
        more.remove(5);
        let again = hues.wear(root, &wearers(&more));
        let kept: Vec<usize> = first
            .iter()
            .enumerate()
            .filter(|&(at, _)| at != 4)
            .map(|(_, hue)| *hue)
            .collect();
        assert_eq!(again[1..], kept[..]);
        // The one that came wears a hue the fewest of the others wear.
        let worn = |hue| again[1..].iter().filter(|&&h| h == hue).count();
        let least = (0..BLOCK_HUES).map(worn).min();
        assert_eq!(Some(worn(again[0])), least);
        // Shown as they were (the hidden one hidden again): the same hues.
        let back: Vec<Option<u64>> = more
            .iter()
            .enumerate()
            .map(|(at, number)| number.filter(|_| at != 0))
            .collect();
        let shown = hues.wear(root, &wearers(&back));
        assert_eq!(shown[1..], again[1..]);
        // Another folder: given anew, as if never seen.
        let elsewhere = hues.wear(Path::new("/s"), &wearers(&more));
        assert_eq!(elsewhere, Hues::default().wear(root, &wearers(&more)));
    }

    fn build(tree: &Node, show_hidden: bool, levels: usize) -> (Vec<TreemapItem>, Vec<Block>) {
        build_in(tree, show_hidden, levels, Mode::Size)
    }

    /// A thousand days after 1970 began.
    const NOW: i64 = 1000 * 86_400;

    fn now() -> jiff::Zoned {
        jiff::Timestamp::from_second(NOW)
            .unwrap()
            .to_zoned(jiff::tz::TimeZone::UTC)
    }

    fn build_in(
        tree: &Node,
        show_hidden: bool,
        levels: usize,
        mode: Mode,
    ) -> (Vec<TreemapItem>, Vec<Block>) {
        let theme = Theme::light();
        let mut build = Build {
            theme: &theme,
            show_hidden,
            levels,
            scanning: false,
            mode,
            ages: age_limits(&now()),
            zoned: &now(),
            root: Path::new("/shown"),
            tree: &tree.path,
            selected: &|_| false,
            hues: &mut Hues::default(),
            blocks: Vec::new(),
            lent: Vec::new(),
        };
        let items = build.level(tree, 0, None);
        (items, build.blocks)
    }

    fn labels(items: &[TreemapItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_ref()).collect()
    }

    /// A folder holding two pictures, a link and an entry the scan could
    /// not say the kind of, beside a file: the map of a folder shown.
    fn holding() -> Node {
        let file = |path: &str, alloc: u64, kind: Option<EntryKind>| Node {
            entry_kind: kind,
            ..node(path, alloc, false, Vec::new())
        };
        node(
            "/tree",
            920,
            true,
            vec![
                node(
                    "/tree/l'été photos",
                    610,
                    true,
                    vec![
                        file("/tree/l'été photos/a.jpg", 400, Some(EntryKind::File)),
                        file("/tree/l'été photos/b.jpg", 150, Some(EntryKind::File)),
                        file(
                            "/tree/l'été photos/it's a link",
                            50,
                            Some(EntryKind::Symlink),
                        ),
                        file("/tree/l'été photos/odd", 10, None),
                    ],
                ),
                file("/tree/notes.txt", 300, Some(EntryKind::File)),
            ],
        )
    }

    /// The map of `tree` with `levels`, as the view keeps it.
    fn built(tree: &Node, levels: u32) -> Built {
        let theme = Theme::light();
        let zoned = now();
        let mut hues = Hues::default();
        let mut build = Build {
            theme: &theme,
            show_hidden: false,
            levels: levels as usize,
            scanning: false,
            mode: Mode::Size,
            ages: age_limits(&zoned),
            zoned: &zoned,
            root: Path::new("/shown"),
            tree: &tree.path,
            selected: &|_| false,
            hues: &mut hues,
            blocks: Vec::new(),
            lent: Vec::new(),
        };
        let items = build.level(tree, 0, None);
        let (blocks, lent) = (build.blocks, build.lent);
        Built {
            from: BuiltFrom {
                root: None,
                revision: None,
                mode: Mode::Size,
                levels,
                show_hidden: false,
                selection: None,
                minute: 0,
                theme,
            },
            items: Rc::new(items),
            keys: keys_of(&blocks),
            blocks: Rc::new(blocks),
            lent,
        }
    }

    fn id_of(tree: &Node, path: &str) -> EntryId {
        fn find<'a>(node: &'a Node, path: &str) -> Option<&'a Node> {
            if node.path == Path::new(path) {
                return Some(node);
            }
            node.children.iter().find_map(|c| find(c, path))
        }
        find(tree, path).and_then(|n| n.id).expect("in the tree")
    }

    /// A block drawn inside the folder's own entries is its own entry,
    /// lent to the pane as what the scan says it is, by the path the pane
    /// names it by; what the scan could not type is not lent, and what is
    /// not drawn (fewer levels) is not either.
    #[test]
    fn entries_drawn_inside_are_their_own_and_lent() {
        let tree = holding();
        let map = built(&tree, 2);
        let photos = map.keys[&id_of(&tree, "/tree/l'été photos")];
        let a = map.keys[&id_of(&tree, "/tree/l'été photos/a.jpg")];
        assert_eq!(map.blocks[a].parent, Some(photos));
        assert_eq!(map.blocks[photos].parent, None);
        let lent: Vec<(String, EntryKind, PathBuf)> = map
            .lent
            .iter()
            .map(|(e, path)| (e.name.to_string_lossy().into_owned(), e.kind, path.clone()))
            .collect();
        assert_eq!(
            lent,
            [
                (
                    "a.jpg".into(),
                    EntryKind::File,
                    PathBuf::from("/shown/l'été photos/a.jpg")
                ),
                (
                    "b.jpg".into(),
                    EntryKind::File,
                    PathBuf::from("/shown/l'été photos/b.jpg")
                ),
                (
                    "it's a link".into(),
                    EntryKind::Symlink,
                    PathBuf::from("/shown/l'été photos/it's a link")
                ),
            ]
        );
        assert!(
            !map.keys
                .contains_key(&id_of(&tree, "/tree/l'été photos/odd"))
        );
        assert!(built(&tree, 1).lent.is_empty(), "nothing drawn inside");
    }

    /// The footer weighs a selection at any depth: what lies in a folder
    /// selected counts with it, not again; an entry not drawn leaves the
    /// weight unknown.
    #[test]
    fn a_selection_inside_folders_is_weighed_once() {
        let tree = holding();
        let map = built(&tree, 2);
        let mut selection = skwir_core::workspace::Selection::default();
        let id = |path: &str| id_of(&tree, path);
        selection.set([
            id("/tree/l'été photos/a.jpg"),
            id("/tree/l'été photos/b.jpg"),
        ]);
        assert_eq!(weigh(&map, &selection), Some(550));
        selection.set([id("/tree/l'été photos"), id("/tree/l'été photos/a.jpg")]);
        assert_eq!(weigh(&map, &selection), Some(610));
        selection.set([id("/tree/notes.txt"), id("/tree/l'été photos/odd")]);
        assert_eq!(weigh(&map, &selection), None);
    }

    /// The arrows go among the blocks beside the cursor's in its folder,
    /// past those that are no entry, then beside that folder; at the map's
    /// edge the cursor stays; on the folder's own entries the pane's order
    /// says.
    #[test]
    fn the_arrows_go_beside_inside_a_folder_then_out_of_it() {
        let tree = holding();
        let map = built(&tree, 2);
        let id = |path: &str| id_of(&tree, path);
        let key = |path| map.keys[&id(path)];
        let odd = map
            .blocks
            .iter()
            .position(|b| b.part == Part::Entry(PathBuf::from("/tree/l'été photos/odd")))
            .unwrap();
        let laid = vec![
            (
                key("/tree/l'été photos"),
                crate::treemap::Rect::new(0., 0., 100., 100.),
            ),
            (
                key("/tree/l'été photos/a.jpg"),
                crate::treemap::Rect::new(0., 22., 40., 78.),
            ),
            (odd, crate::treemap::Rect::new(40., 22., 10., 78.)),
            (
                key("/tree/l'été photos/b.jpg"),
                crate::treemap::Rect::new(50., 22., 25., 78.),
            ),
            (
                key("/tree/l'été photos/it's a link"),
                crate::treemap::Rect::new(75., 22., 25., 78.),
            ),
            (
                key("/tree/notes.txt"),
                crate::treemap::Rect::new(100., 0., 100., 100.),
            ),
        ];
        let view = UsageView {
            laid: Rc::new(std::cell::RefCell::new(laid)),
            built: Some(map),
            ..UsageView::default()
        };
        let to = |from: &str, toward| match view.beside(Some(id(from)), toward) {
            Beside::To(id) => Some(id),
            Beside::Edge => None,
            Beside::Own => panic!("{from} is the folder's own"),
        };
        assert_eq!(
            to("/tree/l'été photos/a.jpg", Toward::Right),
            Some(id("/tree/l'été photos/b.jpg"))
        );
        assert_eq!(
            to("/tree/l'été photos/it's a link", Toward::Right),
            Some(id("/tree/notes.txt"))
        );
        assert_eq!(to("/tree/l'été photos/a.jpg", Toward::Left), None);
        assert!(matches!(
            view.beside(Some(id("/tree/notes.txt")), Toward::Left),
            Beside::Own
        ));
    }

    #[test]
    fn every_level_draws_its_whole_folder() {
        let tree = Node {
            other_count: 3,
            other_alloc: 50,
            other_size: 50,
            ..node(
                "/r",
                1000,
                true,
                vec![
                    node(
                        "/r/Videos",
                        500,
                        true,
                        vec![
                            Node {
                                entry_kind: Some(EntryKind::File),
                                ..node("/r/Videos/a.mkv", 400, false, vec![])
                            },
                            node("/r/Videos/.thumbs", 100, true, vec![]),
                        ],
                    ),
                    node("/r/.cache", 300, true, vec![]),
                    node("/r/notes", 150, false, vec![]),
                ],
            )
        };
        let (items, blocks) = build(&tree, false, 2);
        assert_eq!(
            labels(&items),
            ["Videos", "notes", "1 hidden item", "3 other items"]
        );
        // The hidden entries keep their share of the folder.
        assert!((items[2].weight - 300.).abs() < 1e-9);
        assert_eq!(labels(&items[0].children), ["a.mkv", "1 hidden item"]);
        let mkv = blocks
            .iter()
            .find(|b| b.part == Part::Entry("/r/Videos/a.mkv".into()))
            .unwrap();
        // Named as the pane names the folder, and its own entry (lent to
        // the pane while drawn, ADR-116).
        assert_eq!(mkv.path, Path::new("/shown/Videos/a.mkv"));
        assert_eq!(
            mkv.entry,
            Some(EntryId {
                file: skwir_core::FileId::new(1, 400),
                at: 0
            })
        );
        assert_eq!(
            mkv.hint,
            "Videos › a.mkv · 400 B · 80% of Videos · double-click to go there"
        );
        let thumbs = &blocks[items[0].children[1].key];
        assert_eq!(thumbs.part, Part::Hidden("/r/Videos".into()));
        assert!(thumbs.hint.starts_with("1 hidden item in Videos · 100 B"));

        let (items, _) = build(&tree, true, 1);
        assert_eq!(
            labels(&items),
            ["Videos", ".cache", "notes", "3 other items"]
        );
        assert!(items[0].children.is_empty(), "one level drawn");
    }

    /// What deleting frees is said where names outside make it less than
    /// what is taken, and a file of several names says so (ADR-049).
    #[test]
    fn what_deleting_frees_is_said_when_it_is_less() {
        let folder = Node {
            is_dir: true,
            alloc: 10_000,
            shared: 4096,
            ..Node::default()
        };
        assert_eq!(
            names_said(&folder),
            Some(format!("{} freed if deleted", format::size(5904)))
        );
        let alone = Node {
            is_dir: true,
            alloc: 10_000,
            ..Node::default()
        };
        assert_eq!(names_said(&alone), None);
        let file = Node {
            nlink: 3,
            ..Node::default()
        };
        assert_eq!(
            names_said(&file).as_deref(),
            Some("one of 3 names of its file")
        );
    }

    #[test]
    fn a_size_is_said_once_unless_the_disk_holds_another() {
        assert_eq!(sizes(2048, 2048), "2.0 KB");
        assert_eq!(sizes(10_000_000, 4096), "9.5 MB · 4.0 KB on disk");
        assert_eq!(share(1, 1000), "<1%");
        assert_eq!(share(0, 1000), "0%");
        assert_eq!(share(500, 1000), "50%");
    }

    #[test]
    fn files_size_the_blocks_by_what_they_hold() {
        let tree = Node {
            files: 1003,
            other_count: 2,
            other_alloc: 10,
            other_files: 2,
            ..node(
                "/r",
                1000,
                true,
                vec![
                    Node {
                        files: 1000,
                        ..node("/r/node_modules", 10, true, vec![])
                    },
                    Node {
                        files: 1,
                        ..node("/r/film.mkv", 980, false, vec![])
                    },
                ],
            )
        };
        let (items, blocks) = build_in(&tree, true, 1, Mode::Files);
        assert_eq!(
            labels(&items),
            ["node_modules", "film.mkv", "2 other items"]
        );
        assert!((items[0].weight - 1000.).abs() < 1e-9);
        // At least a two-hundredth of the folder, to be pointed at.
        assert!((items[1].weight - 1003. / 200.).abs() < 1e-9);
        assert_eq!(items[0].detail.as_ref(), "1,000 files");
        assert_eq!(items[2].detail.as_ref(), "2 files");
        assert_eq!(
            blocks[0].hint,
            "node_modules · 1,000 files · 100% of the files in r"
        );
    }

    #[test]
    fn age_and_kinds_color_the_blocks_and_their_legend_says_how() {
        const DAY: i64 = 86_400;
        let limits = age_limits(&now());
        assert_eq!(age(NOW - 3600, &limits), 0);
        assert_eq!(age(NOW - 3 * DAY, &limits), 1);
        assert_eq!(age(NOW - 40 * DAY, &limits), 3);
        assert_eq!(age(NOW - 400 * DAY, &limits), AGES - 1);
        assert_eq!(age(0, &limits), AGES - 1, "unknown is drawn as old");
        assert_eq!(age(NOW + 60, &limits), 0, "a clock ahead is new");
        // A month is the calendar's: thirty days before 15 March is
        // older than a month (February is short), not within 31 days.
        let march: jiff::Zoned = "2026-03-15T12:00[UTC]".parse().unwrap();
        let limits = age_limits(&march);
        let then = march.timestamp().as_second();
        assert_eq!(age(then - 30 * DAY, &limits), 3);
        assert_eq!(age(then - 27 * DAY, &limits), 2);
        // A day across a clock change is the calendar's day: 23 hours in
        // Paris the night summer time begins, and 23.5 hours is older.
        let after: jiff::Zoned = "2026-03-29T12:00[Europe/Paris]".parse().unwrap();
        let limits = age_limits(&after);
        let then = after.timestamp().as_second();
        assert_eq!(age(then - 23 * 3600 - 1800, &limits), 1);
        assert_eq!(age(then - 22 * 3600, &limits), 0);

        let film = Node {
            kinds: [
                ("video".to_owned(), 620),
                ("image".to_owned(), 300),
                ("other".to_owned(), 80),
            ]
            .into(),
            modified: NOW - 3600,
            ..node("/r/film", 1000, true, vec![])
        };
        assert_eq!(kinds_said(&film), "Videos 62% · Images 30%");
        let tree = node("/r", 1000, true, vec![film]);
        let theme = Theme::light();
        let (items, blocks) = build_in(&tree, true, 1, Mode::Types);
        assert_eq!(
            items[0].color,
            theme.block_kind(Some(Kind::Video), 0.62, 0).closed
        );
        assert!(blocks[0].hint.ends_with("Videos 62% · Images 30%"));
        let (items, blocks) = build_in(&tree, true, 1, Mode::Age);
        assert_eq!(items[0].color, theme.block_age(0, 0).closed);
        assert!(blocks[0].hint.contains(" · changed "), "{}", blocks[0].hint);
    }

    #[test]
    fn room_to_have_back_is_lined_and_said() {
        let tree = Node {
            reclaimable: 300,
            ..node(
                "/r",
                1000,
                true,
                vec![
                    Node {
                        reclaim: Some(Reclaim::Cache),
                        reclaimable: 300,
                        ..node("/r/target", 300, true, vec![])
                    },
                    Node {
                        reclaimable: 0,
                        ..node("/r/src", 700, true, vec![])
                    },
                ],
            )
        };
        let (items, blocks) = build(&tree, true, 1);
        assert!(items[0].dashed && !items[1].dashed);
        assert_eq!(items[0].detail.as_ref(), "300 B · cache");
        assert!(
            blocks[0]
                .hint
                .ends_with("a cache, which its program makes again when needed"),
            "{}",
            blocks[0].hint
        );
        assert_eq!(
            reclaim_said(&tree, None).as_deref(),
            Some("300 B of it in caches and the Trash")
        );
        assert_eq!(
            reclaim_said(&tree.children[1], Some(Reclaim::Trash)).as_deref(),
            Some("in the Trash: emptying it frees this")
        );
    }
}
