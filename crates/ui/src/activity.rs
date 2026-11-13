//! Activity: what runs, docked at the window's foot (ADR-114).
//!
//! Work under way is the window's state, which ends the last pane's
//! footer (`status.rs`): a strip there says how far what runs has gone, in
//! as many words as the footer has room for (`Fit`), with Pause and Cancel
//! for a single action. A click on it opens Activity above it: each
//! action's card (its bar, speed and time left, Pause and Cancel), what
//! waits its turn, and what ended this session that the History does not
//! hold (`Earlier`: what failed, was cancelled or cannot be undone, and
//! the answers that are no action's), with its Retry or Undo, kept in
//! memory only. Pinned, Activity stands above the footer instead, taking
//! room rather than covering the files, while there is something under
//! way to show. Nothing floats over the files but the latest answer
//! (`toast.rs`).
//!
//! An action an error stopped stays in the strip, in the error's colour,
//! until it is retried, Activity is opened, or it is dismissed.

use crate::sizes::TextSizes;
use crate::sizes::sized;
use crate::theme::Chosen;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use gpui::{
    AnyElement, Context, FocusHandle, ScrollHandle, SharedString, Window, div, prelude::*, px,
};
use skwir_core::op::{PlanId, Transfer};

use crate::controls::{self, ButtonStyle};
use crate::elements::adaptive::Fit;
use crate::elements::popover::PopoverAnchor;
use crate::format;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toast::{
    Action, Running, Toast, Tone, bar, capitalized, pace, sending, share, waiting_words,
};

/// Activity's popup, as `Shell::toggle_popup` and `toolbar_menu` know it.
pub const POPUP: &str = "activity";

/// What `Earlier` keeps: the answers of this session the History does not
/// hold, newest first.
pub(crate) const EARLIER: usize = 20;

/// The most Activity standing takes above the footer, its heading included:
/// about three running actions' cards; what it holds beyond scrolls.
const TRAY_MOST: f32 = 260.;

/// The key of the next answer kept in `Earlier`.
static NEXT_ENDED: AtomicU64 = AtomicU64::new(1);

/// The bar's width in the strip: as much words as the footer has room for
/// beside it, then less bar.
const BAR_WIDE: f32 = 54.;
const BAR_NARROW: f32 = 24.;

/// What ended this session, as its answer said it: a plan's the History
/// does not hold (`Work::ended`: failed, cancelled, or kept by no
/// journal), or any other answer a toast said (a print's, a message),
/// found again once the toast has gone.
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
    /// Activity folded into the footer's strip by its chevron: it opens
    /// over the files when asked, never by itself (remembered with the
    /// window). Unfolded, as by default, it stands above the footer once
    /// an action the window asked lasted (ADR-114, amended 2026-11-07).
    pub folded: bool,
    /// Activity unfolded for this spell of work: an action the window
    /// asked lasted `ui.activity_unfold_ms` (`Work::unfolds_in`), or it was
    /// unfolded by hand. Kept until it has nothing to show, so that it
    /// never folds and unfolds between the actions of a spell.
    stands: bool,
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
    /// The header's Cancel all, which cancels the whole queue.
    cancel_all: Option<FocusHandle>,
    strip: Option<StripFocus>,
    pin: Option<FocusHandle>,
    /// Earlier unfolded at the foot of Activity standing
    /// (`ui.activity_earlier`), and the button that folds it.
    earlier_unfolded: bool,
    fold: Option<FocusHandle>,
    /// What `ui.activity` gives the keyboard to while it stands.
    tray: Option<FocusHandle>,
    /// What brings the button the keyboard reaches into view: in the
    /// popup, made again as it opens (it opens at its top), and standing.
    popup_reveal: Reveal,
    tray_reveal: Reveal,
}

/// What brings the button the keyboard reaches into view in Activity's
/// rows, which scroll under its heading: the popup's, at most 520 px, and
/// the standing tray's, at most `TRAY_MOST`. Toolbar popups do it by their
/// rows' key context (`toolbar::RevealRow`); Activity's buttons are its
/// own.
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

/// What runs and is shown: the plans that count as an action, each a card,
/// then the tasks (a print); and what the strip counts beside them in this
/// spell of work (ADR-114, amended 2026-11-03): the actions that ended, and
/// those waiting that count (`Waiting::counts`, the same test).
pub(crate) struct Under {
    pub plans: Vec<(Running, Transfer)>,
    pub tasks: Vec<skwir_core::task::Running>,
    pub ended: usize,
    pub waiting: usize,
    /// The first action waiting, what it is and what it waits for
    /// (`toast::waiting_words`): what the strip says while only that waits.
    pub next: Option<(String, String)>,
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
    /// ends, only as more is asked, or as an action stopped goes on, by
    /// what it has left.
    pub fraction: f32,
    /// The share of each running one, plans then tasks, as `under` holds
    /// them: what Activity's header draws. Empty while nothing runs and
    /// something waits.
    pub shares: Vec<f32>,
    /// Those that ended.
    pub ended: usize,
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
    /// Every one running is getting ready: its checks, before its first
    /// step or byte (ADR-114, amended 2026-11-03).
    pub ready: bool,
}

/// Whether `running` is getting ready (`Summary::ready`): no step made,
/// nothing moved, nothing ended yet.
pub(crate) fn getting_ready(running: &Running, t: &Transfer) -> bool {
    running.first().is_none() && t.done == 0 && t.ended == 0 && t.finished == 0
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
    let ready = running > 0
        && under.tasks.is_empty()
        && under.plans.iter().all(|(r, t)| getting_ready(r, t));
    Some(Summary {
        fraction,
        shares,
        ended: under.ended,
        at,
        of,
        waiting: under.waiting,
        left,
        paused,
        stopping,
        ready,
    })
}

