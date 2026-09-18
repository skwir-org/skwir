//! Disk usage: a squarified treemap (see `elements::treemap`) of the usage
//! plugin's scan, replacing the view while open.

use std::path::PathBuf;

use gpui::{AnyElement, Context, Hsla, Window, div, prelude::*, px};
use serde::Deserialize;

use crate::elements::treemap::{TreemapItem, treemap};
use crate::format;
use crate::shell::Shell;
use crate::theme::Theme;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub alloc: u64,
    pub files: u64,
    pub dirs: u64,
    #[serde(default)]
    pub children: Vec<Node>,
    #[serde(default)]
    pub other_alloc: u64,
    #[serde(default)]
    pub other_size: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Status {
    pub root: Option<PathBuf>,
    pub scanning: bool,
    pub files: u64,
    pub dirs: u64,
    pub alloc: u64,
    pub size: u64,
    pub elapsed_ms: u64,
    pub tree: Option<Node>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct UsageState {
    pub active: bool,
    pub status: Option<Status>,
    /// Key of the hovered block (index into `blocks`).
    pub hover_key: Option<usize>,
    pub blocks: Vec<BlockInfo>,
    root: Option<PathBuf>,
}

impl Shell {
    pub fn open_usage(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let dir = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane)
                .and_then(|p| p.location().and_then(bibli_core::Location::path))
        };
        let Some(dir) = dir else {
            self.set_status("disk usage needs a directory", true, cx);
            return;
        };
        self.usage.active = true;
        self.usage.root = Some(dir.clone());
        self.usage.status = None;
        self.usage.hover_key = None;
        let pane = self.pane;
        if self
            .app
            .dispatch(&bibli_core::command::Invocation::with_args(
                "usage.scan",
                &serde_json::json!({ "path": dir.to_string_lossy(), "pane": pane }),
            ))
            .is_err()
        {
            self.set_status("the usage plugin is not loaded", true, cx);
            self.usage.active = false;
            return;
        }
        self.poll_usage(cx);
        cx.notify();
    }

    pub fn close_usage(&mut self, cx: &mut Context<Self>) {
        if self.usage.active {
            self.usage.active = false;
            self.usage.hover_key = None;
            cx.notify();
        }
    }

    fn poll_usage(&mut self, cx: &mut Context<Self>) {
        if let Some(v) = self.query_value("usage.status", serde_json::json!({}), cx) {
            match serde_json::from_value::<Status>(v) {
                Ok(s) => self.usage.status = Some(s),
                Err(e) => tracing::warn!("usage.status: {e}"),
            }
        }
    }

    /// Poll while a scan runs (the plugin wakes us on progress).
    pub fn sync_usage(&mut self, cx: &mut Context<Self>) {
        if !self.usage.active {
            return;
        }
        let scanning = self.usage.status.as_ref().is_none_or(|s| s.scanning);
        if scanning {
            self.poll_usage(cx);
        }
    }
}

fn color_for(theme: &Theme, depth: usize, is_dir: bool, index: usize) -> Hsla {
    let mut c = theme.accent;
    if !is_dir {
        c = theme.text_muted;
    }
    // Vary hue a little per sibling, lighten with depth.
    c.h = (c.h + (index % 7) as f32 * 0.04) % 1.0;
    c.s = if is_dir { 0.55 } else { 0.1 };
    c.l = (0.42 + depth as f32 * 0.12).min(0.75);
    c.a = 1.0;
    c
}

/// What the shell remembers per block for the hover line and clicks.
#[derive(Clone, Debug)]
pub struct BlockInfo {
    pub tip: String,
    pub path: Option<PathBuf>,
}

