//! Drag and drop: internal moves/copies between rows, cells, columns, and
//! sidebar places, and external paths dropped from other applications.

use std::path::{Path, PathBuf};

use bibli_core::op::{self, EntryRef};
use bibli_core::{Plan, fs};
use gpui::{Context, Div, Pixels, Point, Render, Stateful, Window, div, prelude::*, px};

use crate::shell::Shell;
use crate::theme::Theme;

/// What a drag carries: the dragging pane's selection at drag start.
#[derive(Clone, Debug)]
pub struct DragPayload {
    pub count: usize,
    pub label: String,
}

/// The floating preview under the cursor.
pub struct DragPreview {
    payload: DragPayload,
    position: Point<Pixels>,
}

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>().clone();
        let text = if self.payload.count == 1 {
            self.payload.label.clone()
        } else {
            format!("{} items", self.payload.count)
        };
        div()
            .pl(self.position.x + px(12.))
            .pt(self.position.y + px(12.))
            .child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(theme.accent)
                    .text_color(gpui::white())
                    .text_xs()
                    .shadow_md()
                    .child(text),
            )
    }
}

impl Shell {
    /// The payload for a drag starting on the current selection.
    pub fn drag_payload(&self) -> DragPayload {
        let ws = self.app.workspace.read();
        let Some(pane) = ws.pane(self.pane) else {
            return DragPayload {
                count: 0,
                label: String::new(),
            };
        };
        let count = pane.selection.len();
        let label = pane
            .selection
            .iter()
            .next()
            .and_then(|id| pane.listing.get(id))
            .map(|e| e.name_str().into_owned())
            .unwrap_or_default();
        DragPayload { count, label }
    }

    /// Drop the dragging selection into `dest`: a `move` plan, or `copy` with ctrl held.
    pub fn drop_into(&mut self, dest: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let cmd = if window.modifiers().control {
            "copy"
        } else {
            "move"
        };
        self.run(
            cmd,
            serde_json::json!({ "dest": dest.to_string_lossy() }),
            window,
            cx,
        );
    }

    /// Paths dropped from another application: copy them into the current directory.
    pub fn drop_external(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dest = {
            let ws = self.app.workspace.read();
            ws.pane(self.pane)
                .and_then(|p| p.location().and_then(bibli_core::Location::path))
        };
        let Some(dest) = dest else {
            self.set_status("drop target is not a directory", true, cx);
            return;
        };
        let steps: Vec<Box<dyn op::Operation>> = paths
            .iter()
            .filter(|p| p.parent() != Some(dest.as_path()))
            .filter_map(|p| {
                let id = fs::stat::id_of(p)?;
                Some(Box::new(op::Copy {
                    entry: EntryRef::new(id, p.clone()),
                    dest_dir: dest.clone(),
                    new_name: None,
                }) as Box<dyn op::Operation>)
            })
            .collect();
        if steps.is_empty() {
            return;
        }
        let n = steps.len();
        let plan = Plan::with_steps(format!("copy {n} dropped items"), steps);
        if plan.is_applicable() {
            self.app.apply(plan);
        } else {
            self.set_status("dropped items conflict with existing ones", true, cx);
        }
        let _ = window;
    }
}

/// Make a stateful element draggable, carrying the selection.
pub fn draggable(el: Stateful<Div>, shell: &Shell) -> Stateful<Div> {
    let payload = shell.drag_payload();
    if payload.count == 0 {
        return el;
    }
    el.on_drag(payload, |p: &DragPayload, position, _, cx| {
        let payload = p.clone();
        cx.new(|_| DragPreview { payload, position })
    })
}

/// Make an element a drop target for internal drags into `dir`.
pub fn droppable_dir<E: InteractiveElement>(el: E, dir: PathBuf, cx: &mut Context<Shell>) -> E {
    let theme = cx.global::<Theme>().clone();
    el.drag_over::<DragPayload>(move |s, _, _, _| {
        s.bg(theme.selection_bg).border_color(theme.accent)
    })
    .on_drop(cx.listener(move |this, _: &DragPayload, window, cx| {
        this.drop_into(&dir, window, cx);
    }))
}
