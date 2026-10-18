//! Review of proposed plans (from editors, agents, background work): a modal
//! listing each step's change and status, Apply or Cancel. A question an
//! agent asks before a command that acts without a plan (ADR-044) waits
//! in the same queue and is answered the same way: Allow or Refuse; so do
//! the names a copy, a move or a link found taken (ADR-054, `clashes.rs`).

use gpui::{AnyElement, Context, FocusHandle, Window, div, prelude::*, px};
use skwir_core::Plan;
use skwir_core::app::{Asker, ClashQuestion, Question};
use skwir_core::op::{PlanDiff, StepStatus};

use crate::controls::{self, ButtonStyle};
use crate::keymap;
use crate::modal::{self, FocusReturn};
use crate::shell::Shell;
use crate::theme::Theme;

/// How a diff line renders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Pending,
    Done,
    Conflict,
}

/// The lines the dialog shows at most.
const SHOWN: usize = 60;

/// One line per step: the change, a status label, and its kind.
pub fn lines(diff: &PlanDiff) -> impl Iterator<Item = (String, &'static str, LineKind)> + '_ {
    diff.steps.iter().map(|(change, status)| match status {
        StepStatus::Pending { reversible: true } => {
            (change.to_string(), "pending", LineKind::Pending)
        }
        StepStatus::Pending { reversible: false } => (
            change.to_string(),
            "pending, not undoable",
            LineKind::Pending,
        ),
        StepStatus::AlreadyDone => (change.to_string(), "already done", LineKind::Done),
        StepStatus::Conflict { reason } => (
            format!("{change} — {reason}"),
            "conflict",
            LineKind::Conflict,
        ),
    })
}

/// A plan's dry run as the dialog shows it, made once when it is done:
/// its first lines and whether it applies. Made in every frame, a plan of
/// ten thousand steps formatted each one to show sixty.
pub struct Checked {
    lines: Vec<(String, &'static str, LineKind)>,
    applicable: bool,
    /// Its steps as it will be done: a folder merged is many.
    steps: usize,
    /// The names it finds taken that the user is asked about as it is
    /// applied (ADR-054): its lines show them settled as the defaults
    /// would.
    asked: usize,
}

impl Checked {
    fn of((diff, asked): &(PlanDiff, usize)) -> Self {
        Checked {
            lines: lines(diff).take(SHOWN).collect(),
            applicable: !diff
                .steps
                .iter()
                .any(|(_, status)| matches!(status, StepStatus::Conflict { .. })),
            steps: diff.steps.len(),
            asked: *asked,
        }
    }
}

/// What waits for the user's answer.
pub enum Review {
    /// A proposed plan, with its dry run once a worker has done it (it
    /// stats every step's files).
    Plan { plan: Plan, diff: Option<Checked> },
    /// A question: an agent asks to run a command that acts.
    Question(Question),
    /// What becomes of the names a copy, a move or a link found taken.
    Clashes(Box<crate::clashes::Clashes>),
}

impl Review {
    fn applicable(&self) -> Option<bool> {
        match self {
            Review::Plan { diff, .. } => diff.as_ref().map(|checked| checked.applicable),
            Review::Question(_) | Review::Clashes(_) => Some(true),
        }
    }

    pub fn is_question(&self) -> bool {
        matches!(self, Review::Question(_) | Review::Clashes(_))
    }
}

/// The keyboard while a proposal is shown: its own focus, and what had it.
pub struct ReviewFocus {
    pub focus: FocusHandle,
    pub(crate) cancel: FocusHandle,
    pub(crate) apply: FocusHandle,
    back: FocusReturn,
}

impl Shell {
    /// Queue proposals and dry-run each on a worker.
    pub fn add_reviews(&mut self, plans: Vec<Plan>, window: &mut Window, cx: &mut Context<Self>) {
        for plan in plans {
            let id = plan.id;
            let dry = plan.clone();
            let companions = std::sync::Arc::clone(&self.app.companions);
            self.reviews.push_back(Review::Plan { plan, diff: None });
            self.off_thread(
                "review-diff",
                move || Checked::of(&dry.review(&companions)),
                move |this, done, _, cx| {
                    let waiting = this.reviews.iter_mut().find_map(|r| match r {
                        Review::Plan { plan, diff } if plan.id == id => Some(diff),
                        _ => None,
                    });
                    if let Some(diff) = waiting {
                        *diff = Some(done);
                        cx.notify();
                    }
                },
                window,
                cx,
            );
        }
        cx.notify();
    }

    /// Queue questions asked of the user, after what waits already.
    pub fn add_questions(&mut self, questions: Vec<Question>, cx: &mut Context<Self>) {
        self.reviews
            .extend(questions.into_iter().map(Review::Question));
        cx.notify();
    }

    /// Queue the names plans found taken, after what waits already.
    pub fn add_clash_questions(&mut self, questions: Vec<ClashQuestion>, cx: &mut Context<Self>) {
        self.reviews.extend(
            questions
                .into_iter()
                .map(|q| Review::Clashes(Box::new(crate::clashes::Clashes::new(q)))),
        );
        cx.notify();
    }

    /// The front proposal takes the keyboard once no other dialog holds it
    /// (a proposal arriving under an open prompt waits for the prompt), and
    /// gives it back when the last one is answered.
    pub fn sync_review_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match (self.reviews.is_empty(), self.review_focus.is_some()) {
            (false, false) if !self.modal_open() => {
                let back = FocusReturn::capture(window, cx);
                let focus = cx.focus_handle();
                window.focus(&focus, cx);
                self.review_focus = Some(ReviewFocus {
                    focus,
                    cancel: cx.focus_handle(),
                    apply: cx.focus_handle(),
                    back,
                });
                cx.notify();
            }
            (true, true) => {
                if let Some(review) = self.review_focus.take() {
                    review.back.restore(window, &self.ui().focus, cx);
                }
                cx.notify();
            }
            _ => {}
        }
    }

    /// Apply the front proposal, if its dry run found no conflict. It
    /// stays in front otherwise, for the user to discard.
    pub fn review_apply(&mut self, cx: &mut Context<Self>) {
        match self.reviews.front().map(Review::applicable) {
            Some(Some(true)) => match self.reviews.pop_front() {
                // Checked again when its turn comes: all or nothing.
                Some(Review::Plan { plan, .. }) => self.app.apply_whole(plan),
                Some(Review::Question(question)) if !self.app.answer(question.id, true) => {
                    let said = match question.asker {
                        Asker::Agent => "The agent stopped waiting: nothing was run",
                        Asker::Room => "The copy stopped waiting: nothing was copied",
                    };
                    self.set_status(said, false, cx);
                }
                Some(Review::Clashes(clashes))
                    if !self
                        .app
                        .answer_clashes(clashes.question.id, Some(clashes.answer())) =>
                {
                    self.set_status("It stopped waiting: nothing was done", false, cx);
                }
                Some(Review::Question(_) | Review::Clashes(_)) | None => {}
            },
            Some(Some(false)) => {
                self.set_status(
                    "This proposal has conflicts; discard it or fix them",
                    true,
                    cx,
                );
            }
            Some(None) | None => {}
        }
        cx.notify();
    }

    pub fn review_cancel(&mut self, cx: &mut Context<Self>) {
        match self.reviews.pop_front() {
            Some(Review::Plan { plan, .. }) => {
                self.app.dismissed(plan.id);
                self.set_status(format!("Discarded proposal: {}", plan.title), false, cx);
            }
            Some(Review::Question(question)) => {
                self.app.answer(question.id, false);
                let said = match question.asker {
                    Asker::Agent => "Refused: the agent was told",
                    Asker::Room => "Cancelled: nothing was copied",
                };
                self.set_status(said, false, cx);
            }
            Some(Review::Clashes(clashes)) => {
                self.app.answer_clashes(clashes.question.id, None);
                let (_, done) = clashes.words();
                self.set_status(format!("Cancelled: nothing was {done}"), false, cx);
            }
            None => return,
        }
        cx.notify();
    }
}

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let review = shell.reviews.front()?;
    let handles = shell.review_focus.as_ref()?;
    if let Review::Clashes(clashes) = review {
        let pending = shell.reviews.len() - 1;
        return Some(crate::clashes::render(clashes, handles, pending, cx));
    }
    let focus = handles.focus.clone();
    let (cancel_focus, apply_focus) = (handles.cancel.clone(), handles.apply.clone());
    let theme = cx.global::<Theme>().clone();
    let applicable = review.applicable();
    let list = div()
        .flex()
        .flex_col()
        .gap_1()
        .max_h(px(380.))
        .overflow_hidden()
        .text_xs()
        .font_family(shell.mono_font.clone());
    let pending = shell.reviews.len() - 1;
    let more = if pending > 0 {
        format!(" · {pending} more waiting")
    } else {
        String::new()
    };
    // What is asked, how much there is, what it leaves out, its lines, and
    // the words of its keys and buttons.
    let (title, size, note, list, hint, refuse, allow) = match review {
        Review::Plan { plan, diff } => {
            let steps = diff.as_ref().map_or(plan.steps.len(), |d| d.steps);
            // The names taken, asked about as it is applied: its lines say
            // what the defaults would do.
            let asked = diff.as_ref().map_or(0, |d| d.asked);
            let asked = (asked > 0).then(|| {
                format!(
                    "{} already taken: you choose what becomes of {} as it is applied \
                     (shown here as both kept, folders merged).",
                    crate::format::count(asked, "name"),
                    if asked == 1 { "it" } else { "them" },
                )
            });
            let note = match (plan.note.clone(), asked) {
                (Some(note), Some(asked)) => Some(format!("{note} {asked}")),
                (note, asked) => note.or(asked),
            };
            (
                match &plan.by {
                    Some(who) => format!("{who} proposes: {}", plan.title),
                    None => format!("Proposed: {}", plan.title),
                },
                format!("{}{more}", crate::format::count(steps, "step")),
                note,
                plan_lines(shell, list, diff.as_ref(), steps, &theme),
                match applicable {
                    Some(true) => "Enter applies (undoable), Escape discards",
                    Some(false) => "Cannot apply: fix the conflicts first",
                    None => "Checking…",
                },
                "Discard",
                "Apply",
            )
        }
        Review::Clashes(_) => return None,
        Review::Question(question) => {
            let (note, hint, refuse, allow) = match question.asker {
                Asker::Agent => (
                    "It acts without a plan to review: allow it this once, or refuse and the agent is told.",
                    "Enter allows it once, Escape refuses",
                    "Refuse",
                    "Allow",
                ),
                Asker::Room => (
                    "Nothing is copied until you answer.",
                    "Enter copies anyway, Escape cancels",
                    "Cancel",
                    "Copy anyway",
                ),
            };
            (
                question.title.clone(),
                more.trim_start_matches(" · ").to_owned(),
                Some(note.to_owned()),
                question.detail.lines().take(40).fold(list, |list, line| {
                    list.child(div().text_color(theme.text).child(line.to_owned()))
                }),
                hint,
                refuse,
                allow,
            )
        }
    };
    let panel = modal::panel("review", 720., &theme)
        .when(shell.a11y, |d| d.aria_label(title.clone()))
        .p_3()
        .gap_2()
        .track_focus(&focus)
        .key_context(keymap::REVIEW)
        .child(
            div()
                .flex()
                .items_center()
                .child(div().flex_1().child(title))
                .child(div().text_xs().text_color(theme.text_muted).child(size)),
        )
        // What the plan left out, and why; what a question means.
        .when_some(note, |d, note| {
            d.child(div().text_xs().text_color(theme.text_muted).child(note))
        })
        .child(list)
        .child(modal::footer(
            hint,
            [
                controls::button(
                    "review-cancel",
                    refuse,
                    None,
                    ButtonStyle::Plain,
                    &cancel_focus,
                    &theme,
                )
                .on_click(cx.listener(|this, _, _, cx| this.review_cancel(cx))),
                controls::button(
                    "review-apply",
                    allow,
                    Some("check"),
                    ButtonStyle::Plain,
                    &apply_focus,
                    &theme,
                )
                .when(applicable != Some(true), |d| d.opacity(0.5))
                .on_click(cx.listener(|this, _, _, cx| this.review_apply(cx))),
            ],
            &theme,
        ));
    // Neither a press outside nor Escape on another dialog discards a
    // proposal: only its own buttons and keys answer it.
    Some(modal::modal(panel, 80., 6, |_, _, _| {}, &theme, cx))
}

