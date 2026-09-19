//! Plugin-controlled Usage pane view. Filesystem work remains in the plugin.
use std::path::PathBuf;
use std::sync::atomic::Ordering;

use bibli_core::{EntryId, command::Invocation};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};
use serde::Deserialize;

use crate::elements::treemap::{TreemapItem, treemap};
use crate::{
    format,
    shell::Shell,
    theme::Theme,
    toolbar::{self, Button},
    view::{Nav, PaneView},
};

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
    pub hover_path: Option<PathBuf>,
    root: Option<PathBuf>,
    revision: Option<u64>,
}

impl Shell {
    pub fn sync_usage(&mut self, cx: &mut Context<Self>) {
        if self.ui().view_id != "usage" {
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
        if self.ui().usage.root != dir {
            self.ui_mut().usage = UsageState {
                root: dir.clone(),
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
        let revision = self
            .app
            .registry
            .views
            .find(|s| &*s.id == "usage")
            .map(|s| s.revision.load(Ordering::Acquire));
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
        if self.ui().view_id == "usage" {
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

pub struct UsageView;
impl PaneView for UsageView {
    fn id(&self) -> &'static str {
        "usage"
    }
    fn title(&self) -> &'static str {
        "Usage"
    }
    fn scroll_to(&mut self, _: usize) {}
    fn nav_delta(&self, nav: Nav, _: &Shell) -> Option<isize> {
        match nav {
            Nav::Up | Nav::Left => Some(-1),
            Nav::Down | Nav::Right => Some(1),
            _ => None,
        }
    }
    fn render(&mut self, shell: &mut Shell, _: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
        render(shell, cx)
    }
}

fn render(shell: &mut Shell, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let pane = shell.pane;
    let state = &shell.ui().usage;
    let status = state.status.as_ref();
    let mut header = toolbar::row(&theme)
        .text_color(theme.text)
        .id(("usage-tools", pane.0 as usize));
    header = header.child(
        div()
            .flex_1()
            .min_w_0()
            .text_xs()
            .truncate()
            .child(status.map_or_else(
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
                        "{}{} · {} on disk · {} files · {} folders{}",
                        if s.tree.as_ref().is_some_and(|n| n.incomplete) {
                            "Counted so far: "
                        } else {
                            ""
                        },
                        format::size(s.size),
                        format::size(s.alloc),
                        format::thousands(s.files as usize),
                        format::thousands(s.dirs as usize),
                        if s.updating { " · updating…" } else { "" }
                    )
                },
            )),
    );
    header = header.child(
        Button::new(
            format!("usage-refresh-{}", pane.0),
            "refresh",
            "Recalculate sizes · Ctrl+R",
            "ui.usage_refresh",
        )
        .enabled(state.root.is_some())
        .render(shell, cx),
    );
    let mut items = Vec::new();
    let mut details = Vec::new();
    let mut entry_ids = Vec::new();
    let mut paths = Vec::new();
    if let Some(node) = status.and_then(|s| s.tree.as_ref()) {
        let show_hidden = shell.sort_spec().show_hidden;
        let ws = shell.app.workspace.read();
        let p = ws.pane(pane);
        for (index, child) in node.children.iter().enumerate() {
            if !show_hidden && child.name.starts_with('.') {
                continue;
            }
            let key = details.len();
            let selected = child
                .id
                .is_some_and(|id| p.is_some_and(|p| p.selection.contains(id)));
            let mut color = if child.is_dir {
                theme.accent
            } else {
                theme.text_muted
            };
            let color_index = child.id.map_or(index % 7, |id| (id.ino % 7) as usize);
            color.h = (color.h + color_index as f32 * 0.04) % 1.;
            color.s = if child.is_dir { 0.55 } else { 0.1 };
            color.l = if selected { 0.65 } else { 0.42 };
            details.push(format!(
                "{} · {}{} · {} on disk{}",
                child.name,
                if child.incomplete {
                    "counted so far: "
                } else {
                    ""
                },
                format::size(child.size),
                format::size(child.alloc),
                if selected { " · selected" } else { "" }
            ));
            entry_ids.push(child.id);
            paths.push(child.path.clone());
            items.push(TreemapItem {
                key,
                weight: (child.alloc as f64).max((node.alloc as f64 / 200.).max(1.)),
                label: if child.incomplete {
                    format!(
                        "{} · {}",
                        child.name,
                        if status.is_some_and(|s| s.scanning) {
                            "calculating…"
                        } else {
                            "incomplete"
                        }
                    )
                    .into()
                } else {
                    child.name.clone().into()
                },
                color,
                hatched: child.incomplete,
                children: Vec::new(),
            });
        }
        if node.other_count > 0 {
            let key = details.len();
            details.push(format!(
                "{} smaller items · double-click to see all items in List",
                node.other_count
            ));
            entry_ids.push(None);
            paths.push(node.path.clone());
            items.push(TreemapItem {
                key,
                weight: (node.other_alloc as f64).max(1.),
                label: format!(
                    "{} other items{}",
                    node.other_count,
                    if node.other_incomplete > 0 {
                        " · incomplete"
                    } else {
                        ""
                    }
                )
                .into(),
                color: theme.text_muted,
                hatched: true,
                children: Vec::new(),
            });
        }
    }
    let hover = state
        .hover_path
        .as_ref()
        .and_then(|path| paths.iter().position(|p| p == path));
    let hint = hover.and_then(|key| details.get(key)).cloned().unwrap_or_else(|| {
        status.and_then(|s| s.error.clone()).unwrap_or_else(|| {
            if status.is_some_and(|s| s.scanning && s.tree.as_ref().is_some_and(|n| n.incomplete)) { "Striped blocks are still being counted · proportions will change as sizes arrive".into() }
            else if status.is_some_and(|s| s.updating) { format!("Scanning… {} folders read", status.map_or(0, |s| s.scanned_dirs)) }
            else { "Click to select · double-click to open · Back/Up keeps Usage · totals include hidden files".into() }
        })
    });
    let entity = cx.entity();
    let entity2 = entity.clone();
    let root = state.root.clone();
    let map = treemap(
        ("usage-map", pane.0 as usize),
        items,
        theme.bg,
        gpui::white(),
        hover,
        move |key, _, cx| {
            entity.update(cx, |shell, cx| {
                shell.with_pane(pane, |shell| {
                    shell.ui_mut().usage.hover_path = key.and_then(|key| paths.get(key).cloned());
                });
                cx.notify();
            });
        },
        move |key, event, window, cx| {
            entity2.update(cx, |shell, cx| {
                shell.with_pane(pane, |shell| {
                    if shell.ui().view_id != "usage" || shell.ui().usage.root != root {
                        return;
                    }
                    if let Some(id) = entry_ids.get(key).copied().flatten() {
                        if let Some(index) = shell.ui().list.index_of(id) {
                            shell.on_entry_mouse_down(pane, index, event, window, cx);
                        }
                    } else if event.click_count >= 2 {
                        shell.run("ui.view", serde_json::json!({"id":"list"}), window, cx);
                    }
                });
            });
        },
    );
    div()
        .size_full()
        .flex()
        .flex_col()
        .text_color(gpui::white())
        .child(header)
        .child(div().flex_1().min_h_0().w_full().child(map))
        .child(
            div()
                .h(px(24.))
                .px_2()
                .text_xs()
                .text_color(theme.text_muted)
                .truncate()
                .child(hint),
        )
        .into_any_element()
}
