//! Disk usage: a squarified treemap of the usage plugin's scan, replacing
//! the view while open.

use std::path::PathBuf;

use gpui::{AnyElement, Context, Hsla, MouseButton, Window, div, pattern_slash, prelude::*, px};
use serde::Deserialize;

use crate::format;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::treemap::{Rect, squarify};

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
    pub hover: Option<String>,
    root: Option<PathBuf>,
}

/// A laid-out block: depth 0 rectangles hold depth 1 children.
struct Block {
    rect: Rect,
    label: String,
    tip: String,
    color: Hsla,
    is_dir: bool,
    is_other: bool,
    path: Option<PathBuf>,
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
        self.usage.hover = None;
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
            self.usage.hover = None;
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

fn blocks(theme: &Theme, node: &Node, area: Rect) -> Vec<(Block, Vec<Block>)> {
    // Weight by allocated bytes; children with (almost) nothing on disk
    // still get a sliver (0.5% of the parent) so empty trees stay visible.
    let floor = (node.alloc as f64 / 200.0).max(1.0);
    let mut items: Vec<(f64, Option<&Node>)> = node
        .children
        .iter()
        .map(|c| ((c.alloc as f64).max(floor), Some(c)))
        .collect();
    if node.other_alloc > 0 {
        items.push((node.other_alloc as f64, None));
    }
    let weights: Vec<f64> = items.iter().map(|(w, _)| *w).collect();
    let rects = squarify(&weights, area);
    items
        .iter()
        .zip(rects)
        .enumerate()
        .filter(|(_, ((w, _), _))| *w > 0.0)
        .map(|(i, ((_, child), rect))| match child {
            Some(c) => {
                let tip = format!(
                    "{} · {} ({} on disk){}",
                    c.path.display(),
                    format::size(c.size),
                    format::size(c.alloc),
                    if c.is_dir {
                        format!(" · {} files, {} dirs", c.files, c.dirs)
                    } else {
                        String::new()
                    }
                );
                let inner = if c.is_dir && !c.children.is_empty() && rect.w > 40.0 && rect.h > 40.0
                {
                    let pad = 3.0;
                    let label_h = if rect.h > 40.0 { 16.0 } else { 0.0 };
                    let inner_area = Rect::new(
                        rect.x + pad,
                        rect.y + pad + label_h,
                        (rect.w - 2.0 * pad).max(0.0),
                        (rect.h - 2.0 * pad - label_h).max(0.0),
                    );
                    blocks(theme, c, inner_area)
                        .into_iter()
                        .map(|(b, _)| Block {
                            color: color_for(theme, 1, b.is_dir, i + 3),
                            ..b
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                (
                    Block {
                        rect,
                        label: c.name.clone(),
                        tip,
                        color: color_for(theme, 0, c.is_dir, i),
                        is_dir: c.is_dir,
                        is_other: false,
                        path: c.is_dir.then(|| c.path.clone()),
                    },
                    inner,
                )
            }
            None => (
                Block {
                    rect,
                    label: "other".into(),
                    tip: format!(
                        "smaller items in {} · {} on disk",
                        node.path.display(),
                        format::size(node.other_alloc)
                    ),
                    color: theme.text_muted,
                    is_dir: false,
                    is_other: true,
                    path: None,
                },
                Vec::new(),
            ),
        })
        .collect()
}

fn block_el(
    b: &Block,
    depth: usize,
    theme: &Theme,
    ix: usize,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let tip = b.tip.clone();
    let path = b.path.clone();
    let fits_label = b.rect.w > 48.0 && b.rect.h > 16.0;
    let mut el = div()
        .id(("usage", depth * 100_000 + ix))
        .absolute()
        .left(px(b.rect.x as f32))
        .top(px(b.rect.y as f32))
        .w(px(b.rect.w.max(0.0) as f32))
        .h(px(b.rect.h.max(0.0) as f32))
        .border_1()
        .border_color(theme.bg)
        .overflow_hidden()
        .text_xs()
        .text_color(gpui::white())
        .px_1()
        .on_hover(cx.listener(move |this, hovered, _, cx| {
            this.usage.hover = if *hovered { Some(tip.clone()) } else { None };
            cx.notify();
        }));
    if b.is_other {
        el = el.bg(pattern_slash(b.color.opacity(0.5), 2.0, 8.0));
    } else {
        el = el.bg(b.color);
    }
    if let Some(p) = path {
        el = el.cursor_pointer().on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                this.close_usage(cx);
                this.run(
                    "navigate",
                    serde_json::json!({ "location": p.to_string_lossy() }),
                    window,
                    cx,
                );
            }),
        );
    }
    if fits_label {
        el = el.child(div().truncate().child(b.label.clone()));
    }
    let _ = b.is_dir;
    el.into_any_element()
}

pub fn render(shell: &mut Shell, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let width = shell.ui().list.content_width.get().max(10.0);
    let height = (shell.ui().list.content_height.get() - 44.0).max(10.0);
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
    let hover = shell.usage.hover.clone();
    let mut area = div()
        .relative()
        .flex_1()
        .min_h_0()
        .w_full()
        .overflow_hidden();
    if let Some(tree) = status.as_ref().and_then(|s| s.tree.as_ref()) {
        let bounds = Rect::new(0.0, 0.0, f64::from(width), f64::from(height));
        for (i, (outer, inner)) in blocks(&theme, tree, bounds).into_iter().enumerate() {
            area = area.child(block_el(&outer, 0, &theme, i, cx));
            for (j, b) in inner.iter().enumerate() {
                area = area.child(block_el(b, 1, &theme, i * 1000 + j, cx));
            }
        }
    }
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(header)
        .child(area)
        .child(
            div()
                .h(px(22.))
                .px_2()
                .text_xs()
                .text_color(theme.text_muted)
                .border_t_1()
                .border_color(theme.border)
                .truncate()
                .child(hover.unwrap_or_else(|| {
                    "hover a block for details · click a folder to open it · Escape closes".into()
                })),
        )
        .into_any_element()
}