/// A plan's lines: each step's change and status once its dry run is
/// done, the first 60.
fn plan_lines(
    shell: &Shell,
    mut list: gpui::Div,
    diff: Option<&Checked>,
    steps: usize,
    theme: &Theme,
) -> gpui::Div {
    match diff {
        Some(checked) => {
            for (i, (text, status, kind)) in checked.lines.iter().cloned().enumerate() {
                let color = match kind {
                    LineKind::Pending => theme.text,
                    LineKind::Done => theme.text_muted,
                    LineKind::Conflict => theme.error,
                };
                list = list.child(
                    div()
                        .flex()
                        .gap_2()
                        .text_color(color)
                        .child(crate::elements::clip::clipped(
                            div().id(("step", i)).flex_1().min_w_0().truncate(),
                            text,
                            shell.mono_font.clone(),
                        ))
                        .child(div().flex_none().text_color(theme.text_muted).child(status)),
                );
            }
        }
        None => {
            list = list.child(
                div()
                    .text_color(theme.text_muted)
                    .child("Checking each step…"),
            );
        }
    }
    if steps > SHOWN {
        list = list.child(
            div()
                .text_color(theme.text_muted)
                .child(format!("… and {} more steps", steps - SHOWN)),
        );
    }
    list
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use skwir_core::op::{Change, PlanDiff, StepStatus};

    use super::{LineKind, lines};

    #[test]
    fn diff_lines_carry_change_text_and_status() {
        let diff = PlanDiff {
            title: "t".into(),
            note: None,
            steps: vec![
                (
                    Change::Rename {
                        from: PathBuf::from("/a/x"),
                        to: PathBuf::from("/a/y"),
                    },
                    StepStatus::Pending { reversible: true },
                ),
                (
                    Change::Trash {
                        path: PathBuf::from("/a/z"),
                    },
                    StepStatus::AlreadyDone,
                ),
                (
                    Change::Metadata {
                        path: PathBuf::from("/a/w"),
                        field: "tags".into(),
                        words: None,
                    },
                    StepStatus::Conflict {
                        reason: "gone".into(),
                    },
                ),
            ],
        };
        let l: Vec<_> = lines(&diff).collect();
        assert_eq!(
            l[0],
            ("rename /a/x -> /a/y".into(), "pending", LineKind::Pending)
        );
        assert_eq!(l[1], ("trash /a/z".into(), "already done", LineKind::Done));
        assert_eq!(
            l[2],
            (
                "set tags on /a/w — gone".into(),
                "conflict",
                LineKind::Conflict
            )
        );
    }
}
