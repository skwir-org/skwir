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

use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime};

use gpui::{AnyElement, Context, FocusHandle, SharedString, Window, div, prelude::*, px};
use skwir_core::op::{PlanId, Transfer};

use skwir_platform::desktop::Asked;

use crate::controls::{self, ButtonStyle};
use crate::elements::adaptive::Fit;
use crate::elements::popover::PopoverAnchor;
use crate::format;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toast::{Action, Running, Tone, bar, capitalized, pace, sending, waiting_lines};
use crate::work::{Seen, Watched};

/// Activity's popup, as `Shell::toggle_popup` and `toolbar_menu` know it.
pub const POPUP: &str = "activity";

/// What `Earlier` keeps: the answers of this session, newest first.
pub(crate) const EARLIER: usize = 20;

/// The bar's width in the strip: as much words as the footer has room for
/// beside it, then less bar.
const BAR_WIDE: f32 = 54.;
const BAR_NARROW: f32 = 24.;

/// An action that ended this session, as its answer said it.
#[derive(Clone, Debug)]
pub struct Ended {
    pub plan: PlanId,
    pub text: String,
    pub tone: Tone,
    /// Its Undo, or Retry for one an error stopped (never Redo: what it
    /// would redo moves on).
    pub actions: Vec<Action>,
    pub at: SystemTime,
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
    /// Each `Earlier` row's buttons, by plan and place.
    rows: HashMap<(PlanId, usize), FocusHandle>,
    strip: Option<StripFocus>,
    pin: Option<FocusHandle>,
    /// What `ui.activity` gives the keyboard to while pinned and shown.
    tray: Option<FocusHandle>,
}

/// What runs and is shown: the plans worth a card, then the tasks (a print).
pub(crate) struct Under {
    pub plans: Vec<(Running, Transfer)>,
    pub tasks: Vec<skwir_core::task::Running>,
}

impl Under {
    pub(crate) fn is_empty(&self) -> bool {
        self.plans.is_empty() && self.tasks.is_empty()
    }

    pub(crate) fn count(&self) -> usize {
        self.plans.len() + self.tasks.len()
    }
}

/// How far all of it went together.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Summary {
    /// From 0 to 1: their bytes together when each was sized before it
    /// began, else each one's share alike.
    pub fraction: f32,
    /// The longest time left of those that can tell it: they run at once
    /// (ADR-079).
    pub left: Option<Duration>,
    /// Every one is paused.
    pub paused: bool,
    /// Every one was cancelled and is stopping.
    pub stopping: bool,
}

/// `under` summed up, `None` when nothing runs.
pub(crate) fn summary(under: &Under) -> Option<Summary> {
    if under.is_empty() {
        return None;
    }
    let sized: Option<(u64, u64)> = (under.tasks.is_empty())
        .then(|| {
            under
                .plans
                .iter()
                .try_fold((0u64, 0u64), |(done, whole), (_, t)| {
                    let planned = t.planned.filter(|&planned| planned > 0)?;
                    Some((done + t.done, whole + planned.max(t.done)))
                })
        })
        .flatten();
    // Each one's share alike, where one was not sized.
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)] // Shares for a bar.
    let alike = || {
        let shares = under
            .plans
            .iter()
            .map(|(running, t)| bar(running.total, t).0)
            .chain(
                under
                    .tasks
                    .iter()
                    .map(|task| task.went.unwrap_or(0.) as f32),
            );
        shares.sum::<f32>() / under.count() as f32
    };
    #[allow(clippy::cast_precision_loss)] // A share for a bar.
    let fraction = sized.map_or_else(alike, |(done, whole)| {
        (done as f32 / whole.max(1) as f32).clamp(0., 1.)
    });
    let left = under.plans.iter().filter_map(|(_, t)| t.left).max();
    let paused = under.tasks.is_empty() && under.plans.iter().all(|(r, _)| r.paused);
    let stopping = under.plans.iter().all(|(r, _)| r.cancelling)
        && under.tasks.iter().all(|task| task.cancelling);
    Some(Summary {
        fraction,
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

/// What the strip says of how it goes: the share done and the time left;
/// paused, stopping, or sending to its server instead.
fn how(summary: &Summary, sending_files: Option<usize>) -> String {
    let at = percent(summary.fraction);
    if summary.stopping {
        return "stopping…".to_owned();
    }
    if summary.paused {
        return format!("paused at {at}");
    }
    if let Some(files) = sending_files {
        return sending(files);
    }
    match summary.left {
        Some(left) => format!("{at} · {}", format::time_left(left)),
        None => at,
    }
}

/// The strip's words, richest first: what runs and how, how alone, the
/// share alone.
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
        _ => format::count(under.count(), "action"),
    };
    let least = if summary.stopping {
        "Stopping…".to_owned()
    } else {
        percent(summary.fraction)
    };
    [format!("{what} · {how}"), capitalized(&how), least]
}

