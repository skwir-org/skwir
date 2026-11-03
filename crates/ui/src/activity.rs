//! Activity: what runs, docked at the window's foot (ADR-114).
//!
//! Work under way is the window's state, which ends the last pane's
//! footer (`status.rs`): a strip there says how far what runs has gone, in
//! as many words as the footer has room for (`Fit`), with Pause and Cancel
//! for a single action. A click on it opens Activity above it: each
//! action's card (its bar, speed and time left, Pause and Cancel), what
//! waits its turn, and what ended this session (`Earlier`), with its Undo
//! or Retry, kept in memory only. Pinned, Activity stands above the
//! footer instead, taking room rather than covering the files, while there
//! is something under way to show. Nothing floats over the files but the
//! latest answer (`toast.rs`).
//!
//! An action an error stopped stays in the strip, in the error's colour,
//! until it is retried, Activity is opened, or it is dismissed.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use gpui::{
    AnyElement, Context, FocusHandle, ScrollHandle, SharedString, Window, div, prelude::*, px,
};
use skwir_core::op::{PlanId, Transfer};

use skwir_platform::desktop::Asked;

use crate::controls::{self, ButtonStyle};
use crate::elements::adaptive::Fit;
use crate::elements::popover::PopoverAnchor;
use crate::format;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toast::{
    Action, Running, Toast, Tone, bar, capitalized, pace, sending, share, waiting_words,
};
use crate::work::{Seen, Watched};

/// Activity's popup, as `Shell::toggle_popup` and `toolbar_menu` know it.
pub const POPUP: &str = "activity";

/// What `Earlier` keeps: the answers of this session, newest first.
pub(crate) const EARLIER: usize = 20;

/// The key of the next answer kept in `Earlier`.
static NEXT_ENDED: AtomicU64 = AtomicU64::new(1);

/// The bar's width in the strip: as much words as the footer has room for
/// beside it, then less bar.
const BAR_WIDE: f32 = 54.;
const BAR_NARROW: f32 = 24.;

/// What ended this session, as its answer said it: a plan's, or any other
/// answer a toast said (a print's, a message), so that none is lost while
/// Activity is open and no toast is drawn.
#[derive(Clone, Debug)]
pub struct Ended {
    /// What tells it apart in `Earlier`: its row's and its buttons' ids.
    pub key: u64,
    /// The plan it answers; `None` for any other answer.
    pub plan: Option<PlanId>,
    pub text: String,
    pub tone: Tone,
    /// Its Undo, or Retry for one an error stopped (never Redo: what it
    /// would redo moves on); another answer's buttons as its toast had.
    pub actions: Vec<Action>,
    pub at: SystemTime,
}

impl Ended {
    /// The answer of `plan`, as `text`, `tone` and `actions` say it.
    pub(crate) fn of_plan(plan: PlanId, text: &str, tone: Tone, actions: &[Action]) -> Ended {
        let actions = actions
            .iter()
            .filter(|action| action.command != "redo")
            .cloned()
            .collect();
        Ended::new(Some(plan), text.to_owned(), tone, actions)
    }

    /// Any other answer, as its toast says it.
    pub(crate) fn said(toast: &Toast) -> Ended {
        Ended::new(None, toast.text.clone(), toast.tone, toast.actions.clone())
    }

    fn new(plan: Option<PlanId>, text: String, tone: Tone, actions: Vec<Action>) -> Ended {
        Ended {
            key: NEXT_ENDED.fetch_add(1, Ordering::Relaxed),
            plan,
            text,
            tone,
            actions,
            at: SystemTime::now(),
        }
    }
}

/// Keep `ended` first in `earlier`, `EARLIER` at most. An answer that is no
/// plan's said again as the last one was (a message repeated) is that one,
/// moved to now: it does not push the plans' answers out.
pub(crate) fn keep(earlier: &mut VecDeque<Ended>, ended: Ended) {
    if ended.plan.is_none()
        && let Some(last) = earlier.front_mut()
        && last.plan.is_none()
        && last.text == ended.text
        && last.tone == ended.tone
    {
        last.at = ended.at;
        last.actions = ended.actions;
        return;
    }
    earlier.push_front(ended);
    earlier.truncate(EARLIER);
}

/// The last action an error stopped, said in the strip until seen.
#[derive(Clone, Debug)]
pub struct Stopped {
    pub plan: PlanId,
    pub title: String,
    /// Whether it could be retried when it stopped: it is no longer said
    /// once it cannot be (retried, from any surface).
    retries: bool,
}

impl Stopped {
    pub(crate) fn new(plan: PlanId, title: &str, retries: bool) -> Stopped {
        Stopped {
            plan,
            title: title.to_owned(),
            retries,
        }
    }

    /// Whether it could be retried when it stopped.
    pub(crate) fn retries(&self) -> bool {
        self.retries
    }
}

/// The strip's buttons: one handle each, kept as its words change.
#[derive(Clone)]
struct StripFocus {
    open: FocusHandle,
    pause: FocusHandle,
    cancel: FocusHandle,
    retry: FocusHandle,
    dismiss: FocusHandle,
}

/// Activity as the window shows it; what runs and what was said of it is
/// `Work`'s (`work.rs`), which outlives the window.
#[derive(Default)]
pub struct ActivityState {
    /// Activity stands above the footer while something is under way,
    /// rather than opening over the files (remembered with the window).
    pub pinned: bool,
    anchor: PopoverAnchor,
    /// Each card's Pause and Cancel, by plan; each task's Cancel.
    cards: HashMap<PlanId, (FocusHandle, FocusHandle)>,
    tasks: HashMap<u64, FocusHandle>,
    /// Each `Earlier` row's buttons, by its answer's key and place.
    rows: HashMap<(u64, usize), FocusHandle>,
    /// Each waiting action's Cancel, by its place in the order asked, and
    /// the one that cancels them all.
    waiting: HashMap<u64, FocusHandle>,
    cancel_waiting: Option<FocusHandle>,
    strip: Option<StripFocus>,
    pin: Option<FocusHandle>,
    /// What `ui.activity` gives the keyboard to while pinned and shown.
    tray: Option<FocusHandle>,
    /// What brings the button the keyboard reaches into view: in the
    /// popup, made again as it opens (it opens at its top), and pinned.
    popup_reveal: Reveal,
    tray_reveal: Reveal,
}

/// What brings the button the keyboard reaches into view in Activity's
/// rows, which scroll: the popup's, at most 520 px, and the pinned tray's,
/// at most half the pane. Toolbar popups do it by their rows' key context
/// (`toolbar::RevealRow`); Activity's buttons are its own.
#[derive(Clone, Default)]
pub(crate) struct Reveal {
    pub(crate) scroll: ScrollHandle,
    /// The button last brought into view: once, as the keyboard reaches
    /// it, so that the wheel may take it out of view again.
    shown: Rc<RefCell<Option<FocusHandle>>>,
}

