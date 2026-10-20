//! Review of proposed plans (from editors, agents, background work): a modal
//! listing each step's change and status, Apply or Cancel. A question an
//! agent asks before a command that acts without a plan (ADR-044) waits
//! in the same queue and is answered the same way: Allow or Refuse; so do
//! the names a copy, a move or a link found taken (ADR-054, `clashes.rs`).

use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, Context, FocusHandle, UniformListScrollHandle, Window, div, prelude::*, px,
};
use skwir_core::Plan;
use skwir_core::app::{Asker, ClashQuestion, Question};
use skwir_core::op::{Change, PlanDiff, StepStatus};

use crate::controls::{self, ButtonStyle};
use crate::elements::clip::clipped;
use crate::keymap;
use crate::modal::{self, FocusReturn, ListMove, Rows};
use crate::shell::Shell;
use crate::theme::Theme;

/// How a diff line renders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Pending,
    Done,
    Conflict,
}

/// A step's row, and the rows in sight before the list scrolls.
const ROWS: Rows = Rows {
    height: 20.,
    shown: 16,
};

/// A step's line: the change, its paths in `folder` written from there,
/// a status label, and its kind.
pub fn line(
    change: &Change,
    status: &StepStatus,
    folder: Option<&Path>,
) -> (String, &'static str, LineKind) {
    let said = match folder {
        Some(folder) => change.within(folder).to_string(),
        None => change.to_string(),
    };
    match status {
        StepStatus::Pending { reversible: true } => (said, "pending", LineKind::Pending),
        StepStatus::Pending { reversible: false } => {
            (said, "pending, not undoable", LineKind::Pending)
        }
        StepStatus::AlreadyDone => (said, "already done", LineKind::Done),
        StepStatus::Conflict { reason } => {
            (format!("{said} — {reason}"), "conflict", LineKind::Conflict)
        }
    }
}

/// The folder every step's paths are in, said once above the lines and
/// left out of them: the deepest that holds each path's own folder, so no
/// line says the folder itself as nothing. None at the root of a drive,
/// where nothing would be left out.
fn shared_folder(steps: &[(Change, StepStatus)]) -> Option<PathBuf> {
    let mut folders = steps
        .iter()
        .flat_map(|(change, _)| change.paths())
        .filter_map(Path::parent);
    let mut shared = folders.next()?.to_path_buf();
    for folder in folders {
        while !folder.starts_with(&shared) {
            if !shared.pop() {
                return None;
            }
        }
    }
    shared.parent().is_some().then_some(shared)
}

/// A plan's dry run as the dialog shows it, kept from when it is done:
/// every step and whether it applies. A step is made into its line only
/// while its row is on screen: a plan of ten thousand steps formats the
/// dozen shown.
pub struct Checked {
    /// Its steps as it will be done (a folder merged is many), each with
    /// what its dry run found.
    steps: Vec<(Change, StepStatus)>,
    /// The folder its steps' paths share (`shared_folder`), and how the
    /// line above the steps writes it (`~/…`).
    folder: Option<(PathBuf, String)>,
    applicable: bool,
    /// The names it finds taken that the user is asked about as it is
    /// applied (ADR-054): its lines show them settled as the defaults
    /// would.
    asked: usize,
}