impl Shell {
    /// What runs and is shown as a card (`Work::under_way`).
    pub(crate) fn under_way(&self) -> Under {
        self.work.under_way(&self.app)
    }

    /// Whether Activity's popup is open.
    pub(crate) fn activity_open(&self) -> bool {
        self.toolbar_menu
            .as_ref()
            .is_some_and(|popup| popup.kind == POPUP)
    }

    /// Whether Activity stands above the footer: pinned, with something
    /// under way, waiting, or stopped to show.
    pub(crate) fn activity_tray_shown(&self) -> bool {
        self.activity.pinned
            && (self.work.stopped.is_some()
                || !self.under_way().is_empty()
                || !self.app.executor.waiting().is_empty())
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
            // Seen: the strip says it no more.
            self.work.stopped = None;
        }
        cx.notify();
    }

    /// `ui.activity_pin`: Activity stands above the footer while something
    /// is under way, or opens over the files when asked.
    pub(crate) fn toggle_activity_pin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.activity.pinned = !self.activity.pinned;
        self.remember_session();
        // Pinned with something to show, it stands where the popup was.
        if self.activity.pinned && self.activity_open() && self.activity_tray_shown() {
            self.close_popup(window, cx);
        }
        cx.notify();
    }

    /// `Earlier`'s rows' buttons, kept for the answers it still holds.
    pub(crate) fn forget_earlier_rows(&mut self) {
        let kept: HashSet<PlanId> = self.work.earlier.iter().map(|e| e.plan).collect();
        self.activity
            .rows
            .retain(|(plan, _), _| kept.contains(plan));
    }

    fn strip_focus(&mut self, cx: &mut Context<Self>) -> &StripFocus {
        self.activity.strip.get_or_insert_with(|| StripFocus {
            open: cx.focus_handle(),
            pause: cx.focus_handle(),
            cancel: cx.focus_handle(),
            retry: cx.focus_handle(),
            dismiss: cx.focus_handle(),
        })
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
        self.work.carded.extend(plans);
    }
}