impl Reveal {
    /// Drawn in the button tracking `focus`: in prepaint, where its bounds
    /// are real, the first frame it has the keyboard the rows scroll the
    /// least that shows it whole, and the frame that draws them there is
    /// asked for (a refresh asked while drawing is dropped).
    pub(crate) fn marker(&self, focus: &FocusHandle) -> AnyElement {
        let (scroll, shown, focus) = (self.scroll.clone(), Rc::clone(&self.shown), focus.clone());
        gpui::canvas(
            move |bounds, window, _| {
                let mut shown = shown.borrow_mut();
                if !focus.is_focused(window) {
                    if shown.as_ref() == Some(&focus) {
                        *shown = None;
                    }
                    return;
                }
                let area = scroll.bounds();
                // Not laid out yet: the next frame tries again.
                if shown.as_ref() == Some(&focus) || area.size.height <= px(0.) {
                    return;
                }
                *shown = Some(focus.clone());
                if let Some(offset) = showing(bounds, area, scroll.offset()) {
                    scroll.set_offset(offset);
                    window.on_next_frame(|window, _| window.refresh());
                }
            },
            |_, (), _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element()
    }
}

/// What runs and is shown: the plans worth a card, then the tasks (a print);
/// and what the strip counts beside them in this spell of work (ADR-114,
/// amended 2026-11-03): the actions that ended, and those waiting that
/// will count (`Waiting::weighs`).
pub(crate) struct Under {
    pub plans: Vec<(Running, Transfer)>,
    pub tasks: Vec<skwir_core::task::Running>,
    pub ended: usize,
    pub waiting: usize,
}

impl Under {
    /// Nothing runs (what waits aside).
    pub(crate) fn is_empty(&self) -> bool {
        self.plans.is_empty() && self.tasks.is_empty()
    }

    /// What runs.
    pub(crate) fn count(&self) -> usize {
        self.plans.len() + self.tasks.len()
    }
}

/// How far the spell of work went (ADR-114, amended 2026-11-03).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Summary {
    /// From 0 to 1: each action of the spell counting one, those ended
    /// whole, those running by their own share (bytes or items, as their
    /// cards say), those waiting nothing: so it never goes back as one
    /// ends, only as more is asked.
    pub fraction: f32,
    /// The share of each running one, plans then tasks, as `under` holds
    /// them: what Activity's header draws.
    pub shares: Vec<f32>,
    /// How far the queue has started, the ended and the running…
    pub at: usize,
    /// …of every action of the spell, those waiting too.
    pub of: usize,
    /// Those waiting.
    pub waiting: usize,
    /// The longest time left of those running that can tell it: they run
    /// at once (ADR-079). Nothing is said of what waits.
    pub left: Option<Duration>,
    /// Every one running is paused.
    pub paused: bool,
    /// Every one running was cancelled and is stopping.
    pub stopping: bool,
}

/// `under` summed up, `None` when nothing runs or waits.
pub(crate) fn summary(under: &Under) -> Option<Summary> {
    let running = under.count();
    if running == 0 && under.waiting == 0 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)] // A share for a bar.
    let shares: Vec<f32> = under
        .plans
        .iter()
        .map(|(running, t)| share(running.total, t))
        .chain(
            under
                .tasks
                .iter()
                .map(|task| task.went.unwrap_or(0.) as f32),
        )
        .collect();
    let at = under.ended + running;
    let of = at + under.waiting;
    #[allow(clippy::cast_precision_loss)] // Shares for a bar.
    let fraction = ((under.ended as f32 + shares.iter().sum::<f32>()) / of as f32).clamp(0., 1.);
    let left = under.plans.iter().filter_map(|(_, t)| t.left).max();
    let paused = running > 0 && under.tasks.is_empty() && under.plans.iter().all(|(r, _)| r.paused);
    let stopping = running > 0
        && under.plans.iter().all(|(r, _)| r.cancelling)
        && under.tasks.iter().all(|task| task.cancelling);
    Some(Summary {
        fraction,
        shares,
        at,
        of,
        waiting: under.waiting,
        left,
        paused,
        stopping,
    })
}

/// `fraction` as the strip says it: "42 %", never 100 before the end.
fn percent(fraction: f32) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let percent = (fraction.clamp(0., 1.) * 100.).floor() as u32;
    format!("{percent} %")
}

/// What the strip says of how it goes: the share done and the time left
/// of what runs, then how many wait; paused, stopping, or sending to its
/// server instead.
fn how(summary: &Summary, sending_files: Option<usize>) -> String {
    let at = percent(summary.fraction);
    if summary.stopping {
        return "stopping…".to_owned();
    }
    let said = if summary.paused {
        format!("paused at {at}")
    } else if let Some(files) = sending_files {
        sending(files)
    } else {
        match summary.left {
            Some(left) => format!("{at} · {}", format::time_left(left)),
            None => at,
        }
    };
    match summary.waiting {
        0 => said,
        waiting => format!("{said} · {waiting} waiting"),
    }
}

/// The strip's words, richest first: what runs, how far the queue started
/// and how; that without what runs; the share alone.
pub(crate) fn words(under: &Under, summary: &Summary) -> [String; 3] {
    let sending_files = if let [(_, t)] = under.plans.as_slice()
        && under.tasks.is_empty()
    {
        t.sending
    } else {
        None
    };
    let how = how(summary, sending_files);
    let what = match (under.plans.as_slice(), under.tasks.as_slice()) {
        ([(running, _)], []) => capitalized(&running.title),
        ([], [task]) => capitalized(&task.title),
        ([], []) => "Waiting".to_owned(),
        _ => format::count(under.count(), "action"),
    };
    let least = if summary.stopping {
        "Stopping…".to_owned()
    } else {
        percent(summary.fraction)
    };
    if summary.of > 1 {
        let queue = format!("{} of {}", summary.at, summary.of);
        return [
            format!("{what} · {queue} · {how}"),
            format!("{queue} · {how}"),
            least,
        ];
    }
    [format!("{what} · {how}"), capitalized(&how), least]
}

impl Shell {
    /// What runs and is shown as a card (`Work::under_way`).
    pub(crate) fn under_way(&self) -> Under {
        self.work.borrow().under_way(&self.app)
    }

    /// Whether Activity's popup is open.
    pub(crate) fn activity_open(&self) -> bool {
        self.toolbar_menu
            .as_ref()
            .is_some_and(|popup| popup.kind == POPUP)
    }

    /// Whether Activity has something to stand above the footer for, as
    /// `under` runs: under way, waiting, or stopped.
    fn activity_holds(&self, under: &Under) -> bool {
        self.work.borrow().stopped.is_some()
            || !under.is_empty()
            || !self.app.executor.waiting().is_empty()
    }

    /// Whether Activity stands above the footer as `under` runs: pinned,
    /// with something to show, unless it is open as a popup, which stays
    /// where it opened until it is closed (the strip draws it).
    pub(crate) fn activity_tray_shown_for(&self, under: &Under) -> bool {
        self.activity.pinned && !self.activity_open() && self.activity_holds(under)
    }

    /// `activity_tray_shown_for` what runs now, for what is asked between
    /// frames (a command, the desktop).
    pub(crate) fn activity_tray_shown(&self) -> bool {
        self.activity.pinned && !self.activity_open() && self.activity_holds(&self.under_way())
    }

