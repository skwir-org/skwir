//! Drag and drop: internal moves/copies between rows, cells, columns, and
//! sidebar places, and external paths dropped from other applications.

use std::path::{Path, PathBuf};

use bibli_core::op::{self, EntryRef};
use bibli_core::{Plan, fs};
use gpui::{
    Context, Div, ExternalPaths, Pixels, Point, Render, Stateful, Window, div, prelude::*, px,
};

use crate::shell::Shell;
use crate::theme::Theme;

/// What a drag carries: the dragging pane's selection at drag start.
#[derive(Clone, Debug)]
pub struct DragPayload {
    pub count: usize,
    pub label: String,
    /// The selected paths, handed to the compositor so other applications can
    /// accept the drag.
    pub paths: Vec<PathBuf>,
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
                paths: Vec::new(),
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
        let paths = pane
            .open
            .as_ref()
            .map(|open| {
                pane.selection
                    .iter()
                    .filter_map(|id| open.path_of(id))
                    .collect()
            })
            .unwrap_or_default();
        DragPayload {
            count,
            label,
            paths,
        }
    }

    /// Whether `paths` are exactly this pane's current selection: a drag that
    /// left through the compositor and came back to us.
    fn is_own_drag(&self, paths: &[PathBuf]) -> bool {
        let own = self.drag_payload().paths;
        !own.is_empty() && own.len() == paths.len() && own.iter().all(|p| paths.contains(p))
    }

    /// Paths dropped onto `dest`, from another application or from our own
    /// compositor-level drag: our own selection moves (copies with ctrl),
    /// foreign paths are copied.
    pub fn drop_paths_into(
        &mut self,
        paths: &[PathBuf],
        dest: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_own_drag(paths) {
            self.drop_into(dest, window, cx);
            return;
        }
        let steps: Vec<Box<dyn op::Operation>> = paths
            .iter()
            .filter(|p| p.parent() != Some(dest))
            .filter_map(|p| {
                let id = fs::stat::id_of(p)?;
                Some(Box::new(op::Copy {
                    entry: EntryRef::new(id, p.clone()),
                    dest_dir: dest.to_path_buf(),
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

    /// Paths dropped onto the file area: into the current directory.
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
        self.drop_paths_into(paths, &dest, window, cx);
    }
}

/// Make a stateful element draggable, carrying the selection.
pub fn draggable(el: Stateful<Div>, shell: &Shell) -> Stateful<Div> {
    let payload = shell.drag_payload();
    if payload.count == 0 {
        return el;
    }
    el.on_drag(payload, |p: &DragPayload, position, window, cx| {
        // Hand the drag to the compositor so other applications can accept it.
        // When that works, a drop back onto bibli arrives as ExternalPaths and
        // `drop_paths_into` recognizes it as our own selection.
        if !window.start_external_drag(p.paths.clone()) {
            tracing::debug!("external drag unsupported here; in-window drag only");
        }
        let payload = p.clone();
        cx.new(|_| DragPreview { payload, position })
    })
}

/// Make an element a drop target into `dir`, for in-window drags and for
/// compositor drags (from other applications, or our own that went external).
pub fn droppable_dir<E: InteractiveElement>(el: E, dir: PathBuf, cx: &mut Context<Shell>) -> E {
    let theme = cx.global::<Theme>().clone();
    let theme2 = theme.clone();
    let dir2 = dir.clone();
    el.drag_over::<DragPayload>(move |s, _, _, _| {
        s.bg(theme.selection_bg).border_color(theme.accent)
    })
    .on_drop(cx.listener(move |this, _: &DragPayload, window, cx| {
        this.drop_into(&dir, window, cx);
    }))
    .drag_over::<ExternalPaths>(move |s, _, _, _| {
        s.bg(theme2.selection_bg).border_color(theme2.accent)
    })
    .on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
        this.drop_paths_into(paths.paths(), &dir2, window, cx);
    }))
}
