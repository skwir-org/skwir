//! The Tags view, a lens: the tags of the files under the folder the pane
//! shows, as the tags plugin counts them (`tags.lens`, `tags.lens_status`:
//! the index's counts at once, the walk's when it ends). A row opens its
//! tag's search; the selection (a click, Ctrl and Shift as in the file
//! views) is what the menu, Enter, F2 and Delete act on: pin, rename or
//! merge, color, remove from every file under the folder. Files dropped
//! on a row get its tags (the selection's when the row is in it), and a
//! row dragged onto files gives them the same.
//!
//! Tags in a category are a tree under it (`Places/France/Paris`, the
//! category counting each file within it once): a category folds and
//! unfolds (a click on its arrow, Left and Right), and tags dragged onto a
//! row go into it as their category, dropped beside the rows out of theirs
//! (`tags.move`, a plan to review).
//!
//! Tags are not entries, so the pane's own selection stays out of it: the
//! keys that would act on files act on the tags (`PaneView::command`).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bibli_core::command::Invocation;
use bibli_core::metadata::{TAG_COLORS, same_tag, tag_leaf, tag_parent, tag_within};

/// How far a tag in a category is indented, per category.
const INDENT: f32 = 16.;
use bibli_core::os_path::PathArg;
use bibli_core::registry::MenuItem;
use gpui::{
    AnyElement, Context, ExternalPaths, MouseButton, ScrollHandle, SharedString, Window, div,
    prelude::*, px,
};
use serde::Deserialize;

use crate::dnd::DragPayload;
use crate::format;
use crate::shell::Shell;
use crate::sidebar::PlaceDrag;
use crate::theme::Theme;
use crate::toolbar::{self, Button};
use crate::view::{Nav, PaneView};

pub const ID: &str = "tags";
const ROW_HEIGHT: f32 = 30.;

/// A tag as the plugin counted it.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Row {
    pub tag: String,
    pub files: usize,
    pub color: String,
    pub pinned: bool,
}

/// What the plugin counted for this pane.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Status {
    pub root: Option<PathBuf>,
    pub tags: Vec<Row>,
    pub files: usize,
    pub walking: bool,
    /// Checking what changed since `counted`.
    #[serde(default)]
    pub checking: bool,
    /// When the counts held whole, in seconds since the epoch.
    #[serde(default)]
    pub counted: Option<u64>,
    /// More tagged files than the index keeps: some are left out.
    #[serde(default)]
    pub partial: bool,
    pub error: Option<String>,
}

/// One pane's Tags view.
#[derive(Default)]
pub struct TagsLens {
    pub status: Option<Status>,
    root: Option<PathBuf>,
    location_lifetime: Option<bibli_core::CancelToken>,
    /// The plugin's view revision, captured once per registration.
    source: Option<Arc<AtomicU64>>,
    revision: Option<u64>,
    /// The tags selected, by name.
    pub selected: Vec<String>,
    /// The tag the keyboard is on, where a Shift range starts.
    pub cursor: Option<String>,
    /// By how many files have them (most first), else by name.
    pub by_files: bool,
    /// The categories unfolded.
    pub open: Vec<String>,
    pub scroll: ScrollHandle,
}

/// A row as the tree shows it.
pub struct Line<'a> {
    pub row: &'a Row,
    /// How many categories it is in, shown.
    pub depth: usize,
    /// It is a category: tags are within it.
    pub category: bool,
    pub open: bool,
}

impl TagsLens {
    /// Every tag counted, categories too.
    fn all(&self) -> Vec<&Row> {
        self.status.iter().flat_map(|s| s.tags.iter()).collect()
    }