impl Checked {
    fn of((diff, asked): (PlanDiff, usize)) -> Self {
        let home = skwir_core::os_path::home();
        Checked {
            applicable: !diff
                .steps
                .iter()
                .any(|(_, status)| matches!(status, StepStatus::Conflict { .. })),
            folder: shared_folder(&diff.steps).map(|folder| {
                let said = crate::complete::written(&folder, "~", home.as_deref());
                (folder, said)
            }),
            steps: diff.steps,
            asked,
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

/// The keyboard while a proposal is shown: its own focus, and what had it;
/// where its list is scrolled to.
pub struct ReviewFocus {
    pub focus: FocusHandle,
    pub(crate) cancel: FocusHandle,
    pub(crate) apply: FocusHandle,
    scroll: UniformListScrollHandle,
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
                move || Checked::of(dry.review(&companions)),
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
                    scroll: UniformListScrollHandle::new(),
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

    /// The front proposal, answered: the next one is read from its top.
    fn take_review(&mut self) -> Option<Review> {
        if let Some(focus) = &mut self.review_focus {
            focus.scroll = UniformListScrollHandle::new();
        }
        self.reviews.pop_front()
    }

    /// Up, Down and the page keys while a proposal has the keyboard
    /// (`ui.list_move`): they scroll its steps, or what a question says.
    /// The names a plan found taken are rows one walks (`clashes_move`).
    pub(crate) fn review_move(
        &mut self,
        mv: ListMove,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(review) = self
            .review_focus
            .as_ref()
            .filter(|r| r.focus.contains_focused(window, cx))
        else {
            return false;
        };
        ROWS.scroll(&review.scroll, mv);
        cx.notify();
        true
    }

    /// Apply the front proposal, if its dry run found no conflict. It
    /// stays in front otherwise, for the user to discard.
    pub fn review_apply(&mut self, cx: &mut Context<Self>) {
        match self.reviews.front().map(Review::applicable) {
            Some(Some(true)) => match self.take_review() {
                // Checked again when its turn comes: all or nothing.
                Some(Review::Plan { plan, .. }) => self.app.apply_whole(plan),
                Some(Review::Question(question)) if !self.app.answer(question.id, true) => {
                    let said = match question.asker {
                        Asker::Agent => "The agent stopped waiting: nothing was run",
                        Asker::Room => "The copy stopped waiting: nothing was copied",
                        Asker::Connection => "It stopped waiting: nothing was connected",
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
        match self.take_review() {
            Some(Review::Plan { plan, .. }) => {
                self.app.dismissed(plan.id);
                self.set_status(format!("Discarded proposal: {}", plan.title), false, cx);
            }
            Some(Review::Question(question)) => {
                self.app.answer(question.id, false);
                let said = match question.asker {
                    Asker::Agent => "Refused: the agent was told",
                    Asker::Room => "Cancelled: nothing was copied",
                    Asker::Connection => "Cancelled: nothing was connected",
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
            let steps = diff.as_ref().map_or(plan.steps.len(), |d| d.steps.len());
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
                plan_lines(diff.as_ref(), &handles.scroll, &theme, cx),
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
                Asker::Connection => (
                    "Nothing is sent to the server until you answer.",
                    "Enter connects, Escape cancels",
                    "Cancel",
                    "Connect",
                ),
            };
            (
                question.title.clone(),
                more.trim_start_matches(" · ").to_owned(),
                Some(note.to_owned()),
                ROWS.text(
                    "review-detail",
                    div().text_color(theme.text).child(question.detail.clone()),
                    &handles.scroll,
                    &theme,
                ),
                hint,
                refuse,
                allow,
            )
        }
    };
    // Where the steps' paths are, which their lines leave out.
    let within = match review {
        Review::Plan {
            diff: Some(checked),
            ..
        } => checked
            .folder
            .as_ref()
            .map(|(_, said)| format!("In {said}")),
        _ => None,
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
        .when_some(within, |d, within| {
            d.child(clipped(
                div()
                    .id("review-folder")
                    .text_xs()
                    .text_color(theme.text_muted)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_middle(),
                within,
            ))
        })
        .child(list.text_xs().font_family(theme.code_font.clone()))
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

/// A plan's steps, each one's change and status once its dry run is done,
/// all of them under a scrollbar.
fn plan_lines(
    diff: Option<&Checked>,
    scroll: &UniformListScrollHandle,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    match diff {
        Some(checked) => ROWS
            .list(
                "review-steps",
                checked.steps.len(),
                scroll,
                theme,
                |shell, range, _, cx| step_rows(shell, range, cx),
                cx,
            )
            .role(gpui::Role::List),
        None => ROWS.text(
            "review-steps",
            div()
                .text_color(theme.text_muted)
                .child("Checking each step…"),
            scroll,
            theme,
        ),
    }
}

/// The steps in `range` of the plan shown, the only ones made into lines.
fn step_rows(shell: &Shell, range: Range<usize>, cx: &mut Context<Shell>) -> Vec<AnyElement> {
    let Some(Review::Plan {
        diff: Some(checked),
        ..
    }) = shell.reviews.front()
    else {
        return Vec::new();
    };
    let theme = cx.global::<Theme>();
    let font = theme.code_font.clone();
    range
        .filter_map(|i| {
            let (change, status) = checked.steps.get(i)?;
            let folder = checked.folder.as_ref().map(|(folder, _)| folder.as_path());
            let (text, status, kind) = line(change, status, folder);
            let color = match kind {
                LineKind::Pending => theme.text,
                LineKind::Done => theme.text_muted,
                LineKind::Conflict => theme.error,
            };
            let said = shell.a11y.then(|| format!("{text}, {status}"));
            Some(
                div()
                    .id(("step-row", i))
                    .role(gpui::Role::ListItem)
                    .when_some(said, |d, said| d.aria_label(said).aria_position_in_set(i))
                    .w_full()
                    .h(px(ROWS.height))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .font_family(font.clone())
                    .text_color(color)
                    // Cut in the middle: a line ends with what it comes
                    // to (the new name, where it goes).
                    .child(clipped(
                        div()
                            .id(("step", i))
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_middle(),
                        text,
                    ))
                    .child(div().flex_none().text_color(theme.text_muted).child(status))
                    .into_any_element(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use skwir_core::op::{Change, PlanDiff, StepStatus};

    use super::{Checked, LineKind, line, shared_folder};

    /// The lines leave out the folder the steps share, which is said once;
    /// a step on that folder itself is said from the folder above.
    #[test]
    fn lines_are_said_from_the_folder_their_steps_share() {
        let at = |path: &str| PathBuf::from(format!("/data/it's a café{path}"));
        let pending = StepStatus::Pending { reversible: true };
        let steps = vec![
            (
                Change::Rename {
                    from: at("/sub/a  draft.txt"),
                    to: at("/sub/b.txt"),
                },
                pending.clone(),
            ),
            (
                Change::Move {
                    from: at("/c.txt"),
                    to: at("/archive/c.txt"),
                },
                pending.clone(),
            ),
        ];
        let folder = shared_folder(&steps);
        assert_eq!(folder, Some(at("")));
        let said: Vec<_> = steps
            .iter()
            .map(|(c, s)| line(c, s, folder.as_deref()).0)
            .collect();
        assert_eq!(
            said,
            [
                "rename sub/a  draft.txt -> sub/b.txt",
                "move c.txt -> archive/c.txt"
            ]
        );
        // The folder itself made: said from its own folder.
        let mut with_it = steps.clone();
        with_it.push((
            Change::Create {
                path: at(""),
                directory: true,
                words: None,
            },
            pending.clone(),
        ));
        assert_eq!(shared_folder(&with_it), Some(PathBuf::from("/data")));
        // Nothing shared but the root, or no step: every path whole.
        let apart = vec![(
            Change::Move {
                from: PathBuf::from("/a/x"),
                to: PathBuf::from("/b/x"),
            },
            pending,
        )];
        assert_eq!(shared_folder(&apart), None);
        assert_eq!(shared_folder(&[]), None);
        // The dry run says it as the address writes it.
        let checked = Checked::of((
            PlanDiff {
                title: "t".into(),
                note: None,
                steps,
            },
            0,
        ));
        let (folder, said) = checked.folder.unwrap();
        assert_eq!(folder, at(""));
        assert_eq!(
            said,
            crate::complete::written(&at(""), "~", skwir_core::os_path::home().as_deref())
        );
    }

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
        let l: Vec<_> = diff.steps.iter().map(|(c, s)| line(c, s, None)).collect();
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