    /// `ui.activity`: open Activity above its strip, or close it (`args`:
    /// `close`, `pointer`, as every popup's toggle). Pinned and shown, it
    /// is already open: the keyboard goes to it.
    pub(crate) fn toggle_activity(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let closing = args.get("close").and_then(serde_json::Value::as_bool) == Some(true);
        if !closing && self.activity_tray_shown() {
            // One popup at a time: the one open closes.
            self.close_popup(window, cx);
            let focus = self
                .activity
                .tray
                .get_or_insert_with(|| cx.focus_handle().tab_stop(false))
                .clone();
            // Its first button: the tray itself is no stop.
            window.focus(&focus, cx);
            window.focus_next(cx);
            cx.notify();
            return;
        }
        if self.toggle_popup(POPUP, args, window, cx) {
            self.toolbar_menu = Some(crate::toolbar::Popup::new(POPUP, window, cx));
            self.activity.popup_reveal = Reveal::default();
            // Seen: the strip says it no more.
            self.work.borrow_mut().stopped = None;
        }
        cx.notify();
    }

    /// `ui.activity_pin`: Activity stands above the footer while something
    /// is under way, or opens over the files when asked.
    pub(crate) fn toggle_activity_pin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.activity.pinned = !self.activity.pinned;
        self.remember_session();
        // Pinned with something to show, it stands where the popup was.
        if self.activity.pinned && self.activity_open() && self.activity_holds(&self.under_way()) {
            self.close_popup(window, cx);
        }
        cx.notify();
    }

    /// `Earlier`'s rows' buttons, kept for the answers it still holds.
    pub(crate) fn forget_earlier_rows(&mut self) {
        let kept: HashSet<u64> = self.work.borrow().earlier.iter().map(|e| e.key).collect();
        self.activity.rows.retain(|(key, _), _| kept.contains(key));
    }

    fn strip_focus(&mut self, cx: &mut Context<Self>) -> StripFocus {
        self.activity
            .strip
            .get_or_insert_with(|| StripFocus {
                open: cx.focus_handle(),
                pause: cx.focus_handle(),
                cancel: cx.focus_handle(),
                retry: cx.focus_handle(),
                dismiss: cx.focus_handle(),
            })
            .clone()
    }

    /// The focus of each card's buttons, kept by plan and task while they
    /// run, so Tab finds them where they were as cards come and go.
    fn card_focus(&mut self, under: &Under, cx: &mut Context<Self>) {
        let plans: HashSet<PlanId> = under.plans.iter().map(|(r, _)| r.plan).collect();
        self.activity.cards.retain(|plan, _| plans.contains(plan));
        for plan in &plans {
            self.activity
                .cards
                .entry(*plan)
                .or_insert_with(|| (stop(cx), stop(cx)));
        }
        let tasks: HashSet<u64> = under.tasks.iter().map(|task| task.id).collect();
        self.activity.tasks.retain(|id, _| tasks.contains(id));
        for id in tasks {
            self.activity.tasks.entry(id).or_insert_with(|| stop(cx));
        }
        self.work.borrow_mut().carded.extend(plans);
    }
}

/// The scroll offset that shows `row` whole in `area`, scrolled by
/// `offset` now, moving it the least: `None` when it shows already.
fn showing(
    row: gpui::Bounds<gpui::Pixels>,
    area: gpui::Bounds<gpui::Pixels>,
    offset: gpui::Point<gpui::Pixels>,
) -> Option<gpui::Point<gpui::Pixels>> {
    let mut offset = offset;
    if row.top() < area.top() {
        offset.y += area.top() - row.top();
    } else if row.bottom() > area.bottom() {
        offset.y -= row.bottom() - area.bottom();
    } else {
        return None;
    }
    Some(offset)
}

/// A control pressed as every one is (`modal::pressed`): by a click, and by
/// Enter and Space while it has the keyboard, never by gpui's own click on
/// a key's release, which a binding of the key would stop.
pub(crate) trait Press {
    fn press(
        self,
        press: impl Fn(&mut Shell, &mut Window, &mut Context<Shell>) + 'static,
        cx: &mut Context<Shell>,
    ) -> crate::modal::Pressed;
}

impl Press for gpui::Stateful<gpui::Div> {
    fn press(
        self,
        press: impl Fn(&mut Shell, &mut Window, &mut Context<Shell>) + 'static,
        cx: &mut Context<Shell>,
    ) -> crate::modal::Pressed {
        crate::modal::pressed(self, press, cx)
    }
}

/// The focus of a button of Activity's, a stop of its Tab order: an
/// element tracking a handle of its own takes its stop from the handle.
pub(crate) fn stop(cx: &mut Context<Shell>) -> FocusHandle {
    cx.focus_handle().tab_index(0).tab_stop(true)
}

/// The most actions Activity's header draws a segment for: past it, one
/// bar, the segments too thin to tell apart.
const SEGMENTS: usize = 12;

/// Over the cards, while several actions make the spell of work (ADR-114,
/// amended 2026-11-03): how far the queue started and its share, what is
/// left of what runs and how many wait, over a bar of one segment each,
/// full for the `ended`, as far as each running one went, empty for those
/// waiting; one bar past `SEGMENTS`.
fn spell_header(summary: &Summary, ended: usize, a11y: bool, theme: &Theme) -> AnyElement {
    let said = format!(
        "{} of {} · {}",
        summary.at,
        summary.of,
        percent(summary.fraction)
    );
    let after = [
        summary.left.map(format::time_left),
        (summary.waiting > 0).then(|| format!("{} waiting", summary.waiting)),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    let color = if summary.paused || summary.stopping {
        theme.text_muted
    } else {
        theme.accent
    };
    let bar = if summary.of <= SEGMENTS {
        let fills = std::iter::repeat_n(1., ended)
            .chain(summary.shares.iter().copied())
            .chain(std::iter::repeat_n(0., summary.waiting));
        div()
            .flex()
            .gap(px(3.))
            .children(fills.map(|fill| div().flex_1().child(small_bar_full(fill, color, theme))))
    } else {
        div().child(small_bar_full(summary.fraction, color, theme))
    };
    div()
        .id("activity-spell")
        .role(gpui::Role::ProgressIndicator)
        .when(a11y, |d| {
            d.aria_label(format!("{} of {} actions", summary.at, summary.of))
                .aria_numeric_value(f64::from(summary.fraction) * 100.)
                .aria_min_numeric_value(0.)
                .aria_max_numeric_value(100.)
        })
        .flex()
        .flex_col()
        .flex_none()
        .gap_1()
        .px_3()
        .pt_1()
        .pb_2()
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex()
                .justify_between()
                .gap_2()
                .text_xs()
                .child(div().text_color(theme.text).child(said))
                .child(div().text_color(theme.text_muted).child(after)),
        )
        .child(bar)
        .into_any_element()
}

/// A small bar: `fraction` of `width` in `color`.
fn small_bar(width: f32, fraction: f32, color: gpui::Hsla, theme: &Theme) -> gpui::Div {
    div()
        .flex_none()
        .w(px(width))
        .h(px(4.))
        .rounded_full()
        .bg(theme.border)
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(color)
                .w(gpui::relative(fraction.clamp(0., 1.))),
        )
}

/// A button of the strip: an icon in the footer's height.
fn strip_button(
    id: &'static str,
    icon: &str,
    label: &'static str,
    focus: &FocusHandle,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let tip: SharedString = label.into();
    controls::icon_button(id, icon, label, focus, theme)
        .size(px(20.))
        .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), cx))
}