    /// The tree as shown: each tag under its category, those of a folded
    /// category left out, each category's in the order chosen.
    pub fn tree(&self) -> Vec<Line<'_>> {
        let all = self.all();
        // Each row's category's row; a tag whose category is not counted
        // (the counts are partial) stands at the top.
        let mut kids: Vec<Vec<usize>> = vec![Vec::new(); all.len()];
        let mut top = Vec::new();
        for (i, row) in all.iter().enumerate() {
            let parent =
                tag_parent(&row.tag).and_then(|p| all.iter().position(|o| same_tag(&o.tag, p)));
            match parent {
                Some(p) => kids[p].push(i),
                None => top.push(i),
            }
        }
        if self.by_files {
            for list in std::iter::once(&mut top).chain(kids.iter_mut()) {
                list.sort_by_key(|&i| std::cmp::Reverse(all[i].files));
            }
        }
        let mut out = Vec::new();
        let mut stack: Vec<(usize, usize)> = top.iter().rev().map(|&i| (i, 0)).collect();
        while let Some((i, depth)) = stack.pop() {
            let category = !kids[i].is_empty();
            let open = category && self.is_open(&all[i].tag);
            out.push(Line {
                row: all[i],
                depth,
                category,
                open,
            });
            if open {
                stack.extend(kids[i].iter().rev().map(|&k| (k, depth + 1)));
            }
        }
        out
    }

    /// The tags in the order shown.
    pub fn rows(&self) -> Vec<&Row> {
        self.tree().into_iter().map(|l| l.row).collect()
    }

    fn is_open(&self, tag: &str) -> bool {
        self.open.iter().any(|t| same_tag(t, tag))
    }

    /// Fold or unfold the category `tag`.
    fn set_open(&mut self, tag: &str, open: bool) {
        self.open.retain(|t| !same_tag(t, tag));
        if open {
            self.open.push(tag.to_owned());
        }
    }

    /// Left: fold the category the cursor is on, else go to its own: the
    /// index it is on then.
    fn left(&mut self) -> Option<usize> {
        let tree = self.tree();
        let at = self.cursor_at(&tree)?;
        let line = &tree[at];
        let tag = line.row.tag.clone();
        if line.open {
            drop(tree);
            self.set_open(&tag, false);
            return Some(at);
        }
        let parent = tag_parent(&tag)?;
        let to = tree.iter().position(|l| same_tag(&l.row.tag, parent))?;
        let to_tag = tree[to].row.tag.clone();
        drop(tree);
        self.cursor = Some(to_tag.clone());
        self.selected = vec![to_tag];
        Some(to)
    }

    /// Right: unfold the category the cursor is on, else go into it.
    fn right(&mut self) -> Option<usize> {
        let tree = self.tree();
        let at = self.cursor_at(&tree)?;
        let (tag, category, open) = (tree[at].row.tag.clone(), tree[at].category, tree[at].open);
        drop(tree);
        if !category {
            return Some(at);
        }
        if !open {
            self.set_open(&tag, true);
            return Some(at);
        }
        self.step(1)
    }

    fn cursor_at(&self, tree: &[Line<'_>]) -> Option<usize> {
        let cursor = self.cursor.as_deref()?;
        tree.iter().position(|l| same_tag(&l.row.tag, cursor))
    }

    fn is_selected(&self, tag: &str) -> bool {
        self.selected.iter().any(|t| same_tag(t, tag))
    }

    /// What acting on `tag` acts on: the selection when it holds `tag`,
    /// else `tag` alone.
    fn targets(&self, tag: &str) -> Vec<String> {
        if self.is_selected(tag) {
            self.selected.clone()
        } else {
            vec![tag.to_owned()]
        }
    }

    /// A click on `tag`: alone, toggled (`toggle`), or the range from the
    /// cursor (`range`).
    fn click(&mut self, tag: &str, toggle: bool, range: bool) {
        let order: Vec<String> = self.rows().iter().map(|r| r.tag.clone()).collect();
        let at = |t: &str| order.iter().position(|o| same_tag(o, t));
        match (range, self.cursor.as_deref().and_then(at), at(tag)) {
            (true, Some(from), Some(to)) => {
                let (a, b) = (from.min(to), from.max(to));
                let span = order[a..=b].iter().cloned();
                if toggle {
                    for t in span {
                        if !self.is_selected(&t) {
                            self.selected.push(t);
                        }
                    }
                } else {
                    self.selected = span.collect();
                }
                return;
            }
            _ if toggle => {
                if self.is_selected(tag) {
                    self.selected.retain(|t| !same_tag(t, tag));
                } else {
                    self.selected.push(tag.to_owned());
                }
            }
            _ => self.selected = vec![tag.to_owned()],
        }
        self.cursor = Some(tag.to_owned());
    }

    /// Move the cursor `delta` rows (to an end when it is past one), the
    /// selection following it: the index it lands on.
    fn step(&mut self, delta: isize) -> Option<usize> {
        let order: Vec<String> = self.rows().iter().map(|r| r.tag.clone()).collect();
        let last = order.len().checked_sub(1)?;
        let at = self
            .cursor
            .as_deref()
            .and_then(|c| order.iter().position(|o| same_tag(o, c)));
        let to = match at {
            Some(at) => at.saturating_add_signed(delta).min(last),
            None if delta < 0 => last,
            None => 0,
        };
        self.cursor = Some(order[to].clone());
        self.selected = vec![order[to].clone()];
        Some(to)
    }
}

fn color_index(name: &str) -> usize {
    TAG_COLORS.iter().position(|c| *c == name).unwrap_or(0)
}

/// `“a”`, or `3 tags`.
fn naming(tags: &[String]) -> String {
    match tags {
        [one] => format!("“{one}”"),
        _ => format!("{} tags", tags.len()),
    }
}