impl Summary {
    /// Whether it has a share to show: something runs, or something of the
    /// spell ended. Only waiting, nothing ended, there is none: the desktop
    /// draws none (the taskbar, the presence), the strip says how many wait.
    pub(crate) fn has_share(&self) -> bool {
        !self.shares.is_empty() || self.ended > 0
    }
}

/// What Activity's header fills, one segment an action of the spell, as
/// `summary` counts them: full for those ended, as far as each running one
/// went, empty for those waiting. `None` past `SEGMENTS`: one bar, the
/// segments too thin to tell apart.
fn segments(summary: &Summary) -> Option<Vec<f32>> {
    (summary.of <= SEGMENTS).then(|| {
        std::iter::repeat_n(1., summary.ended)
            .chain(summary.shares.iter().copied())
            .chain(std::iter::repeat_n(0., summary.waiting))
            .collect()
    })
}

/// `text` with its first letter lowered, as the strip says a sentence
/// after a title ("waits for the disk").
fn lowered(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_lowercase().chain(chars).collect()
    })
}

/// What the strip announces of an action that joined (ADR-114, amended
/// 2026-11-03): its sentence, a shorter one where that does not fit (a
/// waiting one's title, without what it waits for), and what a screen
/// reader hears of it, once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Announced {
    pub sentence: String,
    pub short: String,
    pub heard: String,
}

/// What the strip announces of the action `plan`, as `under` and
/// `waiting` hold it; `None` once it is no longer under way.
pub(crate) fn announced(
    under: &Under,
    waiting: &[skwir_core::op::Waiting],
    plan: PlanId,
) -> Option<Announced> {
    if let Some((running, t)) = under.plans.iter().find(|(r, _)| r.plan == plan) {
        let title = capitalized(&running.title);
        let doing = if getting_ready(running, t) {
            "getting ready…".to_owned()
        } else {
            percent(share(running.total, t))
        };
        let sentence = format!("{title} · {doing}");
        return Some(Announced {
            short: sentence.clone(),
            sentence,
            heard: format!("{title} started"),
        });
    }
    let waits = waiting.iter().find(|w| w.plan == Some(plan))?;
    let (what, why) = waiting_words(waits);
    let why = lowered(&why);
    Some(Announced {
        sentence: format!("{what} · {why}"),
        short: format!("{what} · waits its turn"),
        heard: format!("{what} {why}"),
    })
}

/// `fraction` as the strip says it: "42 %", never 100 before the end.
pub(crate) fn percent(fraction: f32) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let percent = (fraction.clamp(0., 1.) * 100.).floor() as u32;
    format!("{percent} %")
}

