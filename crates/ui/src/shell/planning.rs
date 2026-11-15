//! Plans made off the window's thread (AGENTS.md, principle 1): a command
//! the window runs that answers a plan (`Effect::Plans`: Delete, a paste,
//! Rename Several's Enter…) is made on a worker of its pane's lane, a
//! remote's where its folder is remote, never on the window's thread,
//! where making it would plan every rename or ask the system (a name
//! taken, a folder listed) while the window waits.
//!
//! - **What it acts on is fixed at the key.** The window takes what was
//!   asked (`App::asked`: the pane, its selection and cursor, the location
//!   it showed) before the worker runs, and the plan is made as asked
//!   (`skwir_core::app::as_asked`): an arrow, a click or another tab
//!   meanwhile changes nothing of it. What a plan finds in the panes
//!   (`Making`'s `shown`) is taken at the key too.
//! - **One at a time; what acts waits, what shows does not.** A command
//!   acts on what the window showed when the user asked. While a plan is
//!   made, what changes only what the window shows runs at once (its
//!   `Effect` says so, `waits`: `Shows`, `Reads`, `UserOnly`: the cursor,
//!   the selection, navigation, panels, a dialog's own keys), so each key
//!   finds the screen as the user saw it and the window stays live. What
//!   changes files or the program (`Plans`, `Acts`, Undo and Redo
//!   whatever they declare; a question's answer that makes a plan, which
//!   comes here as that plan) waits its turn in order (`Plans::waiting`),
//!   each with what its own key found, and runs once the plan is back and
//!   applied: as if the plan had been made at once. What is not a
//!   command (a click, scrolling, typing in a field) never waits.
//! - **Back on the window's thread**, it is the user's plan as before:
//!   `making`, `finishing::ran`, `apply_user_plan` for the pane it was
//!   asked from; an error said, a paste wanting the system's clipboard
//!   read first (`run_with_clipboard`).
//! - **Said when it takes time**: past the start notice's 0.3 s
//!   (`work::SHOWN_AFTER`), a toast says what is being worked out, by the
//!   command's title, for as long as it is, with Cancel; below that,
//!   nothing shows.
//! - **A way out**: Cancel, or Escape in the window (`ui.plan_cancel`,
//!   while `planning` is in the window's key context), lets it go: its
//!   token cancelled, its answer forgotten if it comes later, and what
//!   waited runs on. Closing the window cancels it too (`Shell::leave`).
//!   Agents and scripts dispatch elsewhere, as before.

use std::collections::VecDeque;

use gpui::{ClipboardItem, Context, Window};
use serde_json::json;
use skwir_core::CancelToken;
use skwir_core::app::{Asked, as_asked};
use skwir_core::command::{Effect, Invocation, Outcome};
use skwir_core::workspace::PaneId;

use super::{Making, Shell, Shown};
use crate::toast::{Action, Toast, Tone};

/// The plans this window makes off its thread, and what waits for them.
#[derive(Default)]
pub(crate) struct Plans {
    making: Option<Made>,
    waiting: VecDeque<Waiting>,
    /// Counts the plans made: an answer is taken only by its own.
    made: u64,
}

/// A plan being made.
struct Made {
    /// Which of the window's plans it is.
    id: u64,
    cancel: CancelToken,
    /// The toast saying it is worked out, by its generation, once shown.
    noticed: Option<u64>,
    /// What shows that toast once `work::SHOWN_AFTER` is up.
    _notice: gpui::Task<()>,
}

/// A command the window ran while a plan was made: run in its turn.
struct Waiting {
    name: String,
    args: serde_json::Value,
    /// What it was asked about at its key.
    asked: Asked,
}

impl Plans {
    /// Whether a plan is being made: what the window runs waits.
    pub(crate) fn making(&self) -> bool {
        self.making.is_some()
    }

    /// The window goes: the plan being made is let go, and what waited.
    pub(crate) fn cancel(&mut self) {
        if let Some(made) = self.making.take() {
            made.cancel.cancel();
        }
        self.waiting.clear();
    }
}

/// Whether `name` is made into a plan off the window's thread.
pub(super) fn plans(app: &skwir_core::App, name: &str) -> bool {
    app.commands
        .get(name)
        .is_some_and(|spec| spec.effect == Effect::Plans)
}

