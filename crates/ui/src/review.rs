//! Review of proposed plans (from editors, agents, background work): a modal
//! listing each step's change and status, Apply or Cancel.

use bibli_core::op::{PlanDiff, StepStatus};
use gpui::{AnyElement, Context, MouseButton, deferred, div, prelude::*, px};

use crate::keymap;
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

impl Shell {
    pub fn review_apply(&mut self, cx: &mut Context<Self>) {
        if let Some(plan) = self.reviews.pop_front() {
            if plan.is_applicable() {
                self.app.apply(plan);
            } else {
                self.set_status("plan has conflicts; not applied", true, cx);
            }
            cx.notify();
        }
    }

    pub fn review_cancel(&mut self, cx: &mut Context<Self>) {
        if let Some(plan) = self.reviews.pop_front() {
            self.set_status(format!("discarded proposal: {}", plan.title), false, cx);
            cx.notify();
        }
    }
}

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let plan = shell.reviews.front()?;
    let focus = shell.review_focus.clone()?;
    let theme = cx.global::<Theme>().clone();
    let diff = plan.diff();
    let applicable = plan.is_applicable();
    let mut list = div()
        .flex()
        .flex_col()
        .gap_1()
        .max_h(px(380.))
        .overflow_hidden()
        .text_xs()
        .font_family(shell.mono_font.clone());
    for (text, status, kind) in lines(&diff).into_iter().take(60) {
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
                .child(div().flex_1().min_w_0().truncate().child(text))
                .child(div().flex_none().text_color(theme.text_muted).child(status)),
        );
    }
    if diff.steps.len() > 60 {
        list = list.child(
            div()
                .text_color(theme.text_muted)
                .child(format!("… and {} more steps", diff.steps.len() - 60)),
        );
    }
    let pending = shell.reviews.len() - 1;
    Some(
        deferred(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_start()
                .justify_center()
                .pt(px(80.))
                .bg(gpui::black().opacity(0.3))
                .occlude()
                .child(
                    div()
                        .track_focus(&focus)
                        .key_context(keymap::REVIEW)
                        .w(px(720.))
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .rounded_lg()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.surface)
                        .shadow_lg()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .child(div().flex_1().child(format!("Proposed: {}", plan.title)))
                                .child(div().text_xs().text_color(theme.text_muted).child(
                                    format!(
                                        "{} step{}{}",
                                        diff.steps.len(),
                                        if diff.steps.len() == 1 { "" } else { "s" },
                                        if pending > 0 {
                                            format!(" · {pending} more waiting")
                                        } else {
                                            String::new()
                                        }
                                    ),
                                )),
                        )
                        .child(list)
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .justify_end()
                                .items_center()
                                .child(div().text_xs().text_color(theme.text_muted).flex_1().child(
                                    if applicable {
                                        "Enter applies (undoable), Escape discards"
                                    } else {
                                        "Cannot apply: fix the conflicts first"
                                    },
                                ))
                                .child(
                                    div()
                                        .id("review-cancel")
                                        .px_3()
                                        .py_1()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(theme.border)
                                        .cursor_pointer()
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.review_cancel(cx)),
                                        )
                                        .child("Cancel"),
                                )
                                .child(
                                    div()
                                        .id("review-apply")
                                        .px_3()
                                        .py_1()
                                        .rounded_md()
                                        .bg(if applicable {
                                            theme.accent
                                        } else {
                                            theme.border
                                        })
                                        .text_color(gpui::white())
                                        .when(applicable, gpui::Styled::cursor_pointer)
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.review_apply(cx)),
                                        )
                                        .child("Apply"),
                                ),
                        ),
                ),
        )
        .with_priority(6)
        .into_any_element(),
    )
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
