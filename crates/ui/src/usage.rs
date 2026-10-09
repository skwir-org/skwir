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
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bibli_core::{EntryId, command::Invocation, os_path::PathArg};
use gpui::{AnyElement, Context, MouseButton, Window, div, prelude::*, px};
use serde::Deserialize;

use crate::elements::treemap::{Laidout, TreemapItem, TreemapStyle, treemap};
use crate::treemap::{Toward, neighbour};
use crate::{
    format,
    shell::Shell,
    theme::Theme,
    toolbar::{self, Button},
    view::{Nav, PaneView, Weighs, Zooms},
};

/// Levels drawn (`Shell::levels`): the folder's own entries, and inside
/// them as many more as the plugin sends (its `NESTED_LEVELS`).
pub const LEVELS: [u32; 4] = [1, 2, 3, 4];
pub const MAX_LEVELS: u32 = LEVELS[LEVELS.len() - 1];
pub const DEFAULT_LEVELS: u32 = 3;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Node {
    pub id: Option<EntryId>,
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub incomplete: bool,
    pub size: u64,
    pub alloc: u64,
    pub children: Vec<Node>,
    pub other_alloc: u64,
    #[serde(default)]
    pub other_size: u64,
    pub other_count: u64,
    pub other_incomplete: u64,
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
    location_lifetime: Option<bibli_core::CancelToken>,
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
            .and_then(|p| p.location().and_then(bibli_core::Location::path));
        if self.ui().usage.root != dir
            || self
                .ui()
                .usage
                .location_lifetime
                .as_ref()
                .is_some_and(bibli_core::CancelToken::is_cancelled)
        {
            let location_lifetime = self
                .app
                .workspace
                .read()
                .pane(self.pane)
                .map(bibli_core::workspace::Pane::location_lifetime);
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

/// The blocks of a breakdown, and what each stands for.
struct Build<'a> {
    theme: &'a Theme,
    show_hidden: bool,
    levels: usize,
    scanning: bool,
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
}

impl Build<'_> {
    fn shown(&self, path: &Path) -> PathBuf {
        path.strip_prefix(self.tree)
            .map_or_else(|_| path.to_path_buf(), |inside| self.root.join(inside))
    }

    /// The blocks of what `node` holds, `depth` folders inside the one shown.
    fn level(&mut self, node: &Node, depth: usize, outer: Option<&Outer>) -> Vec<TreemapItem> {
        // Each block is drawn at least this large, so each can be pointed at.
        let floor = (node.alloc as f64 / 200.).max(1.);
        let within = |what: String| match outer {
            Some(outer) => format!("{what} in {}", outer.trail),
            None => what,
        };
        let mut items = Vec::new();
        let mut hidden = (0, 0, 0usize, false);
        for (index, child) in node.children.iter().enumerate() {
            if !self.show_hidden && child.name.starts_with('.') {
                hidden.0 += child.size;
                hidden.1 += child.alloc;
                hidden.2 += 1;
                hidden.3 |= child.incomplete;
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
                    child.id.map_or(index, |id| (id.ino % 7) as usize),
                    child.name.clone(),
                ),
            };
            let counted = if child.incomplete {
                format!("counted so far: {}", sizes(child.size, child.alloc))
            } else {
                sizes(child.size, child.alloc)
            };
            let mut hint = format!(
                "{trail} · {counted} · {} of {}",
                share(child.alloc, node.alloc),
                node.name
            );
            if depth > 0 {
                hint.push_str(" · double-click to go there");
            }
            let key = self.blocks.len();
            self.blocks.push(Block {
                part: Part::Entry(child.path.clone()),
                depth,
                entry,
                path: self.shown(&child.path),
                hint,
            });
            let children = if child.is_dir && depth + 1 < self.levels {
                let outer = Outer { entry, hue, trail };
                self.level(child, depth + 1, Some(&outer))
            } else {
                Vec::new()
            };
            let detail = match (child.incomplete, child.alloc) {
                (false, alloc) => format::size(alloc),
                (true, 0) if self.scanning => "counting…".into(),
                (true, 0) => "incomplete".into(),
                (true, alloc) => format!("≥ {}", format::size(alloc)),
            };
            items.push(TreemapItem {
                key,
                weight: (child.alloc as f64).max(floor),
                label: child.name.clone().into(),
                detail: detail.into(),
                color: self.theme.block(hue, child.is_dir, depth),
                text: self.theme.on_fill,
                hatched: child.incomplete,
                selected: depth == 0 && entry.is_some_and(|id| (self.selected)(id)),
                children,
            });
        }
        let (size, alloc, count, incomplete) = hidden;
        if count > 0 {
            let what = format::count(count, "hidden item");
            let key = self.blocks.len();
            self.blocks.push(Block {
                part: Part::Hidden(node.path.clone()),
                depth,
                entry: None,
                path: self.shown(&node.path),
                hint: format!(
                    "{} · {} · double-click to show hidden files",
                    within(what.clone()),
                    sizes(size, alloc)
                ),
            });
            items.push(self.rest(key, what, alloc, floor, incomplete));
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
                    sizes(node.other_size, node.other_alloc),
                    if depth == 0 {
                        "double-click to see all items in List"
                    } else {
                        "double-click to go there"
                    }
                ),
            });
            items.push(self.rest(
                key,
                format::count(count, "other item"),
                node.other_alloc,
                floor,
                node.other_incomplete > 0,
            ));
        }
        items
    }

    /// A block standing for several entries.
    fn rest(
        &self,
        key: usize,
        what: String,
        alloc: u64,
        floor: f64,
        incomplete: bool,
    ) -> TreemapItem {
        TreemapItem {
            key,
            weight: (alloc as f64).max(floor),
            label: what.into(),
            detail: format::size(alloc).into(),
            color: self.theme.block_rest,
            text: self.theme.text,
            hatched: incomplete,
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
        let selected = shell.app.workspace.read().pane(shell.pane).and_then(|p| {
            p.selection.iter().try_fold(0, |sum, id| {
                let child = tree.children.iter().find(|c| c.id == Some(id))?;
                Some(sum + child.size)
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
    let state = &shell.ui().usage;
    let status = state.status.as_ref();
    let mut header = toolbar::row(&theme)
        .text_color(theme.text)
        .id(("usage-tools", pane.0 as usize));
    header = header.child(crate::elements::clip::clipped(
        div()
            .id("usage-summary")
            .flex_1()
            .min_w_0()
            .text_xs()
            .truncate(),
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
                format!(
                    "{}{} · {} · {}{}",
                    if s.tree.as_ref().is_some_and(|n| n.incomplete) {
                        "Counted so far: "
                    } else {
                        ""
                    },
                    sizes(s.size, s.alloc),
                    format::count(usize::try_from(s.files).unwrap_or(usize::MAX), "file"),
                    format::count(usize::try_from(s.dirs).unwrap_or(usize::MAX), "folder"),
                    if s.updating { " · updating…" } else { "" }
                )
            },
        ),
        font.clone(),
    ));
    header = header.child(
        Button::new(
            format!("usage-refresh-{}", pane.0),
            "refresh",
            "Recalculate sizes",
            "ui.usage_refresh",
        )
        .enabled(state.root.is_some())
        .render(shell, cx),
    );
    let state = &shell.ui().usage;
    let status = state.status.as_ref();
    let mut items = Vec::new();
    let mut blocks = Vec::new();
    if let (Some(node), Some(root)) = (status.and_then(|s| s.tree.as_ref()), state.root.as_deref())
    {
        // Read before the lock below: a second read waits behind a writer.
        let show_hidden = shell.sort_spec().show_hidden;
        let ws = shell.app.workspace.read();
        let p = ws.pane(pane);
        let selected = |id| p.is_some_and(|p| p.selection.contains(id));
        let mut build = Build {
            theme: &theme,
            show_hidden,
            levels: shell.levels as usize,
            scanning: status.is_some_and(|s| s.scanning),
            root,
            tree: &node.path,
            selected: &selected,
            blocks: Vec::new(),
        };
        items = build.level(node, 0, None);
        blocks = build.blocks;
    }
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
    let hint = hover.and_then(|key| blocks.get(key)).map_or_else(
        || {
            status.and_then(|s| s.error.clone()).unwrap_or_else(|| {
                if status.is_some_and(|s| s.scanning && s.tree.as_ref().is_some_and(|n| n.incomplete)) {
                    "Striped blocks are still being counted · proportions will change as sizes arrive".into()
                } else if status.is_some_and(|s| s.updating) {
                    format!("Scanning… {} folders read", status.map_or(0, |s| s.scanned_dirs))
                } else {
                    "Click to select · double-click to open · Ctrl and the wheel for more or fewer levels".into()
                }
            })
        },
        |b| b.hint.clone(),
    );
    let blocks = Rc::new(blocks);
    let hovered = Rc::clone(&blocks);
    let entity = cx.entity();
    let entity2 = entity.clone();
    let root = state.root.clone();
    let style = TreemapStyle {
        gap: theme.bg,
        hover: theme.block_hover,
        selected: theme.block_selected,
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
        .text_color(theme.on_fill)
        .child(header)
        .child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(map)
                .child(crate::views::zoom_wheel(cx)),
        )
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
            if let Some(ix) = block.entry.and_then(|id| shell.ui().list.index_of(id)) {
                shell.on_entry_mouse_down(pane, ix, event, window, cx);
            }
        }
        Part::Hidden(_) if double => {
            shell.run("ui.toggle_hidden", serde_json::json!({}), window, cx);
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
            id: Some(EntryId { dev: 1, ino: alloc }),
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
        let theme = Theme::light();
        let mut build = Build {
            theme: &theme,
            show_hidden,
            levels,
            scanning: false,
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
        assert_eq!(mkv.entry, Some(EntryId { dev: 1, ino: 500 }));
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

    #[test]
    fn a_size_is_said_once_unless_the_disk_holds_another() {
        assert_eq!(sizes(2048, 2048), "2.0 KB");
        assert_eq!(sizes(10_000_000, 4096), "9.5 MB · 4.0 KB on disk");
        assert_eq!(share(1, 1000), "<1%");
        assert_eq!(share(0, 1000), "0%");
        assert_eq!(share(500, 1000), "50%");
    }
}