/// What the strip offers beside its words.
#[derive(Clone, Copy)]
enum Offers {
    /// One plan under way: its Pause (or Resume) and Cancel.
    One { plan: PlanId, paused: bool },
    /// One task (a print): its Cancel.
    Task { id: u64 },
    /// An action an error stopped: Retry where it can be, and Dismiss.
    Stopped { plan: PlanId, retries: bool },
    /// Several, one stopping, or Activity opened with nothing under way:
    /// the way to Activity alone.
    Nothing,
}

/// The buttons `offers` holds, built for one version of the strip: each
/// version is its own element, its buttons with the same ids.
fn strip_buttons(
    offers: Offers,
    focus: &StripFocus,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Vec<AnyElement> {
    match offers {
        Offers::One { plan, paused } => vec![
            strip_button(
                "activity-strip-pause",
                if paused { "play" } else { "pause" },
                if paused { "Resume" } else { "Pause" },
                &focus.pause,
                theme,
            )
            .press(
                move |this, _, cx| {
                    if paused {
                        this.app.executor.resume(plan);
                    } else {
                        this.app.executor.pause(plan);
                    }
                    cx.notify();
                },
                cx,
            )
            .into_any_element(),
            strip_button(
                "activity-strip-cancel",
                "close",
                "Cancel",
                &focus.cancel,
                theme,
            )
            .press(
                move |this, _, cx| {
                    this.cancel_plan(plan);
                    cx.notify();
                },
                cx,
            )
            .into_any_element(),
        ],
        Offers::Task { id } => vec![
            strip_button(
                "activity-strip-cancel",
                "close",
                "Cancel",
                &focus.cancel,
                theme,
            )
            .press(
                move |this, _, cx| {
                    this.app.tasks.cancel(id);
                    cx.notify();
                },
                cx,
            )
            .into_any_element(),
        ],
        Offers::Stopped { plan, retries } => {
            let mut buttons = Vec::new();
            if retries {
                buttons.push(
                    controls::button(
                        "activity-strip-retry",
                        "Retry",
                        None,
                        ButtonStyle::Plain,
                        &focus.retry,
                        theme,
                    )
                    .h(px(20.))
                    .px_2()
                    .press(
                        move |this, window, cx| {
                            this.work.borrow_mut().stopped = None;
                            this.run("retry", serde_json::json!({ "plan": plan }), window, cx);
                        },
                        cx,
                    )
                    .into_any_element(),
                );
            }
            buttons.push(
                strip_button(
                    "activity-strip-dismiss",
                    "close",
                    "Dismiss",
                    &focus.dismiss,
                    theme,
                )
                .press(
                    |this, _, cx| {
                        this.work.borrow_mut().stopped = None;
                        cx.notify();
                    },
                    cx,
                )
                .into_any_element(),
            );
            buttons
        }
        Offers::Nothing => Vec::new(),
    }
}

/// The strip at the end of the last pane's footer, as `under` runs: what
/// runs, how far, and the way to Activity, opened above it; `None` while
/// there is nothing to say, or while Activity, pinned, says it above.
pub fn strip(
    shell: &mut Shell,
    under: &Under,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    if shell.activity_tray_shown_for(under) {
        return None;
    }
    let open = shell.activity_open();
    if under.is_empty() && shell.work.borrow().stopped.is_none() && !open {
        return None;
    }
    shell.card_focus(under, cx);
    let focus = shell.strip_focus(cx);
    let stopped = shell.work.borrow().stopped.clone();
    // What it says, richest first, its colour and bar, and what it offers.
    let (said, color, fraction, offers) = match summary(under) {
        Some(summary) => {
            let color = if summary.paused || summary.stopping {
                theme.text_muted
            } else {
                theme.accent
            };
            let offers = match (under.plans.as_slice(), under.tasks.as_slice()) {
                ([(running, _)], []) if !running.cancelling => Offers::One {
                    plan: running.plan,
                    paused: running.paused,
                },
                ([], [task]) if !task.cancelling => Offers::Task { id: task.id },
                _ => Offers::Nothing,
            };
            (
                words(under, &summary),
                color,
                Some(summary.fraction),
                offers,
            )
        }
        None => match stopped {
            // Stopped by an error, nothing else under way: said until
            // seen, retried or dismissed.
            Some(stopped) => {
                let title = capitalized(&stopped.title);
                (
                    [
                        format!("{title}: stopped"),
                        "Stopped".to_owned(),
                        "Stopped".to_owned(),
                    ],
                    theme.error,
                    None,
                    Offers::Stopped {
                        plan: stopped.plan,
                        retries: stopped.retries,
                    },
                )
            }
            // Opened with nothing under way: what ended, in Earlier.
            None => (
                [
                    "Activity".to_owned(),
                    "Activity".to_owned(),
                    "Activity".to_owned(),
                ],
                theme.text_muted,
                None,
                Offers::Nothing,
            ),
        },
    };
    // Several actions in the spell: Activity says each.
    let several = under.ended + under.count() + under.waiting > 1;
    let error = matches!(offers, Offers::Stopped { .. });
    let anchor = shell.activity.anchor.clone();
    let a11y = shell.a11y;
    // Each version: the opener (its bar and words), then the buttons, which
    // keep their ids from one version to the next, and so the keyboard.
    let mut versions = Vec::with_capacity(said.len());
    for (n, words) in said.iter().enumerate() {
        let last = n + 1 == said.len();
        let label: SharedString = format!("Activity: {words}").into();
        let opener = div()
            .id("activity-strip")
            .relative()
            .role(gpui::Role::Button)
            .when(a11y, |d| d.aria_label(label))
            .track_focus(&focus.open)
            .flex()
            .items_center()
            .gap_1()
            .min_w_0()
            .h(px(20.))
            .px_1()
            .rounded_md()
            .border_1()
            .border_color(gpui::transparent_black())
            .cursor_pointer()
            .text_color(if error { theme.error } else { theme.text })
            .hover({
                let hover = theme.hover;
                move |s| s.bg(hover)
            })
            .focus({
                let accent = theme.accent;
                move |s| s.border_color(accent)
            })
            .child(anchor.marker())
            .when(error, |d| {
                d.child(controls::glyph("warning", theme.error, 13.))
            })
            .when_some(fraction, |d, fraction| {
                d.child(small_bar(
                    if last { BAR_NARROW } else { BAR_WIDE },
                    fraction,
                    color,
                    theme,
                ))
            })
            .child({
                // Each version is drawn only where it fits whole.
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(words.clone())
            })
            .when(several, |d| {
                d.child(controls::glyph("up", theme.text_muted, 11.))
            })
            .press(
                |this, window, cx| {
                    this.run("ui.activity", serde_json::json!({}), window, cx);
                },
                cx,
            );
        versions.push(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap_0p5()
                .min_w_0()
                // A click on the strip leaves the keyboard where it was:
                // the keys go on acting on the pane while it runs, paused
                // or not. `ui.activity` brings the keyboard to Activity.
                .capture_any_mouse_down(|_, window, _| window.prevent_default())
                .child(opener)
                .children(strip_buttons(offers, &focus, theme, cx))
                .into_any_element(),
        );
    }
    let popup = popover(shell, under, theme, cx);
    Some(
        div()
            .flex()
            .min_w_0()
            // The last version is the least it takes: cut narrower, its
            // buttons, which cannot be, would be drawn over Undo.
            .child(Fit {
                variants: versions,
                cuts: false,
                start: false,
            })
            .children(popup)
            .into_any_element(),
    )
}

/// Activity, opened above its strip: what `body` holds.
fn popover(
    shell: &mut Shell,
    under: &Under,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    if !shell.activity_open() {
        return None;
    }
    let reveal = shell.activity.popup_reveal.clone();
    let body = body(shell, under, &reveal, theme, cx);
    let popup = shell.toolbar_menu.as_ref()?;
    let panel = crate::toolbar::popup_panel(popup, "ui.activity", theme, cx)
        .role(gpui::Role::Group)
        .aria_label("Activity")
        // Its rows bring the button the keyboard reaches into view.
        .track_scroll(&reveal.scroll)
        .w(px(400.))
        .max_h(px(520.))
        .p_0()
        .gap_0()
        .children(body);
    Some(shell.activity.anchor.popup_above_for(
        &crate::elements::adaptive::Chosen::default(),
        vec![panel.into_any_element()],
    ))
}

/// Activity pinned, above the last pane's footer while something is under
/// way (`under`): what the popup holds, taking room rather than covering
/// the files.
pub fn tray(
    shell: &mut Shell,
    under: &Under,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    if !shell.activity_tray_shown_for(under) {
        return None;
    }
    let focus = shell
        .activity
        .tray
        .get_or_insert_with(|| cx.focus_handle().tab_stop(false))
        .clone();
    let reveal = shell.activity.tray_reveal.clone();
    let body = body(shell, under, &reveal, theme, cx);
    Some(
        div()
            .id("activity-tray")
            .role(gpui::Role::Group)
            .aria_label("Activity")
            // A place of its own, as the pinned player is: Escape gives
            // the keyboard back to the pane (`keymap::PLACES`).
            .key_context(crate::keymap::ACTIVITY)
            .track_focus(&focus)
            // Its own place in the window's Tab order, its buttons walked
            // in theirs (as `toolbar::Popup`'s).
            .tab_group()
            .tab_index(1)
            // A click in it leaves the keyboard where it was, as on the
            // strip: the keys go on acting on the pane while it stands.
            .capture_any_mouse_down(|_, window, _| window.prevent_default())
            .flex_none()
            .max_h(gpui::relative(0.5))
            .overflow_y_scroll()
            .track_scroll(&reveal.scroll)
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.bg)
            .text_color(theme.text)
            .children(body)
            .into_any_element(),
    )
}