/// Whether `name` waits its turn behind a plan being made: what changes
/// files or the program (`Plans`, `Acts`, one that declares nothing, and
/// Undo and Redo whatever they declare) does; what changes only what the
/// window shows (`Shows`, `Reads`, `UserOnly`) runs at once. The one
/// place the window decides it.
pub(super) fn waits(app: &skwir_core::App, name: &str) -> bool {
    match app.commands.get(name).map(|spec| spec.effect) {
        Some(Effect::Shows | Effect::Reads | Effect::UserOnly) => matches!(name, "undo" | "redo"),
        Some(Effect::Plans | Effect::Acts) | None => true,
    }
}

impl Shell {
    /// What a command with `args` is asked about, now: its `pane`, else
    /// the active one, as `App::pane_or_active` reads it.
    pub(super) fn asked_now(&self, args: &serde_json::Value) -> Asked {
        let pane = args
            .get("pane")
            .and_then(|pane| serde_json::from_value::<PaneId>(pane.clone()).ok());
        self.app.asked(Some(self.app.pane_or_active(pane)))
    }

    /// `name` waits its turn behind the plan being made, with what it is
    /// asked about now.
    pub(super) fn wait_turn(&mut self, name: &str, args: serde_json::Value, asked: Option<Asked>) {
        let asked = asked.unwrap_or_else(|| self.asked_now(&args));
        self.plans.waiting.push_back(Waiting {
            name: name.to_owned(),
            args,
            asked,
        });
    }