/// A control pressed as every one is (`modal::pressed`): by a click, and by
/// Enter and Space while it has the keyboard, never by gpui's own click on
/// a key's release, which a binding of the key would stop.
trait Press {
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
fn stop(cx: &mut Context<Shell>) -> FocusHandle {
    cx.focus_handle().tab_index(0).tab_stop(true)
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

/// The strip's buttons' handles, cloned out of the shell for a render.
struct Handles {
    open: FocusHandle,
    pause: FocusHandle,
    cancel: FocusHandle,
    retry: FocusHandle,
    dismiss: FocusHandle,
}

/// The buttons `offers` holds, built for one version of the strip: each
/// version is its own element, its buttons with the same ids.
fn strip_buttons(
    offers: Offers,
    focus: &Handles,
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
                            this.work.stopped = None;
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
                        this.work.stopped = None;
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

/// The strip at the end of the last pane's footer: what runs, how far, and
/// the way to Activity, opened above it; `None` while there is nothing to
/// say, or while Activity, pinned, says it above.
pub fn strip(shell: &mut Shell, theme: &Theme, cx: &mut Context<Shell>) -> Option<AnyElement> {
    if shell.activity_tray_shown() {
        return None;
    }
    let under = shell.under_way();
    let open = shell.activity_open();
    if under.is_empty() && shell.work.stopped.is_none() && !open {
        return None;
    }
    shell.card_focus(&under, cx);
    let focus = {
        let f = shell.strip_focus(cx);
        Handles {
            open: f.open.clone(),
            pause: f.pause.clone(),
            cancel: f.cancel.clone(),
            retry: f.retry.clone(),
            dismiss: f.dismiss.clone(),
        }
    };
    // What it says, richest first, its colour and bar, and what it offers.
    let (said, color, fraction, offers) = match summary(&under) {
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
                words(&under, &summary),
                color,
                Some(summary.fraction),
                offers,
            )
        }
        None => match shell.work.stopped.clone() {
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
    let several = under.count() > 1;
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
                // The last version, drawn when nothing else fits, is cut.
                let text = div().min_w_0().overflow_hidden().whitespace_nowrap();
                if last { text.text_ellipsis() } else { text }.child(words.clone())
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
                .child(opener)
                .children(strip_buttons(offers, &focus, theme, cx))
                .into_any_element(),
        );
    }
    let popup = popover(shell, theme, cx);
    Some(
        div()
            .flex()
            .min_w_0()
            .child(Fit {
                variants: versions,
                cuts: true,
                start: false,
            })
            .children(popup)
            .into_any_element(),
    )
}

/// Activity, opened above its strip: what `body` holds.
fn popover(shell: &mut Shell, theme: &Theme, cx: &mut Context<Shell>) -> Option<AnyElement> {
    if !shell.activity_open() {
        return None;
    }
    let body = body(shell, theme, cx);
    let popup = shell.toolbar_menu.as_ref()?;
    let panel = crate::toolbar::popup_panel(popup, "ui.activity", theme, cx)
        .role(gpui::Role::Group)
        .aria_label("Activity")
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
/// way: what the popup holds, taking room rather than covering the files.
pub fn tray(shell: &mut Shell, theme: &Theme, cx: &mut Context<Shell>) -> Option<AnyElement> {
    if !shell.activity_tray_shown() {
        return None;
    }
    let under = shell.under_way();
    shell.card_focus(&under, cx);
    let focus = shell
        .activity
        .tray
        .get_or_insert_with(|| cx.focus_handle().tab_stop(false))
        .clone();
    let body = body(shell, theme, cx);
    Some(
        div()
            .id("activity-tray")
            .role(gpui::Role::Group)
            .aria_label("Activity")
            .track_focus(&focus)
            // Its own place in the window's Tab order, its buttons walked
            // in theirs (as `toolbar::Popup`'s).
            .tab_group()
            .tab_index(1)
            .flex_none()
            .max_h(gpui::relative(0.5))
            .overflow_y_scroll()
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

/// What Activity holds, in the popup or pinned: its name and pin, the
/// cards of what runs, what waits, and what ended this session.
fn body(shell: &mut Shell, theme: &Theme, cx: &mut Context<Shell>) -> Vec<AnyElement> {
    let under = shell.under_way();
    shell.card_focus(&under, cx);
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
                    .press(
                        |this, window, cx| {
                            this.run("ui.activity_pin", serde_json::json!({}), window, cx);
                        },
                        cx,
                    ),
            )
            .into_any_element(),
    );
    let cards = under.count();
    for (running, transfer) in &under.plans {
        let focus = shell.activity.cards[&running.plan].clone();
        parts.push(
            progress_card(running, transfer, (&focus.0, &focus.1), a11y, theme, cx)
                .into_any_element(),
        );
    }
    for task in &under.tasks {
        let focus = shell.activity.tasks[&task.id].clone();
        parts.push(task_card(task, &focus, a11y, theme, cx).into_any_element());
    }
    // What the user did meanwhile waits behind what it meets, and paused,
    // for as long as it stays paused: each with what it waits for.
    let waiting = waiting_lines(&shell.app.executor.waiting());
    if !waiting.is_empty() {
        parts.push(
            div()
                .id("activity-waiting")
                .role(gpui::Role::Status)
                .when(a11y, |d| d.aria_label(waiting.join(". ")))
                .flex()
                .flex_col()
                .gap_1()
                .px_3()
                .pb_2()
                .text_xs()
                .text_color(theme.text_muted)
                .children(waiting)
                .into_any_element(),
        );
    }
    let earlier: Vec<Ended> = shell.work.earlier.iter().cloned().collect();
    if !earlier.is_empty() {
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
        for ended in &earlier {
            parts.push(earlier_row(shell, ended, theme, cx));
        }
    } else if cards == 0 && waiting_is_empty(shell) {
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

fn waiting_is_empty(shell: &Shell) -> bool {
    shell.app.executor.waiting().is_empty()
}

/// A row of `Earlier`: how it ended, what, when, and its way back (Undo),
/// or Retry while it can be.
fn earlier_row(
    shell: &mut Shell,
    ended: &Ended,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let glyph = match ended.tone {
        Tone::Success => controls::glyph("check", theme.success, 13.),
        Tone::Error => controls::glyph("warning", theme.error, 13.),
        Tone::Info => controls::glyph("info", theme.text_muted, 13.),
    };
    let plan = ended.plan;
    let actions: Vec<(usize, Action)> = ended
        .actions
        .iter()
        .cloned()
        .enumerate()
        .filter(|(_, action)| action.command != "retry" || shell.app.executor.can_retry(plan))
        .collect();
    let mut row = div()
        .id(("activity-earlier", plan.0))
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
                .id(("activity-earlier-text", plan.0))
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
        let focus = shell
            .activity
            .rows
            .entry((plan, n))
            .or_insert_with(|| stop(cx))
            .clone();
        row = row.child(
            controls::button(
                SharedString::from(format!("activity-earlier-{}-{n}", plan.0)),
                action.label.clone(),
                None,
                ButtonStyle::Plain,
                &focus,
                theme,
            )
            .h(px(22.))
            .px_2()
            .text_color(theme.accent_text)
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

/// The card of the running plan `running` in Activity: its title, how far
/// it is, how fast and how long, and its Pause and Cancel, which `focus`
/// holds.
fn progress_card(
    running: &Running,
    transfer: &Transfer,
    focus: (&FocusHandle, &FocusHandle),
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
            && let Some(running) = self.work.running.iter_mut().find(|r| r.plan == plan)
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
            self.work.looked_at();
        }
    }

    /// The taskbar's entry, the machine kept awake, the session's end held
    /// off, as what runs goes (`Work::tell`).
    pub(crate) fn tell_desktop(&mut self) {
        let under = self.under_way();
        let watched = self.watched();
        self.work.tell(&self.app, &under, watched);
    }

    /// What the desktop asked: a notice or the presence clicked brings the
    /// window forward, with the token the desktop gave where it gave one,
    /// and opens Activity; its menu pauses, lets go on or stops what runs
    /// (`finishing::act`).
    pub(crate) fn take_asked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut token = None;
        let mut shown = false;
        for asked in self.work.take_asked() {
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
                    crate::finishing::act(&self.app, &self.work, &asked);
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
    use super::{Summary, Under, how, percent, summary, words};
    use crate::toast::Running;
    use skwir_core::op::{PlanId, RunKind, Transfer};
    use std::time::Duration;

    fn running(plan: u64, title: &str, total: usize) -> Running {
        Running::for_test(PlanId(plan), title, RunKind::Apply, total)
    }

    #[test]
    fn a_share_is_said_whole_and_never_full_before_the_end() {
        assert_eq!(percent(0.0), "0 %");
        assert_eq!(percent(0.429), "42 %");
        assert_eq!(percent(0.999), "99 %");
        assert_eq!(percent(1.0), "100 %");
    }

    /// Plans sized before they began count their bytes together; one that
    /// was not makes each count alike. The time left is the longest.
    #[test]
    fn several_actions_add_up_by_their_bytes_when_each_was_sized() {
        let sized = |done: u64, planned: u64, left: u64| Transfer {
            done,
            planned: Some(planned),
            left: Some(Duration::from_secs(left)),
            ..Transfer::default()
        };
        let under = Under {
            plans: vec![
                (running(1, "copy a", 3), sized(1 << 30, 3 << 30, 120)),
                (running(2, "copy b", 2), sized(0, 1 << 30, 300)),
            ],
            tasks: Vec::new(),
        };
        let s = summary(&under).unwrap();
        assert!((s.fraction - 0.25).abs() < 1e-6, "{}", s.fraction);
        assert_eq!(s.left, Some(Duration::from_secs(300)));
        assert!(!s.paused && !s.stopping);

        let unequal = Under {
            plans: vec![
                (running(1, "copy a", 3), sized(1 << 30, 3 << 30, 120)),
                (
                    running(2, "rename b", 4),
                    Transfer {
                        ended: 2,
                        ..Transfer::default()
                    },
                ),
            ],
            tasks: Vec::new(),
        };
        let s = summary(&unequal).unwrap();
        // A third and a half, alike.
        assert!(
            (s.fraction - f32::midpoint(1. / 3., 0.5)).abs() < 1e-3,
            "{}",
            s.fraction
        );
        assert!(
            summary(&Under {
                plans: Vec::new(),
                tasks: Vec::new()
            })
            .is_none()
        );
    }

    /// One action is said by its title; several by how many; the least
    /// the footer can hold is the share alone.
    #[test]
    fn the_strip_says_what_runs_as_room_allows() {
        let one = Under {
            plans: vec![(
                running(1, "copy 3 items to Backup", 3),
                Transfer {
                    done: 42,
                    planned: Some(100),
                    left: Some(Duration::from_secs(170)),
                    ..Transfer::default()
                },
            )],
            tasks: Vec::new(),
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
        let paused = Summary { paused: true, ..s };
        assert_eq!(how(&paused, None), "paused at 42 %");
        let stopping = Summary {
            stopping: true,
            ..s
        };
        assert_eq!(words(&one, &stopping)[2], "Stopping…");
        assert_eq!(how(&s, Some(3)), "sending 3 files to the server");

        let two = Under {
            plans: vec![
                one.plans[0].clone(),
                (running(2, "move b", 2), Transfer::default()),
            ],
            tasks: Vec::new(),
        };
        let s = summary(&two).unwrap();
        assert!(
            words(&two, &s)[0].starts_with("2 actions · "),
            "{:?}",
            words(&two, &s)
        );
    }
}