/// What Activity holds, in the popup or pinned, as `under` runs: its name
/// and pin, the cards of what runs, what waits, and what ended this
/// session; `reveal` brings the button the keyboard reaches into view.
fn body(
    shell: &mut Shell,
    under: &Under,
    reveal: &Reveal,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Vec<AnyElement> {
    shell.card_focus(under, cx);
    let a11y = shell.a11y;
    let mut parts = Vec::new();
    let pinned = shell.activity.pinned;
    let pin = shell.activity.pin.get_or_insert_with(|| stop(cx)).clone();
    let pin_tip: SharedString = if pinned {
        "Let Activity close"
    } else {
        "Keep Activity open above the footer while something runs"
    }
    .into();
    parts.push(
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .pl_3()
            .pr_1()
            .pt_1()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child("Activity"),
            )
            .child(
                controls::icon_button("activity-pin", "pin", "Keep open", &pin, theme)
                    .size(px(24.))
                    .aria_toggled(if pinned {
                        gpui::Toggled::True
                    } else {
                        gpui::Toggled::False
                    })
                    .when(pinned, |d| d.bg(theme.selection_bg))
                    .tooltip(move |_, cx| crate::toolbar::tooltip(pin_tip.clone(), cx))
                    .child(reveal.marker(&pin))
                    .press(
                        |this, window, cx| {
                            this.run("ui.activity_pin", serde_json::json!({}), window, cx);
                        },
                        cx,
                    ),
            )
            .into_any_element(),
    );
    // The spell of work as a whole, where it holds several actions.
    if let Some(summary) = summary(under).filter(|s| s.of > 1) {
        parts.push(spell_header(&summary, under.ended, a11y, theme));
    }
    let cards = under.count();
    for (running, transfer) in &under.plans {
        let focus = shell.activity.cards[&running.plan].clone();
        parts.push(
            progress_card(
                running,
                transfer,
                (&focus.0, &focus.1),
                reveal,
                a11y,
                theme,
                cx,
            )
            .into_any_element(),
        );
    }
    for task in &under.tasks {
        let focus = shell.activity.tasks[&task.id].clone();
        parts.push(task_card(task, &focus, reveal, a11y, theme, cx).into_any_element());
    }
    // What the user did meanwhile waits behind what it meets, and paused,
    // for as long as it stays paused: each with what it waits for, and its
    // Cancel.
    let waiting = shell.app.executor.waiting();
    let asked: HashSet<u64> = waiting.iter().map(|w| w.asked).collect();
    shell.activity.waiting.retain(|id, _| asked.contains(id));
    if !waiting.is_empty() {
        parts.push(waiting_heading(shell, waiting.len(), reveal, theme, cx));
        let rows: Vec<AnyElement> = waiting
            .iter()
            .map(|w| waiting_row(shell, w, reveal, theme, cx))
            .collect();
        parts.push(
            div()
                .id("activity-waiting")
                .role(gpui::Role::List)
                .when(a11y, |d| d.aria_label("Waiting"))
                .flex()
                .flex_col()
                .pb_1()
                .children(rows)
                .into_any_element(),
        );
    }
    // Each answer's buttons' focus, kept by answer and place; then its rows,
    // read where they are kept.
    for ended in &shell.work.borrow().earlier {
        for n in 0..ended.actions.len() {
            shell
                .activity
                .rows
                .entry((ended.key, n))
                .or_insert_with(|| stop(cx));
        }
    }
    // What each answer's Undo can do now, as the History says it.
    let by_plan = if shell.work.borrow().earlier.is_empty() {
        HashMap::new()
    } else {
        shell.history_by_plan()
    };
    let shell: &Shell = shell;
    let work = shell.work.borrow();
    if !work.earlier.is_empty() {
        parts.push(
            div()
                .mt_1()
                .px_3()
                .pt_2()
                .pb_1()
                .border_t_1()
                .border_color(theme.border)
                .text_xs()
                .text_color(theme.text_muted)
                .child("Earlier")
                .into_any_element(),
        );
        let rows: Vec<AnyElement> = work
            .earlier
            .iter()
            .map(|ended| earlier_row(shell, ended, &by_plan, reveal, theme, cx))
            .collect();
        parts.push(
            div()
                .id("activity-earlier")
                .role(gpui::Role::List)
                .when(a11y, |d| d.aria_label("Earlier"))
                .flex()
                .flex_col()
                .children(rows)
                .into_any_element(),
        );
    } else if cards == 0 && waiting.is_empty() {
        parts.push(
            div()
                .px_3()
                .py_2()
                .text_xs()
                .text_color(theme.text_muted)
                .child("Nothing has run yet")
                .into_any_element(),
        );
    }
    parts.push(div().flex_none().h(px(6.)).into_any_element());
    parts
}