impl Shell {
    /// Keep the pane's Tags view counted: ask the plugin to count when its
    /// folder changes, read what it counted when its revision moves, and
    /// let go when the pane shows another view.
    pub fn sync_tags_lens(&mut self, cx: &mut Context<Self>) {
        if self.ui().view_id != ID {
            if self.ui().tags.root.is_some() {
                let _ = self.app.dispatch(&Invocation::with_args(
                    "tags.lens_release",
                    &serde_json::json!({ "pane": self.pane }),
                ));
                let (by_files, open) = (self.ui().tags.by_files, self.ui().tags.open.clone());
                self.ui_mut().tags = TagsLens {
                    by_files,
                    open,
                    ..TagsLens::default()
                };
            }
            return;
        }
        let (dir, lifetime) = {
            let ws = self.app.workspace.read();
            let p = ws.pane(self.pane);
            (
                p.and_then(|p| p.location().and_then(bibli_core::Location::path)),
                p.map(bibli_core::workspace::Pane::location_lifetime),
            )
        };
        let gone = self
            .ui()
            .tags
            .location_lifetime
            .as_ref()
            .is_some_and(bibli_core::CancelToken::is_cancelled);
        if self.ui().tags.root != dir || gone {
            let (by_files, open) = (self.ui().tags.by_files, self.ui().tags.open.clone());
            self.ui_mut().tags = TagsLens {
                root: dir.clone(),
                location_lifetime: lifetime,
                by_files,
                open,
                ..TagsLens::default()
            };
            // Tags are not entries: nothing hidden behind them stays
            // selected for a key to act on.
            self.clear_selection_for_lens();
            if dir.is_some()
                && let Err(error) = self.app.dispatch(&Invocation::with_args(
                    "tags.lens",
                    &serde_json::json!({ "pane": self.pane }),
                ))
            {
                self.set_status(format!("{error:#}"), true, cx);
            }
        }
        if dir.is_none() {
            return;
        }
        if self.ui().tags.source.is_none() {
            let source = self
                .app
                .registry
                .views
                .find(|s| &*s.id == ID)
                .map(|s| Arc::clone(&s.revision));
            self.ui_mut().tags.source = source;
        }
        let revision = self
            .ui()
            .tags
            .source
            .as_ref()
            .map(|s| s.load(Ordering::Acquire));
        if revision != self.ui().tags.revision {
            self.ui_mut().tags.revision = revision;
            if let Some(value) = self.query_value(
                "tags.lens_status",
                serde_json::json!({ "pane": self.pane }),
                cx,
            ) && let Ok(status) = serde_json::from_value::<Status>(value)
                && status.root == dir
            {
                let lens = &mut self.ui_mut().tags;
                lens.selected
                    .retain(|t| status.tags.iter().any(|r| same_tag(&r.tag, t)));
                lens.status = Some(status);
                cx.notify();
            }
        }
    }

    /// `ui.tags_refresh` (F5 too): count again.
    pub fn tags_refresh(&mut self, cx: &mut Context<Self>) {
        if self.ui().tags.root.is_none() {
            return;
        }
        if let Err(error) = self.app.dispatch(&Invocation::with_args(
            "tags.lens",
            &serde_json::json!({ "pane": self.pane, "again": true }),
        )) {
            self.set_status(format!("{error:#}"), true, cx);
        }
        cx.notify();
    }

    fn clear_selection_for_lens(&mut self) {
        let mut ws = self.app.workspace.write();
        if let Some(p) = ws.pane_mut(self.pane) {
            p.selection.clear();
        }
    }

    /// `ui.tags_open`: show the files tagged `tag` under the view's folder,
    /// in the listing the pane showed before (`tab`: in a tab behind).
    pub fn tags_open(&mut self, tag: &str, tab: bool, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.ui().tags.root.clone();
        let mut args = serde_json::json!({ "facet": "tag", "value": tag });
        if let Some(root) = &root {
            args["root"] = serde_json::json!(PathArg::from(root.as_path()));
        }
        let Some(location) = self
            .query_value("query.location", args, cx)
            .and_then(|v| v["location"].as_str().map(str::to_owned))
        else {
            return;
        };
        if tab {
            self.run(
                "tab.new",
                serde_json::json!({ "location": location, "show": false }),
                window,
                cx,
            );
            return;
        }
        let listing = self.ui().listing_view;
        self.run(
            "navigate",
            serde_json::json!({ "location": location }),
            window,
            cx,
        );
        self.run("ui.view", serde_json::json!({ "id": listing }), window, cx);
    }

