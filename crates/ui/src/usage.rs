//! Plugin-controlled Usage pane view. Filesystem work remains in the plugin.
//!
//! The map draws the folder's own entries, and inside each folder what it
//! holds, as many levels as the zoom asks and the blocks have room for
//! (`elements::treemap`). The folder's own entries are the pane's: a
//! block inside one stands for its outer entry, which a click selects and
//! the menu acts on, and a double click goes to the block itself. Every
//! level draws its whole folder: the entries too small to draw and, while
//! hidden files are not shown, the hidden ones each take a block of their
//! own, so no block looks larger than it is.
//!
//! What a block's size and color say is the map's mode (`Mode`, the
//! window's): the room it takes in its folder's hue, the files it holds,
//! when it last changed, or the kind of files that fill it. A block whose
//! room can be had back as a whole (a cache, a trash) is lined, in every
//! mode.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use gpui::{AnyElement, Context, MouseButton, Window, div, prelude::*, px};
use serde::{Deserialize, Serialize};
use skwir_core::{EntryId, command::Invocation, os_path::PathArg};
use skwir_platform::mime::Kind;

use crate::elements::treemap::{Laidout, TreemapItem, TreemapStyle, treemap};
use crate::elements::zoom_wheel::{self, Zoomed};
use crate::treemap::{Toward, neighbour};
use crate::{
    format,
    shell::Shell,
    theme::{AGES, Theme},
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
    /// The folder's own entry it lies in, itself at the top: what a click
    /// selects.
    entry: Option<EntryId>,
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

/// How long ago `modified` was, as `Theme::block_age` steps: within a
/// day, a week, a month, a year, or longer (or unknown).
fn age(modified: i64, now: i64) -> usize {
    const DAY: i64 = 86_400;
    if modified <= 0 {
        return AGES - 1;
    }
    let elapsed = now - modified;
    [DAY, 7 * DAY, 31 * DAY, 365 * DAY]
        .iter()
        .position(|limit| elapsed < *limit)
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

/// The blocks of a breakdown, and what each stands for.
struct Build<'a> {
    theme: &'a Theme,
    show_hidden: bool,
    levels: usize,
    scanning: bool,
    mode: Mode,
    /// Now, in seconds since 1970, and as the user's calendar says it.
    now: i64,
    zoned: &'a jiff::Zoned,
    /// The folder as the pane names it, and as the plugin does (it counts
    /// by canonical path): a block's path is read from one into the other.
    root: &'a Path,
    tree: &'a Path,
    selected: &'a dyn Fn(EntryId) -> bool,
    blocks: Vec<Block>,
}

/// What a level inherits from the entry of the folder shown it lies in.
struct Outer {
    entry: Option<EntryId>,
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

    /// The fill of a block: its outer entry's hue, its last change, or
    /// the kind of what fills it.
    fn color(&self, node: &Node, hue: usize, depth: usize) -> gpui::Hsla {
        match self.mode {
            Mode::Size | Mode::Files => self.theme.block(hue, node.is_dir, depth),
            Mode::Age => self.theme.block_age(age(node.modified, self.now), depth),
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
        for (index, child) in node.children.iter().enumerate() {
            if !self.show_hidden && child.name.starts_with('.') {
                hidden.add(child);
                continue;
            }
            let (entry, hue, trail) = match outer {
                Some(outer) => (
                    outer.entry,
                    outer.hue,
                    format!("{} › {}", outer.trail, child.name),
                ),
                // Colored by identity, so a block keeps its color as the map reflows.
                None => (
                    child.id,
                    child.id.map_or(index, |id| (id.file.ino % 7) as usize),
                    child.name.clone(),
                ),
            };
            let key = self.blocks.len();
            self.blocks.push(Block {
                part: Part::Entry(child.path.clone()),
                depth,
                entry,
                path: self.shown(&child.path),
                hint: self.hint(child, &trail, node, outer),
            });
            let children = if child.is_dir && depth + 1 < self.levels {
                let outer = Outer {
                    entry,
                    hue,
                    trail,
                    within: child.reclaim.or(outer.and_then(|o| o.within)),
                };
                self.level(child, depth + 1, Some(&outer))
            } else {
                Vec::new()
            };
            items.push(TreemapItem {
                key,
                weight: (child.measure(self.mode) as f64).max(floor),
                label: child.name.clone().into(),
                detail: self.detail(child).into(),
                color: self.color(child, hue, depth),
                text: self.theme.block_text,
                hatched: child.incomplete,
                dashed: child.reclaim.is_some(),
                selected: depth == 0 && entry.is_some_and(|id| (self.selected)(id)),
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
            text: self.theme.text,
            hatched: incomplete,
            dashed: false,
            selected: false,
            children: Vec::new(),
        }
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
}

/// The blocks of the map, built from `from`.
struct Built {
    from: BuiltFrom,
    items: Rc<Vec<TreemapItem>>,
    blocks: Rc<Vec<Block>>,
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
    fn zooms(&self) -> Zooms {
        Zooms::Levels
    }
    /// Its folders measured: the footer weighs them too, once counted.
    fn weighs(&self, shell: &Shell) -> Option<Weighs> {
        let status = shell.ui().usage.status.as_ref()?;
        let Some(tree) = status.tree.as_ref().filter(|tree| !tree.incomplete) else {
            return Some(Weighs::default());
        };
        // A file of several names selected under each counts once; two
        // folders sharing one each count it, as each holds it.
        let selected = shell.app.workspace.read().pane(shell.pane).and_then(|p| {
            let mut files = std::collections::HashSet::new();
            p.selection.iter().try_fold(0, |sum, id| {
                let child = tree.children.iter().find(|c| c.id == Some(id))?;
                let once = child.is_dir || files.insert(id.file);
                Some(sum + if once { child.size } else { 0 })
            })
        });
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
    let font = shell.font.clone();
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
    let state = &shell.ui().usage;
    let from = BuiltFrom {
        root: state.root.clone(),
        revision: state.revision,
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
    if view.built.as_ref().is_none_or(|built| built.from != from) {
        let status = state.status.as_ref();
        let mut items = Vec::new();
        let mut blocks = Vec::new();
        if let (Some(node), Some(root)) =
            (status.and_then(|s| s.tree.as_ref()), state.root.as_deref())
        {
            let ws = shell.app.workspace.read();
            let p = ws.pane(pane);
            let selected = |id| p.is_some_and(|p| p.selection.contains(id));
            let mut build = Build {
                theme: &theme,
                show_hidden,
                levels: shell.levels as usize,
                scanning: status.is_some_and(|s| s.scanning),
                mode,
                now: zoned.timestamp().as_second(),
                zoned: &zoned,
                root,
                tree: &node.path,
                selected: &selected,
                blocks: Vec::new(),
            };
            items = build.level(node, 0, None);
            blocks = build.blocks;
        }
        view.built = Some(Built {
            from,
            items: Rc::new(items),
            blocks: Rc::new(blocks),
        });
    }
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
                .child(zoom_wheel::overlay(Zoomed::View, cx)),
        )
        .children(legend)
        .child(crate::elements::clip::clipped(
            div()
                .id("usage-hint")
                .h(px(24.))
                .px_2()
                .text_xs()
                .text_color(theme.text_muted)
                .truncate(),
            hint,
            font,
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
                            .text_xs()
                            .truncate(),
                        summary.to_owned(),
                        shell.font.clone(),
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
                    .text_xs()
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
            .map(|step| (theme.block_age(step, 0), AGE_NAMES[step].to_owned()))
            .collect(),
        Mode::Types => {
            let tree = shell.ui().usage.status.as_ref()?.tree.as_ref()?;
            tree.kinds()
                .into_iter()
                .map(|(kind, bytes)| {
                    (
                        theme.block_kind(kind, 1., 0),
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
            .h(px(24.))
            .flex()
            .items_center()
            .gap(px(4.))
            .child(div().size(px(10.)).rounded_sm().bg(color))
            .child(label)
    };
    let lead = (mode == Mode::Age).then(|| {
        div()
            .flex_none()
            .h(px(24.))
            .flex()
            .items_center()
            .child("Changed within")
    });
    Some(
        div()
            .w_full()
            .h(px(24.))
            .px_2()
            .overflow_hidden()
            .flex()
            .flex_wrap()
            .gap_x(px(12.))
            .text_xs()
            .text_color(theme.text_muted)
            .children(lead)
            .children(entries.into_iter().map(|(color, label)| item(color, label))),
    )
}

/// A press on a block. One of the folder's own entries is pressed as a
/// row is; a block inside one presses its outer entry (selects it, opens
/// its menu), and a double click or the middle button goes to the block
/// itself. A block standing for several entries shows them.
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
                file: skwir_core::FileId { dev: 1, ino: alloc },
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

    fn build(tree: &Node, show_hidden: bool, levels: usize) -> (Vec<TreemapItem>, Vec<Block>) {
        build_in(tree, show_hidden, levels, Mode::Size)
    }

    /// A thousand days after 1970 began.
    const NOW: i64 = 1000 * 86_400;

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
            now: NOW,
            zoned: &jiff::Zoned::now(),
            root: Path::new("/shown"),
            tree: &tree.path,
            selected: &|_| false,
            blocks: Vec::new(),
        };
        let items = build.level(tree, 0, None);
        (items, build.blocks)
    }

    fn labels(items: &[TreemapItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_ref()).collect()
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
                            node("/r/Videos/a.mkv", 400, false, vec![]),
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
        // Named as the pane names the folder, and standing for its outer entry.
        assert_eq!(mkv.path, Path::new("/shown/Videos/a.mkv"));
        assert_eq!(
            mkv.entry,
            Some(EntryId {
                file: skwir_core::FileId { dev: 1, ino: 500 },
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
        assert_eq!(age(NOW - 3600, NOW), 0);
        assert_eq!(age(NOW - 3 * DAY, NOW), 1);
        assert_eq!(age(NOW - 40 * DAY, NOW), 3);
        assert_eq!(age(NOW - 400 * DAY, NOW), AGES - 1);
        assert_eq!(age(0, NOW), AGES - 1, "unknown is drawn as old");
        assert_eq!(age(NOW + 60, NOW), 0, "a clock ahead is new");

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
        assert_eq!(items[0].color, theme.block_kind(Some(Kind::Video), 0.62, 0));
        assert!(blocks[0].hint.ends_with("Videos 62% · Images 30%"));
        let (items, blocks) = build_in(&tree, true, 1, Mode::Age);
        assert_eq!(items[0].color, theme.block_age(0, 0));
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
