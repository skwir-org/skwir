//! Drag and drop: internal moves/copies between rows, cells, columns, and
//! sidebar places, and external paths dropped from other applications.

use std::path::{Path, PathBuf};

use bibli_core::op::{self, EntryRef};
use bibli_core::os_path::PathArg;
use bibli_core::workspace::PaneId;
use bibli_core::{Plan, app::NotifyLevel, fs, scheduler::Priority, settings::Settings};
use gpui::{
    Context, Div, ExternalPaths, Pixels, Point, Render, Stateful, Window, div, prelude::*, px,
};

use crate::shell::Shell;
use crate::theme::Theme;

/// The copy plan for foreign paths dropped into `dest`. Entries are
/// identified by device and inode and the plan is checked before anything
/// applies, both of which stat every path: call this on a worker.
fn dropped_copy_plan(
    paths: &[PathBuf],
    dest: &Path,
    settings: &Settings,
) -> Result<Plan, &'static str> {
    let steps: Vec<Box<dyn op::Operation>> = paths
        .iter()
        .filter_map(|p| {
            let id = fs::stat::id_of(p)?;
            Some(Box::new(op::Copy::from_settings(
                EntryRef::new(id, p.clone()),
                dest.to_path_buf(),
                settings,
            )) as Box<dyn op::Operation>)
        })
        .collect();
    if steps.is_empty() {
        return Err("dropped items no longer exist");
    }
    let n = steps.len();
    let plan = Plan::with_steps(format!("copy {n} dropped items"), steps);
    if plan.is_applicable() {
        Ok(plan)
    } else {
        Err("dropped items conflict with existing ones")
    }
}

/// What a drag carries: only the pane it started in. Rows are made
/// draggable every frame, so the payload must cost nothing to build; the
/// selection it moves is read when the drag starts and when it drops.
#[derive(Clone, Copy, Debug)]
pub struct DragPayload {
    pub pane: PaneId,
}

/// The floating preview under the cursor. Empty once the compositor owns
/// the drag: it draws the icon itself, and this element would only freeze
/// at the last in-window pointer position.
pub struct DragPreview {
    count: usize,
    label: String,
    position: Point<Pixels>,
    hidden: bool,
}

impl DragPreview {
    /// One thing dragged inside the window, under its name.
    pub fn named(label: String, position: Point<Pixels>) -> Self {
        DragPreview {
            count: 1,
            label,
            position,
            hidden: false,
        }
    }
}

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.hidden {
            return div();
        }
        let theme = cx.global::<Theme>().clone();
        let text = if self.count == 1 {
            self.label.clone()
        } else {
            format!("{} items", self.count)
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
                    .text_color(theme.on_fill)
                    .text_xs()
                    .shadow_md()
                    .child(text),
            )
    }
}

impl Shell {
    /// The paths of `pane`'s selection: what a drag from it carries.
    pub fn drag_paths(&self, pane: PaneId) -> Vec<PathBuf> {
        let ws = self.app.workspace.read();
        ws.pane(pane)
            .and_then(|pane| {
                let open = pane.open.as_ref()?;
                Some(
                    pane.selection
                        .iter()
                        .filter_map(|id| open.path_of(id))
                        .collect(),
                )
            })
            .unwrap_or_default()
    }

    /// Count and first name of `pane`'s selection, for the drag preview.
    fn drag_label(&self, pane: PaneId) -> (usize, String) {
        let ws = self.app.workspace.read();
        ws.pane(pane).map_or((0, String::new()), |pane| {
            let label = pane
                .selection
                .iter()
                .next()
                .and_then(|id| pane.listing.get(id))
                .map(|e| e.name_str().into_owned())
                .unwrap_or_default();
            (pane.selection.len(), label)
        })
    }

    /// Whether `paths` are exactly this pane's current selection: a drag that
    /// left through the compositor and came back to us.
    fn is_own_drag(&self, paths: &[PathBuf]) -> bool {
        let own = self.drag_paths(self.pane);
        !own.is_empty() && own.len() == paths.len() && own.iter().all(|p| paths.contains(p))
    }

    /// Paths dropped onto `dest`, from another application or from our own
    /// compositor-level drag: our own selection moves (copies with ctrl),
    /// foreign paths are copied.
    ///
    /// Only the own-drag check (a path comparison) runs here. Identifying the
    /// dropped entries and checking the plan stat every path, possibly on a
    /// sleeping disk, so they run on a worker that applies the result itself.
    pub fn drop_paths_into(
        &mut self,
        paths: &[PathBuf],
        dest: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_own_drag(paths) {
            self.drop_into(self.pane, dest, window, cx);
            return;
        }
        let paths: Vec<PathBuf> = paths
            .iter()
            .filter(|p| p.parent() != Some(dest))
            .cloned()
            .collect();
        if paths.is_empty() {
            return;
        }
        let app = self.app.clone();
        let dest = dest.to_path_buf();
        // The drop belongs to the pane it landed on, whatever is active later.
        let pane = self.pane;
        self.app.scheduler.spawn(
            Priority::Visible,
            "drop-copy-plan",
            self.app.lifetime().child(),
            move || match dropped_copy_plan(&paths, &dest, &app.settings) {
                Ok(plan) => {
                    tracing::debug!(?pane, steps = plan.steps.len(), "apply dropped paths");
                    app.apply(plan);
                }
                Err(reason) => app.notify(NotifyLevel::Error, reason),
            },
        );
    }

