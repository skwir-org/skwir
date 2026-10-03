//! Review of proposed plans (from editors, agents, background work): a modal
//! listing each step's change and status, Apply or Cancel.

use bibli_core::Plan;
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

/// A proposed plan waiting for the user, with its dry run once a worker has
/// done it (it stats every step's files).
pub struct Review {
    pub plan: Plan,
    pub diff: Option<PlanDiff>,
}

impl Review {
    fn applicable(&self) -> Option<bool> {
        self.diff.as_ref().map(|diff| {
            !diff
                .steps
                .iter()
                .any(|(_, status)| matches!(status, StepStatus::Conflict { .. }))
        })
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
            self.reviews.push_back(Review { plan, diff: None });
            self.off_thread(
                "review-diff",
                move || dry.diff(),
                move |this, diff, _, cx| {
                    if let Some(review) = this.reviews.iter_mut().find(|r| r.plan.id == id) {
                        review.diff = Some(diff);
                        cx.notify();
                    }
                },
                window,
                cx,
            );
        }
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
            Some(Some(true)) => {
                if let Some(review) = self.reviews.pop_front() {
                    // Checked again when its turn comes: all or nothing.
                    self.app.apply_whole(review.plan);
                }
            }
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
        if let Some(review) = self.reviews.pop_front() {
            self.set_status(
                format!("Discarded proposal: {}", review.plan.title),
                false,
                cx,
            );
            cx.notify();
        }
    }
}

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let review = shell.reviews.front()?;
    let handles = shell.review_focus.as_ref()?;
    let focus = handles.focus.clone();
    let (cancel_focus, apply_focus) = (handles.cancel.clone(), handles.apply.clone());
    let theme = cx.global::<Theme>().clone();
    let plan = &review.plan;
    let applicable = review.applicable();
    let mut list = div()
        .flex()
        .flex_col()
        .gap_1()
        .max_h(px(380.))
        .overflow_hidden()
        .text_xs()
        .font_family(shell.mono_font.clone());
    let steps = plan.steps.len();
    match &review.diff {
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
    let pending = shell.reviews.len() - 1;
    let panel = modal::panel(720., &theme)
        .p_3()
        .gap_2()
        .track_focus(&focus)
        .key_context(keymap::REVIEW)
        .child(
            div()
                .flex()
                .items_center()
                .child(div().flex_1().child(format!("Proposed: {}", plan.title)))
                .child(div().text_xs().text_color(theme.text_muted).child(format!(
                    "{}{}",
                    crate::format::count(steps, "step"),
                    if pending > 0 {
                        format!(" · {pending} more waiting")
                    } else {
                        String::new()
                    }
                ))),
        )
        // What the plan left out, and why.
        .when_some(plan.note.clone(), |d, note| {
            d.child(div().text_xs().text_color(theme.text_muted).child(note))
        })
        .child(list)
        .child(
            div()
                .flex()
                .gap_2()
                .justify_end()
                .items_center()
                .child(div().text_xs().text_color(theme.text_muted).flex_1().child(
                    match applicable {
                        Some(true) => "Enter applies (undoable), Escape discards",
                        Some(false) => "Cannot apply: fix the conflicts first",
                        None => "Checking…",
                    },
                ))
                .child(
                    controls::button(
                        "review-cancel",
                        "Discard",
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
                        "Apply",
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