/// What the strip says of how it goes: the share done and the time left
/// of what runs, then how many wait; paused, stopping, or sending to its
/// server instead. While nothing runs, the share of what ended, if any,
/// and how many wait.
fn how(summary: &Summary, sending_files: Option<usize>) -> String {
    let at = percent(summary.fraction);
    if summary.stopping {
        return "stopping…".to_owned();
    }
    let said = if summary.shares.is_empty() {
        (summary.ended > 0).then_some(at)
    } else if summary.ready && !summary.paused {
        // Never silent: what it does before its first step or byte.
        Some("getting ready…".to_owned())
    } else if summary.paused {
        Some(format!("paused at {at}"))
    } else if let Some(files) = sending_files {
        Some(sending(files))
    } else {
        Some(match summary.left {
            Some(left) => format!("{at} · {}", format::time_left(left)),
            None => at,
        })
    };
    let waiting = (summary.waiting > 0).then(|| format!("{} waiting", summary.waiting));
    [said, waiting]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The strip's words, richest first: what runs, how far the queue started
/// and how; that without what runs; the share alone. While only something
/// waits, with nothing of the spell ended yet, how many wait, in each.
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
        ([(running, _)], []) => Some(capitalized(&running.title)),
        ([], [task]) => Some(capitalized(&task.title)),
        // Only waiting: what waits first, and what for (never silent).
        ([], []) => under
            .next
            .as_ref()
            .map(|(what, why)| format!("{what} · {}", lowered(why))),
        _ => Some(format::count(under.count(), "action")),
    };
    let least = if summary.stopping {
        "Stopping…".to_owned()
    } else if summary.shares.is_empty() && summary.ended == 0 {
        how.clone()
    } else {
        percent(summary.fraction)
    };
    // How far the queue started, once something of it did.
    let queue =
        (summary.of > 1 && summary.at > 0).then(|| format!("{} of {}", summary.at, summary.of));
    let rich = [what, queue.clone(), Some(how.clone())]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    let without = match queue {
        Some(queue) => format!("{queue} · {how}"),
        None => capitalized(&how),
    };
    [capitalized(&rich), without, least]
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
    /// `under` runs: under way, waiting (any of what it lists, which a
    /// count of the queue tells), or stopped.
    fn activity_holds(&self, under: &Under) -> bool {
        self.work.borrow().stopped.is_some()
            || !under.is_empty()
            || under.waiting > 0
            || self.app.executor.queued() > 0
    }

    /// Whether Activity stands above the footer as `under` runs: unfolded
    /// for this spell (`sync_unfolded`), with something to show, unless it
    /// is open as a popup, which stays where it opened until it is closed
    /// (the strip draws it).
    pub(crate) fn activity_tray_shown_for(&self, under: &Under) -> bool {
        self.activity_stands() && self.activity_holds(under)
    }

    /// `activity_tray_shown_for` what runs now, for what is asked between
    /// frames (a command, the desktop).
    pub(crate) fn activity_tray_shown(&self) -> bool {
        self.activity_stands() && self.activity_holds(&self.under_way())
    }

    fn activity_stands(&self) -> bool {
        !self.activity.folded && self.activity.stands && !self.activity_open()
    }

    /// Before each frame of the last pane: Activity unfolds above its
    /// footer once an action this window asked has lasted
    /// `ui.activity_unfold_ms`, where it was asked and nowhere else
    /// (`Work::unfolds_in`; ADR-114, amended 2026-11-07), and stays for the
    /// spell of work, until it has nothing to show. Quicker ones, and
    /// those asked elsewhere, the strip says alone.
    pub(crate) fn sync_unfolded(&mut self, under: &Under) {
        if !self.activity_holds(under) {
            self.activity.stands = false;
        } else if !self.activity.stands
            && self
                .work
                .borrow()
                .unfolds_in(self.window_id, std::time::Instant::now())
        {
            self.activity.stands = true;
        }
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

    /// `ui.activity` with `plan`: Activity open, standing or as its popup,
    /// the keyboard on that action's card (its Pause), or on its row's
    /// Cancel while it waits, once drawn: what a press on what it brings
    /// into a folder opens (ADR-135). Never closes it.
    pub(crate) fn show_activity_of(
        &mut self,
        plan: PlanId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.activity_open() {
            self.toggle_activity(&serde_json::json!({}), window, cx);
        }
        let shell = cx.weak_entity();
        window.on_next_frame(move |window, cx| {
            shell
                .update(cx, |this, cx| this.focus_card(plan, window, cx))
                .ok();
        });
        cx.notify();
    }

    /// The keyboard on `plan`'s card, or on its Cancel while it waits: the
    /// card brought into view as the keyboard reaches it (`Reveal`).
    fn focus_card(&mut self, plan: PlanId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((pause, _)) = self.activity.cards.get(&plan) {
            window.focus(&pause.clone(), cx);
            return;
        }
        let asked = self
            .app
            .executor
            .waiting()
            .into_iter()
            .find(|waiting| waiting.plan == Some(plan))
            .map(|waiting| waiting.asked);
        if let Some(cancel) = asked.and_then(|asked| self.activity.waiting.get(&asked)) {
            window.focus(&cancel.clone(), cx);
        }
    }

    /// `ui.activity_pin`: fold Activity into the footer's strip, where it
    /// opens over the files when asked, or unfold it again, to stand above
    /// the footer once an action asked here lasted.
    pub(crate) fn toggle_activity_pin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.activity.folded = !self.activity.folded;
        self.remember_session();
        // Unfolded by hand with something to show, it stands at once,
        // where the popup was.
        if !self.activity.folded && self.activity_holds(&self.under_way()) {
            self.activity.stands = true;
            if self.activity_open() {
                self.close_popup(window, cx);
            }
        }
        cx.notify();
    }

    /// `ui.activity_earlier`: unfold Earlier at the foot of Activity
    /// standing, or fold it again.
    pub(crate) fn toggle_activity_earlier(&mut self, cx: &mut Context<Self>) {
        self.activity.earlier_unfolded = !self.activity.earlier_unfolded;
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
/// left of what runs and how many wait, over a bar of one segment each
/// (`segments`); one bar past `SEGMENTS`.
fn spell_header(
    summary: &Summary,
    cancel: Option<AnyElement>,
    a11y: bool,
    theme: &Theme,
) -> AnyElement {
    // Nothing of it started yet: how many wait, once.
    let started = summary.at > 0;
    let said = if started {
        format!(
            "{} of {} · {}",
            summary.at,
            summary.of,
            percent(summary.fraction)
        )
    } else {
        format!("{} waiting", summary.waiting)
    };
    let after = [
        summary.left.map(format::time_left),
        (started && summary.waiting > 0).then(|| format!("{} waiting", summary.waiting)),
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
    let bar = match segments(summary) {
        Some(fills) => div().flex().gap(sized(3.)).children(
            fills
                .into_iter()
                .map(|fill| div().flex_1().child(small_bar_full(fill, color, theme))),
        ),
        None => div().child(small_bar_full(summary.fraction, color, theme)),
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
                .items_center()
                .justify_between()
                .gap_2()
                .min_h(sized(22.))
                .text_body()
                .child(div().text_color(theme.text).child(said))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().text_color(theme.text_muted).child(after))
                        .children(cancel),
                ),
        )
        .child(bar)
        .into_any_element()
}

/// Activity's Cancel all, in its header: the whole queue, `count` actions
/// under way or waiting (`ui.activity_cancel_all`).
fn cancel_all_button(
    shell: &mut Shell,
    count: usize,
    reveal: &Reveal,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let focus = shell
        .activity
        .cancel_all
        .get_or_insert_with(|| stop(cx))
        .clone();
    let tip: SharedString = cancel_all_said(count).into();
    controls::button(
        "activity-cancel-all",
        "Cancel all",
        None,
        ButtonStyle::Plain,
        &focus,
        theme,
    )
    .h(sized(22.))
    .px_2()
    .text_body()
    .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), cx))
    .child(reveal.marker(&focus))
    .press(
        |this, window, cx| {
            this.run("ui.activity_cancel_all", serde_json::json!({}), window, cx);
        },
        cx,
    )
    .into_any_element()
}

/// What Cancel all says it cancels: `count` actions under way or waiting.
fn cancel_all_said(count: usize) -> String {
    match count {
        1 => "Cancel 1 action".to_owned(),
        count => format!("Cancel all {count} actions"),
    }
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
    label: impl Into<SharedString>,
    focus: &FocusHandle,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let tip: SharedString = label.into();
    controls::icon_button(id, icon, tip.clone(), focus, theme)
        .size(px(20.))
        .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), cx))
}

/// What the strip offers beside its words: its buttons act on what it
/// says (ADR-114, amended 2026-11-03), one action alone or the whole
/// queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Offers {
    /// One plan under way, nothing else: its Pause (or Resume) and Cancel.
    One { plan: PlanId, paused: bool },
    /// One task (a print), nothing else: its Cancel.
    Task { id: u64 },
    /// Several under way or waiting, `count` of them not stopping: Pause
    /// all (Resume all, `pause` saying whether every plan running is
    /// paused; none while only tasks run, which do not pause) and Cancel
    /// all (`ui.activity_pause_all`, `ui.activity_cancel_all`).
    All { count: usize, pause: Option<bool> },
    /// An action an error stopped: Retry where it can be, and Dismiss.
    Stopped { plan: PlanId, retries: bool },
    /// Every one stopping, or Activity opened with nothing under way: the
    /// way to Activity alone.
    Nothing,
}

