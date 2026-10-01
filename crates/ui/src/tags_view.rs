//! The Tags view, a lens: the tags of the files under the folder the pane
//! shows, as the tags plugin counts them (`tags.lens`, `tags.lens_status`:
//! the index's counts at once, the walk's when it ends). A row opens its
//! tag's search; the selection (a click, Ctrl and Shift as in the file
//! views) is what the menu, Enter, F2 and Delete act on: pin, rename or
//! merge, color, remove from every file under the folder. Files dropped
//! on a row get its tags (the selection's when the row is in it), and a
//! row dragged onto files gives them the same.
//!
//! Tags are not entries, so the pane's own selection stays out of it: the
//! keys that would act on files act on the tags (`PaneView::command`).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bibli_core::command::Invocation;
use bibli_core::metadata::{TAG_COLORS, same_tag};
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
    pub scroll: ScrollHandle,
}

impl TagsLens {
    /// The tags in the order shown.
    pub fn rows(&self) -> Vec<&Row> {
        let mut rows: Vec<&Row> = self.status.iter().flat_map(|s| s.tags.iter()).collect();
        if self.by_files {
            rows.sort_by_key(|r| std::cmp::Reverse(r.files));
        }
        rows
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
                let by_files = self.ui().tags.by_files;
                self.ui_mut().tags = TagsLens {
                    by_files,
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
            let by_files = self.ui().tags.by_files;
            self.ui_mut().tags = TagsLens {
                root: dir.clone(),
                location_lifetime: lifetime,
                by_files,
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
                format!("{said} · more tagged files than bibli keeps: some are not counted")
            } else {
                said
            }
        }
    };
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
    let rows: Vec<Row> = lens.rows().into_iter().cloned().collect();
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
    for ((i, row), targets) in rows.iter().enumerate().zip(targets) {
        let is_selected = selected.iter().any(|t| same_tag(t, &row.tag));
        let at_cursor = cursor.as_deref().is_some_and(|c| same_tag(c, &row.tag));
        list = list.child(tag_row(
            i,
            row,
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

#[allow(
    clippy::too_many_arguments,
    reason = "one row, drawn from what the view holds"
)]
fn tag_row(
    i: usize,
    row: &Row,
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
    let (click, menu, open, dropped, from_paths) = (
        tag.clone(),
        tag.clone(),
        tag.clone(),
        targets.clone(),
        targets.clone(),
    );
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
        .child(crate::tags::dot(color, theme))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_sm()
                .child(SharedString::from(row.tag.clone())),
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
}