/// Over what waits its turn: its name, and Cancel all when there are
/// several.
fn waiting_heading(
    shell: &mut Shell,
    count: usize,
    reveal: &Reveal,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let all = (count > 1).then(|| {
        let focus = shell
            .activity
            .cancel_waiting
            .get_or_insert_with(|| stop(cx))
            .clone();
        controls::button(
            "activity-waiting-cancel-all",
            "Cancel all",
            None,
            ButtonStyle::Plain,
            &focus,
            theme,
        )
        .h(px(22.))
        .px_2()
        .text_xs()
        .child(reveal.marker(&focus))
        .press(
            |this, _, cx| {
                // What waits when pressed, none that started meanwhile, in
                // one pass over the queue.
                let asked: Vec<u64> = this
                    .app
                    .executor
                    .waiting()
                    .iter()
                    .map(|waiting| waiting.asked)
                    .collect();
                this.app.executor.cancel_waiting_all(&asked);
                cx.notify();
            },
            cx,
        )
    });
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_between()
        .min_h(px(26.))
        .pl_3()
        .pr_1()
        .child(
            div()
                .text_xs()
                .text_color(theme.text_muted)
                .child("Waiting"),
        )
        .children(all)
        .into_any_element()
}

/// An action waiting its turn: what it is and, under it, what it waits
/// for, each cut in the middle (where a path's end tells two apart), and
/// its Cancel, which ends it at once, having done nothing.
fn waiting_row(
    shell: &mut Shell,
    waiting: &skwir_core::op::Waiting,
    reveal: &Reveal,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let asked = waiting.asked;
    let (what, why) = waiting_words(waiting);
    let focus = shell
        .activity
        .waiting
        .entry(asked)
        .or_insert_with(|| stop(cx))
        .clone();
    div()
        .id(("activity-waiting-row", asked))
        .role(gpui::Role::ListItem)
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .min_h(px(26.))
        .pl_3()
        .pr_1()
        .py_1()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .text_xs()
                .child(crate::elements::clip::clipped(
                    div()
                        .id(("activity-waiting-what", asked))
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis_middle(),
                    what,
                ))
                .child(crate::elements::clip::clipped(
                    div()
                        .id(("activity-waiting-why", asked))
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis_middle()
                        .text_color(theme.text_muted),
                    why,
                )),
        )
        .child(
            controls::button(
                SharedString::from(format!("activity-waiting-cancel-{asked}")),
                "Cancel",
                None,
                ButtonStyle::Plain,
                &focus,
                theme,
            )
            .h(px(22.))
            .px_2()
            .text_xs()
            .child(reveal.marker(&focus))
            .press(
                move |this, _, cx| {
                    this.app.executor.cancel_waiting(asked);
                    cx.notify();
                },
                cx,
            ),
        )
        .into_any_element()
}

/// A row of `Earlier`: how it ended, what, when, and its way back (Undo),
/// or Retry while it can be.
fn earlier_row(
    shell: &Shell,
    ended: &Ended,
    by_plan: &HashMap<PlanId, skwir_core::op::HistoryRow>,
    reveal: &Reveal,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let glyph = match ended.tone {
        Tone::Success => controls::glyph("check", theme.success, 13.),
        Tone::Error => controls::glyph("warning", theme.error, 13.),
        Tone::Info => controls::glyph("info", theme.text_muted, 13.),
    };
    let key = ended.key;
    // Retry while its plan can be retried.
    let retries = ended
        .plan
        .is_none_or(|plan| shell.app.executor.can_retry(plan));
    let actions: Vec<(usize, Action)> = ended
        .actions
        .iter()
        .cloned()
        .enumerate()
        .filter(|(_, action)| action.command != "retry" || retries)
        .collect();
    let mut row = div()
        .id(("activity-earlier", key))
        .role(gpui::Role::ListItem)
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .min_h(px(26.))
        .pl_3()
        .pr_1()
        .child(glyph)
        .child(crate::elements::clip::clipped(
            div()
                .id(("activity-earlier-text", key))
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_xs(),
            ended.text.clone(),
        ))
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(theme.text_muted)
                .child(format::clock(ended.at)),
        );
    for (n, action) in actions {
        // Made for each of its buttons before its rows are (`body`).
        let Some(focus) = shell.activity.rows.get(&(key, n)) else {
            continue;
        };
        // An Undo that cannot take its action back now says why, as the
        // History does (ADR-079, 13), rather than being refused pressed.
        if let Some(why) = undo_held(&action, by_plan) {
            let why: SharedString = why.into();
            row = row.child(
                controls::inert_button(
                    SharedString::from(format!("activity-earlier-{key}-{n}")),
                    action.label.clone(),
                    focus,
                    theme,
                )
                .h(px(22.))
                .px_2()
                // No frame but the keyboard's, as in the History.
                .border_color(gpui::transparent_black())
                .focus({
                    let accent = theme.accent;
                    move |s| s.border_color(accent)
                })
                .child(reveal.marker(focus))
                .tooltip(move |_, cx| crate::toolbar::tooltip(why.clone(), cx)),
            );
            continue;
        }
        row = row.child(
            controls::button(
                SharedString::from(format!("activity-earlier-{key}-{n}")),
                action.label.clone(),
                None,
                ButtonStyle::Plain,
                focus,
                theme,
            )
            .h(px(22.))
            .px_2()
            .text_color(theme.accent_text)
            .child(reveal.marker(focus))
            .press(
                move |this, window, cx| {
                    // Its answer is said as any is, the popup closed.
                    this.close_popup(window, cx);
                    this.run(&action.command, action.args.clone(), window, cx);
                },
                cx,
            ),
        );
    }
    row.into_any_element()
}

/// Why `action`, an answer's Undo, cannot take its plan back now, as the
/// History says it (`by_plan`); `None` for another action, or one it takes
/// back.
fn undo_held(
    action: &Action,
    by_plan: &HashMap<PlanId, skwir_core::op::HistoryRow>,
) -> Option<String> {
    if action.command != "undo" {
        return None;
    }
    let plan: PlanId = serde_json::from_value(action.args.get("plan")?.clone()).ok()?;
    match by_plan.get(&plan) {
        Some(row) if row.state == skwir_core::op::Done::Undone => Some("Undone already".to_owned()),
        Some(row) if crate::history::can_now(row) => None,
        Some(row) => crate::history::why(row),
        None => Some("No longer in the history".to_owned()),
    }
}

