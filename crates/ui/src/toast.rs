//! Toasts: what just happened, with the way back. One answer at a time,
//! drawn over the files at the last pane's lower right, above Activity's
//! strip (`activity.rs`, ADR-114), or over the preview on the whole screen.
//! What runs is not a toast: Activity shows it, docked at the window's foot.
//! A plan's answer is built here as it ends ("Moved 3 items to Trash",
//! Undo), and kept in Activity's `Earlier` too.
//!
//! The only animation is a short entrance that ends by itself; nothing here
//! keeps the UI awake.

use crate::sizes::TextSizes;
use crate::sizes::sized;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::{Animation, AnimationExt, AnyElement, Context, div, prelude::*, px};
use skwir_core::op::{Change, Counted, PlanId, RunKind, Tally, Transfer};

use crate::controls::{self, ButtonStyle};
use crate::format;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::work::{Taken, WAITS_FOR_SERVER};

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
    /// Its buttons, in their order (a plan stopped by an error: Retry,
    /// Undo).
    pub actions: Vec<Action>,
}

/// A plan being applied.
#[derive(Clone, Debug)]
pub struct Running {
    pub plan: PlanId,
    pub title: String,
    pub run: RunKind,
    pub total: usize,
    /// First change that was applied, to name a single item in the result,
    /// and the name the user knows its file by where its path says another
    /// (`Progress::named`: an item of a trash).
    first: Option<Change>,
    first_named: Option<String>,
    /// The change whose step failed, to say what could not be done, and
    /// its file's name as the user knows it.
    failed: Option<Change>,
    failed_named: Option<String>,
    /// What the user asked for, where the steps are not its items (a
    /// merge, a replace), as `Started` said it, before the plan was
    /// checked: what the result says when `Finished` says nothing better.
    /// Shared: a retried plan's names every item placed, and what runs is
    /// read once a frame (`activity::Under`).
    items: Option<Arc<skwir_core::op::Items>>,
    /// Waiting where it would answer Cancel, until resumed.
    pub(crate) paused: bool,
    pub(crate) cancelling: bool,
    /// What a plan its server stopped had left, going on by itself once
    /// the server answered again (ADR-080, 9).
    pub(crate) going_on: bool,
}

impl Running {
    /// A plan just started (`ExecEvent::Started`): `going_on` when it goes
    /// on from one its server stopped (`ExecEvent::GoesOn`).
    pub(crate) fn started(
        plan: PlanId,
        title: String,
        run: RunKind,
        total: usize,
        items: Option<skwir_core::op::Items>,
        going_on: bool,
    ) -> Self {
        Running {
            plan,
            title,
            run,
            total,
            first: None,
            first_named: None,
            failed: None,
            failed_named: None,
            items: items.map(Arc::new),
            paused: false,
            cancelling: false,
            going_on,
        }
    }

    /// A step made (`ExecEvent::Step`): the count, and the change that
    /// names what was done or what could not be.
    pub(crate) fn stepped(&mut self, progress: &skwir_core::op::Progress) {
        self.total = progress.total;
        if progress.error.is_some() {
            self.failed = Some(progress.change.clone());
            self.failed_named.clone_from(&progress.named);
        } else if self.first.is_none() {
            self.first = Some(progress.change.clone());
            self.first_named.clone_from(&progress.named);
        }
    }

    /// The first change applied.
    pub(crate) fn first(&self) -> Option<&Change> {
        self.first.as_ref()
    }

    /// A plan just started, for tests elsewhere.
    #[cfg(test)]
    pub(crate) fn for_test(plan: PlanId, title: &str, run: RunKind, total: usize) -> Self {
        Running::started(plan, title.to_owned(), run, total, None, false)
    }
}

#[derive(Default)]
pub struct ToastState {
    pub current: Option<Toast>,
    generation: u64,
    /// Whether the time of the current one runs: from when it is said, or,
    /// held back while Activity or the History is open, from when it is
    /// first drawn.
    timed: bool,
    close: Option<gpui::FocusHandle>,
    /// Kept across frames, so Tab can reach the toast's buttons.
    action_focus: Vec<gpui::FocusHandle>,
}

impl ToastState {
    /// `toast` is said, replacing the current one: its generation, and how
    /// many seconds it stays from now, unless `held` (Activity or the
    /// History is open, and none is drawn): its time then begins as it is
    /// first drawn (`drawn`).
    fn say(&mut self, toast: Toast, held: bool) -> (u64, Option<u64>) {
        let seconds = shown_for(&toast);
        self.current = Some(toast);
        self.generation += 1;
        self.timed = !held;
        (self.generation, self.timed.then_some(seconds))
    }

    /// The current one is drawn: its generation and how many seconds it
    /// stays, when its time begins now.
    fn drawn(&mut self) -> Option<(u64, u64)> {
        if self.timed {
            return None;
        }
        let seconds = shown_for(self.current.as_ref()?);
        self.timed = true;
        Some((self.generation, seconds))
    }

    /// The time of the one said at `generation` is over: it goes, unless
    /// another replaced it since. Whether it went.
    fn over(&mut self, generation: u64) -> bool {
        if self.generation != generation {
            return false;
        }
        self.current = None;
        true
    }
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

pub(crate) fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// How a sentence about `change` quotes a path: the file it acts on by
/// `named` where the step said it (an item of a trash, by the name it had),
/// any other path by its own name.
fn quoting<'a>(
    change: Option<&'a Change>,
    named: Option<&'a str>,
) -> impl Fn(&Path) -> String + 'a {
    let subject = change.and_then(Change::subject);
    move |path: &Path| match named {
        Some(named) if subject == Some(path) => format!("“{named}”"),
        _ => format!("“{}”", name_of(path)),
    }
}