    /// The menu of the tags `tags` (the selection, or the row clicked).
    fn open_tags_lens_menu(
        &mut self,
        tags: &[String],
        position: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(first) = tags.first() else { return };
        let run = |title: String, command: Invocation, order, icon: &str| MenuItem {
            menu: "tags".into(),
            title,
            command,
            when: None,
            order,
            icon: Some(icon.into()),
            submenu: None,
        };
        let item = |title: String, name: &str, args: serde_json::Value, order, icon: &str| {
            run(title, Invocation::with_args(name, &args), order, icon)
        };
        let one = tags.len() == 1;
        let mut items = Vec::new();
        if one {
            items.push(item(
                "Open".into(),
                "ui.tags_open",
                serde_json::json!({ "tag": first }),
                0,
                "open",
            ));
            items.push(item(
                "Open in New Tab".into(),
                "ui.tags_open",
                serde_json::json!({ "tag": first, "tab": true }),
                2,
                "tab-new",
            ));
        }
        let lens = &self.ui().tags;
        let all_pinned = tags
            .iter()
            .all(|t| lens.rows().iter().any(|r| r.pinned && same_tag(&r.tag, t)));
        items.push(if all_pinned {
            item(
                "Unpin From Sidebar".into(),
                "tags.unpin",
                serde_json::json!({ "tags": tags }),
                50,
                "pin",
            )
        } else {
            item(
                "Pin to Sidebar".into(),
                "tags.pin",
                serde_json::json!({ "tags": tags }),
                50,
                "pin",
            )
        });
        let (rename, remove) = self.tags_edits(tags);
        items.push(run(
            if one {
                "Rename Tag…"
            } else {
                "Merge Tags…"
            }
            .into(),
            rename,
            100,
            "rename",
        ));
        items.push(run(
            "Move to Category…".into(),
            self.ask_category(tags),
            110,
            "move",
        ));
        if let Some(category) = tags.iter().find_map(|t| tag_parent(t)) {
            items.push(run(
                format!("Move Out of “{}”", tag_leaf(category)),
                self.move_tags(tags, None),
                111,
                "move",
            ));
        }
        let remove_title = format!(
            "Remove {} From Every File…",
            if one { "Tag" } else { "Tags" }
        );
        items.push(run(remove_title, remove, 300, "trash"));
        let mut color = |title: &str, color: Option<&str>, order| {
            items.push(MenuItem {
                submenu: Some("Color".into()),
                ..item(
                    title.into(),
                    "ui.tags_color",
                    serde_json::json!({ "tags": tags, "color": color }),
                    order,
                    "appearance",
                )
            });
        };
        color("From Its Name", None, 200);
        for (i, name) in TAG_COLORS.iter().enumerate() {
            let mut title = (*name).to_owned();
            title[..1].make_ascii_uppercase();
            color(
                &title,
                Some(name),
                201 + i32::try_from(i).unwrap_or_default(),
            );
        }
        items.sort_by_key(|m| m.order);
        self.menu = Some(crate::menu::MenuState::open(position, items, window, cx));
        cx.notify();
    }

    /// What renames `tags` (merges them, when several) and what removes
    /// them, on every file under the view's folder: the question that
    /// asks for the new name, and the plan to review.
    fn tags_edits(&self, tags: &[String]) -> (Invocation, Invocation) {
        let root = self.ui().tags.root.as_ref();
        let place = root.map_or_else(String::new, |r| {
            format!(
                " under “{}”",
                r.file_name().unwrap_or(r.as_os_str()).to_string_lossy()
            )
        });
        let mut args = serde_json::json!({ "tags": tags });
        if let Some(root) = root {
            args["root"] = serde_json::json!(PathArg::from(root.as_path()));
        }
        let first = tags.first().cloned().unwrap_or_default();
        let one = tags.len() == 1;
        let rename = Invocation::with_args(
            "ui.ask",
            &serde_json::json!({
                "title": if one {
                    format!("Rename the tag “{first}” on every file{place}")
                } else {
                    format!("Merge {} into one tag on every file{place}", naming(tags))
                },
                "placeholder": if one {
                    "New name (an existing tag's merges them)"
                } else {
                    "The tag they become"
                },
                "text": if one { first } else { String::new() },
                "command": "tags.rename",
                "args": args.clone(),
                "field": "to",
            }),
        );
        (rename, Invocation::with_args("tags.delete", &args))
    }