/// What the strip offers as `under` runs: one action's buttons while it
/// is all there is, the whole queue's while there are more (what waits
/// counted as the strip counts it); nothing once every one is stopping.
fn offers(under: &Under) -> Offers {
    let plans: Vec<&Running> = under
        .plans
        .iter()
        .map(|(running, _)| running)
        .filter(|running| !running.cancelling)
        .collect();
    let tasks: Vec<u64> = under
        .tasks
        .iter()
        .filter(|task| !task.cancelling)
        .map(|task| task.id)
        .collect();
    match (plans.as_slice(), tasks.as_slice(), under.waiting) {
        ([], [], 0) => Offers::Nothing,
        ([running], [], 0) => Offers::One {
            plan: running.plan,
            paused: running.paused,
        },
        ([], [id], 0) => Offers::Task { id: *id },
        _ => Offers::All {
            count: plans.len() + tasks.len() + under.waiting,
            pause: (!plans.is_empty()).then(|| plans.iter().all(|running| running.paused)),
        },
    }
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
        Offers::All { count, pause } => {
            let mut buttons = Vec::new();
            if let Some(paused) = pause {
                buttons.push(
                    strip_button(
                        "activity-strip-pause",
                        if paused { "play" } else { "pause" },
                        if paused { "Resume all" } else { "Pause all" },
                        &focus.pause,
                        theme,
                    )
                    .press(
                        |this, window, cx| {
                            this.run("ui.activity_pause_all", serde_json::json!({}), window, cx);
                        },
                        cx,
                    )
                    .into_any_element(),
                );
            }
            buttons.push(
                strip_button(
                    "activity-strip-cancel",
                    "close",
                    cancel_all_said(count),
                    &focus.cancel,
                    theme,
                )
                .press(
                    |this, window, cx| {
                        this.run("ui.activity_cancel_all", serde_json::json!({}), window, cx);
                    },
                    cx,
                )
                .into_any_element(),
            );
            buttons
        }
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
                    .h(sized(20.))
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
/// there is nothing to say, or while Activity, standing, says it above.
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
    // Shown while anything of the spell runs or waits, an action handed
    // over to the next included (what is admitted and not yet told
    // started is counted as waiting).
    let summary = summary(under);
    if summary.is_none() && shell.work.borrow().stopped.is_none() && !open {
        return None;
    }
    shell.card_focus(under, cx);
    let focus = shell.strip_focus(cx);
    let stopped = shell.work.borrow().stopped.clone();
    // What it says, richest first, its colour and bar, and what it offers.
    let (said, color, fraction, offers) = match summary {
        Some(summary) => {
            let color = if summary.paused || summary.stopping {
                theme.text_muted
            } else {
                theme.accent
            };
            (
                words(under, &summary),
                color,
                Some(summary.fraction),
                offers(under),
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
            // Opened with nothing under way: what Earlier keeps.
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
    // An action that joined, announced on a tint for a moment: its own
    // sentence first, and once to a screen reader (ADR-114, amended).
    let joined = shell
        .work
        .borrow()
        .announce
        .filter(|a| std::time::Instant::now() < a.until)
        .map(|a| a.plan);
    // What a waiting one waits for is read only while it is announced.
    let announce = joined.and_then(|plan| {
        let runs = under.plans.iter().any(|(r, _)| r.plan == plan);
        let waiting = if runs {
            Vec::new()
        } else {
            shell.app.executor.waiting()
        };
        announced(under, &waiting, plan)
    });
    // One action's buttons act on it: while another is named, none (the
    // whole queue's act on the one named too).
    let offers = match (announce.as_ref().and(joined), offers) {
        (Some(named), Offers::One { plan, .. }) if named != plan => Offers::Nothing,
        (Some(_), Offers::Task { .. }) => Offers::Nothing,
        (_, offers) => offers,
    };
    // Its own words first, richest first, then the strip's as ever.
    let said: Vec<String> = match &announce {
        Some(a) => {
            let mut said: Vec<String> = [&a.sentence, &a.short, &said[1], &said[2]]
                .into_iter()
                .cloned()
                .collect();
            said.dedup();
            said
        }
        None => said.to_vec(),
    };
    let tinted = announce.is_some() && !matches!(offers, Offers::Stopped { .. });
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
            .h(sized(20.))
            .px_1()
            .rounded_md()
            .border_1()
            .border_color(gpui::transparent_black())
            .cursor_pointer()
            .text_color(if error { theme.error } else { theme.text })
            .when(tinted, |d| d.chosen(theme))
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
    // Said once, as a toast is: polite, while it is announced (its words
    // stay the same meanwhile, so it is not said again).
    let heard = announce.filter(|_| a11y).map(|Announced { heard, .. }| {
        div()
            .id("activity-joined")
            .aria_live(gpui::Live::Polite)
            .aria_label(heard)
            .size_0()
            .into_any_element()
    });
    Some(
        div()
            .flex()
            .min_w_0()
            .children(heard)
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

/// Activity, opened above its strip: its heading, and what `body` holds
/// beneath, scrolled.
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
    let heading = heading(shell, false, theme, cx);
    let body = body(shell, under, Shows::All, &reveal, theme, cx);
    let popup = shell.toolbar_menu.as_ref()?;
    let panel = crate::toolbar::popup_panel(popup, "ui.activity", theme, cx)
        .role(gpui::Role::Group)
        .aria_label("Activity")
        // Its rows scroll, beside their scrollbar.
        .overflow_hidden()
        .w(sized(400.))
        .max_h(sized(520.))
        .p_0()
        .gap_0()
        .child(heading)
        .child(scrolled("activity-popup", body, &reveal));
    Some(shell.activity.anchor.popup_above_for(
        &crate::elements::adaptive::Chosen::default(),
        vec![panel.into_any_element()],
    ))
}

/// Activity standing above the last pane's footer, unfolded for this spell
/// of work (`Shell::sync_unfolded`) while something is under way
/// (`under`): what runs and waits, taking room rather than covering
/// the files, `TRAY_MOST` high at most (half the pane, in a low one),
/// scrolled beyond.
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
    let heading = heading(shell, true, theme, cx);
    let body = body(shell, under, Shows::Pinned, &reveal, theme, cx);
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
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(theme.border)
            // Opaque whatever the files' ground: Activity standing is its
            // popover kept open, read at a glance over any background.
            .bg(theme.bg)
            .text_color(theme.text)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_h_0()
                    .max_h(crate::sizes::sized(TRAY_MOST))
                    .child(heading)
                    .child(scrolled("activity-tray", body, &reveal)),
            )
            .into_any_element(),
    )
}

/// Activity's `body`, scrolled by `reveal` (which brings the button the
/// keyboard reaches into view) beside its scrollbar: in the popup, and
/// standing.
fn scrolled(id: &'static str, body: Vec<AnyElement>, reveal: &Reveal) -> AnyElement {
    div()
        .relative()
        .flex()
        .flex_col()
        .min_h_0()
        .child(
            div()
                .id(SharedString::from(format!("{id}-rows")))
                .flex()
                .flex_col()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&reveal.scroll)
                .children(body),
        )
        .child(crate::elements::scrollbar::vertical(
            SharedString::from(format!("{id}-scrollbar")),
            crate::elements::scrollbar::Scrolls::Offset(reveal.scroll.clone()),
        ))
        .into_any_element()
}