    /// Make `inv`'s plan on a worker, as `asked`, and take it on return.
    pub(super) fn plan_off(
        &mut self,
        inv: Invocation,
        asked: Asked,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = asked.pane;
        let shown = self.shown_now();
        let lane = self.pane_lane(pane);
        let cancel = self.app.lifetime().child();
        let title = self
            .app
            .commands
            .get(&inv.name)
            .map_or_else(|| inv.name.to_string(), |spec| spec.title.clone());
        self.plans.made += 1;
        let id = self.plans.made;
        let notice = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(crate::work::SHOWN_AFTER)
                .await;
            let _ = this.update(cx, |shell, cx| {
                if shell.plans.making.as_ref().is_none_or(|m| m.id != id) {
                    return;
                }
                shell.notice(
                    Toast {
                        text: format!("Working out what {title} does…"),
                        tone: Tone::Info,
                        actions: vec![Action {
                            label: "Cancel".to_owned(),
                            command: "ui.plan_cancel".to_owned(),
                            args: json!({}),
                        }],
                    },
                    cx,
                );
                let shown = shell.toast_shown();
                if let Some(made) = shell.plans.making.as_mut() {
                    made.noticed = Some(shown);
                }
            });
        });
        self.plans.making = Some(Made {
            id,
            cancel: cancel.clone(),
            noticed: None,
            _notice: notice,
        });
        let app = std::sync::Arc::clone(&self.app);
        let asked_of = inv.clone();
        self.off_thread_until(
            lane,
            "plan",
            cancel,
            move || as_asked(asked, || app.dispatch(&asked_of)),
            move |shell, outcome, window, cx| {
                shell.planned(id, inv, pane, shown, outcome, window, cx);
            },
            window,
            cx,
        );
        cx.notify();
    }

    /// The plan being made stops being made: its toast taken away while it
    /// is the one shown. Whether it was plan `id` (any, when `None`).
    fn stop_making(&mut self, id: Option<u64>, cx: &mut Context<Self>) -> bool {
        let Some(made) = self
            .plans
            .making
            .take_if(|made| id.is_none_or(|id| made.id == id))
        else {
            return false;
        };
        made.cancel.cancel();
        if made.noticed == Some(self.toast_shown()) {
            self.dismiss_toast(cx);
        }
        true
    }

    /// Cancel, or Escape in the window (`ui.plan_cancel`): the plan being
    /// made is let go, its answer forgotten if it comes, and what waited
    /// runs on.
    pub(super) fn plan_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.stop_making(None, cx) {
            self.run_waiting(window, cx);
        }
        cx.notify();
    }

    /// What waited, in order, until the next plan to be made.
    fn run_waiting(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while !self.plans.making()
            && let Some(waiting) = self.plans.waiting.pop_front()
        {
            let Waiting { name, args, asked } = waiting;
            if plans(&self.app, &name) {
                self.carry_out_asked(&name, args, Some(asked), window, cx);
            } else {
                // As its key found the window, whatever it shows now.
                as_asked(asked, || {
                    self.carry_out_asked(&name, args, None, window, cx);
                });
            }
        }
    }

    /// Plan `id` of `inv`, asked of `pane`, is back: taken as the user's,
    /// then what waited runs in its turn. One let go of meanwhile is
    /// forgotten.
    #[allow(clippy::too_many_arguments)] // What the plan was asked with, and its answer.
    fn planned(
        &mut self,
        id: u64,
        inv: Invocation,
        pane: PaneId,
        shown: Vec<Shown>,
        outcome: anyhow::Result<Outcome>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.stop_making(Some(id), cx) {
            return;
        }
        match outcome {
            Ok(Outcome::Plan(plan)) => {
                self.making = (!shown.is_empty()).then_some(Making {
                    plan: plan.id,
                    asked: pane,
                    shown,
                    placed: Vec::new(),
                    renamed: Vec::new(),
                });
                crate::finishing::ran(cx, plan.id, self.window_id);
                self.apply_user_plan(plan, window, cx);
            }
            Ok(Outcome::Done | Outcome::Value(_)) => {}
            Ok(Outcome::Clipboard { text, said }) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.set_status(said, false, cx);
            }
            Ok(Outcome::Files { paths, cut }) => self.put_files_on_clipboard(paths, cut, cx),
            Err(e) if skwir_core::command::wants_clipboard(&e) => {
                self.run_with_clipboard(inv, window, cx);
            }
            Err(e) => self.report_error(&e, cx),
        }
        self.run_waiting(window, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;
    use skwir_core::App;
    use skwir_core::app::{AppConfig, as_asked};
    use skwir_core::command::{Invocation, Outcome};
    use skwir_core::location::Location;
    use skwir_core::workspace::PaneId;

    use super::waits;

    fn select(app: &App, pane: PaneId, names: &[&str]) {
        let mut ws = app.workspace.write();
        let p = ws.pane_mut(pane).expect("the pane");
        let ids: Vec<_> = names
            .iter()
            .map(|name| {
                p.listing
                    .find_by_name(std::ffi::OsStr::new(name))
                    .expect("listed")
                    .id
            })
            .collect();
        p.selection.set(ids);
    }

    /// The names a plan's steps touch, in words.
    fn touched(outcome: anyhow::Result<Outcome>) -> String {
        let Ok(Outcome::Plan(plan)) = outcome else {
            panic!("no plan: {outcome:?}");
        };
        plan.steps
            .iter()
            .map(|step| format!("{:?}", step.describe()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// While a plan is made, what only shows runs at once and what acts
    /// waits, each as its own key found the window: Delete after Ctrl+A,
    /// both behind a plan still made, deletes what Ctrl+A selected, and
    /// an arrow after it changes nothing of that (the window's order,
    /// `Shell::carry_out_asked` and `run_waiting`).
    #[test]
    fn what_shows_runs_at_once_and_what_acts_waits_with_its_own_key() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("a folder ‘asked’");
        std::fs::create_dir_all(&root).unwrap();
        for name in ["one's.txt", "two.txt", "three.txt"] {
            std::fs::write(root.join(name), b"x").unwrap();
        }
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        crate::commands::declare_all(&app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&root)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(30)));

        // Which side each is on, by what it declares.
        for shows in ["select_all", "ui.cursor", "navigate", "ui.plan_cancel"] {
            assert!(!waits(&app, shows), "{shows} runs at once");
        }
        for acts in ["trash", "delete", "undo", "redo", "ui.ask"] {
            assert!(waits(&app, acts), "{acts} waits its turn");
        }

        // A plan is made of one file, asked at its key.
        select(&app, pane, &["one's.txt"]);
        let first = app.asked(None);
        // Ctrl+A meanwhile: it shows, so it runs at once.
        app.dispatch(&Invocation::new("select_all")).unwrap();
        // Delete: it waits, with what its key found, every file.
        let second = app.asked(None);
        // An arrow after it, at once.
        select(&app, pane, &["two.txt"]);

        // In their turn: the first plan, then the one that waited.
        let trash = Invocation::with_args("trash", &json!({}));
        let said = touched(as_asked(first, || app.dispatch(&trash)));
        assert!(
            said.contains("one's.txt") && !said.contains("two.txt"),
            "{said}"
        );
        let said = touched(as_asked(second, || app.dispatch(&trash)));
        for name in ["one's.txt", "two.txt", "three.txt"] {
            assert!(said.contains(name), "{name}: {said}");
        }
    }
}