    /// Drop `from`'s selection into `dest`: a `move` plan, or `copy` with
    /// ctrl held. The pane the drag started in says what moves, whichever
    /// pane is active when it lands.
    pub fn drop_into(
        &mut self,
        from: PaneId,
        dest: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cmd = if window.modifiers().control {
            "copy"
        } else {
            "move"
        };
        self.run(
            cmd,
            serde_json::json!({ "dest": PathArg::from(dest), "pane": from }),
            window,
            cx,
        );
    }

    /// The folder `pane` shows, if it is one.
    pub fn pane_folder(&self, pane: PaneId) -> Option<PathBuf> {
        let ws = self.app.workspace.read();
        ws.pane(pane)
            .and_then(|p| p.location().and_then(bibli_core::Location::path))
    }

    /// Paths dropped onto the file area: into the current directory.
    pub fn drop_external(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dest) = self.pane_folder(self.pane) else {
            self.set_status("drop target is not a directory", true, cx);
            return;
        };
        self.drop_paths_into(paths, &dest, window, cx);
    }
}

/// Make a row or cell of `pane` draggable. Every row is, selected or not:
/// GPUI only starts a drag from an element that had a drag listener when it
/// was pressed, and the press is what selects the row. What the drag
/// carries (paths, label, icon) is built when it starts, not per frame.
pub fn draggable(el: Stateful<Div>, pane: PaneId, cx: &mut Context<Shell>) -> Stateful<Div> {
    let shell = cx.weak_entity();
    el.on_drag(
        DragPayload { pane },
        move |p: &DragPayload, position, window, cx| {
            let pane = p.pane;
            let (paths, (count, label), icon) = shell
                .update(cx, |shell, _| {
                    // A drag is not the click that collapses a multi-selection.
                    shell.press_collapse = None;
                    shell.with_pane(pane, |shell| {
                        (
                            shell.drag_paths(pane),
                            shell.drag_label(pane),
                            Some(shell.drag_icon()),
                        )
                    })
                })
                .unwrap_or_default();
            // Hand the drag to the compositor so other applications can accept it.
            // When that works, a drop back onto bibli arrives as ExternalPaths and
            // `drop_paths_into` recognizes it as our own selection.
            let external = !paths.is_empty() && window.start_external_drag(paths, icon);
            if !external {
                tracing::debug!("external drag unsupported here; in-window drag only");
            }
            cx.new(|_| DragPreview {
                count,
                label,
                position,
                hidden: external,
            })
        },
    )
}

/// Make an element a drop target into `dir`, for in-window drags and for
/// compositor drags (from other applications, or our own that went external).
pub fn droppable_dir<E: InteractiveElement>(el: E, dir: PathBuf, cx: &mut Context<Shell>) -> E {
    let theme = cx.global::<Theme>().clone();
    let (tint, edge) = (theme.drop_target, theme.accent);
    let dir2 = dir.clone();
    el.drag_over::<DragPayload>(move |s, _, _, _| s.bg(tint).border_color(edge))
        .on_drop(cx.listener(move |this, payload: &DragPayload, window, cx| {
            this.drop_into(payload.pane, &dir, window, cx);
        }))
        .drag_over::<ExternalPaths>(move |s, _, _, _| s.bg(tint).border_color(edge))
        .on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
            this.drop_paths_into(paths.paths(), &dir2, window, cx);
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch tree under the user's cache, never `/tmp` (see AGENTS.md).
    fn scratch(name: &str) -> PathBuf {
        let base = PathBuf::from(std::env::var_os("HOME").expect("home"))
            .join(".cache")
            .join("bibli-tests")
            .join(format!("dnd-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("scratch");
        base
    }

    #[test]
    fn dropped_paths_become_a_checked_copy_plan() {
        let base = scratch("plan");
        let src = base.join("from");
        let dest = base.join("into");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dest).unwrap();
        let file = src.join("it's a file.txt");
        std::fs::write(&file, b"hello").unwrap();
        let settings = Settings::in_memory();

        let plan = dropped_copy_plan(std::slice::from_ref(&file), &dest, &settings).expect("plan");
        assert_eq!(plan.steps.len(), 1);

        // A vanished source contributes no step; nothing is left to apply.
        assert_eq!(
            dropped_copy_plan(&[base.join("missing")], &dest, &settings).err(),
            Some("dropped items no longer exist")
        );

        // A different file under the same name conflicts instead of applying.
        std::fs::write(dest.join("it's a file.txt"), b"other contents").unwrap();
        assert_eq!(
            dropped_copy_plan(&[file], &dest, &settings).err(),
            Some("dropped items conflict with existing ones")
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
