//! Toasts: what just happened, with the way back. One message at a time sits
//! above the panes' footers; a running operation shows its progress there with
//! Pause and Cancel buttons and turns into its result ("Moved 3 items to
//! Trash", Undo).
//!
//! The only animation is a short entrance that ends by itself; nothing here
//! keeps the UI awake.

use std::path::Path;
use std::time::Duration;

use bibli_core::op::{Change, ExecEvent, PlanId, RunKind, Transfer};
use gpui::{Animation, AnimationExt, AnyElement, Context, div, prelude::*, px};

use crate::controls::{self, ButtonStyle};
use crate::format;
use crate::shell::Shell;
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Info,
    Success,
    Error,
}

/// A button on a toast: a label and the command it runs.
#[derive(Clone, Debug)]
pub struct Action {
    pub label: String,
    pub command: String,
    pub args: serde_json::Value,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub text: String,
    pub tone: Tone,
    pub action: Option<Action>,
}

/// A plan being applied.
#[derive(Clone, Debug)]
pub struct Running {
    pub plan: PlanId,
    pub title: String,
    pub run: RunKind,
    pub total: usize,
    /// First change that was applied, to name a single item in the result.
    first: Option<Change>,
    /// The change whose step failed, to say what could not be done.
    failed: Option<Change>,
    /// Waiting where it would answer Cancel, until resumed.
    paused: bool,
    cancelling: bool,
}