/// The card of the running plan `running` in Activity: its title, how far
/// it is, how fast and how long, and its Pause and Cancel, which `focus`
/// holds.
fn progress_card(
    running: &Running,
    transfer: &Transfer,
    focus: (&FocusHandle, &FocusHandle),
    reveal: &Reveal,
    a11y: bool,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let (pause_focus, cancel_focus) = focus;
    let (fraction, amount) = bar(running.total, transfer);
    let plan = running.plan;
    let paused = running.paused;
    // Cancelled, it stops where it next answers Cancel and takes back what
    // it was in the middle of: no speed nor time left any more, and
    // nothing more to ask of it.
    let stopping = running.cancelling;
    let state = match (paused, running.going_on, transfer.sending) {
        _ if stopping => "stopping at ".to_owned(),
        (true, ..) => "paused at ".to_owned(),
        // Copied onto a mount that sends to its server after: what the
        // server still has to take before the copy is done (ADR-102).
        (false, _, Some(files)) => format!("{}, ", sending(files)),
        (false, true, None) => "going on, ".to_owned(),
        (false, false, None) => String::new(),
    };
    // What runs, said by its title; how far, as a value, said when asked,
    // never at each step.
    div()
        .id(("activity-card", plan.0))
        .role(gpui::Role::ProgressIndicator)
        .when(a11y, |d| {
            d.aria_label(capitalized(&running.title))
                .aria_numeric_value(f64::from(fraction) * 100.)
                .aria_min_numeric_value(0.)
                .aria_max_numeric_value(100.)
        })
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .pl_3()
        .pr_1()
        .py_2()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(crate::elements::clip::clipped(
                    div()
                        .id(("activity-card-title", plan.0))
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis(),
                    capitalized(&running.title),
                ))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(format!("{}{amount}", capitalized(&state))),
                )
                .child(small_bar_full(
                    fraction,
                    if paused || stopping {
                        theme.text_muted
                    } else {
                        theme.accent
                    },
                    theme,
                ))
                // How fast, how long: under the bar, quieter than what it
                // is and how far (ADR-113).
                .when_some(
                    pace(transfer).filter(|_| !paused && !stopping),
                    |d, pace| d.child(div().text_xs().text_color(theme.text_muted).child(pace)),
                ),
        )
        .when(!stopping, |card| {
            card.child(
                controls::button(
                    format!("activity-pause-{}", plan.0),
                    if paused { "Resume" } else { "Pause" },
                    None,
                    ButtonStyle::Plain,
                    pause_focus,
                    theme,
                )
                .child(reveal.marker(pause_focus))
                .press(
                    move |this, _, cx| {
                        if paused {
                            this.app.executor.resume(plan);
                        } else {
                            this.app.executor.pause(plan);
                        }
                        cx.notify();
                    },
                    cx,
                ),
            )
        })
        .child(if stopping {
            // Said, not offered: nothing more to ask of it.
            controls::inert_button(
                format!("activity-cancel-{}", plan.0),
                "Stopping…",
                cancel_focus,
                theme,
            )
            .child(reveal.marker(cancel_focus))
            .into_any_element()
        } else {
            controls::button(
                format!("activity-cancel-{}", plan.0),
                "Cancel",
                None,
                ButtonStyle::Plain,
                cancel_focus,
                theme,
            )
            .child(reveal.marker(cancel_focus))
            .press(
                move |this, _, cx| {
                    this.cancel_plan(plan);
                    cx.notify();
                },
                cx,
            )
            .into_any_element()
        })
}

/// A bar across its card.
fn small_bar_full(fraction: f32, color: gpui::Hsla, theme: &Theme) -> gpui::Div {
    div()
        .h(px(4.))
        .w_full()
        .rounded_full()
        .bg(theme.border)
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(color)
                .w(gpui::relative(fraction.clamp(0., 1.))),
        )
}

/// The card of the task `task` (work under way that is no plan: a print):
/// its title, how far it went, and Cancel, which `focus` holds.
fn task_card(
    task: &skwir_core::task::Running,
    focus: &FocusHandle,
    reveal: &Reveal,
    a11y: bool,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    #[allow(clippy::cast_possible_truncation)]
    let fraction = task.went.unwrap_or(0.0) as f32;
    let id = task.id;
    let amount = task.went.map_or_else(String::new, |_| percent(fraction));
    div()
        .id(("activity-task", id))
        .role(gpui::Role::ProgressIndicator)
        .when(a11y, |d| {
            d.aria_label(capitalized(&task.title))
                .aria_numeric_value(f64::from(fraction) * 100.)
                .aria_min_numeric_value(0.)
                .aria_max_numeric_value(100.)
        })
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .pl_3()
        .pr_1()
        .py_2()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(crate::elements::clip::clipped(
                    div()
                        .id(("activity-task-title", id))
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis(),
                    capitalized(&task.title),
                ))
                .when(!amount.is_empty(), |d| {
                    d.child(div().text_xs().text_color(theme.text_muted).child(amount))
                })
                .child(small_bar_full(fraction, theme.accent, theme)),
        )
        .child(if task.cancelling {
            controls::inert_button(
                format!("activity-task-cancel-{id}"),
                "Stopping…",
                focus,
                theme,
            )
            .child(reveal.marker(focus))
            .into_any_element()
        } else {
            controls::button(
                format!("activity-task-cancel-{id}"),
                "Cancel",
                None,
                ButtonStyle::Plain,
                focus,
                theme,
            )
            .child(reveal.marker(focus))
            .press(
                move |this, _, cx| {
                    this.app.tasks.cancel(id);
                    cx.notify();
                },
                cx,
            )
            .into_any_element()
        })
}

impl Shell {
    /// Cancel `plan`, from its card or the strip: it says it stops until
    /// it let go of what it held (operations.md, "Running").
    pub(crate) fn cancel_plan(&mut self, plan: PlanId) {
        if self.app.executor.cancel(plan)
            && let Some(running) = self
                .work
                .borrow_mut()
                .running
                .iter_mut()
                .find(|r| r.plan == plan)
        {
            running.cancelling = true;
        }
    }
}

impl Shell {
    /// How the desktop is told while this window is up (`work.rs`).
    pub(crate) fn watched(&self) -> Watched {
        Watched {
            seen: if self.window_active {
                Seen::LookedAt
            } else {
                Seen::Away
            },
            notify: self.prefs.notify_desktop,
            keep_awake: self.prefs.keep_awake,
        }
    }

    /// The window has the focus again, or lost it: looked at again, what
    /// the desktop was told is taken back.
    pub(crate) fn window_activated(&mut self, active: bool) {
        self.window_active = active;
        if active {
            self.work.borrow_mut().looked_at();
        }
    }

    /// The taskbar's entry, the machine kept awake, the session's end held
    /// off, as what runs goes (`Work::tell`).
    pub(crate) fn tell_desktop(&mut self) {
        let under = self.under_way();
        let watched = self.watched();
        self.work.borrow_mut().tell(&self.app, &under, watched);
    }