/// How Activity shows `Earlier` (ADR-114, amended 2026-11-03).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shows {
    /// All it keeps: in the popup.
    All,
    /// Standing: the action an error stopped, which the tray stands for
    /// while its Retry waits (`Work::stopped`), and the rest folded at its
    /// foot, a line until unfolded (`ui.activity_earlier`).
    Pinned,
}

/// Activity's name and what folds it, above its rows, which scroll
/// beneath it: standing above the footer (`standing`), its chevron folds
/// it into the strip; in the popup, its pin, pressed while it is unfolded,
/// folds it, and unfolds it again (ADR-114, amended 2026-11-07).
fn heading(
    shell: &mut Shell,
    standing: bool,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let unfolded = !shell.activity.folded;
    let focus = shell.activity.pin.get_or_insert_with(|| stop(cx)).clone();
    let button = if standing {
        controls::icon_button(
            "activity-fold",
            "chevron",
            "Fold into the footer",
            &focus,
            theme,
        )
        .tooltip(|_, cx| crate::toolbar::tooltip("Fold Activity into the footer".into(), cx))
    } else {
        let tip: SharedString = if unfolded {
            "Fold Activity into the footer"
        } else {
            "Unfold Activity above the footer when an action lasts"
        }
        .into();
        controls::icon_button(
            "activity-pin",
            "pin",
            "Unfold above the footer",
            &focus,
            theme,
        )
        .aria_toggled(if unfolded {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .when(unfolded, |d| d.chosen(theme))
        .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), cx))
    };
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
                .text_small()
                .text_color(theme.text_muted)
                .child("Activity"),
        )
        .child(button.size(px(24.)).press(
            |this, window, cx| {
                this.run("ui.activity_pin", serde_json::json!({}), window, cx);
            },
            cx,
        ))
        .into_any_element()
}