/// Build the treemap items for `node`'s children (one level of nesting),
/// registering each block's info in `infos` under its key.
fn items_for(
    theme: &Theme,
    node: &Node,
    depth: usize,
    infos: &mut Vec<BlockInfo>,
) -> Vec<TreemapItem> {
    // Weight by allocated bytes; children with (almost) nothing on disk
    // still get a sliver (0.5% of the parent) so empty trees stay visible.
    let floor = (node.alloc as f64 / 200.0).max(1.0);
    let mut out = Vec::with_capacity(node.children.len() + 1);
    for (i, c) in node.children.iter().enumerate() {
        let key = infos.len();
        infos.push(BlockInfo {
            tip: format!(
                "{} · {} ({} on disk){}",
                c.path.display(),
                format::size(c.size),
                format::size(c.alloc),
                if c.is_dir {
                    format!(" · {} files, {} dirs", c.files, c.dirs)
                } else {
                    String::new()
                }
            ),
            path: c.is_dir.then(|| c.path.clone()),
        });
        let children = if depth == 0 && c.is_dir && !c.children.is_empty() {
            items_for(theme, c, depth + 1, infos)
        } else {
            Vec::new()
        };
        out.push(TreemapItem {
            key,
            weight: (c.alloc as f64).max(floor),
            label: c.name.clone().into(),
            color: color_for(theme, depth, c.is_dir, i),
            hatched: false,
            children,
        });
    }
    if node.other_alloc > 0 {
        let key = infos.len();
        infos.push(BlockInfo {
            tip: format!(
                "smaller items in {} · {} on disk",
                node.path.display(),
                format::size(node.other_alloc)
            ),
            path: None,
        });
        out.push(TreemapItem {
            key,
            weight: node.other_alloc as f64,
            label: "other".into(),
            color: theme.text_muted,
            hatched: true,
            children: Vec::new(),
        });
    }
    out
}

pub fn render(shell: &mut Shell, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let status = shell.usage.status.clone();
    let mut header = div()
        .h(px(44.))
        .px_2()
        .flex()
        .flex_col()
        .justify_center()
        .text_xs()
        .border_b_1()
        .border_color(theme.border);
    match &status {
        Some(s) => {
            header = header.child(div().flex().gap_3().child(format!(
                "{}{}",
                s.root.as_ref().map(|r| r.display().to_string()).unwrap_or_default(),
                if s.scanning { "  · scanning…" } else { "" }
            )));
            header = header.child(div().text_color(theme.text_muted).child(format!(
                "{} · {} on disk · {} files · {} dirs · {:.1} s{}",
                format::size(s.size),
                format::size(s.alloc),
                format::thousands(s.files as usize),
                format::thousands(s.dirs as usize),
                s.elapsed_ms as f64 / 1000.0,
                s.error.as_ref().map(|e| format!(" · {e}")).unwrap_or_default()
            )));
        }
        None => {
            header = header.child("scanning…");
        }
    }
    let mut infos = Vec::new();
    let items = status
        .as_ref()
        .and_then(|s| s.tree.as_ref())
        .map(|t| items_for(&theme, t, 0, &mut infos))
        .unwrap_or_default();
    shell.usage.blocks = infos;
    let hovered = shell.usage.hover_key;
    let hover_line = hovered.and_then(|k| shell.usage.blocks.get(k)).map_or_else(
        || "hover a block for details · click a folder to open it · Escape closes".to_owned(),
        |b| b.tip.clone(),
    );
    let entity = cx.entity();
    let entity2 = entity.clone();
    let map = treemap(
        "usage-treemap",
        items,
        theme.bg,
        gpui::white(),
        hovered,
        move |key, _window, cx| {
            entity.update(cx, |shell, cx| {
                shell.usage.hover_key = key;
                cx.notify();
            });
        },
        move |key, window, cx| {
            entity2.update(cx, |shell, cx| {
                if let Some(p) = shell.usage.blocks.get(key).and_then(|b| b.path.clone()) {
                    shell.close_usage(cx);
                    shell.run(
                        "navigate",
                        serde_json::json!({ "location": p.to_string_lossy() }),
                        window,
                        cx,
                    );
                }
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
                .h(px(22.))
                .px_2()
                .text_xs()
                .text_color(theme.text_muted)
                .border_t_1()
                .border_color(theme.border)
                .truncate()
                .child(hover_line),
        )
        .into_any_element()
}