    /// What the desktop asked: a notice or the presence clicked brings the
    /// window forward, with the token the desktop gave where it gave one,
    /// and opens Activity; its menu pauses, lets go on or stops what runs
    /// (`finishing::act`).
    pub(crate) fn take_asked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut token = None;
        let mut shown = false;
        let asked = self.work.borrow().take_asked();
        for asked in asked {
            match asked {
                Asked::Notice(clicked) => {
                    shown = true;
                    token = clicked.token.or(token);
                }
                Asked::Open { token: given } => {
                    shown = true;
                    token = given.or(token);
                }
                Asked::PauseAll | Asked::GoOn => {
                    crate::finishing::act(&self.app, &self.work.borrow(), &asked);
                }
                Asked::StopAll => {
                    self.stop_and_quit(window, cx);
                    return;
                }
            }
        }
        if !shown {
            return;
        }
        window.activate_window_with_token(token.as_deref());
        if !self.activity_open() && !self.activity_tray_shown() {
            self.toggle_activity(&serde_json::json!({}), window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{EARLIER, Ended, Summary, Under, how, keep, percent, showing, summary, words};
    use crate::toast::Running;
    use skwir_core::op::{PlanId, RunKind, Transfer};
    use std::time::Duration;

    fn running(plan: u64, title: &str, total: usize) -> Running {
        Running::for_test(PlanId(plan), title, RunKind::Apply, total)
    }

    /// Every answer is kept in Earlier, a plan's or not, newest first and
    /// twenty at most; a message said again as the last one was is that
    /// one, moved to now, so that it never pushes the plans' answers out.
    #[test]
    fn every_answer_is_kept_in_earlier_and_a_repeat_is_one() {
        use crate::toast::{Toast, Tone};
        let said = |text: &str| {
            Ended::said(&Toast {
                text: text.to_owned(),
                tone: Tone::Error,
                actions: Vec::new(),
            })
        };
        let mut earlier = std::collections::VecDeque::new();
        keep(
            &mut earlier,
            Ended::of_plan(PlanId(3), "Copied “it's.txt” to Stick", Tone::Success, &[]),
        );
        keep(&mut earlier, said("The printer refused “it's.pdf”"));
        keep(&mut earlier, said("The printer refused “it's.pdf”"));
        assert_eq!(earlier.len(), 2, "{earlier:?}");
        assert_eq!(earlier[0].plan, None);
        assert_eq!(earlier[1].plan, Some(PlanId(3)));
        assert_ne!(earlier[0].key, earlier[1].key);
        for n in 0..EARLIER {
            keep(&mut earlier, said(&format!("message {n}")));
        }
        assert_eq!(earlier.len(), EARLIER);
        assert_eq!(earlier[0].text, format!("message {}", EARLIER - 1));
    }

    /// The button the keyboard reaches is brought into view by the least
    /// scroll: up to its top above, down to its bottom below, not at all
    /// when it shows.
    #[test]
    fn the_button_reached_scrolls_into_view_the_least() {
        use gpui::{bounds, point, px, size};
        let area = bounds(point(px(0.), px(100.)), size(px(400.), px(200.)));
        let row = |top: f32| bounds(point(px(10.), px(top)), size(px(60.), px(22.)));
        let at = point(px(0.), px(-50.));
        assert_eq!(showing(row(150.), area, at), None);
        assert_eq!(
            showing(row(290.), area, at),
            Some(point(px(0.), px(-62.))),
            "its bottom at the area's"
        );
        assert_eq!(
            showing(row(80.), area, at),
            Some(point(px(0.), px(-30.))),
            "its top at the area's"
        );
    }

    #[test]
    fn a_share_is_said_whole_and_never_full_before_the_end() {
        assert_eq!(percent(0.0), "0 %");
        assert_eq!(percent(0.429), "42 %");
        assert_eq!(percent(0.999), "99 %");
        assert_eq!(percent(1.0), "100 %");
    }

    /// The spell of work counts each action once (ADR-114, amended
    /// 2026-11-03): those ended whole, those running by their own share,
    /// those waiting nothing; an action ending never takes the bar back.
    /// The time left is the longest of what runs.
    #[test]
    fn the_spell_counts_each_action_once_ended_running_or_waiting() {
        let sized = |done: u64, planned: u64, left: u64| Transfer {
            done,
            planned: Some(planned),
            left: Some(Duration::from_secs(left)),
            ..Transfer::default()
        };
        let under = |plans: Vec<(Running, Transfer)>, ended: usize, waiting: usize| Under {
            plans,
            tasks: Vec::new(),
            ended,
            waiting,
        };
        let a = (running(1, "copy a", 3), sized(1 << 30, 3 << 30, 120));
        let b = (running(2, "copy b", 2), sized(0, 1 << 30, 300));
        // Two at once, each alike whatever its bytes.
        let s = summary(&under(vec![a.clone(), b.clone()], 0, 0)).unwrap();
        assert!((s.fraction - 1. / 6.).abs() < 1e-6, "{}", s.fraction);
        assert_eq!((s.at, s.of, s.waiting), (2, 2, 0));
        assert_eq!(s.left, Some(Duration::from_secs(300)));
        assert!(!s.paused && !s.stopping);
        // A third of the one running, one ended before it, two waiting.
        let s = summary(&under(vec![a], 1, 2)).unwrap();
        assert!((s.fraction - 1. / 3.).abs() < 1e-6, "{}", s.fraction);
        assert_eq!((s.at, s.of), (2, 4));
        // It ends, the next starts: the bar does not go back.
        let next = summary(&under(vec![b], 2, 1)).unwrap();
        assert!(
            next.fraction >= s.fraction,
            "{} < {}",
            next.fraction,
            s.fraction
        );
        assert!((next.fraction - 0.5).abs() < 1e-6);
        // Between two, what waits is said, nothing stops.
        let between = summary(&under(Vec::new(), 2, 1)).unwrap();
        assert!((between.fraction - 2. / 3.).abs() < 1e-6);
        assert!(!between.stopping && !between.paused);
        assert!(summary(&under(Vec::new(), 3, 0)).is_none());
    }

    /// One action is said by its title; several by how many; a queue by
    /// how far it started and how many wait; the least the footer can
    /// hold is the share alone.
    #[test]
    fn the_strip_says_what_runs_as_room_allows() {
        let copy = (
            running(1, "copy 3 items to Backup", 3),
            Transfer {
                done: 42,
                planned: Some(100),
                left: Some(Duration::from_secs(170)),
                ..Transfer::default()
            },
        );
        let one = Under {
            plans: vec![copy.clone()],
            tasks: Vec::new(),
            ended: 0,
            waiting: 0,
        };
        let s = summary(&one).unwrap();
        assert_eq!(
            words(&one, &s),
            [
                "Copy 3 items to Backup · 42 % · about 3 min left".to_owned(),
                "42 % · about 3 min left".to_owned(),
                "42 %".to_owned(),
            ]
        );
        let paused = Summary {
            paused: true,
            ..s.clone()
        };
        assert_eq!(how(&paused, None), "paused at 42 %");
        let stopping = Summary {
            stopping: true,
            ..s.clone()
        };
        assert_eq!(words(&one, &stopping)[2], "Stopping…");
        assert_eq!(how(&s, Some(3)), "sending 3 files to the server");

        let two = Under {
            plans: vec![copy.clone(), (running(2, "move b", 2), Transfer::default())],
            tasks: Vec::new(),
            ended: 0,
            waiting: 0,
        };
        let s = summary(&two).unwrap();
        assert!(
            words(&two, &s)[0].starts_with("2 actions · 2 of 2 · "),
            "{:?}",
            words(&two, &s)
        );

        // One ended before it, one waiting after it.
        let queued = Under {
            plans: vec![copy],
            tasks: Vec::new(),
            ended: 1,
            waiting: 1,
        };
        let s = summary(&queued).unwrap();
        assert_eq!(
            words(&queued, &s),
            [
                "Copy 3 items to Backup · 2 of 3 · 47 % · about 3 min left · 1 waiting".to_owned(),
                "2 of 3 · 47 % · about 3 min left · 1 waiting".to_owned(),
                "47 %".to_owned(),
            ]
        );
        let paused = Summary { paused: true, ..s };
        assert_eq!(how(&paused, None), "paused at 47 % · 1 waiting");
    }
}