    /// `ui.tags_color`: each of `tags` wears `color` (`tags.color`, one
    /// tag at a time).
    pub fn tags_color(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tags: Vec<String> = args["tags"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|t| t.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        for tag in tags {
            self.run(
                "tags.color",
                serde_json::json!({ "tag": tag, "color": args["color"] }),
                window,
                cx,
            );
        }
    }

    /// `tags.move`: `tags` into the category `into`, or out of theirs,
    /// on every file under the view's folder.
    fn move_tags(&self, tags: &[String], into: Option<&str>) -> Invocation {
        let mut args = serde_json::json!({ "tags": tags });
        if let Some(into) = into {
            args["into"] = serde_json::json!(into);
        }
        if let Some(root) = &self.ui().tags.root {
            args["root"] = serde_json::json!(PathArg::from(root.as_path()));
        }
        Invocation::with_args("tags.move", &args)
    }

    /// Ask for the category `tags` go into.
    fn ask_category(&self, tags: &[String]) -> Invocation {
        let mut ask = self.move_tags(tags, None);
        let current = tags
            .iter()
            .find_map(|t| tag_parent(t))
            .unwrap_or_default()
            .to_owned();
        ask = Invocation::with_args(
            "ui.ask",
            &serde_json::json!({
                "title": format!("Move {} into a category", naming(tags)),
                "placeholder": "A category, or one within another: Places/France",
                "text": current,
                "command": "tags.move",
                "args": ask.args,
                "field": "into",
            }),
        );
        ask
    }

    /// Tags dragged (a row of this view, a sidebar pill): those its drop
    /// gives.
    fn dragged_tags(drag: &PlaceDrag) -> Vec<String> {
        drag.gives
            .as_ref()
            .filter(|g| &*g.name == "tags.add")
            .and_then(|g| g.args["tags"].as_array())
            .map(|tags| {
                tags.iter()
                    .filter_map(|t| t.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// What the selection's tags give files dropped on them.
    fn give_tags(tags: &[String]) -> Invocation {
        Invocation::with_args("tags.add", &serde_json::json!({ "tags": tags }))
    }
}

/// The view.
#[derive(Default)]
pub struct TagsView;

impl PaneView for TagsView {
    fn id(&self) -> &'static str {
        ID
    }
    fn scroll_to(&mut self, _: usize) {}
    fn nav_delta(&self, _: Nav, _: &Shell) -> Option<isize> {
        None
    }
    fn nav(
        &mut self,
        nav: Nav,
        shell: &mut Shell,
        _: &mut Window,
        cx: &mut Context<Shell>,
    ) -> bool {
        let page = 10;
        // Left and Right fold and unfold categories, as in a tree.
        if matches!(nav, Nav::Left | Nav::Right) {
            let lens = &mut shell.ui_mut().tags;
            let at = if nav == Nav::Left {
                lens.left()
            } else {
                lens.right()
            };
            if let Some(at) = at {
                lens.scroll.scroll_to_item(at);
            }
            cx.notify();
            return true;
        }
        let delta = match nav {
            Nav::Up | Nav::Left => -1,
            Nav::Down | Nav::Right => 1,
            Nav::PageUp => -page,
            Nav::PageDown => page,
            Nav::HalfPageUp => -page / 2,
            Nav::HalfPageDown => page / 2,
            Nav::Home => isize::MIN / 2,
            Nav::End => isize::MAX / 2,
        };
        let lens = &mut shell.ui_mut().tags;
        if let Some(at) = lens.step(delta) {
            lens.scroll.scroll_to_item(at);
        }
        cx.notify();
        true
    }
    fn command(
        &mut self,
        name: &str,
        _args: &serde_json::Value,
        shell: &mut Shell,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) -> bool {
        let selected = shell.ui().tags.selected.clone();
        match name {
            "open" | "ui.open" => {
                if let [tag] = selected.as_slice() {
                    shell.tags_open(tag, false, window, cx);
                }
                true
            }
            "ui.open_in_tab" => {
                for tag in &selected {
                    shell.tags_open(tag, true, window, cx);
                }
                true
            }
            "select_all" => {
                let lens = &mut shell.ui_mut().tags;
                lens.selected = lens.rows().iter().map(|r| r.tag.clone()).collect();
                true
            }
            // F2 renames (merges several), Delete removes them from every
            // file: each a plan to review before anything changes.
            "ui.rename_prompt" | "trash" | "delete" | "ui.confirm_delete" => {
                if !selected.is_empty() {
                    let (rename, remove) = shell.tags_edits(&selected);
                    let run = if name == "ui.rename_prompt" {
                        rename
                    } else {
                        remove
                    };
                    shell.run(&run.name, run.args, window, cx);
                }
                true
            }
            "ui.context_menu" => {
                if !selected.is_empty() {
                    let position = window.mouse_position();
                    shell.open_tags_lens_menu(&selected, position, window, cx);
                }
                true
            }
            // What would act on hidden files does nothing here.
            "copy" | "cut" | "ui.copy" | "ui.cut" | "select" | "ui.select_sweep" => true,
            _ => false,
        }
    }
    fn render(&mut self, shell: &mut Shell, _: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
        render(shell, cx)
    }
}

fn render(shell: &mut Shell, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let pane = shell.pane;
    let lens = &shell.ui().tags;
    let status = lens.status.as_ref();
    let summary = match (lens.root.as_ref(), status) {
        (None, _) => "Tags are counted in a folder".to_owned(),
        (Some(_), None) => "Counting tags…".to_owned(),
        (Some(_), Some(s)) => {
            let n = s.tags.len();
            let tags = if n == 1 {
                "1 tag".to_owned()
            } else {
                format!("{} tags", format::thousands(n))
            };
            let files = if s.files == 1 {
                "1 file".to_owned()
            } else {
                format!("{} files", format::thousands(s.files))
            };
            let when = s.counted.map(|secs| {
                format::friendly_time(
                    std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs),
                    &jiff::Zoned::now(),
                )
            });
            let said = match (&s.error, s.walking, s.checking, when) {
                (Some(e), ..) => {
                    format!("{tags} bibli knows; the files were not all read: {e}")
                }
                (None, true, ..) => format!("{tags} so far · counting…"),
                (None, false, true, Some(when)) => {
                    format!("{tags} on {files} as counted {when} · checking what changed…")
                }
                (None, false, true, None) => format!("{tags} so far · checking…"),
                (None, false, false, _) => {
                    format!("{tags} on {files}; hidden files and folders left out")
                }
            };
            if s.partial {
                format!("{said} · more tagged files than bibli has room for: some are not counted")
            } else {
                said
            }
        }
    };
    let partial = status.is_some_and(|s| s.partial);
    let room = partial.then(|| {
        Button::new(
            format!("tags-room-{}", pane.0),
            "settings",
            "Give bibli more room for tagged files",
            "ui.settings",
        )
        .args(serde_json::json!({ "key": "tags.index_mb" }))
        .render(shell, cx)
    });
    let lens = &shell.ui().tags;
    let header = toolbar::row(&theme)
        .id(("tags-tools", pane.0 as usize))
        .text_color(theme.text)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .px_1()
                .text_xs()
                .truncate()
                .child(summary),
        )
        .children(room)
        .child(
            Button::new(
                format!("tags-sort-{}", pane.0),
                "sort",
                if lens.by_files {
                    "Sort by name"
                } else {
                    "Sort by how many files"
                },
                "ui.tags_sort",
            )
            .render(shell, cx),
        )
        .child(
            Button::new(
                format!("tags-refresh-{}", pane.0),
                "refresh",
                "Count again",
                "ui.tags_refresh",
            )
            .enabled(shell.ui().tags.root.is_some())
            .render(shell, cx),
        );

    let lens = &shell.ui().tags;
    let lines: Vec<(Row, usize, bool, bool)> = lens
        .tree()
        .into_iter()
        .map(|l| (l.row.clone(), l.depth, l.category, l.open))
        .collect();
    let rows: Vec<Row> = lines.iter().map(|l| l.0.clone()).collect();
    let selected: Vec<String> = lens.selected.clone();
    let targets: Vec<Vec<String>> = rows.iter().map(|r| lens.targets(&r.tag)).collect();
    let cursor = lens.cursor.clone();
    let scroll = lens.scroll.clone();
    let empty = rows.is_empty() && status.is_some_and(|s| !s.walking && !s.checking);
    let mut list = div()
        .id(("tags-rows", pane.0 as usize))
        .size_full()
        .flex()
        .flex_col()
        .py_1()
        .overflow_y_scroll()
        .track_scroll(&scroll)
        // A click beside the rows selects nothing.
        .on_click(cx.listener(|this, _, _, cx| {
            this.ui_mut().tags.selected.clear();
            cx.notify();
        }))
        // Tags dropped beside the rows leave their category.
        .on_drop(cx.listener(|this, drag: &PlaceDrag, window, cx| {
            let tags: Vec<String> = Shell::dragged_tags(drag)
                .into_iter()
                .filter(|t| tag_parent(t).is_some())
                .collect();
            if !tags.is_empty() {
                let run = this.move_tags(&tags, None);
                this.run(&run.name, run.args, window, cx);
            }
        }));
    if empty {
        list = list.child(
            div()
                .p_4()
                .text_sm()
                .text_color(theme.text_muted)
                .child("No file here has a tag"),
        );
    }
    for ((i, (row, depth, category, open)), targets) in lines.iter().enumerate().zip(targets) {
        let is_selected = selected.iter().any(|t| same_tag(t, &row.tag));
        let at_cursor = cursor.as_deref().is_some_and(|c| same_tag(c, &row.tag));
        list = list.child(tag_row(
            i,
            row,
            Tree {
                depth: *depth,
                category: *category,
                open: *open,
            },
            is_selected,
            at_cursor,
            targets,
            pane,
            &theme,
            cx,
        ));
    }
    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(theme.bg)
        .child(header)
        .child(div().relative().flex_1().min_h_0().child(list).child(
            crate::elements::scrollbar::vertical(
                ("tags-scrollbar", pane.0 as usize),
                crate::elements::scrollbar::Scrolls::Offset(scroll),
            ),
        ))
        .into_any_element()
}

/// Where a row is in the tree.
#[derive(Clone, Copy)]
struct Tree {
    depth: usize,
    category: bool,
    open: bool,
}

#[allow(
    clippy::too_many_arguments,
    reason = "one row, drawn from what the view holds"
)]
fn tag_row(
    i: usize,
    row: &Row,
    tree: Tree,
    is_selected: bool,
    at_cursor: bool,
    targets: Vec<String>,
    pane: bibli_core::workspace::PaneId,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let color = color_index(&row.color);
    let hover = theme.hover;
    let (tint, edge) = (theme.drop_target, theme.accent);
    let tag = row.tag.clone();
    let label = naming(&targets);
    let drag = PlaceDrag {
        group: String::new(),
        index: 0,
        reorder: None,
        gives: Some(Shell::give_tags(&targets)),
    };
    let (click, menu, open, dropped, from_paths, into) = (
        tag.clone(),
        tag.clone(),
        tag.clone(),
        targets.clone(),
        targets.clone(),
        tag.clone(),
    );
    let muted = if is_selected {
        theme.selection_text
    } else {
        theme.text_muted
    };
    // The arrow of a category, a blank as wide elsewhere so names align.
    let arrow = div()
        .id(("tag-fold", i))
        .flex_none()
        .size(px(14.))
        .flex()
        .items_center()
        .justify_center()
        .when(tree.category, |d| {
            let tag = tag.clone();
            d.cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &gpui::MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let lens = &mut this.ui_mut().tags;
                        let open = !lens.is_open(&tag);
                        lens.set_open(&tag, open);
                        cx.notify();
                    }),
                )
                .child(
                    gpui::svg()
                        .path(if tree.open {
                            "toolbar/chevron.svg"
                        } else {
                            "toolbar/right.svg"
                        })
                        .size(px(12.))
                        .text_color(muted),
                )
        });
    div()
        .id(("tag-row", i))
        .flex_none()
        .h(px(ROW_HEIGHT))
        .mx_1()
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .rounded_md()
        .border_1()
        .border_color(if at_cursor && is_selected {
            theme.accent
        } else {
            gpui::transparent_black()
        })
        .when(is_selected, |d| {
            d.bg(theme.selection_bg).text_color(theme.selection_text)
        })
        .when(!is_selected, |d| {
            d.text_color(theme.text).hover(move |s| s.bg(hover))
        })
        .cursor_pointer()
        .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, window, cx| {
            cx.stop_propagation();
            this.activate_pane(pane, cx);
            window.focus(&this.ui().focus);
            if ev.click_count() >= 2 {
                this.tags_open(&open, false, window, cx);
                return;
            }
            let m = ev.modifiers();
            this.ui_mut()
                .tags
                .click(&click, m.control || m.platform, m.shift);
            cx.notify();
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, ev: &gpui::MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.activate_pane(pane, cx);
                let lens = &mut this.ui_mut().tags;
                if !lens.is_selected(&menu) {
                    lens.click(&menu, false, false);
                }
                let tags = lens.selected.clone();
                this.open_tags_lens_menu(&tags, ev.position, window, cx);
            }),
        )
        .on_mouse_down(
            MouseButton::Middle,
            cx.listener({
                let tag = tag.clone();
                move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.tags_open(&tag, true, window, cx);
                }
            }),
        )
        .on_drag(drag, move |_, position, _, cx| {
            cx.new(|_| crate::dnd::DragPreview::named(label.clone(), position))
        })
        // Tags dropped on it go into it, as their category.
        .drag_over::<PlaceDrag>(move |s, _, _, _| s.bg(tint).border_color(edge))
        .on_drop(cx.listener(move |this, drag: &PlaceDrag, window, cx| {
            cx.stop_propagation();
            let tags: Vec<String> = Shell::dragged_tags(drag)
                .into_iter()
                .filter(|t| !tag_within(&into, t))
                .collect();
            if !tags.is_empty() {
                let run = this.move_tags(&tags, Some(&into));
                this.run(&run.name, run.args, window, cx);
            }
        }))
        // Files dropped on it get its tags.
        .drag_over::<DragPayload>(move |s, _, _, _| s.bg(tint).border_color(edge))
        .on_drop(cx.listener(move |this, payload: &DragPayload, window, cx| {
            cx.stop_propagation();
            let mut args = Shell::give_tags(&dropped).args;
            args["pane"] = serde_json::json!(payload.pane);
            this.run("tags.add", args, window, cx);
        }))
        .drag_over::<ExternalPaths>(move |s, _, _, _| s.bg(tint).border_color(edge))
        .on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
            cx.stop_propagation();
            let entries: Vec<bibli_core::op::EntryRef> = paths
                .paths()
                .iter()
                .filter_map(bibli_core::op::EntryRef::from_path)
                .collect();
            let mut args = Shell::give_tags(&from_paths).args;
            args["entries"] = serde_json::json!(entries);
            this.run("tags.add", args, window, cx);
        }))
        .child(div().flex_none().w(px(INDENT * tree.depth as f32)))
        .child(arrow)
        .child(crate::tags::dot(color, theme))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_sm()
                .child(SharedString::from(tag_leaf(&row.tag).to_owned())),
        )
        .when(row.pinned, |d| {
            d.child(crate::controls::glyph(
                "pin",
                if is_selected {
                    theme.selection_text
                } else {
                    theme.text_muted
                },
                12.,
            ))
        })
        .child(
            div()
                .flex_none()
                .w(px(64.))
                .text_right()
                .text_xs()
                .text_color(if is_selected {
                    theme.selection_text
                } else {
                    theme.text_muted
                })
                .child(format::thousands(row.files)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{Row, Status, TagsLens};

    fn lens(tags: &[(&str, usize)]) -> TagsLens {
        TagsLens {
            status: Some(Status {
                tags: tags
                    .iter()
                    .map(|(t, n)| Row {
                        tag: (*t).into(),
                        files: *n,
                        ..Row::default()
                    })
                    .collect(),
                ..Status::default()
            }),
            ..TagsLens::default()
        }
    }

    #[test]
    fn clicks_select_as_in_the_file_views() {
        let mut l = lens(&[("a", 1), ("b", 5), ("c", 2), ("d", 1)]);
        l.click("b", false, false);
        l.click("d", false, true);
        assert_eq!(l.selected, ["b", "c", "d"]);
        l.click("A", true, false);
        assert_eq!(l.selected, ["b", "c", "d", "A"]);
        l.click("c", true, false);
        assert_eq!(l.selected, ["b", "d", "A"]);
        assert_eq!(l.targets("d"), ["b", "d", "A"]);
        assert_eq!(l.targets("c"), ["c"]);
        // The keys move one row, the selection following.
        l.cursor = Some("a".into());
        l.by_files = true;
        assert_eq!(l.step(1), Some(3), "b, c, a, d by files");
        assert_eq!(l.selected, ["d"]);
        assert_eq!(l.step(isize::MAX / 2), Some(3));
        assert_eq!(l.step(-2), Some(1));
        assert_eq!(l.selected, ["c"]);
    }

    #[test]
    fn tags_in_a_category_fold_under_it() {
        let mut l = lens(&[
            ("Places", 3),
            ("Places/France", 2),
            ("Places/France/Paris", 1),
            ("Places/Japan", 1),
            ("work", 4),
        ]);
        let shown = |l: &TagsLens| -> Vec<(String, usize, bool)> {
            l.tree()
                .iter()
                .map(|line| (line.row.tag.clone(), line.depth, line.category))
                .collect()
        };
        assert_eq!(
            shown(&l),
            [("Places".into(), 0, true), ("work".into(), 0, false)],
            "folded at first"
        );
        // Right unfolds, then goes in; Left goes out, then folds.
        l.cursor = Some("Places".into());
        assert_eq!(l.right(), Some(0));
        assert_eq!(l.rows().len(), 4);
        assert_eq!(l.right(), Some(1));
        assert_eq!(l.selected, ["Places/France"]);
        assert_eq!(l.right(), Some(1));
        assert_eq!(
            shown(&l),
            [
                ("Places".into(), 0, true),
                ("Places/France".into(), 1, true),
                ("Places/France/Paris".into(), 2, false),
                ("Places/Japan".into(), 1, false),
                ("work".into(), 0, false),
            ]
        );
        assert_eq!(l.left(), Some(1), "folds France");
        assert_eq!(l.rows().len(), 4);
        assert_eq!(l.left(), Some(0), "goes out to Places");
        assert_eq!(l.selected, ["Places"]);
        assert_eq!(l.left(), Some(0));
        assert_eq!(l.rows().len(), 2);
        // Sorted by files, each category's own in their order.
        l.by_files = true;
        l.set_open("places", true);
        assert_eq!(l.rows()[0].tag, "work");
        assert_eq!(l.rows()[2].tag, "Places/France");
    }
}