/// What Activity holds under its heading, in the popup or standing, as
/// `under` runs: the cards of what runs, what waits, and what of `Earlier`
/// it `shows`; `reveal` brings the button the keyboard reaches into view.
fn body(
    shell: &mut Shell,
    under: &Under,
    shows: Shows,
    reveal: &Reveal,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Vec<AnyElement> {
    shell.card_focus(under, cx);
    let a11y = shell.a11y;
    let mut parts = Vec::new();
    // The spell of work as a whole, where it holds several actions, with
    // Cancel all while several are under way or waiting.
    if let Some(summary) = summary(under).filter(|s| s.of > 1) {
        let cancel = match offers(under) {
            Offers::All { count, .. } => Some(cancel_all_button(shell, count, reveal, theme, cx)),
            _ => None,
        };
        parts.push(spell_header(&summary, cancel, a11y, theme));
    }
    let cards = under.count();
    for (running, transfer) in &under.plans {
        let focus = shell.activity.cards[&running.plan].clone();
        let into = into_said(&shell.app.executor.brings_into(running.plan));
        parts.push(
            progress_card(
                running,
                into,
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
        // Those that end as they start (a single rename), which the strip
        // and the header above do not count among those waiting.
        let quick = waiting.iter().filter(|w| !w.counts).count();
        parts.push(waiting_heading(
            shell,
            waiting.len(),
            quick,
            reveal,
            theme,
            cx,
        ));
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
    // Pinned, the action an error stopped is said; the rest is folded.
    let stopped = shell.work.borrow().stopped.as_ref().map(|s| s.plan);
    let (said, folded): (Vec<Ended>, Vec<Ended>) = match shows {
        Shows::All => (
            shell.work.borrow().earlier.iter().cloned().collect(),
            Vec::new(),
        ),
        Shows::Pinned => shell
            .work
            .borrow()
            .earlier
            .iter()
            .cloned()
            .partition(|ended| stopped.is_some() && ended.plan == stopped),
    };
    let unfolded = shell.activity.earlier_unfolded;
    let fold =
        (!folded.is_empty()).then(|| shell.activity.fold.get_or_insert_with(|| stop(cx)).clone());
    // What each answer's Undo can do now, as the History says it.
    let by_plan = if said.is_empty() && !(unfolded && fold.is_some()) {
        Rc::default()
    } else {
        shell.history_by_plan()
    };
    let shell: &Shell = shell;
    let list = |id: &'static str, rows: &[Ended], cx: &mut Context<Shell>| {
        let rows: Vec<AnyElement> = rows
            .iter()
            .map(|ended| earlier_row(shell, ended, &by_plan, reveal, theme, cx))
            .collect();
        div()
            .id(id)
            .role(gpui::Role::List)
            .when(a11y, |d| d.aria_label("Earlier"))
            .flex()
            .flex_col()
            .children(rows)
            .into_any_element()
    };
    if !said.is_empty() {
        parts.push(
            div()
                .mt_1()
                .px_3()
                .pt_2()
                .pb_1()
                .border_t_1()
                .border_color(theme.border)
                .text_small()
                .text_color(theme.text_muted)
                .child(match shows {
                    Shows::All => "Earlier",
                    Shows::Pinned => "Stopped",
                })
                .into_any_element(),
        );
        parts.push(list("activity-earlier", &said, cx));
    }
    if let Some(fold) = fold {
        parts.push(earlier_fold(
            folded.len(),
            unfolded,
            &fold,
            reveal,
            theme,
            cx,
        ));
        if unfolded {
            parts.push(list("activity-earlier-folded", &folded, cx));
        }
    } else if said.is_empty() && cards == 0 && waiting.is_empty() {
        // What was done is the History's (`ui.undo_history`).
        parts.push(
            div()
                .px_3()
                .py_2()
                .text_body()
                .text_color(theme.text_muted)
                .child("Nothing under way. What was done is in the Undo history.")
                .into_any_element(),
        );
    }
    parts.push(div().flex_none().h(px(6.)).into_any_element());
    parts
}

/// Earlier folded at the foot of Activity standing, `count` answers: a line
/// that unfolds them (`ui.activity_earlier`), its chevron a disclosure's,
/// as a sidebar group's: right while folded, down unfolded.
fn earlier_fold(
    count: usize,
    unfolded: bool,
    focus: &FocusHandle,
    reveal: &Reveal,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let hover = theme.hover;
    let accent = theme.accent;
    div()
        .id("activity-earlier-fold")
        .role(gpui::Role::Button)
        .aria_label("Earlier")
        .aria_expanded(unfolded)
        .track_focus(focus)
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .mt_1()
        .px_3()
        .py_1()
        .border_t_1()
        .border_color(theme.border)
        .text_body()
        .text_color(theme.text_muted)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .focus(move |s| s.text_color(accent))
        .child(controls::glyph(
            if unfolded { "chevron" } else { "chevron-right" },
            theme.text_muted,
            11.,
        ))
        .child(format!("Earlier ({count})"))
        .child(reveal.marker(focus))
        .press(
            |this, window, cx| {
                this.run("ui.activity_earlier", serde_json::json!({}), window, cx);
            },
            cx,
        )
        .into_any_element()
}

/// What Activity's Waiting heading says of the actions it lists, `quick`
/// of them ending as they start (a single rename): those the strip and the
/// header leave out of "N waiting" and the bar, said, so that the count
/// and the rows agree.
fn waiting_said(quick: usize) -> String {
    match quick {
        0 => "Waiting".to_owned(),
        quick => format!("Waiting · {} not counted", quick_ones(quick)),
    }
}

/// "1 quick one", "2 quick ones".
fn quick_ones(quick: usize) -> String {
    match quick {
        1 => "1 quick one".to_owned(),
        quick => format!("{quick} quick ones"),
    }
}

/// Over what waits its turn: its name, how many the strip counts where some
/// end as they start, and Cancel waiting when there are several (the
/// header's Cancel all cancels what runs too).
fn waiting_heading(
    shell: &mut Shell,
    count: usize,
    quick: usize,
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
            "Cancel waiting",
            None,
            ButtonStyle::Plain,
            &focus,
            theme,
        )
        .h(sized(22.))
        .px_2()
        .text_body()
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
        .min_h(sized(26.))
        .pl_3()
        .pr_1()
        .child(
            div()
                .text_small()
                .text_color(theme.text_muted)
                .child(waiting_said(quick)),
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
        .min_h(sized(26.))
        .pl_3()
        .pr_1()
        .py_1()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .text_body()
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
            .h(sized(22.))
            .px_2()
            .text_body()
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
    // An Undo that cannot take its action back now says why, as the
    // History does (ADR-079, 13), rather than being refused pressed: on a
    // line of its own under the answer, and to a screen reader with the
    // button, not only in a tooltip.
    let held = actions
        .iter()
        .find_map(|(_, action)| crate::history::undo_held(action, by_plan));
    let mut row = div()
        .id(("activity-earlier", key))
        .role(gpui::Role::ListItem)
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .min_h(sized(26.))
        .pl_3()
        .pr_1()
        .child(glyph)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(crate::elements::clip::clipped(
                    div()
                        .id(("activity-earlier-text", key))
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_body(),
                    ended.text.clone(),
                ))
                // Said whole, wrapped: the keyboard's user reads it here.
                .children(held.clone().map(|why| {
                    div()
                        .min_w_0()
                        .text_body()
                        .text_color(theme.text_muted)
                        .child(why)
                })),
        )
        .child(
            div()
                .flex_none()
                .text_small()
                .text_color(theme.text_muted)
                .child(format::clock(ended.at)),
        );
    for (n, action) in actions {
        // Made for each of its buttons before its rows are (`body`).
        let Some(focus) = shell.activity.rows.get(&(key, n)) else {
            continue;
        };
        // The held Undo: inert, its why under the answer (`held`), with it
        // for a screen reader and in its tooltip.
        if action.command == "undo"
            && let Some(why) = held.clone()
        {
            let why: SharedString = why.into();
            row = row.child(
                controls::inert_button(
                    SharedString::from(format!("activity-earlier-{key}-{n}")),
                    action.label.clone(),
                    focus,
                    theme,
                )
                .h(sized(22.))
                .px_2()
                // No frame but the keyboard's, as in the History.
                .border_color(gpui::transparent_black())
                .focus({
                    let accent = theme.accent;
                    move |s| s.border_color(accent)
                })
                .when(shell.a11y, |d| d.aria_description(why.clone()))
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
            .h(sized(22.))
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

/// Where an action goes, as its card says it when pointed at (ADR-135):
/// the whole path of each folder it brings something into, its title
/// naming them alone.
fn into_said(folders: &[std::path::PathBuf]) -> Option<SharedString> {
    let paths: Vec<String> = folders.iter().map(|f| f.display().to_string()).collect();
    (!paths.is_empty()).then(|| format!("Into {}", paths.join(", ")).into())
}

/// The card of the running plan `running` in Activity: its title, how far
/// it is, how fast and how long, and its Pause and Cancel, which `focus`
/// holds.
fn progress_card(
    running: &Running,
    into: Option<SharedString>,
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
        // Its checks, before its first step or byte (ADR-114, amended).
        _ if getting_ready(running, transfer) => "getting ready, ".to_owned(),
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
                .child(crate::elements::clip::clipped_with(
                    div()
                        .id(("activity-card-title", plan.0))
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis(),
                    capitalized(&running.title),
                    into,
                ))
                .child(
                    div()
                        .text_body()
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
                    |d, pace| d.child(div().text_small().text_color(theme.text_muted).child(pace)),
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
                    d.child(div().text_body().text_color(theme.text_muted).child(amount))
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
    /// `ui.activity_cancel_all`: cancel every action under way and every
    /// one waiting its turn, what waits first, so that none starts as
    /// another ends. What each did stays, undoable; what it left half done
    /// is taken back, as by its own Cancel. What runs is the executor's,
    /// read after what waited is cancelled: one that started meanwhile, or
    /// before the window took in that it did, is cancelled too.
    pub(crate) fn activity_cancel_all(&mut self) {
        let asked: Vec<u64> = self
            .app
            .executor
            .waiting()
            .iter()
            .map(|waiting| waiting.asked)
            .collect();
        self.app.executor.cancel_waiting_all(&asked);
        let plans: Vec<PlanId> = self
            .app
            .executor
            .running()
            .iter()
            .map(|running| running.plan)
            .collect();
        for plan in plans {
            self.cancel_plan(plan);
        }
        for task in self.app.tasks.running() {
            self.app.tasks.cancel(task.id);
        }
    }

    /// `ui.activity_pause_all`: pause every action running, or resume them
    /// all once every one is paused. What waits keeps waiting: a paused
    /// action keeps what it holds.
    pub(crate) fn activity_pause_all(&mut self) {
        let plans: Vec<(PlanId, bool)> = self
            .work
            .borrow()
            .running
            .iter()
            .filter(|running| !running.cancelling)
            .map(|running| (running.plan, running.paused))
            .collect();
        let resume = !plans.is_empty() && plans.iter().all(|(_, paused)| *paused);
        for (plan, _) in plans {
            if resume {
                self.app.executor.resume(plan);
            } else {
                self.app.executor.pause(plan);
            }
        }
    }

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
    /// The window has the focus again, or lost it: looked at again, what
    /// the desktop was told is taken back.
    pub(crate) fn window_activated(&mut self, active: bool) {
        self.window_active = active;
        if active {
            self.work.borrow_mut().looked_at();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EARLIER, Ended, Offers, Summary, Under, announced, cancel_all_said, how, keep, offers,
        percent, segments, showing, summary, waiting_said, words,
    };
    use crate::toast::{Running, waiting_words};
    use skwir_core::op::{PlanId, RunKind, Transfer};
    use std::time::Duration;

    fn running(plan: u64, title: &str, total: usize) -> Running {
        Running::for_test(PlanId(plan), title, RunKind::Apply, total)
    }

    /// The strip's buttons act on what it says (ADR-114, amended
    /// 2026-11-03): one action alone, its own; several under way or
    /// waiting, the whole queue's, Pause all only while a plan runs and
    /// Resume all once every one is paused; those stopping are not
    /// counted, and once every one is, nothing.
    #[test]
    fn the_strip_offers_one_action_s_buttons_or_the_whole_queue_s() {
        let under = |plans: Vec<Running>, tasks: Vec<skwir_core::task::Running>, waiting| Under {
            plans: plans
                .into_iter()
                .map(|r| (r, Transfer::default()))
                .collect(),
            tasks,
            ended: 0,
            waiting,
            next: None,
        };
        let task = |id: u64, cancelling| skwir_core::task::Running {
            id,
            title: "Print “it's a.pdf”".into(),
            went: None,
            cancelling,
        };
        let paused = |plan: u64| {
            let mut r = running(plan, "copy “it's b” to Backup", 3);
            r.paused = true;
            r
        };
        let stopping = |plan: u64| {
            let mut r = running(plan, "copy “it's c” to Backup", 3);
            r.cancelling = true;
            r
        };
        let alone = running(1, "copy “it's a” to Backup", 3);
        assert_eq!(
            offers(&under(vec![alone.clone()], Vec::new(), 0)),
            Offers::One {
                plan: PlanId(1),
                paused: false
            }
        );
        assert_eq!(
            offers(&under(Vec::new(), vec![task(7, false)], 0)),
            Offers::Task { id: 7 }
        );
        assert_eq!(
            offers(&under(vec![alone.clone()], Vec::new(), 3)),
            Offers::All {
                count: 4,
                pause: Some(false)
            },
            "one running and three waiting: the queue's"
        );
        assert_eq!(
            offers(&under(vec![paused(1), paused(2)], Vec::new(), 0)),
            Offers::All {
                count: 2,
                pause: Some(true)
            }
        );
        assert_eq!(
            offers(&under(vec![alone.clone(), paused(2)], Vec::new(), 0)),
            Offers::All {
                count: 2,
                pause: Some(false)
            },
            "some running: Pause all"
        );
        assert_eq!(
            offers(&under(Vec::new(), vec![task(7, false)], 1)),
            Offers::All {
                count: 2,
                pause: None
            },
            "a print does not pause"
        );
        assert_eq!(
            offers(&under(vec![stopping(2), alone.clone()], Vec::new(), 0)),
            Offers::One {
                plan: PlanId(1),
                paused: false
            },
            "the one not stopping is all there is"
        );
        assert_eq!(
            offers(&under(vec![stopping(1)], vec![task(7, true)], 0)),
            Offers::Nothing
        );
        assert_eq!(cancel_all_said(4), "Cancel all 4 actions");
    }

    /// Never silent (ADR-114, amended 2026-11-03): an action before its
    /// first step or byte says it is getting ready, in the strip and on
    /// its card's words; one only waiting says what it waits for; an
    /// action that joined is announced by its own sentence, and heard as
    /// started, or as what it waits for.
    #[test]
    fn the_strip_says_an_action_gets_ready_or_what_it_waits_for() {
        let ready = Under {
            plans: vec![(
                running(1, "copy “it's a” to Backup", 1),
                Transfer::default(),
            )],
            tasks: Vec::new(),
            ended: 0,
            waiting: 0,
            next: None,
        };
        let s = summary(&ready).unwrap();
        assert!(s.ready);
        assert_eq!(
            words(&ready, &s)[0],
            "Copy “it's a” to Backup · getting ready…"
        );
        let said = announced(&ready, &[], PlanId(1)).unwrap();
        assert_eq!(said.sentence, "Copy “it's a” to Backup · getting ready…");
        assert_eq!(said.heard, "Copy “it's a” to Backup started");
        let waits = skwir_core::op::Waiting {
            asked: 9,
            named: "copy “interview.mkv” to Backup".into(),
            plan: Some(PlanId(2)),
            waits_for: skwir_core::op::WaitsFor::Action("copy 3 items to Backup".into()),
            steps: Some(1),
            heavy: true,
            counts: true,
        };
        let only = Under {
            plans: Vec::new(),
            tasks: Vec::new(),
            ended: 0,
            waiting: 1,
            next: Some(waiting_words(&waits)),
        };
        let s = summary(&only).unwrap();
        assert_eq!(
            words(&only, &s)[0],
            "Copy “interview.mkv” to Backup · waits for copy 3 items to Backup · 1 waiting"
        );
        let said = announced(&only, std::slice::from_ref(&waits), PlanId(2)).unwrap();
        assert_eq!(
            said.sentence,
            "Copy “interview.mkv” to Backup · waits for copy 3 items to Backup"
        );
        assert_eq!(
            said.short,
            "Copy “interview.mkv” to Backup · waits its turn"
        );
        assert_eq!(
            said.heard,
            "Copy “interview.mkv” to Backup waits for copy 3 items to Backup"
        );
        assert_eq!(
            announced(&only, &[], PlanId(3)),
            None,
            "no longer under way"
        );
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
            next: None,
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
        assert!(between.has_share());
        // Only waiting, nothing of it ended: a summary with no share.
        let first = summary(&under(Vec::new(), 0, 2)).unwrap();
        assert_eq!((first.at, first.of, first.fraction), (0, 2, 0.));
        assert!(!first.has_share() && !first.paused && !first.stopping);
        assert!(summary(&under(Vec::new(), 3, 0)).is_none());
    }

    /// Activity's header draws a segment an action up to twelve, full for
    /// those ended, as far as each running one went, empty for those
    /// waiting; one bar past twelve.
    #[test]
    fn the_header_draws_a_segment_an_action_up_to_twelve() {
        let under = |plans: Vec<(Running, Transfer)>, ended: usize, waiting: usize| Under {
            plans,
            tasks: Vec::new(),
            ended,
            waiting,
            next: None,
        };
        let half = (
            running(1, "copy a", 2),
            Transfer {
                ended: 1,
                ..Transfer::default()
            },
        );
        let twelve = summary(&under(vec![half.clone()], 3, 8)).unwrap();
        assert_eq!(twelve.of, 12);
        let fills = segments(&twelve).expect("twelve segments");
        assert_eq!(fills.len(), 12);
        assert_eq!(&fills[..4], &[1., 1., 1., 0.5]);
        assert!(fills[4..].iter().all(|&fill| fill == 0.));
        let thirteen = summary(&under(vec![half], 3, 9)).unwrap();
        assert_eq!(thirteen.of, 13);
        assert_eq!(segments(&thirteen), None, "one bar");
    }

    /// While nothing runs and something waits, the strip is shown and says
    /// so: how far the spell went once something of it ended, else how
    /// many wait, in each of its versions. Activity's Waiting heading says
    /// how many quick ones it lists that the strip leaves out.
    #[test]
    fn the_strip_says_what_waits_while_nothing_runs() {
        let waiting = |ended: usize, waiting: usize| Under {
            plans: Vec::new(),
            tasks: Vec::new(),
            ended,
            waiting,
            next: None,
        };
        let between = waiting(1, 1);
        let s = summary(&between).expect("shown between two");
        assert_eq!(
            words(&between, &s),
            [
                "1 of 2 · 50 % · 1 waiting".to_owned(),
                "1 of 2 · 50 % · 1 waiting".to_owned(),
                "50 %".to_owned(),
            ]
        );
        let first = waiting(0, 2);
        let s = summary(&first).expect("shown before the first starts");
        assert_eq!(
            words(&first, &s),
            [
                "2 waiting".to_owned(),
                "2 waiting".to_owned(),
                "2 waiting".to_owned()
            ]
        );
        assert_eq!(waiting_said(0), "Waiting");
        assert_eq!(waiting_said(1), "Waiting · 1 quick one not counted");
        assert_eq!(waiting_said(3), "Waiting · 3 quick ones not counted");
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
            next: None,
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
            next: None,
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
            next: None,
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