#[derive(Default)]
pub struct ToastState {
    pub current: Option<Toast>,
    /// Plans started and not finished, oldest first. Events name their
    /// plan, and each is matched to its own: a second plan starting never
    /// takes over the first one's bar or result.
    pub running: Vec<Running>,
    generation: u64,
    close: Option<gpui::FocusHandle>,
    /// Kept across frames, so Tab can reach the toast's button.
    action_focus: Option<gpui::FocusHandle>,
    /// Pause and Resume, one button: its focus stays as its label changes.
    pause_focus: Option<gpui::FocusHandle>,
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn folder_of(path: &Path) -> String {
    path.parent().map_or_else(String::new, name_of)
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// What an applied plan did, in a sentence: the item by name when there is
/// one, a count otherwise. Falls back to the plan's title.
pub fn describe(title: &str, run: RunKind, first: Option<&Change>, applied: usize) -> String {
    let many = format::count(applied, "item");
    let one = applied == 1;
    let quoted = |path: &Path| format!("“{}”", name_of(path));
    match first {
        _ if run == RunKind::Undo => {
            format!("Undid: {}", title.strip_prefix("undo ").unwrap_or(title))
        }
        Some(Change::Trash { path }) if one => format!("Moved {} to Trash", quoted(path)),
        Some(Change::Trash { .. }) => format!("Moved {many} to Trash"),
        Some(Change::Restore { path }) if one => format!("Restored {}", quoted(path)),
        Some(Change::Restore { .. }) => format!("Restored {many}"),
        Some(Change::Delete { path }) if one => format!("Deleted {} permanently", quoted(path)),
        Some(Change::Delete { .. }) => format!("Deleted {many} permanently"),
        Some(Change::Rename { from, to }) if one => {
            format!("Renamed {} to {}", quoted(from), quoted(to))
        }
        Some(Change::Rename { .. }) => format!("Renamed {many}"),
        // By the name it arrived under: a file of the trash goes back to
        // its own (`draft.txt`, not the trash's `draft.txt.2`).
        Some(Change::Move { to, .. }) if one => {
            format!("Moved {} to {}", quoted(to), folder_of(to))
        }
        Some(Change::Move { to, .. }) => format!("Moved {many} to {}", folder_of(to)),
        Some(Change::Copy { from, to }) if one => {
            format!("Copied {} to {}", quoted(from), folder_of(to))
        }
        Some(Change::Copy { to, .. }) => format!("Copied {many} to {}", folder_of(to)),
        Some(Change::Create {
            path,
            directory: true,
        }) if one => format!("Created folder {}", quoted(path)),
        Some(Change::Create { path, .. }) if one => format!("Created {}", quoted(path)),
        // A step that says what it did says it best.
        Some(Change::Metadata {
            words: Some(words), ..
        }) if one => capitalized(&words.did),
        _ if applied == 0 => format!("{}: nothing to do", capitalized(title)),
        _ => format!("{} ({many})", capitalized(title)),
    }
}

/// What a plan that did nothing could not do, in a sentence: the step that
/// failed, by name, then why. An error that begins by naming that step
/// ("“a.txt” could not be renamed …") gives its reason alone after it.
pub fn failure(title: &str, failed: Option<&Change>, error: &str) -> String {
    let quoted = |path: &Path| format!("“{}”", name_of(path));
    let tried = match failed {
        Some(Change::Copy { from, to }) => {
            format!("Couldn't copy {} to {}", quoted(from), folder_of(to))
        }
        Some(Change::Move { from, to }) => {
            format!("Couldn't move {} to {}", quoted(from), folder_of(to))
        }
        Some(Change::Trash { path }) => format!("Couldn't move {} to Trash", quoted(path)),
        Some(Change::Restore { path }) => format!("Couldn't restore {}", quoted(path)),
        Some(Change::Delete { path }) => format!("Couldn't delete {}", quoted(path)),
        Some(Change::Rename { from, to }) => {
            format!("Couldn't rename {} to {}", quoted(from), quoted(to))
        }
        Some(Change::Create {
            path,
            directory: true,
        }) => format!("Couldn't create folder {}", quoted(path)),
        Some(Change::Create { path, .. }) => format!("Couldn't create {}", quoted(path)),
        _ => return format!("{}: {error}", capitalized(title)),
    };
    let named = failed.and_then(|change| match change {
        Change::Copy { from: path, .. }
        | Change::Move { from: path, .. }
        | Change::Rename { from: path, .. }
        | Change::Trash { path }
        | Change::Restore { path }
        | Change::Delete { path }
        | Change::Create { path, .. } => Some(quoted(path)),
        _ => None,
    });
    let reason = named
        .and_then(|name| error.strip_prefix(&format!("{name} could not be ")))
        .and_then(|rest| rest.split_once(": "))
        .map_or(error, |(_, reason)| reason);
    format!("{tried}: {reason}")
}

/// What a running plan's bar says: how far it is, from 0 to 1, and in
/// words. A plan of several items counts them, the bytes of the one under
/// way moving the bar between two counts: its bytes are known only as each
/// item starts. A plan of one item counts its bytes, known before it
/// starts.
pub fn bar(items: usize, t: &Transfer) -> (f32, String) {
    if items <= 1 {
        let words = format!("{} of {}", format::size(t.done), format::size(t.total));
        return ((t.done as f32 / t.total.max(1) as f32).clamp(0., 1.), words);
    }
    let within = if t.step_total > 0 {
        (t.step_done as f32 / t.step_total as f32).clamp(0., 1.)
    } else {
        0.
    };
    let count = format!(
        "{} of {}",
        format::thousands(t.ended),
        format::count(items, "item")
    );
    let words = if t.done > 0 {
        format!("{count} · {}", format::size(t.done))
    } else {
        count
    };
    let fraction = (t.ended as f32 + within) / items as f32;
    (fraction.clamp(0., 1.), words)
}

impl Shell {
    /// Show `toast`, replacing the current one, for a time that fits its
    /// weight: errors and anything with a button stay longer.
    pub fn toast(&mut self, toast: Toast, cx: &mut Context<Self>) {
        let seconds = match (&toast.tone, &toast.action) {
            (Tone::Error, _) => 10,
            (_, Some(_)) => 8,
            _ => 4,
        };
        self.toasts.current = Some(toast);
        self.toasts.generation += 1;
        let generation = self.toasts.generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(seconds))
                .await;
            this.update(cx, |shell, cx| {
                if shell.toasts.generation == generation {
                    shell.toasts.current = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub fn dismiss_toast(&mut self, cx: &mut Context<Self>) {
        self.toasts.current = None;
        self.toasts.generation += 1;
        cx.notify();
    }

    /// Apply pending executor events: progress while a plan runs, then one
    /// toast saying what it did and offering the way back.
    pub fn drain_executor(&mut self, cx: &mut Context<Self>) {
        let rx = self.app.executor.events().clone();
        let mut any = false;
        while let Ok(event) = rx.try_recv() {
            any = true;
            match event {
                ExecEvent::Started {
                    plan,
                    title,
                    total,
                    run,
                } => {
                    self.toasts.running.push(Running {
                        plan,
                        title,
                        run,
                        total,
                        first: None,
                        failed: None,
                        paused: false,
                        cancelling: false,
                    });
                }
                ExecEvent::Paused { plan } | ExecEvent::Resumed { plan } => {
                    let paused = matches!(event, ExecEvent::Paused { .. });
                    if let Some(running) = self.toasts.running.iter_mut().find(|r| r.plan == plan) {
                        running.paused = paused;
                    }
                }
                ExecEvent::Step(progress) => {
                    if let Some(running) = self
                        .toasts
                        .running
                        .iter_mut()
                        .find(|r| r.plan == progress.plan)
                    {
                        running.total = progress.total;
                        if progress.error.is_some() {
                            running.failed = Some(progress.change);
                        } else if running.first.is_none() {
                            running.first = Some(progress.change);
                        }
                    }
                }
                ExecEvent::Finished {
                    plan,
                    applied,
                    error,
                    cancelled,
                    undoable,
                    ..
                } => {
                    let Some(index) = self.toasts.running.iter().position(|r| r.plan == plan)
                    else {
                        continue;
                    };
                    let running = self.toasts.running.remove(index);
                    let (title, run, first) = (running.title, running.run, running.first);
                    let done = describe(&title, run, first.as_ref(), applied);
                    let (text, tone) = match (error, cancelled) {
                        (Some(error), _) if applied > 0 => {
                            (format!("{done}, then stopped: {error}"), Tone::Error)
                        }
                        (Some(error), _) => (
                            failure(&title, running.failed.as_ref(), &error),
                            Tone::Error,
                        ),
                        (None, true) => (format!("Cancelled. {done}"), Tone::Info),
                        (None, false) => (done, Tone::Success),
                    };
                    let action = if run == RunKind::Undo {
                        self.app.executor.can_redo().then(|| Action {
                            label: "Redo".into(),
                            command: "redo".into(),
                            args: serde_json::json!({}),
                        })
                    } else {
                        (undoable && applied > 0).then(|| Action {
                            label: "Undo".into(),
                            command: "undo".into(),
                            args: serde_json::json!({}),
                        })
                    };
                    self.toast(Toast { text, tone, action }, cx);
                }
            }
        }
        if any {
            cx.notify();
        }
    }
}

pub fn render(shell: &mut Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let theme = cx.global::<Theme>().clone();
    let close = shell
        .toasts
        .close
        .get_or_insert_with(|| cx.focus_handle())
        .clone();
    let card = |tone: Tone| {
        div()
            .max_w(px(560.))
            .min_w(px(260.))
            .pl_3()
            .pr_1()
            .py_1()
            .flex()
            .items_center()
            .gap_3()
            .rounded_lg()
            .border_1()
            .border_color(match tone {
                Tone::Error => theme.error,
                _ => theme.border,
            })
            .bg(theme.bg)
            .shadow_lg()
            .occlude()
            // A click on the toast leaves the keyboard where it was: the
            // toast stays up while its plan is paused, and the keys must go
            // on acting on the pane. Tab still reaches its buttons.
            .capture_any_mouse_down(|_, window, _| window.prevent_default())
    };
    let glyph = |tone: Tone| match tone {
        Tone::Success => controls::glyph("check", theme.success, 15.),
        Tone::Error => controls::glyph("warning", theme.error, 15.),
        Tone::Info => controls::glyph("info", theme.text_muted, 15.),
    };
    // A plan with a single step finishes before anyone could read a bar,
    // unless the step moves bytes (one large copy): then the bytes are the bar.
    let progress = shell.toasts.running.iter().find_map(|running| {
        let transfer = shell.app.executor.transfer(running.plan)?;
        (running.total > 1 || transfer.total > 0).then_some((running, transfer))
    });
    let content = if let Some((running, transfer)) = progress {
        let (fraction, amount) = bar(running.total, &transfer);
        let plan = running.plan;
        let paused = running.paused;
        let focus = close.clone();
        // What the user did meanwhile waits on the lane behind this plan,
        // and paused, for as long as it stays paused: say so.
        let waiting = match shell.app.executor.queued() {
            0 => String::new(),
            n => format!(" · {} waiting", format::count(n, "action")),
        };
        let state = if paused { "paused at " } else { "" };
        let pause_focus = shell
            .toasts
            .pause_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        card(Tone::Info)
            .py_2()
            .child(
                div()
                    .flex_1()
                    .min_w(px(240.))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(format!(
                        "{} — {state}{amount}{waiting}",
                        capitalized(&running.title)
                    ))
                    .child(
                        div()
                            .h(px(4.))
                            .w_full()
                            .rounded_full()
                            .bg(theme.border)
                            .child(
                                div()
                                    .h_full()
                                    .rounded_full()
                                    .bg(if paused {
                                        theme.text_muted
                                    } else {
                                        theme.accent
                                    })
                                    .w(gpui::relative(fraction)),
                            ),
                    ),
            )
            .when(!running.cancelling, |card| {
                card.child(
                    controls::button(
                        "toast-pause",
                        if paused { "Resume" } else { "Pause" },
                        None,
                        ButtonStyle::Plain,
                        &pause_focus,
                        &theme,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if paused {
                            this.app.executor.resume(plan);
                        } else {
                            this.app.executor.pause(plan);
                        }
                        cx.notify();
                    })),
                )
            })
            .child(
                controls::button(
                    "toast-cancel",
                    if running.cancelling {
                        "Stopping…"
                    } else {
                        "Cancel"
                    },
                    None,
                    ButtonStyle::Plain,
                    &focus,
                    &theme,
                )
                .mr_2()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.app.executor.cancel(plan)
                        && let Some(running) =
                            this.toasts.running.iter_mut().find(|r| r.plan == plan)
                    {
                        running.cancelling = true;
                    }
                    cx.notify();
                })),
            )
    } else {
        let toast = shell.toasts.current.clone()?;
        let mut card = card(toast.tone)
            .child(glyph(toast.tone))
            .child(div().flex_1().min_w_0().py_1().child(toast.text.clone()));
        if let Some(action) = toast.action {
            let focus = shell
                .toasts
                .action_focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone();
            let label = action.label.clone();
            card = card.child(
                controls::button(
                    "toast-action",
                    label,
                    None,
                    ButtonStyle::Plain,
                    &focus,
                    &theme,
                )
                .text_color(theme.accent)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.dismiss_toast(cx);
                    this.run(&action.command, action.args.clone(), window, cx);
                })),
            );
        }
        card.child(
            controls::icon_button("toast-close", "close", &close, &theme)
                .on_click(cx.listener(|this, _, _, cx| this.dismiss_toast(cx))),
        )
    };
    let generation = shell.toasts.generation;
    Some(
        div()
            .absolute()
            .bottom(px(38.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(content.with_animation(
                ("toast", generation),
                Animation::new(Duration::from_millis(140)).with_easing(gpui::ease_out_quint()),
                |card, delta| card.opacity(delta).mt(px(8. * (1. - delta))),
            ))
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::{bar, describe, failure};
    use bibli_core::op::{Change, RunKind, Transfer};
    use std::path::PathBuf;

    #[test]
    fn a_bar_counts_items_and_moves_with_the_bytes_of_the_one_under_way() {
        // Many small files: the count, never a byte total that grows as
        // fast as the bytes do.
        let small = Transfer {
            done: 82_304,
            total: 82_368,
            ended: 1286,
            step_done: 0,
            step_total: 64,
        };
        let (fraction, words) = bar(40_000, &small);
        assert_eq!(words, "1,286 of 40,000 items · 80.4 KB");
        assert!((fraction - 1286. / 40_000.).abs() < 1e-6);
        // Three large folders: halfway through the second is half of a third.
        let large = Transfer {
            done: 3 << 30,
            total: 4 << 30,
            ended: 1,
            step_done: 1 << 30,
            step_total: 2 << 30,
        };
        assert!((bar(3, &large).0 - 0.5).abs() < 1e-6);
        // Nothing moved yet, or nothing to move: the count alone.
        assert_eq!(bar(2, &Transfer::default()), (0., "0 of 2 items".into()));
        // One item counts its bytes, known before it starts.
        let one = Transfer {
            done: 1 << 30,
            total: 4 << 30,
            ..Transfer::default()
        };
        assert_eq!(bar(1, &one), (0.25, "1.0 GB of 4.0 GB".into()));
    }

    #[test]
    fn a_failure_says_what_could_not_be_done_and_why() {
        let copy = Change::Copy {
            from: PathBuf::from("/a/notes/it's a draft.txt"),
            to: PathBuf::from("/a/locked/it's a draft.txt"),
        };
        assert_eq!(
            failure("copy to /a/locked", Some(&copy), "permission denied"),
            "Couldn't copy “it's a draft.txt” to locked: permission denied"
        );
        let rename = Change::Rename {
            from: PathBuf::from("/a/a.txt"),
            to: PathBuf::from("/a/b.txt"),
        };
        assert_eq!(
            failure(
                "rename",
                Some(&rename),
                "“a.txt” could not be renamed “b.txt”: the name is too long, or not allowed there"
            ),
            "Couldn't rename “a.txt” to “b.txt”: the name is too long, or not allowed there",
            "the reason alone, not the step twice"
        );
        assert_eq!(
            failure("tag 2 files", None, "the drive is full"),
            "Tag 2 files: the drive is full"
        );
    }

    #[test]
    fn results_name_one_item_and_count_several() {
        let trash = Change::Trash {
            path: PathBuf::from("/example/it's a draft.txt"),
        };
        assert_eq!(
            describe("move to trash", RunKind::Apply, Some(&trash), 1),
            "Moved “it's a draft.txt” to Trash"
        );
        assert_eq!(
            describe("move to trash", RunKind::Apply, Some(&trash), 3),
            "Moved 3 items to Trash"
        );
        let copy = Change::Copy {
            from: PathBuf::from("/example/a.txt"),
            to: PathBuf::from("/example/Archive 2026/a.txt"),
        };
        assert_eq!(
            describe("copy", RunKind::Apply, Some(&copy), 2),
            "Copied 2 items to Archive 2026"
        );
        assert_eq!(
            describe("undo move to trash", RunKind::Undo, Some(&trash), 1),
            "Undid: move to trash"
        );
        assert_eq!(
            describe("add tag", RunKind::Apply, None, 2),
            "Add tag (2 items)"
        );
        assert_eq!(
            describe("paste", RunKind::Apply, None, 0),
            "Paste: nothing to do"
        );
    }
}
