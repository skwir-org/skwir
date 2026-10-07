//! Review of proposed plans (from editors, agents, background work): a modal
//! listing each step's change and status, Apply or Cancel. A question an
//! agent asks before a command that acts without a plan (ADR-044) waits
//! in the same queue and is answered the same way: Allow or Refuse.

use bibli_core::Plan;
use bibli_core::app::Question;
use bibli_core::op::{PlanDiff, StepStatus};
use gpui::{AnyElement, Context, FocusHandle, Window, div, prelude::*, px};

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

/// One line per step: the change, a status label, and its kind.
pub fn lines(diff: &PlanDiff) -> Vec<(String, &'static str, LineKind)> {
    diff.steps
        .iter()
        .map(|(change, status)| match status {
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
        .collect()
}

/// What waits for the user's answer.
pub enum Review {
    /// A proposed plan, with its dry run once a worker has done it (it
    /// stats every step's files).
    Plan { plan: Plan, diff: Option<PlanDiff> },
    /// A question: an agent asks to run a command that acts.
    Question(Question),
}

impl Review {
    fn applicable(&self) -> Option<bool> {
        match self {
            Review::Plan { diff, .. } => diff.as_ref().map(|diff| {
                !diff
                    .steps
                    .iter()
                    .any(|(_, status)| matches!(status, StepStatus::Conflict { .. }))
            }),
            Review::Question(_) => Some(true),
        }
    }

    pub fn is_question(&self) -> bool {
        matches!(self, Review::Question(_))
    }
}

/// The keyboard while a proposal is shown: its own focus, and what had it.
pub struct ReviewFocus {
    pub focus: FocusHandle,
    cancel: FocusHandle,
    apply: FocusHandle,
    back: FocusReturn,
}

impl Shell {
    /// Queue proposals and dry-run each on a worker.
    pub fn add_reviews(&mut self, plans: Vec<Plan>, window: &mut Window, cx: &mut Context<Self>) {
        for plan in plans {
            let id = plan.id;
            let dry = plan.clone();
            self.reviews.push_back(Review::Plan { plan, diff: None });
            self.off_thread(
                "review-diff",
                move || dry.diff(),
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

    /// The front proposal takes the keyboard once no other dialog holds it
    /// (a proposal arriving under an open prompt waits for the prompt), and
    /// gives it back when the last one is answered.
    pub fn sync_review_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match (self.reviews.is_empty(), self.review_focus.is_some()) {
            (false, false) if !self.modal_open() => {
                let back = FocusReturn::capture(window, cx);
                let focus = cx.focus_handle();
                window.focus(&focus);
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
                    review.back.restore(window, &self.ui().focus);
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
                    self.set_status("The agent stopped waiting: nothing was run", false, cx);
                }
                Some(Review::Question(_)) | None => {}
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
                self.set_status("Refused: the agent was told", false, cx);
            }
            None => return,
        }
        cx.notify();
    }
}

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let review = shell.reviews.front()?;
    let handles = shell.review_focus.as_ref()?;
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
            let steps = plan.steps.len();
            (
                match &plan.by {
                    Some(who) => format!("{who} proposes: {}", plan.title),
                    None => format!("Proposed: {}", plan.title),
                },
                format!("{}{more}", crate::format::count(steps, "step")),
                plan.note.clone(),
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
        Review::Question(question) => (
            question.title.clone(),
            more.trim_start_matches(" · ").to_owned(),
            Some(
                "It acts without a plan to review: allow it this once, or refuse and the agent is told."
                    .to_owned(),
            ),
            question
                .detail
                .lines()
                .take(40)
                .fold(list, |list, line| {
                    list.child(div().text_color(theme.text).child(line.to_owned()))
                }),
            "Enter allows it once, Escape refuses",
            "Refuse",
            "Allow",
        ),
    };
    let panel = modal::panel(720., &theme)
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
        .child(
            div()
                .flex()
                .gap_2()
                .justify_end()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .flex_1()
                        .child(hint),
                )
                .child(
                    controls::button(
                        "review-cancel",
                        refuse,
                        None,
                        ButtonStyle::Plain,
                        &cancel_focus,
                        &theme,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.review_cancel(cx))),
                )
                .child(
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
                ),
        );
    // Neither a press outside nor Escape on another dialog discards a
    // proposal: only its own buttons and keys answer it.
    Some(modal::modal(panel, 80., 6, |_, _, _| {}, &theme, cx))
}

/// A plan's lines: each step's change and status once its dry run is
/// done, the first 60.
fn plan_lines(
    shell: &Shell,
    mut list: gpui::Div,
    diff: Option<&PlanDiff>,
    steps: usize,
    theme: &Theme,
) -> gpui::Div {
    match diff {
        Some(diff) => {
            for (i, (text, status, kind)) in lines(diff).into_iter().take(60).enumerate() {
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
    if steps > 60 {
        list = list.child(
            div()
                .text_color(theme.text_muted)
                .child(format!("… and {} more steps", steps - 60)),
        );
    }
    list
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bibli_core::op::{Change, PlanDiff, StepStatus};

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
        let l = lines(&diff);
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