/// What an applied plan did, in a sentence: the item by name when there is
/// one (by `named`, where the step said the name the user knows it by), a
/// count otherwise. Falls back to the plan's title.
pub fn describe(
    title: &str,
    run: RunKind,
    first: Option<&Change>,
    named: Option<&str>,
    applied: usize,
    kept_in: Option<&str>,
) -> String {
    let many = format::count(applied, "item");
    let one = applied == 1;
    let quoted = quoting(first, named);
    match first {
        _ if run == RunKind::Undo => {
            format!("Undid: {}", title.strip_prefix("undo ").unwrap_or(title))
        }
        Some(Change::Trash { path }) if one => format!("Moved {} to Trash", quoted(path)),
        Some(Change::Trash { .. }) => format!("Moved {many} to Trash"),
        Some(Change::Restore { path }) if one => format!("Restored {}", quoted(path)),
        Some(Change::Restore { .. }) => format!("Restored {many}"),
        // Kept by its server (a cloud's trash, ADR-086), never said gone.
        Some(Change::Delete { path }) if one => match kept_in {
            Some(trash) => format!("Deleted {} (in {trash})", quoted(path)),
            None => format!("Deleted {} permanently", quoted(path)),
        },
        Some(Change::Delete { .. }) => match kept_in {
            Some(trash) => format!("Deleted {many} (in {trash})"),
            None => format!("Deleted {many} permanently"),
        },
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
        Some(Change::Link { from, to }) if one => {
            format!("Made a link to {} in {}", quoted(from), folder_of(to))
        }
        Some(Change::Link { to, .. }) => format!(
            "Made {} in {}",
            format::count(applied, "link"),
            folder_of(to)
        ),
        Some(Change::Create {
            words: Some(words), ..
        }) if one => capitalized(&words.did),
        Some(Change::Create {
            path,
            directory: true,
            ..
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
/// failed, by name (`named`, as `describe`), then why. An error that begins
/// by naming that step ("“a.txt” could not be renamed …", "… could not all
/// be deleted") gives its reason alone after it.
pub fn failure(title: &str, failed: Option<&Change>, named: Option<&str>, error: &str) -> String {
    let quoted = quoting(failed, named);
    let tried = match failed {
        Some(Change::Copy { from, to }) => {
            format!("Couldn't copy {} to {}", quoted(from), folder_of(to))
        }
        Some(Change::Move { from, to }) => {
            format!("Couldn't move {} to {}", quoted(from), folder_of(to))
        }
        Some(Change::Link { from, to }) => {
            format!(
                "Couldn't make a link to {} in {}",
                quoted(from),
                folder_of(to)
            )
        }
        Some(Change::Trash { path }) => format!("Couldn't move {} to Trash", quoted(path)),
        Some(Change::Restore { path }) => format!("Couldn't restore {}", quoted(path)),
        Some(Change::Delete { path }) => format!("Couldn't delete {}", quoted(path)),
        Some(Change::Rename { from, to }) => {
            format!("Couldn't rename {} to {}", quoted(from), quoted(to))
        }
        // What it said it would do, it could not.
        Some(Change::Create {
            words: Some(words), ..
        }) => format!("Couldn't {}", words.will),
        Some(Change::Create {
            path,
            directory: true,
            ..
        }) => format!("Couldn't create folder {}", quoted(path)),
        Some(Change::Create { path, .. }) => format!("Couldn't create {}", quoted(path)),
        _ => return format!("{}: {error}", capitalized(title)),
    };
    // The error names the step's file by its path, as the step reads it.
    let own = failed
        .and_then(Change::subject)
        .map(|path| format!("“{}”", name_of(path)));
    let reason = own
        .and_then(|name| error.strip_prefix(&format!("{name} could not ")))
        .and_then(|rest| rest.split_once(": "))
        .map_or(error, |(_, reason)| reason);
    format!("{tried}: {reason}")
}

/// What a running plan's bar says: how far it is, from 0 to 1 (`share`),
/// and in words. A plan whose bytes are known in all (sized before it
/// began, ADR-113) moves by them, its items counted beside; one of one item
/// counts the bytes it said as it began. Else it counts its items, the
/// bytes of the one under way moving the bar between two counts: its bytes
/// are known only as each item starts.
pub fn bar(items: usize, t: &Transfer) -> (f32, String) {
    let fraction = share(items, t);
    if items <= 1 {
        let whole = t.planned.unwrap_or(t.total).max(t.done);
        let words = format!("{} of {}", format::size(t.done), format::size(whole));
        return (fraction, words);
    }
    if let Some(planned) = t.planned.filter(|&planned| planned > 0) {
        // Items as they come whole, a batch's before their steps place
        // them (`Transfer::finished`).
        let words = format!(
            "{} of {} · {} of {}",
            format::thousands(t.finished),
            format::count(items, "item"),
            format::size(t.done),
            format::size(planned.max(t.done))
        );
        return (fraction, words);
    }
    let count = format!(
        "{} of {}",
        format::thousands(t.finished),
        format::count(items, "item")
    );
    let words = if t.done > 0 {
        format!("{count} · {}", format::size(t.done))
    } else {
        count
    };
    (fraction, words)
}

/// How far a running plan's bar is, from 0 to 1, as `bar` says it: by its
/// bytes where they are known in all, else by its items.
pub(crate) fn share(items: usize, t: &Transfer) -> f32 {
    let by_bytes = |whole: u64| (t.done as f32 / whole.max(1) as f32).clamp(0., 1.);
    if items <= 1 {
        return by_bytes(t.planned.unwrap_or(t.total).max(t.done));
    }
    if let Some(planned) = t.planned.filter(|&planned| planned > 0) {
        return by_bytes(planned.max(t.done));
    }
    let within = if t.step_total > 0 {
        (t.step_done as f32 / t.step_total as f32).clamp(0., 1.)
    } else {
        0.
    };
    ((t.ended as f32 + within) / items as f32).clamp(0., 1.)
}

/// What Quit asks while a plan runs (ADR-115): which (`running`: its
/// title, and whether it is paused, when it has begun), and the two ways
/// on, for it and the `waiting` ones behind it: Skwir finishes without its
/// window, then quits; or stops now. No figures: the bar under the
/// question goes on with them.
pub fn quit_question(running: Option<(&str, bool)>, others: usize, waiting: usize) -> String {
    let (what, them) = match (running, others) {
        (Some((title, paused)), 0) => {
            let state = if paused { "paused" } else { "still running" };
            (format!("{} is {state}.", capitalized(title)), false)
        }
        // Plans that share nothing run at once (ADR-079).
        (Some((title, paused)), others) => {
            let state = if paused { "paused" } else { "still running" };
            let what = format!(
                "{} is {state}, and {} beside it.",
                capitalized(title),
                format::count(others, "other action")
            );
            (what, true)
        }
        (None, _) => ("A change is about to begin.".to_owned(), false),
    };
    let (it, its) = if them { ("them", "they") } else { ("it", "it") };
    let after = match waiting {
        0 => String::new(),
        n => format!(
            ", and the {} waiting after {it},",
            format::count(n, "action")
        ),
    };
    format!(
        "{what} Skwir can finish {it}{after} without its window, then quit. Or stop now: \
         what {its} finished stays, what {its} {was} in the middle of is taken back.",
        was = if them { "were" } else { "was" }
    )
}

/// Words read in a second, unhurried.
const WORDS_A_SECOND: u64 = 3;

/// How many seconds `toast` stays: by its weight, and at least the time its
/// words take to read.
fn shown_for(toast: &Toast) -> u64 {
    let weight = match (&toast.tone, toast.actions.is_empty()) {
        (Tone::Error, _) => 10,
        (_, false) => 8,
        _ => 4,
    };
    let words = toast.text.split_whitespace().count() as u64;
    weight.max(words.div_ceil(WORDS_A_SECOND))
}

impl Shell {
    /// Show `toast` (`show_toast`), and keep it in Activity's Earlier: what
    /// a toast said is found there again once it has gone. A plan's answer
    /// is kept by `Work` as the plan ends, unless the History holds it.
    pub fn toast(&mut self, toast: Toast, cx: &mut Context<Self>) {
        crate::activity::keep(
            &mut self.work.borrow_mut().earlier,
            crate::activity::Ended::said(&toast),
        );
        self.forget_earlier_rows();
        self.show_toast(toast, cx);
    }

    /// Show `toast`, replacing the current one, for a time that fits its
    /// weight: errors and anything with a button stay longer, and a long
    /// one (what a plan's steps said after what it did) as long as it
    /// takes to read. Held back while Activity or the History is open, it
    /// keeps its time: that begins as it is first drawn (`render_answer`).
    fn show_toast(&mut self, toast: Toast, cx: &mut Context<Self>) {
        let held = self.activity_open() || self.undo_history_open();
        if let (generation, Some(seconds)) = self.toasts.say(toast, held) {
            self.time_toast(generation, seconds, cx);
        }
        cx.notify();
    }

    /// The toast said at `generation` goes in `seconds`, unless another
    /// replaced it by then.
    fn time_toast(&mut self, generation: u64, seconds: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(seconds))
                .await;
            this.update(cx, |shell, cx| {
                if shell.toasts.over(generation) {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn dismiss_toast(&mut self, cx: &mut Context<Self>) {
        self.toasts.current = None;
        self.toasts.generation += 1;
        cx.notify();
    }

    /// What the plans this window ran did, as the program took it from
    /// the executor (`Work::take`, `finishing::deliver`): the window shows
    /// what a plan made, and one toast saying what it did and offering the
    /// way back.
    pub(crate) fn take_plans(&mut self, taken: Vec<Taken>, cx: &mut Context<Self>) {
        if taken.is_empty() {
            return;
        }
        for taken in taken {
            match taken {
                Taken::Nothing => {}
                Taken::Step(progress) => self.made_step(&progress),
                // Its toast says it goes on by itself, while it is the
                // one shown (its Retry names the plan).
                Taken::WaitsForServer(plan) => {
                    let said = WAITS_FOR_SERVER;
                    if let Some(toast) = self.toasts.current.as_mut()
                        && toast.actions.iter().any(|a| {
                            a.command == "retry" && a.args["plan"] == serde_json::json!(plan)
                        })
                        && !toast.text.ends_with(said)
                    {
                        toast.text.push_str(said);
                    }
                }
                Taken::Ended {
                    plan,
                    done,
                    items,
                    answer,
                } => {
                    self.made(plan, done, items.as_ref());
                    if let Some(answer) = answer {
                        self.forget_earlier_rows();
                        self.show_toast(answer, cx);
                    }
                }
            }
        }
        cx.notify();
    }
}

/// What the toast of `running`, ended, says: what it did, item by item
/// when it went through (`items`, as done: unfolded, names taken settled,
/// which `Started` could not say yet; else what it was asked), as far as
/// its steps went when cut short; what stopped it; or that it was
/// cancelled, before anything was done (a question about names taken
/// answered Cancel) or after. What it found done already it says it
/// found (`tally`), never that it did it: alone when it did nothing else
/// ("“photos.zip” was already extracted to Downloads"), else after what it
/// did ("Copied 3 items to Docs; 2 were already there"). What the steps
/// said follows; alone when they did nothing else (a smaller copy not
/// made).
pub(crate) fn ended(running: Running, end: End<'_>) -> (String, Tone) {
    let End {
        applied,
        error,
        cancelled,
        items,
        kept_in,
        said,
        moved,
        ran,
        tally,
    } = end;
    let said = said.join(". ");
    let already = tally.filter(|t| t.already.count > 0 && error.is_none() && !cancelled);
    let nothing_else = applied == 0 && error.is_none() && !cancelled && already.is_none();
    let (title, run) = (running.title.clone(), running.run);
    let (mut text, tone) = match already {
        Some(tally) if tally.done.count == 0 => {
            (found_done(&title, run, &tally.already), Tone::Info)
        }
        Some(tally) => (
            describe(
                &title,
                run,
                tally.done.first.as_ref(),
                tally.done.named.as_deref(),
                tally.done.count,
                kept_in,
            ),
            Tone::Success,
        ),
        None => ended_as(running, applied, error, cancelled, items, kept_in),
    };
    // A long one says how much it moved, in how long (ADR-113).
    if tone == Tone::Success && moved > 0 && ran >= AVERAGED_FROM {
        text = format!("{text} ({} in {})", format::size(moved), format::took(ran));
    }
    if let Some(tally) = already.filter(|t| t.done.count > 0) {
        text = format!("{text}; {}", found_done_too(run, &tally.already));
    }
    match (said.is_empty(), nothing_else) {
        (true, _) => (text, tone),
        (false, true) => (said, Tone::Info),
        (false, false) => (format!("{text}. {said}"), tone),
    }
}

/// From how long a plan ran its toast says what it moved in that time.
const AVERAGED_FROM: Duration = Duration::from_secs(5);

/// What a plan that found all it was asked done already says
/// (`Tally::already`): its item by name where there is one, else how
/// many, as its change says it ("“photos.zip” was already extracted to
/// Downloads", "3 items were already in Docs").
pub fn found_done(title: &str, run: RunKind, found: &Counted) -> String {
    let (first, count) = (found.first.as_ref(), found.count);
    let quoted = quoting(first, found.named.as_deref());
    let one = count == 1;
    let many = format::count(count, "item");
    match first {
        _ if run == RunKind::Undo => {
            format!(
                "Already undone: {}",
                title.strip_prefix("undo ").unwrap_or(title)
            )
        }
        // In the folder, under the name it would have had there.
        Some(Change::Copy { to, .. } | Change::Move { to, .. }) if one => {
            format!("{} was already in {}", quoted(to), folder_of(to))
        }
        Some(Change::Copy { to, .. } | Change::Move { to, .. }) => {
            format!("{many} were already in {}", folder_of(to))
        }
        Some(Change::Link { from, to }) if one => {
            format!(
                "A link to {} was already in {}",
                quoted(from),
                folder_of(to)
            )
        }
        Some(Change::Link { to, .. }) => format!(
            "{} were already in {}",
            format::count(count, "link"),
            folder_of(to)
        ),
        Some(Change::Trash { path } | Change::Delete { path } | Change::Remove { path }) if one => {
            format!("{} was already gone", quoted(path))
        }
        Some(Change::Trash { .. } | Change::Delete { .. } | Change::Remove { .. }) => {
            format!("{many} were already gone")
        }
        Some(Change::Restore { path }) if one => format!("{} was already restored", quoted(path)),
        Some(Change::Restore { .. }) => format!("{many} were already restored"),
        Some(Change::Rename { from, to }) if one => {
            format!("{} was already named {}", quoted(from), quoted(to))
        }
        Some(Change::Rename { .. }) => format!("{many} already had their names"),
        Some(Change::Swap { a, b }) if one => {
            format!("{} and {} were already swapped", quoted(a), quoted(b))
        }
        // A step that says what it found says it best.
        Some(
            Change::Create {
                words: Some(words), ..
            }
            | Change::Metadata {
                words: Some(words), ..
            },
        ) if one && !words.already.is_empty() => capitalized(&words.already),
        Some(Change::Create {
            path,
            directory: true,
            ..
        }) if one => format!("Folder {} was already there", quoted(path)),
        Some(Change::Create { path, .. }) if one => format!("{} was already there", quoted(path)),
        Some(Change::Metadata { path, .. }) if one => {
            format!("{} was already as asked", quoted(path))
        }
        _ => format!("{}: already done", capitalized(title)),
    }
}

/// What a plan that did some of what it was asked says, after what it
/// did, of what it found done already (`Tally::already`): "Copied 3
/// items to Docs; 2 were already there".
pub fn found_done_too(run: RunKind, found: &Counted) -> String {
    let (first, count) = (found.first.as_ref(), found.count);
    let quoted = quoting(first, found.named.as_deref());
    let one = count == 1;
    let was = |what: String, how: &str| format!("{what} was already {how}");
    let were = |how: &str| format!("{count} were already {how}");
    match first {
        _ if run == RunKind::Undo => "part of it was already undone".to_owned(),
        Some(Change::Copy { to, .. } | Change::Move { to, .. }) if one => was(quoted(to), "there"),
        Some(Change::Link { from, .. }) if one => {
            was(format!("a link to {}", quoted(from)), "there")
        }
        Some(Change::Copy { .. } | Change::Move { .. } | Change::Link { .. }) => were("there"),
        Some(Change::Trash { path } | Change::Delete { path } | Change::Remove { path }) if one => {
            was(quoted(path), "gone")
        }
        Some(Change::Trash { .. } | Change::Delete { .. } | Change::Remove { .. }) => were("gone"),
        Some(Change::Restore { path }) if one => was(quoted(path), "restored"),
        Some(Change::Restore { .. }) => were("restored"),
        Some(Change::Rename { from, to }) if one => {
            was(quoted(from), &format!("named {}", quoted(to)))
        }
        Some(Change::Rename { .. }) => format!("{count} already had their names"),
        Some(Change::Swap { a, b }) if one => {
            format!("{} and {} were already swapped", quoted(a), quoted(b))
        }
        Some(
            Change::Create {
                words: Some(words), ..
            }
            | Change::Metadata {
                words: Some(words), ..
            },
        ) if one && !words.already.is_empty() => words.already.clone(),
        Some(Change::Create { path, .. }) if one => was(quoted(path), "there"),
        Some(Change::Metadata { path, .. }) if one => was(quoted(path), "as asked"),
        _ if one => "1 was already done".to_owned(),
        _ => were("done"),
    }
}

/// How a plan ended, as `ExecEvent::Finished` says it, for its toast.
#[derive(Default)]
pub(crate) struct End<'a> {
    pub applied: usize,
    pub error: Option<String>,
    pub cancelled: bool,
    pub items: Option<skwir_core::op::Items>,
    /// Deleted where its server keeps a trash (ADR-086): that trash.
    pub kept_in: Option<&'a str>,
    pub said: &'a [String],
    pub moved: u64,
    pub ran: Duration,
    /// Its items done and found done already (`ExecEvent::Finished`).
    pub tally: Option<&'a Tally>,
}

/// What the toast of `running`, ended, says of what was done (`ended`).
fn ended_as(
    running: Running,
    applied: usize,
    error: Option<String>,
    cancelled: bool,
    items: Option<skwir_core::op::Items>,
    kept_in: Option<&str>,
) -> (String, Tone) {
    let (title, run) = (running.title, running.run);
    let (first, named, count) = match items.map(Arc::new).or(running.items) {
        Some(items) if error.is_none() && !cancelled => {
            (Some(items.first.clone()), None, items.count)
        }
        _ => (running.first, running.first_named, applied),
    };
    let done = describe(
        &title,
        run,
        first.as_ref(),
        named.as_deref(),
        count,
        kept_in,
    );
    match (error, cancelled) {
        (Some(error), _) if applied > 0 => (format!("{done}, then stopped: {error}"), Tone::Error),
        (Some(error), _) => (
            failure(
                &title,
                running.failed.as_ref(),
                running.failed_named.as_deref(),
                &error,
            ),
            Tone::Error,
        ),
        (None, true) if applied == 0 => (
            format!(
                "{}: cancelled before anything was done",
                capitalized(&title)
            ),
            Tone::Info,
        ),
        (None, true) => (format!("Cancelled. {done}"), Tone::Info),
        (None, false) => (done, Tone::Success),
    }
}

/// The buttons of the toast saying `plan` ended, in their order: Retry
/// first for one a step's error stopped (`Executor::retry`), then Redo
/// after an undo, else Undo of what it did. Undo names the plan it
/// reports, never the last one asked (a rename beside a long copy): taken
/// back alone when nothing asked after it touched what it did, else its
/// run says why (`Executor::undo_of`).
pub(crate) fn finished_actions(
    plan: PlanId,
    run: RunKind,
    undoes: bool,
    retries: bool,
    redoes: bool,
) -> Vec<Action> {
    let mut actions = Vec::new();
    if retries {
        actions.push(Action {
            label: "Retry".into(),
            command: "retry".into(),
            args: serde_json::json!({ "plan": plan }),
        });
    }
    if run == RunKind::Undo {
        actions.extend(redoes.then(|| Action {
            label: "Redo".into(),
            command: "redo".into(),
            args: serde_json::json!({}),
        }));
    } else if undoes {
        actions.push(Action {
            label: "Undo".into(),
            command: "undo".into(),
            args: serde_json::json!({ "plan": plan }),
        });
    }
    actions
}

/// What `running` made alone, done: the one thing a plan made new (a PDF
/// saved, an archive made, a folder), under the name settled; `None` for
/// any other plan.
pub(crate) fn made_alone(
    running: &Running,
    items: Option<&skwir_core::op::Items>,
) -> Option<PathBuf> {
    if let Some(items) = items {
        return match (&items.first, items.placed.as_slice()) {
            (Change::Create { .. }, [made]) => Some(made.clone()),
            _ => None,
        };
    }
    match running.first() {
        Some(Change::Create { path, .. }) if running.total == 1 => Some(path.clone()),
        _ => None,
    }
}

/// Show: the folder `made` is in, `made` selected there (`navigate`'s
/// `select`), in the pane commands act on; Back returns.
pub(crate) fn show_action(made: &Path) -> Action {
    use skwir_core::os_path::PathArg;
    Action {
        label: "Show".into(),
        command: "navigate".into(),
        args: serde_json::json!({
            "location": PathArg::from(made.parent().unwrap_or(made)),
            "select": made.file_name().map(|name| PathArg::from(Path::new(name))),
        }),
    }
}

/// An action waiting its turn, as its row under the cards of what runs
/// says it (ADR-079, 12): what it is ("Rename to b.txt"), and what it
/// waits for ("Waits for copy to /media/stick").
pub(crate) fn waiting_words(waiting: &skwir_core::op::Waiting) -> (String, String) {
    use skwir_core::op::WaitsFor;
    let why = match &waiting.waits_for {
        WaitsFor::Action(named) => format!("Waits for {named}"),
        WaitsFor::Disk => "Waits for the disk".to_owned(),
        WaitsFor::Server => "Waits for the server".to_owned(),
        WaitsFor::Room => format!(
            "Waits for one of the {} running to end",
            skwir_core::op::ACTIONS_AT_ONCE
        ),
        WaitsFor::Everything => "Waits for what was asked before it".to_owned(),
        WaitsFor::Starting => "Starts".to_owned(),
    };
    (capitalized(&waiting.named), why)
}

/// What a plan copying onto a mount that sends later says while the
/// server takes what it wrote.
pub(crate) fn sending(files: usize) -> String {
    match files {
        0 => "sending to the server".to_owned(),
        n => format!("sending {} to the server", format::count(n, "file")),
    }
}

/// How fast a running plan goes and how long it has left, when that can
/// be told (ADR-113): `23.4 MB/s · about 3 min left`; writing into a
/// mount that sends after, which its end waits for, why no time is said.
pub fn pace(t: &Transfer) -> Option<String> {
    let speed = t.speed.map(format::speed);
    let left = t.left.map(format::time_left);
    let then = t.then_sends.then(|| "then sent to the server".to_owned());
    let said: Vec<String> = [speed, left.or(then)].into_iter().flatten().collect();
    (!said.is_empty()).then(|| said.join(" · "))
}

/// The answer said last, as a card: what it says, its buttons, and Close.
/// Its caller places it: over the files at the last pane's lower right,
/// above Activity's strip, or over the preview on the whole screen. Not
/// while Activity or the History is open, where it would cover them: one
/// said meanwhile keeps its time, which begins once it is drawn
/// (`ToastState::drawn`).
pub fn render_answer(shell: &mut Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    if shell.activity_open() || shell.undo_history_open() {
        return None;
    }
    let toast = shell.toasts.current.clone()?;
    // Held back while Activity or the History was open, its time begins
    // now.
    if let Some((generation, seconds)) = shell.toasts.drawn() {
        shell.time_toast(generation, seconds, cx);
    }
    // An Undo that cannot take its plan back now is said, not offered,
    // with why, as the History says it (`history::undo_held`), rather
    // than refused once pressed.
    let by_plan = toast
        .actions
        .iter()
        .any(|action| action.command == "undo")
        .then(|| shell.history_by_plan());
    let held: Vec<Option<String>> = toast
        .actions
        .iter()
        .map(|action| {
            by_plan
                .as_ref()
                .and_then(|by_plan| crate::history::undo_held(action, by_plan))
        })
        .collect();
    let theme = cx.global::<Theme>().clone();
    let close = shell
        .toasts
        .close
        .get_or_insert_with(|| cx.focus_handle())
        .clone();
    let generation = shell.toasts.generation;
    let a11y = shell.a11y;
    // Said as it appears: a failure at once, the rest after what is being
    // said.
    let error = matches!(toast.tone, Tone::Error);
    let glyph = match toast.tone {
        Tone::Success => controls::glyph("check", theme.success, 15.),
        Tone::Error => controls::glyph("warning", theme.error, 15.),
        Tone::Info => controls::glyph("info", theme.text_muted, 15.),
    };
    let mut card = div()
        .id(("toast-card", generation))
        .max_w(sized(480.))
        .min_w(sized(220.))
        .pl_3()
        .pr_1()
        .py_1()
        .flex()
        .items_center()
        .gap_3()
        .rounded_lg()
        .border_1()
        .border_color(if error { theme.error } else { theme.border })
        .bg(theme.bg)
        .shadow_lg()
        .occlude()
        // A click on the toast leaves the keyboard where it was: the keys
        // must go on acting on the pane. Tab still reaches its buttons.
        .capture_any_mouse_down(|_, window, _| window.prevent_default())
        .role(if error {
            gpui::Role::Alert
        } else {
            gpui::Role::Status
        })
        .aria_live(if error {
            gpui::Live::Assertive
        } else {
            gpui::Live::Polite
        })
        .when(a11y, |d| d.aria_label(toast.text.clone()))
        .child(glyph)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .py_1()
                .flex()
                .flex_col()
                .child(toast.text.clone())
                // Why an Undo is held, under what was said.
                .children(held.iter().flatten().map(|why| {
                    div()
                        .text_body()
                        .text_color(theme.text_muted)
                        .child(why.clone())
                })),
        );
    for ((n, action), held) in toast.actions.into_iter().enumerate().zip(held) {
        while shell.toasts.action_focus.len() <= n {
            shell.toasts.action_focus.push(cx.focus_handle());
        }
        let focus = shell.toasts.action_focus[n].clone();
        let label = action.label.clone();
        if let Some(why) = held {
            // Said, not offered: no frame but the keyboard's.
            let accent = theme.accent;
            card = card.child(
                controls::inert_button(format!("toast-action-{n}"), label, &focus, &theme)
                    .aria_live(gpui::Live::Off)
                    .when(a11y, |d| d.aria_description(why))
                    .border_color(gpui::transparent_black())
                    .focus(move |s| s.border_color(accent)),
            );
            continue;
        }
        card = card.child(
            controls::button(
                format!("toast-action-{n}"),
                label,
                None,
                ButtonStyle::Plain,
                &focus,
                &theme,
            )
            // Its buttons are not part of what is announced.
            .aria_live(gpui::Live::Off)
            .text_color(theme.accent_text)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.dismiss_toast(cx);
                this.run(&action.command, action.args.clone(), window, cx);
            })),
        );
    }
    let card = card.child(
        controls::icon_button("toast-close", "close", "Close", &close, &theme)
            .aria_live(gpui::Live::Off)
            .on_click(cx.listener(|this, _, _, cx| this.dismiss_toast(cx))),
    );
    Some(
        card.with_animation(
            ("toast", generation),
            Animation::new(Duration::from_millis(140)).with_easing(gpui::ease_out_quint()),
            |card, delta| card.opacity(delta).relative().top(px(8. * (1. - delta))),
        )
        .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        Action, End, Running, Toast, ToastState, Tone, bar, describe, ended, failure,
        finished_actions, made_alone, pace, quit_question, show_action, shown_for, waiting_words,
    };
    use skwir_core::op::{Change, PlanId, RunKind, Transfer};
    use std::path::PathBuf;
    use std::time::Duration;

    /// A plan ended says what it did as it was done: one file replaced is
    /// one item, though its steps were two (put in the trash, then copied),
    /// which `Started` could not know before the names taken were settled.
    /// Cancelled before anything was done, it says so.
    #[test]
    fn a_plan_ended_says_what_was_done_as_it_was_done() {
        let running = |items: Option<skwir_core::op::Items>| Running {
            plan: skwir_core::op::Plan::new("copy to /example/it's here").id,
            title: "copy to /example/it's here".into(),
            run: RunKind::Apply,
            total: 1,
            first: Some(Change::Trash {
                path: PathBuf::from("/example/it's here/a.txt"),
            }),
            first_named: None,
            failed: None,
            failed_named: None,
            items: items.map(std::sync::Arc::new),
            paused: false,
            cancelling: false,
            going_on: false,
        };
        let copied = skwir_core::op::Items {
            first: Change::Copy {
                from: PathBuf::from("/example/a.txt"),
                to: PathBuf::from("/example/it's here/a.txt"),
            },
            count: 1,
            placed: vec![PathBuf::from("/example/it's here/a.txt")],
            steps: Vec::new(),
        };
        assert_eq!(
            ended(
                running(None),
                End {
                    applied: 2,
                    items: Some(copied.clone()),
                    ..End::default()
                }
            ),
            ("Copied “a.txt” to it's here".to_owned(), Tone::Success)
        );
        // Said as asked when nothing better comes.
        assert_eq!(
            ended(
                running(Some(copied)),
                End {
                    applied: 2,
                    ..End::default()
                }
            )
            .0,
            "Copied “a.txt” to it's here"
        );
        assert_eq!(
            ended(
                running(None),
                End {
                    cancelled: true,
                    ..End::default()
                }
            ),
            (
                "Copy to /example/it's here: cancelled before anything was done".to_owned(),
                Tone::Info
            )
        );
    }

    /// What a plan found done already it says it found, never that it did
    /// it: alone, by name or by count, as information; after what it did,
    /// what it did said by what it did. An extraction says it in its
    /// step's words; a copy, a trash, a rename and an undo by their kind.
    #[test]
    fn a_plan_says_what_it_found_done_already() {
        use skwir_core::op::{Counted, Plan, Tally, Words};
        fn end(tally: &Tally) -> End<'_> {
            End {
                tally: Some(tally),
                ..End::default()
            }
        }
        let running =
            |title: &str, run: RunKind| Running::for_test(Plan::new(title).id, title, run, 1);
        let counted = |first: Change, count: usize| Counted {
            count,
            first: Some(first),
            named: None,
        };
        let copy = |name: &str| Change::Copy {
            from: PathBuf::from("/example").join(name),
            to: PathBuf::from("/example/it's here").join(name),
        };
        let extracted = Change::Create {
            path: PathBuf::from("/example/Downloads/photos"),
            directory: true,
            words: Some(Words {
                will: "extract “photos.zip” to Downloads".into(),
                did: "extracted “photos.zip” to Downloads".into(),
                already: "“photos.zip” was already extracted to Downloads".into(),
            }),
        };
        let all = |found: Counted| Tally {
            done: Counted::default(),
            already: found,
        };
        let cases = [
            (
                "extract “photos.zip”",
                RunKind::Apply,
                all(counted(extracted.clone(), 1)),
                "“photos.zip” was already extracted to Downloads",
            ),
            (
                "extract 2 archives",
                RunKind::Apply,
                all(counted(extracted, 2)),
                "Extract 2 archives: already done",
            ),
            (
                "copy to /example/it's here",
                RunKind::Apply,
                all(counted(copy("a.txt"), 1)),
                "“a.txt” was already in it's here",
            ),
            (
                "copy to /example/it's here",
                RunKind::Apply,
                all(counted(copy("a.txt"), 3)),
                "3 items were already in it's here",
            ),
            (
                "trash “it's.txt”",
                RunKind::Apply,
                all(counted(
                    Change::Trash {
                        path: PathBuf::from("/example/it's.txt"),
                    },
                    1,
                )),
                "“it's.txt” was already gone",
            ),
            (
                "rename “a.txt”",
                RunKind::Apply,
                all(counted(
                    Change::Rename {
                        from: PathBuf::from("/example/a.txt"),
                        to: PathBuf::from("/example/b.txt"),
                    },
                    1,
                )),
                "“a.txt” was already named “b.txt”",
            ),
            (
                "undo rename “a.txt” to “b.txt”",
                RunKind::Undo,
                all(counted(
                    Change::Rename {
                        from: PathBuf::from("/example/b.txt"),
                        to: PathBuf::from("/example/a.txt"),
                    },
                    1,
                )),
                "Already undone: rename “a.txt” to “b.txt”",
            ),
        ];
        for (title, run, tally, said) in &cases {
            assert_eq!(
                ended(running(title, *run), end(tally)),
                ((*said).to_owned(), Tone::Info),
                "{title}"
            );
        }
        // Some done: said by what was done, then what was found.
        let some = |done: usize, found: usize| Tally {
            done: counted(copy("a.txt"), done),
            already: counted(copy("b.txt"), found),
        };
        let copying = || running("copy to /example/it's here", RunKind::Apply);
        assert_eq!(
            ended(copying(), end(&some(3, 2))),
            (
                "Copied 3 items to it's here; 2 were already there".to_owned(),
                Tone::Success
            )
        );
        assert_eq!(
            ended(copying(), end(&some(1, 1))).0,
            "Copied “a.txt” to it's here; “b.txt” was already there"
        );
        // A long one says what it moved after what it did.
        let (moved, ran) = (3_000_000, Duration::from_secs(6));
        assert_eq!(
            ended(
                copying(),
                End {
                    moved,
                    ran,
                    ..end(&some(3, 2))
                }
            )
            .0,
            format!(
                "Copied 3 items to it's here ({} in {}); 2 were already there",
                crate::format::size(moved),
                crate::format::took(ran)
            )
        );
        // What the steps said follows.
        let said = ["Fold each sheet in half".to_owned()];
        assert_eq!(
            ended(
                copying(),
                End {
                    said: &said,
                    ..end(&all(counted(copy("a.txt"), 1)))
                }
            ),
            (
                "“a.txt” was already in it's here. Fold each sheet in half".to_owned(),
                Tone::Info
            )
        );
        // Stopped by an error, it says that, as before.
        let failed = ended(
            copying(),
            End {
                error: Some("disk full".into()),
                ..end(&some(1, 1))
            },
        );
        assert_eq!(failed.1, Tone::Error);
        assert!(!failed.0.contains("already"), "{}", failed.0);
    }

    /// What the steps said follows what was done, in the plan's own toast:
    /// nothing covers it, nor does it cover the Undo. Said alone when the
    /// steps did nothing else.
    #[test]
    fn what_the_steps_said_follows_what_was_done() {
        let running = || Running {
            plan: skwir_core::op::Plan::new("save “it's.md” as booklet").id,
            title: "save “it's.md” as booklet".into(),
            run: RunKind::Apply,
            total: 1,
            first: Some(Change::Create {
                path: PathBuf::from("/example/it's (booklet).pdf"),
                directory: false,
                words: Some(skwir_core::op::Words {
                    will: "save “it's.md” as “it's (booklet).pdf”".into(),
                    did: "saved “it's.md” as “it's (booklet).pdf”".into(),
                    already: "“it's (booklet).pdf” was already saved".into(),
                }),
            }),
            first_named: None,
            failed: None,
            failed_named: None,
            items: None,
            paused: false,
            cancelling: false,
            going_on: false,
        };
        assert_eq!(
            ended(
                running(),
                End {
                    applied: 1,
                    said: &["Fold each sheet in half twice".to_owned()],
                    ..End::default()
                }
            ),
            (
                "Saved “it's.md” as “it's (booklet).pdf”. Fold each sheet in half twice".to_owned(),
                Tone::Success
            )
        );
        assert_eq!(
            ended(
                running(),
                End {
                    said: &["“it's.md” is as small as it gets".to_owned()],
                    ..End::default()
                }
            ),
            ("“it's.md” is as small as it gets".to_owned(), Tone::Info)
        );
    }

    /// A toast stays by its weight, and a long one as long as it takes to
    /// read.
    #[test]
    fn a_long_toast_stays_until_read() {
        let toast = |text: &str, tone, actions: Vec<Action>| Toast {
            text: text.to_owned(),
            tone,
            actions,
        };
        let undo = || {
            vec![Action {
                label: "Undo".into(),
                command: "undo".into(),
                args: serde_json::json!({}),
            }]
        };
        assert_eq!(
            shown_for(&toast("Renamed “a”", Tone::Success, Vec::new())),
            4
        );
        assert_eq!(shown_for(&toast("Renamed “a”", Tone::Success, undo())), 8);
        assert_eq!(shown_for(&toast("It failed", Tone::Error, Vec::new())), 10);
        let folding = "Saved “it's.md” as “it's (booklet).pdf”. Fold each sheet in half three times, page 1 outside and upright, cut its closed folds open, and put the sheets one after another";
        assert_eq!(shown_for(&toast(folding, Tone::Success, undo())), 10);
    }

    /// A toast's Undo takes back the plan it reports, by its id, as the
    /// core's `undo` takes it; Retry names it too, and Redo follows an undo.
    #[test]
    fn a_toasts_buttons_name_the_plan_it_reports() {
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let plan = skwir_core::op::Plan::new("rename “it's”").id;
        let actions = finished_actions(plan, RunKind::Apply, true, true, false);
        let named: Vec<(&str, &str)> = actions
            .iter()
            .map(|a| (a.label.as_str(), a.command.as_str()))
            .collect();
        assert_eq!(named, [("Retry", "retry"), ("Undo", "undo")]);
        for action in &actions {
            assert_eq!(action.args["plan"], serde_json::json!(plan));
            let asked =
                skwir_core::command::Invocation::with_args(action.command.as_str(), &action.args);
            app.commands.check(&asked).expect("the command takes it");
        }
        // Nothing done: no Undo. After an undo: Redo, of the last undone.
        assert!(finished_actions(plan, RunKind::Apply, false, false, false).is_empty());
        let redo = finished_actions(plan, RunKind::Undo, true, false, true);
        assert_eq!(redo.len(), 1);
        assert_eq!(redo[0].command, "redo");
    }

    #[test]
    fn what_waits_says_what_for() {
        use skwir_core::op::{Waiting, WaitsFor};
        let words = |named: &str, waits_for| {
            let (what, why) = waiting_words(&Waiting {
                asked: 7,
                named: named.into(),
                plan: None,
                waits_for,
                steps: Some(1),
                heavy: false,
                counts: false,
            });
            format!("{what} / {why}")
        };
        assert_eq!(
            words(
                "rename to b.txt",
                WaitsFor::Action("copy to /media/stick".into())
            ),
            "Rename to b.txt / Waits for copy to /media/stick"
        );
        assert_eq!(
            words("copy to /media/it's here", WaitsFor::Disk),
            "Copy to /media/it's here / Waits for the disk"
        );
        assert_eq!(
            words("copy to /mnt/server", WaitsFor::Server),
            "Copy to /mnt/server / Waits for the server"
        );
        assert_eq!(
            words("undo copy to /tmp", WaitsFor::Room),
            "Undo copy to /tmp / Waits for one of the 4 running to end"
        );
        assert_eq!(
            words("tag", WaitsFor::Everything),
            "Tag / Waits for what was asked before it"
        );
    }

    #[test]
    fn quitting_while_a_plan_runs_says_what_finishes_and_what_stops() {
        assert_eq!(
            quit_question(Some(("copy to /media/stick", true)), 0, 2),
            "Copy to /media/stick is paused. Skwir can finish it, and the 2 actions waiting \
             after it, without its window, then quit. Or stop now: what it finished stays, \
             what it was in the middle of is taken back."
        );
        assert_eq!(
            quit_question(Some(("move to Trash", false)), 0, 0),
            "Move to Trash is still running. Skwir can finish it without its window, then \
             quit. Or stop now: what it finished stays, what it was in the middle of is \
             taken back."
        );
        assert_eq!(
            quit_question(Some(("copy to /media/stick", false)), 2, 1),
            "Copy to /media/stick is still running, and 2 other actions beside it. Skwir can \
             finish them, and the 1 action waiting after them, without its window, then \
             quit. Or stop now: what they finished stays, what they were in the middle of \
             is taken back."
        );
        assert_eq!(
            quit_question(None, 0, 0),
            "A change is about to begin. Skwir can finish it without its window, then quit. \
             Or stop now: what it finished stays, what it was in the middle of is taken back."
        );
    }

    #[test]
    fn a_bar_counts_items_and_moves_with_the_bytes_of_the_one_under_way() {
        // Many small files: the count, never a byte total that grows as
        // fast as the bytes do.
        let small = Transfer {
            done: 82_304,
            total: 82_368,
            ended: 1286,
            finished: 1286,
            step_done: 0,
            step_total: 64,
            ..Transfer::default()
        };
        let (fraction, words) = bar(40_000, &small);
        assert_eq!(words, "1,286 of 40,000 items · 80.4 KB");
        assert!((fraction - 1286. / 40_000.).abs() < 1e-6);
        // Three large folders: halfway through the second is half of a third.
        let large = Transfer {
            done: 3 << 30,
            total: 4 << 30,
            ended: 1,
            finished: 1,
            step_done: 1 << 30,
            step_total: 2 << 30,
            ..Transfer::default()
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
        // Sized before it began (ADR-113): its bytes in all move it, its
        // items counted beside.
        let sized = Transfer {
            planned: Some(8 << 30),
            ..large
        };
        assert_eq!(
            bar(3, &sized),
            (0.375, "1 of 3 items · 3.0 GB of 8.0 GB".into())
        );
        // Items a batch finished before their steps place them count as
        // they come whole; the bar still by the bytes.
        let batched = Transfer {
            ended: 0,
            finished: 2,
            ..sized
        };
        assert_eq!(
            bar(3, &batched),
            (0.375, "2 of 3 items · 3.0 GB of 8.0 GB".into())
        );
        // One item sized before it began: what it was sized at.
        let one_sized = Transfer {
            planned: Some(4 << 30),
            total: 0,
            ..one
        };
        assert_eq!(bar(1, &one_sized), (0.25, "1.0 GB of 4.0 GB".into()));
    }

    /// How fast and how long, when that can be told: nothing before, a
    /// speed alone, both; writing into a mount that sends after, why no
    /// time is said.
    #[test]
    fn a_card_says_its_speed_and_time_left_when_it_can() {
        assert_eq!(pace(&Transfer::default()), None);
        let speed = Transfer {
            speed: Some(24.0 * 1024.0 * 1024.0),
            ..Transfer::default()
        };
        assert_eq!(pace(&speed).as_deref(), Some("24.0 MB/s"));
        let both = Transfer {
            left: Some(Duration::from_secs(170)),
            ..speed
        };
        assert_eq!(pace(&both).as_deref(), Some("24.0 MB/s · about 3 min left"));
        let then = Transfer {
            then_sends: true,
            ..speed
        };
        assert_eq!(
            pace(&then).as_deref(),
            Some("24.0 MB/s · then sent to the server")
        );
        // A plan of no bytes: its time left alone.
        let steps = Transfer {
            left: Some(Duration::from_secs(40)),
            ..Transfer::default()
        };
        assert_eq!(pace(&steps).as_deref(), Some("about 40 s left"));
    }

    /// A long copy says what it moved, in how long; a short one, or one
    /// that failed, does not.
    #[test]
    fn a_long_copy_says_what_it_moved_in_how_long() {
        let running = || Running {
            plan: skwir_core::op::Plan::new("copy to /example/it's here").id,
            title: "copy to /example/it's here".into(),
            run: RunKind::Apply,
            total: 1,
            first: Some(Change::Copy {
                from: PathBuf::from("/example/it's big.iso"),
                to: PathBuf::from("/example/it's here/it's big.iso"),
            }),
            first_named: None,
            failed: None,
            failed_named: None,
            items: None,
            paused: false,
            cancelling: false,
            going_on: false,
        };
        let long = End {
            applied: 1,
            moved: 5 << 30,
            ran: Duration::from_secs(200),
            ..End::default()
        };
        assert_eq!(
            ended(running(), long).0,
            "Copied “it's big.iso” to it's here (5.0 GB in 3 min 20 s)"
        );
        let short = End {
            applied: 1,
            moved: 5 << 20,
            ran: Duration::from_secs(2),
            ..End::default()
        };
        assert_eq!(
            ended(running(), short).0,
            "Copied “it's big.iso” to it's here"
        );
    }

    #[test]
    fn a_failure_says_what_could_not_be_done_and_why() {
        let copy = Change::Copy {
            from: PathBuf::from("/a/notes/it's a draft.txt"),
            to: PathBuf::from("/a/locked/it's a draft.txt"),
        };
        assert_eq!(
            failure("copy to /a/locked", Some(&copy), None, "permission denied"),
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
                None,
                "“a.txt” could not be renamed “b.txt”: the name is too long, or not allowed there"
            ),
            "Couldn't rename “a.txt” to “b.txt”: the name is too long, or not allowed there",
            "the reason alone, not the step twice"
        );
        let link = Change::Link {
            from: PathBuf::from("/a/notes/it's a draft.txt"),
            to: PathBuf::from("/a/locked/it's a draft.txt"),
        };
        assert_eq!(
            failure(
                "link in /a/locked",
                Some(&link),
                None,
                "“it's a draft.txt” could not be linked in “locked”: permission denied"
            ),
            "Couldn't make a link to “it's a draft.txt” in locked: permission denied"
        );
        assert_eq!(
            failure("tag 2 files", None, None, "the drive is full"),
            "Tag 2 files: the drive is full"
        );
        // An item of a trash, by the name it had: the error names the
        // trash's, and its reason alone is said.
        let trashed = Change::Delete {
            path: PathBuf::from("/a/.Trash-1000/files/it's a draft.txt.2"),
        };
        assert_eq!(
            failure(
                "delete permanently",
                Some(&trashed),
                Some("it's a draft.txt"),
                "“it's a draft.txt.2” could not all be deleted: permission denied"
            ),
            "Couldn't delete “it's a draft.txt”: permission denied"
        );
    }

    #[test]
    fn results_name_one_item_and_count_several() {
        let trash = Change::Trash {
            path: PathBuf::from("/example/it's a draft.txt"),
        };
        assert_eq!(
            describe("move to trash", RunKind::Apply, Some(&trash), None, 1, None),
            "Moved “it's a draft.txt” to Trash"
        );
        assert_eq!(
            describe("move to trash", RunKind::Apply, Some(&trash), None, 3, None),
            "Moved 3 items to Trash"
        );
        let copy = Change::Copy {
            from: PathBuf::from("/example/a.txt"),
            to: PathBuf::from("/example/Archive 2026/a.txt"),
        };
        assert_eq!(
            describe("copy", RunKind::Apply, Some(&copy), None, 2, None),
            "Copied 2 items to Archive 2026"
        );
        let link = Change::Link {
            from: PathBuf::from("/example/a.txt"),
            to: PathBuf::from("/example/Archive 2026/a.txt"),
        };
        assert_eq!(
            describe("link", RunKind::Apply, Some(&link), None, 1, None),
            "Made a link to “a.txt” in Archive 2026"
        );
        assert_eq!(
            describe("link", RunKind::Apply, Some(&link), None, 3, None),
            "Made 3 links in Archive 2026"
        );
        assert_eq!(
            describe(
                "undo move to trash",
                RunKind::Undo,
                Some(&trash),
                None,
                1,
                None
            ),
            "Undid: move to trash"
        );
        assert_eq!(
            describe("add tag", RunKind::Apply, None, None, 2, None),
            "Add tag (2 items)"
        );
        assert_eq!(
            describe("paste", RunKind::Apply, None, None, 0, None),
            "Paste: nothing to do"
        );
        // An item of a trash, by the name the step said it had there; a
        // move out of it, by the name it arrived under.
        let trashed = Change::Delete {
            path: PathBuf::from("/example/Trash/files/it's a draft.txt.2"),
        };
        assert_eq!(
            describe(
                "delete permanently",
                RunKind::Apply,
                Some(&trashed),
                Some("it's a draft.txt"),
                1,
                None
            ),
            "Deleted “it's a draft.txt” permanently"
        );
        // Kept by its server's own trash (a cloud's): never said gone.
        let on_drive = Change::Delete {
            path: PathBuf::from("/example/remote/it's a photo.jpg"),
        };
        assert_eq!(
            describe(
                "delete permanently",
                RunKind::Apply,
                Some(&on_drive),
                None,
                1,
                Some("Google Drive's trash")
            ),
            "Deleted “it's a photo.jpg” (in Google Drive's trash)"
        );
        let out = Change::Move {
            from: PathBuf::from("/example/Trash/files/it's a draft.txt.2"),
            to: PathBuf::from("/example/Archive 2026/it's a draft.txt"),
        };
        assert_eq!(
            describe(
                "move",
                RunKind::Apply,
                Some(&out),
                Some("it's a draft.txt"),
                1,
                None
            ),
            "Moved “it's a draft.txt” to Archive 2026"
        );
    }

    /// A toast held back while Activity or the History is open keeps its
    /// time: it begins as the toast is first drawn, once; one said with
    /// nothing holding it back begins at once. A time over goes only with
    /// the toast it was for.
    #[test]
    fn a_toast_held_back_keeps_its_time_until_drawn() {
        let said = |text: &str| Toast {
            text: text.into(),
            tone: Tone::Error,
            actions: Vec::new(),
        };
        let mut toasts = ToastState::default();
        let (held, timed) = toasts.say(said("Could not undo “it's a.txt”"), true);
        assert_eq!(timed, None);
        let seconds = shown_for(&said("Could not undo “it's a.txt”"));
        assert_eq!(toasts.drawn(), Some((held, seconds)));
        assert_eq!(toasts.drawn(), None, "its time begins once");
        let (shown, timed) = toasts.say(said("Moved “b.txt”"), false);
        assert_eq!(timed, Some(shown_for(&said("Moved “b.txt”"))));
        assert_eq!(toasts.drawn(), None, "its time runs already");
        assert!(
            !toasts.over(held),
            "the time of one replaced is not this one's"
        );
        assert!(toasts.current.is_some());
        assert!(toasts.over(shown));
        assert!(toasts.current.is_none());
        assert_eq!(toasts.drawn(), None, "nothing to time");
    }

    /// The one thing a plan made new is shown with Show: the folder it is
    /// in, it selected there; a plan of several steps, or of a copy, is
    /// not.
    #[test]
    fn what_a_plan_made_alone_is_shown() {
        let made = PathBuf::from("/example/l'été/ma liste.pdf");
        let create = Change::Create {
            path: made.clone(),
            directory: false,
            words: None,
        };
        let progress = |change: Change, total: usize| skwir_core::op::Progress {
            plan: PlanId(7),
            step: 0,
            total,
            change,
            named: None,
            status: skwir_core::op::StepStatus::Pending { reversible: true },
            error: None,
        };
        let mut running = Running::for_test(PlanId(7), "save as PDF", RunKind::Apply, 1);
        running.stepped(&progress(create.clone(), 1));
        assert_eq!(made_alone(&running, None), Some(made.clone()));
        let action = show_action(&made);
        assert_eq!(
            (action.label.as_str(), action.command.as_str()),
            ("Show", "navigate")
        );
        assert_eq!(action.args["location"], "/example/l'été");
        assert_eq!(action.args["select"], "ma liste.pdf");
        let mut several = Running::for_test(PlanId(8), "save as PDFs", RunKind::Apply, 2);
        several.stepped(&progress(create, 2));
        assert_eq!(made_alone(&several, None), None);
        let mut copied = Running::for_test(PlanId(9), "copy", RunKind::Apply, 1);
        copied.stepped(&progress(
            Change::Copy {
                from: PathBuf::from("/example/a.txt"),
                to: PathBuf::from("/example/b/a.txt"),
            },
            1,
        ));
        assert_eq!(made_alone(&copied, None), None);
    }
}
