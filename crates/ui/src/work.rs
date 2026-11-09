//! What runs and what was said of it (ADR-114, ADR-115), kept apart from
//! the window that shows it, so that it outlives the window.
//!
//! While a window is up, the shell holds it: the executor's events taken
//! in (`Work::take`), the strip, Activity's cards and `Earlier` read from
//! it, the desktop told. When the last window closes while something is
//! left, the program keeps it and goes on taking the events without a
//! window (`finishing.rs`); a window opened again takes it back, `Earlier`
//! and the stopped strip with it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use skwir_core::App;
use skwir_core::op::{Change, ExecEvent, PlanId, RunKind};
use skwir_core::workspace::WindowId;
use skwir_platform::desktop::{Asked, Desktop, Kind, Notice, Presence, Progress, State};

use crate::activity::{Ended, Stopped, Under, summary};
use crate::format;
use crate::toast::{self, Action, Running, Toast, Tone, capitalized};

/// From how long an action ran the desktop is told that it ended while a
/// window is up: the same 5 s from which its answer says what it moved in
/// how long (ADR-113). Without a window, every end is told.
const LONG: Duration = Duration::from_secs(5);

/// The key of the notice saying that something waits for an answer.
const ASKS: u64 = u64::MAX;

/// What runs and what was said of it.
#[derive(Default)]
pub(crate) struct Work {
    /// Plans started and not finished, oldest first. Events name their
    /// plan, and each is matched to its own: a second plan starting never
    /// takes over the first one's bar or result.
    pub(crate) running: Vec<Running>,
    /// The runs about to start that go on by themselves (`ExecEvent::GoesOn`).
    going_on: HashSet<PlanId>,
    /// The answers of this session, newest first (`EARLIER` at most).
    pub(crate) earlier: VecDeque<Ended>,
    /// The last action an error stopped, said in the strip until seen.
    pub(crate) stopped: Option<Stopped>,
    /// The plans shown as a card: one an error stops stays in the strip.
    pub(crate) carded: HashSet<PlanId>,
    /// The runs told started that count as an action
    /// (`ExecEvent::Started::counts`, the core's one test, as what waits
    /// is counted by it): those get a card, and count in the spell.
    counting: HashSet<PlanId>,
    /// What of the spell of work ended (ADR-114, amended 2026-11-03).
    spell: Spell,
    /// The plans a server stopped, which go on by themselves once it
    /// answers again (ADR-080, 9).
    servers: HashSet<PlanId>,
    /// What ended while no one looked, since someone last did.
    away: Away,
    /// The progress the taskbar's entry was last told.
    told: Option<Progress>,
    /// Whether the machine is kept awake.
    awake: bool,
    /// What the session's end is held off for, while it is: told again as
    /// it changes (a plan's card comes after its start).
    held: Option<String>,
    /// Whether the desktop was told that something waits for an answer.
    asks_told: bool,
    /// What tells the desktop: `None` in tests.
    desktop: Option<Desktop>,
    /// What the desktop asked (a notice or the presence clicked, its
    /// menu), kept for the next sync.
    asked: Arc<Mutex<Vec<Asked>>>,
    /// When each action under way joined, waiting or started, as first
    /// seen (`Work::watch`): one still under way `SHOWN_AFTER` later is
    /// shown, and announced once.
    joined: HashMap<PlanId, Instant>,
    /// Those announced, while under way.
    announced: HashSet<PlanId>,
    /// The window each action under way was asked in, as first seen while
    /// windows are up (`Askers`): Activity unfolds there once it lasted
    /// (ADR-114, amended 2026-11-07).
    asked_in: HashMap<PlanId, WindowId>,
    /// What runs that is no plan (a task: a print), by id, when it was
    /// first seen and in the window in front then, as nothing says who
    /// asked for one: it unfolds Activity there as a plan does.
    tasks_asked_in: HashMap<u64, (Instant, WindowId)>,
    /// After how long they unfold it (`ui.activity_unfold_ms`), as the
    /// last `watch` was told; `None` without a window.
    unfold_after: Option<Duration>,
    /// The light actions waiting since `SHOWN_AFTER` or more: counted
    /// among what waits, as they will be once they start.
    late_waiting: usize,
    /// What the strip announces, until when (ADR-114, amended 2026-11-03).
    pub(crate) announce: Option<Announce>,
    /// When the program is to be woken for the strip's next change (an
    /// action to show, a tint to end): one timer at a time, which clears it
    /// as it fires, whether a window is up then or not (`wake_after`).
    pub(crate) wake_at: Option<Instant>,
}

/// Wake the program at `at` for what `watch` said changes then, unless a
/// wake at or before it is set already: one timer, never a loop. The timer
/// holds `Work` itself, not the window: it clears its time as it fires
/// even when the window that set it has closed, and wakes the core's
/// listener (`App::wake`), whose sync, the window's or the windowless
/// one, asks `watch` again.
pub(crate) fn wake_after(
    work: &std::rc::Rc<std::cell::RefCell<Work>>,
    app: &Arc<App>,
    at: Instant,
    cx: &mut gpui::App,
) {
    if !work.borrow_mut().wake_for(at) {
        return;
    }
    let (work, app) = (std::rc::Rc::clone(work), Arc::clone(app));
    let wait = at.saturating_duration_since(Instant::now());
    cx.spawn(async move |cx| {
        cx.background_executor().timer(wait).await;
        work.borrow_mut().woken_at(at);
        app.wake();
    })
    .detach();
}

impl Work {
    /// Whether a timer is to be set for `at`, noting it: not when one at
    /// or before it is set already.
    fn wake_for(&mut self, at: Instant) -> bool {
        if self.wake_at.is_some_and(|set| set <= at) {
            return false;
        }
        self.wake_at = Some(at);
        true
    }

    /// The timer set for `at` fired: a later one may be set again.
    fn woken_at(&mut self, at: Instant) {
        if self.wake_at == Some(at) {
            self.wake_at = None;
        }
    }
}

/// After how long an action still under way is shown and announced, if
/// not before (`Started::counts`): an instant rename never flashes.
pub(crate) const SHOWN_AFTER: Duration = Duration::from_millis(300);

/// How long the strip announces an action that joined, on its tint.
pub(crate) const ANNOUNCED_FOR: Duration = Duration::from_secs(2);

/// Who asked what joins, while windows are up, and after how long Activity
/// unfolds there (`ui.activity_unfold_ms`): the window that ran its
/// command (`finishing::ran`), else the one in front, where an undo, a
/// retry or a script's action was asked from.
#[derive(Clone, Copy)]
pub(crate) struct Askers<'a> {
    pub ran: &'a HashMap<PlanId, WindowId>,
    pub front: WindowId,
    pub unfold_after: Duration,
}

/// An action the strip announces: its own sentence on a tint, said once to
/// a screen reader, until `until`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Announce {
    pub plan: PlanId,
    pub until: Instant,
}

/// What ended of the spell of work the strip counts (ADR-114, amended
/// 2026-11-03): from an action asked with nothing under way until nothing
/// runs, waits or goes on by itself (`Work::settle`). Each action that
/// ended counts whole, so the bar never goes back as one ends.
#[derive(Default)]
struct Spell {
    /// The runs that ended. One a retry continues, asked or going on by
    /// itself, leaves as its continuation is asked: an action that goes on
    /// is the same action (ADR-080, 9), counted once, by what it has left.
    plans: HashSet<PlanId>,
    /// The tasks seen running at the last settle, each with whether it was
    /// asked to stop before it went anywhere: that one ends having done
    /// nothing, and leaves the count.
    tasks: HashMap<u64, bool>,
    /// The tasks that ended.
    tasks_ended: usize,
}

impl Spell {
    fn ended(&self) -> usize {
        self.plans.len() + self.tasks_ended
    }

    /// The tasks seen at the last settle that are gone from `now`, each
    /// counted as it ended: whole, unless stopped before it went anywhere.
    fn tasks_gone(&self, now: &HashSet<u64>) -> usize {
        self.tasks
            .iter()
            .filter(|&(id, quit)| !*quit && !now.contains(id))
            .count()
    }
}

/// What ended unwatched: one notice says them all.
#[derive(Clone, Copy, Debug, Default)]
struct Away {
    finished: usize,
    stopped: usize,
}

/// Who sees what runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Seen {
    /// A window is up and has the focus: what it shows is not told.
    LookedAt,
    /// A window is up, another has the focus: what ran long is told.
    Away,
    /// No window is up: every end is told, however short.
    NoWindow,
}

/// How the desktop is told (ADR-114, ADR-115).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Watched {
    pub seen: Seen,
    /// `ui.notify_desktop`.
    pub notify: bool,
    /// `ui.keep_awake`.
    pub keep_awake: bool,
}

impl Watched {
    /// As the program finishing without a window is watched: by no one.
    pub(crate) fn windowless(app: &App) -> Watched {
        let switch = |key: &str| app.settings.get::<bool>(key).unwrap_or(true);
        Watched {
            seen: Seen::NoWindow,
            notify: switch(crate::settings::NOTIFY_DESKTOP_KEY),
            keep_awake: switch(crate::settings::KEEP_AWAKE_KEY),
        }
    }
}

/// What the window does with an event taken in.
pub(crate) enum Taken {
    Nothing,
    /// A step was made: the window shows what it made (`Shell::made_step`).
    Step(skwir_core::op::Progress),
    /// A plan its server stopped waits for it to answer again.
    WaitsForServer(PlanId),
    /// A plan ended: whether it went through and what it made, for the
    /// window to show (`Shell::made`), and its answer, when it was one
    /// this knew start.
    Ended {
        plan: PlanId,
        done: bool,
        items: Option<skwir_core::op::Items>,
        answer: Option<Toast>,
    },
}

/// What is said, after the plan's answer, of one its server stopped.
pub(crate) const WAITS_FOR_SERVER: &str = ". It goes on by itself once the server answers again";

impl Work {
    /// Work told to the desktop: a click on what it shows is kept for the
    /// next sync, and wakes the program.
    pub(crate) fn new(app: &Arc<App>) -> Work {
        let asked: Arc<Mutex<Vec<Asked>>> = Arc::default();
        let (wake, kept) = (app.waker_handle(), Arc::clone(&asked));
        let desktop = Desktop::new(
            skwir_core::APP_ID,
            "skwir",
            crate::icons::APP_ICON,
            move |asked| {
                kept.lock().push(asked);
                wake();
            },
        );
        Work {
            desktop: Some(desktop),
            asked,
            ..Work::default()
        }
    }

    /// The window shown, whose taskbar button shows the progress (Windows),
    /// none while there is none.
    #[cfg(windows)]
    pub(crate) fn shown_in(&self, window: Option<&gpui::Window>) {
        let Some(desktop) = &self.desktop else {
            return;
        };
        let hwnd = window
            .and_then(|window| raw_window_handle::HasWindowHandle::window_handle(window).ok())
            .and_then(|handle| match handle.as_raw() {
                raw_window_handle::RawWindowHandle::Win32(win32) => Some(win32.hwnd.get()),
                _ => None,
            });
        desktop.set_window(hwnd.unwrap_or(0));
    }

    #[cfg(not(windows))]
    pub(crate) fn shown_in(&self, window: Option<&gpui::Window>) {
        let _ = window;
    }

    /// What the desktop asked since the last call, oldest first.
    pub(crate) fn take_asked(&self) -> Vec<Asked> {
        std::mem::take(&mut *self.asked.lock())
    }

    /// What runs and is shown as a card: the runs that count as an action
    /// (a single rename ends before anyone could read a bar), and the
    /// tasks; beside them, what the spell of work counts that ended and
    /// what waits. Asked once a frame and once a sync: what waits is a
    /// count the executor keeps cheap (`Executor::waiting_count`), never a
    /// look at what each waits for.
    pub(crate) fn under_way(&self, app: &App) -> Under {
        let mut ended = self.spell.ended();
        let mut plans = Vec::new();
        for running in self
            .running
            .iter()
            .filter(|r| self.counting.contains(&r.plan))
        {
            match app.executor.transfer(running.plan) {
                Some(transfer) => plans.push((running.clone(), transfer)),
                // Ended in the core, its `Finished` not taken in yet: as it
                // will be counted, so that the bar never dips meanwhile.
                None if !running.cancelling => ended += 1,
                None => {}
            }
        }
        let tasks = app.tasks.running();
        let now: HashSet<u64> = tasks.iter().map(|task| task.id).collect();
        ended += self.spell.tasks_gone(&now);
        let waiting = app.executor.waiting_count() + self.late_waiting;
        // What waits first is read only while nothing runs, the strip
        // then saying what it waits for (never silent).
        let next = (plans.is_empty() && tasks.is_empty() && waiting > 0)
            .then(|| app.executor.waiting().first().map(toast::waiting_words))
            .flatten();
        Under {
            plans,
            tasks,
            ended,
            waiting,
            next,
        }
    }

    /// Follow what joined (ADR-114, amended 2026-11-03): each action
    /// waiting or started is noted as first seen; one still under way
    /// `SHOWN_AFTER` after it joined is shown (counted, as one of several
    /// steps is at once) and announced once, the latest of them on the
    /// strip's tint for `ANNOUNCED_FOR`. Each is noted with the window that
    /// asked it, while windows are up (`askers`): it unfolds Activity there
    /// once it lasted their `unfold_after` (`unfolds_in`). What is no longer
    /// under way is let go of. Returns when the strip or Activity changes
    /// next by itself, to wake the window then. `now` is the caller's, a
    /// test's own.
    pub(crate) fn watch(
        &mut self,
        app: &App,
        now: Instant,
        askers: Option<Askers<'_>>,
    ) -> Option<Instant> {
        let running: HashSet<PlanId> = self.running.iter().map(|r| r.plan).collect();
        // Not yet told started: the plans waiting, as ids alone.
        let waits: HashMap<PlanId, bool> = app
            .executor
            .waiting_plans()
            .into_iter()
            .filter(|(plan, _)| !running.contains(plan))
            .collect();
        for plan in running.iter().chain(waits.keys()) {
            self.joined.entry(*plan).or_insert(now);
            if let Some(askers) = askers {
                self.asked_in
                    .entry(*plan)
                    .or_insert_with(|| askers.ran.get(plan).copied().unwrap_or(askers.front));
            }
        }
        let under = |plan: &PlanId| running.contains(plan) || waits.contains_key(plan);
        self.joined.retain(|plan, _| under(plan));
        self.announced.retain(|plan| under(plan));
        self.asked_in.retain(|plan, _| under(plan));
        let tasks: HashSet<u64> = app.tasks.running().iter().map(|task| task.id).collect();
        self.tasks_asked_in.retain(|task, _| tasks.contains(task));
        if let Some(askers) = askers {
            for task in tasks {
                self.tasks_asked_in
                    .entry(task)
                    .or_insert((now, askers.front));
            }
        }
        self.unfold_after = askers.map(|askers| askers.unfold_after);
        let mut next: Option<Instant> = None;
        let mut sooner = |at: Instant| next = Some(next.map_or(at, |next| next.min(at)));
        if let Some(after) = self.unfold_after {
            for &(at, _) in self.tasks_asked_in.values() {
                if now < at + after {
                    sooner(at + after);
                }
            }
        }
        let mut newest: Option<(Instant, PlanId)> = None;
        self.late_waiting = 0;
        for (&plan, &at) in &self.joined {
            // Activity unfolds then where it was asked, once.
            if let Some(after) = self.unfold_after
                && self.asked_in.contains_key(&plan)
                && now < at + after
            {
                sooner(at + after);
            }
            let due = at + SHOWN_AFTER;
            if now < due {
                sooner(due);
                continue;
            }
            if running.contains(&plan) {
                self.counting.insert(plan);
            } else if waits.get(&plan) == Some(&false) {
                self.late_waiting += 1;
            }
            if self.announced.insert(plan) && newest.is_none_or(|(when, _)| at > when) {
                newest = Some((at, plan));
            }
        }
        if let Some((_, plan)) = newest {
            self.announce = Some(Announce {
                plan,
                until: now + ANNOUNCED_FOR,
            });
        }
        match self.announce {
            Some(announce) if now < announce.until && under(&announce.plan) => {
                sooner(announce.until);
            }
            _ => self.announce = None,
        }
        next
    }

    /// Whether an action asked in `window` has been under way, waiting or
    /// running, for `unfold_after` or more by `now`: Activity unfolds there
    /// (`Shell::sync_unfolded`). Never without a window.
    pub(crate) fn unfolds_in(&self, window: WindowId, now: Instant) -> bool {
        let Some(after) = self.unfold_after else {
            return false;
        };
        self.asked_in
            .iter()
            .filter(|&(_, asker)| *asker == window)
            .any(|(plan, _)| self.joined.get(plan).is_some_and(|&at| now >= at + after))
            || self
                .tasks_asked_in
                .values()
                .any(|&(at, asker)| asker == window && now >= at + after)
    }

    /// The actions of this spell of work that ended (`Spell`).
    #[cfg(test)]
    pub(crate) fn spell_ended(&self) -> usize {
        self.spell.ended()
    }

    /// After each sync's events: the tasks that ended since the last are
    /// counted, and the spell of work ends once nothing runs, waits or
    /// goes on by itself, its plans' ends taken in: what the strip counts
    /// begins again. The executor wakes the window once it is idle, and a
    /// task's end wakes it too.
    pub(crate) fn settle(&mut self, app: &App) {
        let tasks = app.tasks.running();
        let now: HashSet<u64> = tasks.iter().map(|task| task.id).collect();
        self.spell.tasks_ended += self.spell.tasks_gone(&now);
        self.spell.tasks = tasks
            .iter()
            .map(|task| (task.id, task.cancelling && task.went.unwrap_or(0.) <= 0.))
            .collect();
        if self.spell.ended() > 0
            && self.running.is_empty()
            && tasks.is_empty()
            && !app.executor.busy_or_waiting()
        {
            self.spell.plans.clear();
            self.spell.tasks_ended = 0;
        }
    }

    /// Take in one of the executor's events: what runs and what was said
    /// follow it, the desktop is told as `watched` says, and what the
    /// window has to do with it is returned.
    pub(crate) fn take(&mut self, app: &App, event: ExecEvent, watched: Watched) -> Taken {
        match event {
            ExecEvent::Started {
                plan,
                title,
                total,
                run,
                items,
                counts,
            } => {
                let going_on = self.going_on.remove(&plan);
                if counts {
                    self.counting.insert(plan);
                }
                self.running
                    .push(Running::started(plan, title, run, total, items, going_on));
                Taken::Nothing
            }
            ExecEvent::GoesOn { plan, next, .. } => {
                self.going_on.insert(next);
                self.servers.remove(&plan);
                // The same action, counted by what it has left.
                self.spell.plans.remove(&plan);
                // What the desktop was told of its wait is taken back.
                if let Some(desktop) = &self.desktop {
                    desktop.withdraw_key(plan.0);
                }
                Taken::Nothing
            }
            ExecEvent::Retried { plan, .. } => {
                // The same action, counted by what it has left.
                self.spell.plans.remove(&plan);
                Taken::Nothing
            }
            ExecEvent::WaitsForServer { plan } => {
                self.servers.insert(plan);
                self.tell_waits_for_server(plan, watched);
                Taken::WaitsForServer(plan)
            }
            ExecEvent::Paused { plan } | ExecEvent::Resumed { plan } => {
                let paused = matches!(event, ExecEvent::Paused { .. });
                if let Some(running) = self.running.iter_mut().find(|r| r.plan == plan) {
                    running.paused = paused;
                }
                Taken::Nothing
            }
            ExecEvent::Step(progress) => {
                if let Some(running) = self.running.iter_mut().find(|r| r.plan == progress.plan) {
                    running.stepped(&progress);
                }
                Taken::Step(progress)
            }
            ExecEvent::Finished {
                plan,
                applied,
                skipped,
                error,
                cancelled,
                undoable,
                items,
                said,
                moved,
                ran,
                tally,
            } => {
                let done = error.is_none() && !cancelled;
                let counts = self.counting.remove(&plan);
                let Some(index) = self.running.iter().position(|r| r.plan == plan) else {
                    return Taken::Ended {
                        plan,
                        done,
                        items,
                        answer: None,
                    };
                };
                // Ended, it counts whole in the spell, as it counted while
                // it waited and ran; cancelled before it did anything (one
                // cancelled while it waited), it leaves the count: nothing
                // ended, nothing waits.
                if counts && !(cancelled && applied == 0 && skipped == 0 && moved == 0) {
                    self.spell.plans.insert(plan);
                }
                let made = items.clone();
                let running = self.running.remove(index);
                let show = done
                    .then(|| toast::made_alone(&running, items.as_ref()))
                    .flatten()
                    .filter(|path| !shown_in_a_pane(app, path));
                let run = running.run;
                let title = running.title.clone();
                let stopped_by_error = error.is_some() && !cancelled;
                // Deleted where its server keeps a trash (ADR-086).
                let kept_in = match running.first() {
                    Some(Change::Delete { path }) => {
                        app.remote(path).and_then(|(_, known)| known?.trash)
                    }
                    _ => None,
                };
                let (text, tone) = toast::ended(
                    running,
                    toast::End {
                        applied,
                        error,
                        cancelled,
                        items,
                        kept_in: kept_in.as_deref(),
                        said: &said,
                        moved,
                        ran,
                        tally: tally.as_deref(),
                        skipped,
                    },
                );
                let mut actions = toast::finished_actions(
                    plan,
                    run,
                    undoable && applied > 0,
                    run == RunKind::Apply && app.executor.can_retry(plan),
                    run == RunKind::Undo && app.executor.can_redo(),
                );
                if let Some(path) = show
                    && run == RunKind::Apply
                {
                    actions.insert(0, toast::show_action(&path));
                }
                // Without a window every action is as a card: one started
                // and stopped between two syncs keeps its Retry too.
                let carded = watched.seen == Seen::NoWindow;
                // Done, and kept by the journal: the History holds it, with
                // its Undo, and Earlier does not say it again.
                let in_history = done && undoable && applied > 0;
                self.ended(
                    app,
                    plan,
                    &title,
                    (&text, tone, &actions),
                    (stopped_by_error, carded, in_history),
                );
                self.tell_ended(plan, &text, stopped_by_error, cancelled, ran, watched);
                Taken::Ended {
                    plan,
                    done,
                    items: made,
                    answer: Some(Toast {
                        text,
                        tone,
                        actions,
                    }),
                }
            }
        }
    }

    /// An action ended, as its answer says it (`text`, `tone`, `actions`):
    /// kept in `Earlier`, unless the History holds it (`in_history`: done,
    /// and kept by the journal); one an error stopped stays in the strip,
    /// if it was shown there (or `carded`, as every one is without a
    /// window).
    fn ended(
        &mut self,
        app: &App,
        plan: PlanId,
        title: &str,
        answer: (&str, Tone, &[Action]),
        (stopped_by_error, carded, in_history): (bool, bool, bool),
    ) {
        let (text, tone, actions) = answer;
        let carded = self.carded.remove(&plan) || carded;
        if !in_history {
            crate::activity::keep(&mut self.earlier, Ended::of_plan(plan, text, tone, actions));
        }
        if stopped_by_error && carded {
            self.stopped = Some(Stopped::new(plan, title, app.executor.can_retry(plan)));
        }
    }

    /// The plans shown as a card (those `under` holds): one an error stops
    /// stays in the strip.
    pub(crate) fn card(&mut self, under: &Under) {
        self.carded.extend(under.plans.iter().map(|(r, _)| r.plan));
    }

    /// A stopped action retried, wherever from (its toast, the palette, an
    /// agent), or let go: the strip says it no more; a server's that goes
    /// on by itself or was let go waits no more.
    pub(crate) fn sync_stopped(&mut self, app: &App) {
        if let Some(stopped) = &self.stopped
            && stopped.retries()
            && !app.executor.can_retry(stopped.plan)
        {
            self.stopped = None;
        }
        self.servers
            .retain(|plan| app.executor.waits_for_server(*plan));
    }

    /// Whether a plan its server stopped waits for it to answer again.
    pub(crate) fn waits_for_server(&self) -> bool {
        !self.servers.is_empty()
    }

    /// Someone looks again (the window has the focus): what the desktop
    /// was told is taken back.
    pub(crate) fn looked_at(&mut self) {
        self.away = Away::default();
        self.asks_told = false;
        if let Some(desktop) = &self.desktop {
            desktop.withdraw();
        }
    }

    /// An action ended as `answer` says, after `ran`: told to the desktop
    /// when no one looked and it was not cancelled; while a window is up,
    /// only if it ran long. One notice says what ended since someone last
    /// looked.
    fn tell_ended(
        &mut self,
        plan: PlanId,
        answer: &str,
        stopped: bool,
        cancelled: bool,
        ran: Duration,
        watched: Watched,
    ) {
        if watched.seen == Seen::LookedAt
            || cancelled
            || (ran < LONG && watched.seen == Seen::Away)
            || !watched.notify
        {
            return;
        }
        let Some(desktop) = &self.desktop else {
            return;
        };
        let away = &mut self.away;
        if stopped {
            away.stopped += 1;
        } else {
            away.finished += 1;
        }
        let title = match (away.finished, away.stopped) {
            (1, 0) => "Finished".to_owned(),
            (0, 1) => "Stopped".to_owned(),
            (finished, 0) => format!("{finished} actions finished"),
            (0, stopped) => format!("{stopped} actions stopped"),
            (finished, stopped) => format!(
                "{} finished, {stopped} stopped",
                format::count(finished, "action")
            ),
        };
        let kind = if away.stopped > 0 {
            Kind::Stopped
        } else {
            Kind::Finished
        };
        desktop.notify(
            plan.0,
            Notice {
                title,
                body: answer.to_owned(),
                kind,
            },
        );
    }

    /// An action its server stopped waits for it to answer again: told to
    /// the desktop while no one looks.
    fn tell_waits_for_server(&mut self, plan: PlanId, watched: Watched) {
        if watched.seen == Seen::LookedAt || !watched.notify {
            return;
        }
        let Some(desktop) = &self.desktop else {
            return;
        };
        let what = self
            .earlier
            .iter()
            .find(|ended| ended.plan == Some(plan))
            .map_or_else(String::new, |ended| ended.text.clone());
        desktop.notify(
            plan.0,
            Notice {
                title: "Waiting for the server".to_owned(),
                body: format!("{what}{WAITS_FOR_SERVER}"),
                kind: Kind::Waiting,
            },
        );
    }

    /// Something waits for an answer while no window is up to ask it:
    /// told once, until someone looks.
    pub(crate) fn tell_asks(&mut self, asks: bool, watched: Watched) {
        if !asks {
            self.asks_told = false;
            return;
        }
        if self.asks_told || watched.seen == Seen::LookedAt {
            return;
        }
        self.asks_told = true;
        if let Some(desktop) = &self.desktop {
            desktop.notify(
                ASKS,
                Notice {
                    title: "Skwir needs an answer".to_owned(),
                    body: "An action waits for your answer: open Skwir to give it.".to_owned(),
                    kind: Kind::Waiting,
                },
            );
        }
    }

    /// The taskbar's entry shows how far what runs has gone, stopped by an
    /// error in its colour where the system has one; the machine is kept
    /// awake while something runs and is not paused; the session's end is
    /// held off while anything runs or waits its turn (ADR-115). Told only
    /// as it changes.
    pub(crate) fn tell(&mut self, app: &App, under: &Under, watched: Watched) {
        let Some(desktop) = &self.desktop else {
            return;
        };
        let summary = summary(under);
        let progress = match summary.as_ref().filter(|summary| summary.has_share()) {
            Some(summary) => Some(Progress {
                fraction: summary.fraction,
                state: if summary.paused {
                    State::Paused
                } else {
                    State::Normal
                },
            }),
            None => self.stopped.as_ref().map(|_| Progress {
                fraction: self.told.map_or(1., |told| told.fraction),
                state: State::Error,
            }),
        };
        // A thousandth is all a taskbar's bar shows.
        let moved = match (progress, self.told) {
            (Some(now), Some(told)) => {
                now.state != told.state || (now.fraction - told.fraction).abs() >= 0.001
            }
            (now, told) => now.is_some() != told.is_some(),
        };
        if moved {
            desktop.progress(progress);
            self.told = progress;
        }
        let awake = (watched.keep_awake && summary.is_some_and(|s| !s.paused && !s.stopping))
            .then(|| why(under));
        if awake.is_some() != self.awake {
            self.awake = awake.is_some();
            desktop.keep_awake(awake);
        }
        let held = (app.executor.is_busy() || !under.tasks.is_empty()).then(|| why(under));
        if held != self.held {
            self.held.clone_from(&held);
            desktop.hold_session(held);
        }
    }

    /// Show `presence` while no window is up, `None` to take it back.
    pub(crate) fn present(&self, presence: Option<Presence>) {
        if let Some(desktop) = &self.desktop {
            desktop.presence(presence);
        }
    }

    /// The program ends: what was asked of the desktop is told first (the
    /// last notice), within `within`.
    pub(crate) fn finish(&mut self, within: Duration) {
        if let Some(desktop) = self.desktop.take() {
            desktop.finish(within);
        }
    }
}

/// What the machine is kept awake for and the session's end held off for,
/// as the desktop lists it.
pub(crate) fn why(under: &Under) -> String {
    match (under.plans.as_slice(), under.tasks.as_slice()) {
        ([(running, _)], []) => capitalized(&running.title),
        ([], [task]) => capitalized(&task.title),
        ([], []) => "Actions under way".to_owned(),
        _ => format!("{} under way", format::count(under.count(), "action")),
    }
}

/// Whether a pane shows the folder `path` is in: what a plan made there is
/// selected in it (`Shell::made`), and Show is not offered.
fn shown_in_a_pane(app: &skwir_core::App, path: &std::path::Path) -> bool {
    let ws = app.workspace.read();
    ws.panes().iter().any(|pane| {
        pane.location()
            .and_then(skwir_core::Location::path)
            .is_some_and(|folder| Some(folder.as_path()) == path.parent())
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use skwir_core::op::{Change, ExecEvent, PlanId, Progress, RunKind, StepStatus};

    use super::{Seen, Taken, Watched, Work};

    /// A window up and looked at: nothing is told.
    const LOOKED_AT: Watched = Watched {
        seen: Seen::LookedAt,
        notify: true,
        keep_awake: true,
    };

    fn app() -> (tempfile::TempDir, Arc<skwir_core::App>) {
        let tmp = tempfile::tempdir().unwrap();
        let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        (tmp, app)
    }

    /// Told started, counting as an action as the core says a plan of
    /// several steps does.
    fn started(plan: PlanId, title: &str, total: usize) -> ExecEvent {
        counted(plan, title, total, total > 1)
    }

    /// Told started, counting as an action as `counts` says (a single
    /// heavy step counts, a single rename does not).
    fn counted(plan: PlanId, title: &str, total: usize, counts: bool) -> ExecEvent {
        ExecEvent::Started {
            plan,
            title: title.into(),
            total,
            run: RunKind::Apply,
            items: None,
            counts,
        }
    }

    fn renamed(plan: PlanId, error: Option<&str>) -> ExecEvent {
        ExecEvent::Step(Progress {
            plan,
            step: 0,
            total: 2,
            change: Change::Rename {
                from: "/tmp/example/it's here.txt".into(),
                to: "/tmp/example/b.txt".into(),
            },
            named: None,
            status: StepStatus::Pending { reversible: true },
            error: error.map(str::to_owned),
        })
    }

    fn finished(plan: PlanId, applied: usize, error: Option<&str>) -> ExecEvent {
        ExecEvent::Finished {
            plan,
            applied,
            skipped: 0,
            error: error.map(str::to_owned),
            cancelled: false,
            undoable: true,
            items: None,
            said: Vec::new(),
            moved: 0,
            ran: Duration::from_millis(30),
            tally: None,
        }
    }

    /// `Finished`, cancelled as asked, having done what is said.
    fn cancelled(plan: PlanId, applied: usize, moved: u64) -> ExecEvent {
        ExecEvent::Finished {
            plan,
            applied,
            skipped: 0,
            error: None,
            cancelled: true,
            undoable: applied > 0,
            items: None,
            said: Vec::new(),
            moved,
            ran: Duration::from_millis(30),
            tally: None,
        }
    }

    /// What joined is shown and announced once it lasted (ADR-114,
    /// amended 2026-11-03): a single rename still going 0.3 s after it
    /// started counts and is announced for 2 s, the latest of several
    /// taking the tint; one that ended sooner never is. Each step says when
    /// the strip changes next by itself.
    #[test]
    fn what_joined_is_shown_and_announced_once_it_lasted() {
        use std::time::Instant;

        use super::{ANNOUNCED_FOR, Announce, SHOWN_AFTER};
        let (_tmp, app) = app();
        let mut work = Work::default();
        let t0 = Instant::now();
        let ms = |n: u64| Duration::from_millis(n);
        let (slow, later, quick) = (PlanId(21), PlanId(22), PlanId(23));
        work.take(
            &app,
            counted(slow, "rename it's a.txt", 1, false),
            LOOKED_AT,
        );
        assert_eq!(work.watch(&app, t0, None), Some(t0 + SHOWN_AFTER));
        assert_eq!(work.watch(&app, t0 + ms(100), None), Some(t0 + SHOWN_AFTER));
        assert!(!work.counting.contains(&slow) && work.announce.is_none());
        let at = t0 + SHOWN_AFTER;
        assert_eq!(work.watch(&app, at, None), Some(at + ANNOUNCED_FOR));
        assert!(work.counting.contains(&slow), "shown once it lasted");
        assert_eq!(
            work.announce,
            Some(Announce {
                plan: slow,
                until: at + ANNOUNCED_FOR
            })
        );
        // Another joins meanwhile: its time comes before the tint ends.
        work.take(&app, counted(later, "rename b.txt", 1, false), LOOKED_AT);
        let second = t0 + ms(1000);
        assert_eq!(work.watch(&app, second, None), Some(second + SHOWN_AFTER));
        let due = second + SHOWN_AFTER;
        assert_eq!(work.watch(&app, due, None), Some(due + ANNOUNCED_FOR));
        assert_eq!(work.announce.map(|a| a.plan), Some(later), "the latest");
        // One that ends before its time is never shown nor announced.
        work.take(&app, counted(quick, "rename c.txt", 1, false), LOOKED_AT);
        work.watch(&app, due + ms(10), None);
        work.take(&app, finished(quick, 1, None), LOOKED_AT);
        work.watch(&app, due + ms(500), None);
        assert!(!work.counting.contains(&quick));
        assert_eq!(work.announce.map(|a| a.plan), Some(later));
        // The tint ends; nothing more to wake for.
        assert_eq!(work.watch(&app, due + ANNOUNCED_FOR, None), None);
        assert!(work.announce.is_none());
        // Each announced once: still running, neither is announced again.
        assert_eq!(work.watch(&app, due + ANNOUNCED_FOR + ms(500), None), None);
        assert!(work.announce.is_none());
    }

    /// An action unfolds Activity in the window that asked it once it lasted
    /// `unfold_after` (ADR-114, amended 2026-11-07), and in no other; one
    /// whose window is not known (an undo, a retry, a script's) in the
    /// window in front as it joined. The program is woken then, and nothing
    /// unfolds without a window.
    #[test]
    fn an_action_unfolds_activity_where_it_was_asked_once_it_lasted() {
        use std::collections::HashMap;
        use std::time::Instant;

        use skwir_core::workspace::WindowId;

        use super::{Askers, SHOWN_AFTER};
        let (_tmp, app) = app();
        let mut work = Work::default();
        let t0 = Instant::now();
        let ms = |n: u64| Duration::from_millis(n);
        let (here, there) = (WindowId(1), WindowId(2));
        let (copy, undo) = (PlanId(31), PlanId(32));
        let ran = HashMap::from([(copy, there)]);
        let askers = Askers {
            ran: &ran,
            front: here,
            unfold_after: ms(1500),
        };
        work.take(
            &app,
            counted(copy, "copy 3 items to “Backup”", 3, true),
            LOOKED_AT,
        );
        assert_eq!(work.watch(&app, t0, Some(askers)), Some(t0 + SHOWN_AFTER));
        assert_eq!(
            work.watch(&app, t0 + SHOWN_AFTER, Some(askers)),
            Some(t0 + ms(1500))
        );
        assert!(!work.unfolds_in(there, t0 + ms(1499)));
        // Its window's: asked from there, though the other is in front.
        assert!(work.unfolds_in(there, t0 + ms(1500)));
        assert!(!work.unfolds_in(here, t0 + ms(1500)));
        // Asked from no window known: the one in front as it joined, even
        // once another comes to the front.
        let t1 = t0 + ms(2000);
        work.take(&app, counted(undo, "undo rename “a”", 1, true), LOOKED_AT);
        let front_then = Askers {
            front: here,
            ..askers
        };
        work.watch(&app, t1, Some(front_then));
        let front_now = Askers {
            front: there,
            ..askers
        };
        assert_eq!(
            work.watch(&app, t1 + ms(500), Some(front_now)),
            Some(t1 + ms(1500))
        );
        assert!(work.unfolds_in(here, t1 + ms(1500)));
        // Ended, it unfolds nothing more.
        work.take(&app, finished(undo, 1, None), LOOKED_AT);
        work.take(&app, finished(copy, 3, None), LOOKED_AT);
        assert_eq!(work.watch(&app, t1 + ms(1600), Some(front_now)), None);
        assert!(!work.unfolds_in(here, t1 + ms(1600)));
        assert!(!work.unfolds_in(there, t1 + ms(1600)));
        // Without a window, nothing unfolds.
        let late = PlanId(33);
        work.take(&app, counted(late, "copy “b”", 1, true), LOOKED_AT);
        work.watch(&app, t1, None);
        assert!(!work.unfolds_in(here, t1 + ms(5000)));
    }

    /// A task (a print) unfolds Activity as a plan does: in the window in
    /// front as it was first seen, once it lasted, the program woken then;
    /// ended, it unfolds nothing more, and without a window it unfolds
    /// nothing.
    #[test]
    fn a_task_unfolds_activity_as_a_plan_does() {
        use std::collections::HashMap;
        use std::time::Instant;

        use skwir_core::CancelToken;
        use skwir_core::workspace::WindowId;

        use super::Askers;
        let (_tmp, app) = app();
        let mut work = Work::default();
        let t0 = Instant::now();
        let ms = |n: u64| Duration::from_millis(n);
        let (here, there) = (WindowId(1), WindowId(2));
        let ran = HashMap::new();
        let askers = Askers {
            ran: &ran,
            front: here,
            unfold_after: ms(1500),
        };
        let task = app.tasks.start("Print “it's a.pdf”", CancelToken::new());
        assert_eq!(work.watch(&app, t0, Some(askers)), Some(t0 + ms(1500)));
        assert!(!work.unfolds_in(here, t0 + ms(1499)));
        assert!(work.unfolds_in(here, t0 + ms(1500)));
        assert!(!work.unfolds_in(there, t0 + ms(1500)));
        // Seen first as it joined, whichever comes to the front later.
        let front_now = Askers {
            front: there,
            ..askers
        };
        work.watch(&app, t0 + ms(500), Some(front_now));
        assert!(work.unfolds_in(here, t0 + ms(1500)));
        assert!(!work.unfolds_in(there, t0 + ms(1500)));
        drop(task);
        assert_eq!(work.watch(&app, t0 + ms(1600), Some(askers)), None);
        assert!(!work.unfolds_in(here, t0 + ms(1600)));
        // Without a window, nothing unfolds.
        let late = app.tasks.start("Print “b.pdf”", CancelToken::new());
        work.watch(&app, t0, None);
        assert!(!work.unfolds_in(here, t0 + ms(5000)));
        drop(late);
    }

    /// One timer at a time wakes the program for the strip (ADR-114): a
    /// sooner time takes the place of a later one, a later one waits for
    /// the sooner to fire, and once it fired a wake may be set again, so a
    /// time set by a window that closed is cleared by its own timer, never
    /// left to hold off every later wake.
    #[test]
    fn one_wake_at_a_time_is_set_for_the_strip() {
        let mut work = Work::default();
        let t0 = std::time::Instant::now();
        let ms = |n: u64| Duration::from_millis(n);
        assert!(work.wake_for(t0 + ms(300)));
        assert!(!work.wake_for(t0 + ms(2300)), "the sooner one wakes first");
        assert!(work.wake_for(t0 + ms(100)), "a sooner one is set");
        work.woken_at(t0 + ms(300));
        assert_eq!(work.wake_at, Some(t0 + ms(100)), "not the one that fired");
        work.woken_at(t0 + ms(100));
        assert_eq!(work.wake_at, None);
        assert!(work.wake_for(t0 + ms(2300)), "set again once fired");
    }

    /// The strip's spell of work counts each action that ended as the
    /// core counted it while it waited and ran (`Started::counts`: several
    /// steps, or a heavy one), and begins again once nothing runs or
    /// waits (ADR-114, amended 2026-11-03): a single rename between two is
    /// no action of its own.
    #[test]
    fn the_spell_counts_what_ended_until_nothing_is_under_way() {
        let (_tmp, app) = app();
        let mut work = Work::default();
        let (first, quick, copy) = (PlanId(11), PlanId(12), PlanId(13));
        work.take(&app, started(first, "rename 2 items", 2), LOOKED_AT);
        work.take(&app, finished(first, 2, None), LOOKED_AT);
        assert_eq!(work.spell_ended(), 1);
        work.take(&app, started(quick, "rename it's a.txt", 1), LOOKED_AT);
        work.take(&app, finished(quick, 1, None), LOOKED_AT);
        assert_eq!(work.spell_ended(), 1, "a single rename counts for nothing");
        // A single heavy step (one file copied, a folder deleted for good)
        // counts, whatever it moved.
        work.take(
            &app,
            counted(copy, "copy it's a.txt to Backup", 1, true),
            LOOKED_AT,
        );
        // Still running: the spell goes on.
        work.settle(&app);
        assert_eq!(work.spell_ended(), 1);
        work.take(&app, finished(copy, 1, None), LOOKED_AT);
        assert_eq!(work.spell_ended(), 2, "a single heavy step");
        assert_eq!(work.under_way(&app).ended, 2);
        // Nothing runs, waits or goes on by itself: it begins again.
        work.settle(&app);
        assert_eq!(work.spell_ended(), 0);
    }

    /// One the core ended whose `Finished` is not taken in yet counts as it
    /// will, so that the bar never dips between the two; one cancelled
    /// meanwhile does not.
    #[test]
    fn a_run_ended_in_the_core_counts_before_its_answer_is_taken() {
        let (_tmp, app) = app();
        let mut work = Work::default();
        // Told started, gone from the core's running ones: ended there.
        work.take(&app, started(PlanId(21), "copy 2 items", 2), LOOKED_AT);
        let under = work.under_way(&app);
        assert!(under.plans.is_empty());
        assert_eq!(under.ended, 1);
        work.running[0].cancelling = true;
        assert_eq!(work.under_way(&app).ended, 0, "cancelled, it may leave");
    }

    /// An action cancelled before it did anything, while it waited (its
    /// Cancel, Cancel all) or as it started, leaves the count: nothing
    /// ended, nothing waits. One cancelled after it did something ended,
    /// and counts whole.
    #[test]
    fn an_action_cancelled_before_it_did_anything_leaves_the_count() {
        let (_tmp, app) = app();
        let mut work = Work::default();
        work.take(&app, started(PlanId(1), "copy 4 items", 4), LOOKED_AT);
        // Cancel all: three waiting, each told started and cancelled.
        for plan in [PlanId(2), PlanId(3), PlanId(4)] {
            work.take(&app, started(plan, "copy 2 items", 2), LOOKED_AT);
            work.take(&app, cancelled(plan, 0, 0), LOOKED_AT);
        }
        assert_eq!(work.spell_ended(), 0, "nothing of them ended");
        // Cancelled halfway through a file, or after a step: it did
        // something, and ended.
        work.take(&app, started(PlanId(5), "copy 2 items", 2), LOOKED_AT);
        work.take(&app, cancelled(PlanId(5), 0, 4096), LOOKED_AT);
        work.take(&app, started(PlanId(6), "copy 2 items", 2), LOOKED_AT);
        work.take(&app, cancelled(PlanId(6), 1, 0), LOOKED_AT);
        assert_eq!(work.spell_ended(), 2);
    }

    /// An action that goes on, by itself once its server answers again or
    /// retried, is the same action (ADR-080, 9): the run that stopped
    /// leaves what ended as its continuation is asked, which counts once,
    /// by what it has left.
    #[test]
    fn an_action_that_goes_on_is_counted_once() {
        let (_tmp, app) = app();
        let mut work = Work::default();
        // Another runs, holding the spell open.
        work.take(&app, started(PlanId(1), "copy 9 items", 9), LOOKED_AT);
        let (stopped, next) = (PlanId(2), PlanId(3));
        work.take(
            &app,
            started(stopped, "copy 4 items to /mnt/server", 4),
            LOOKED_AT,
        );
        work.take(&app, finished(stopped, 1, Some("Host is down")), LOOKED_AT);
        assert_eq!(work.spell_ended(), 1, "stopped: it ended for now");
        work.take(&app, ExecEvent::WaitsForServer { plan: stopped }, LOOKED_AT);
        work.take(
            &app,
            ExecEvent::GoesOn {
                plan: stopped,
                next,
                title: "copy 4 items to /mnt/server".into(),
            },
            LOOKED_AT,
        );
        assert_eq!(work.spell_ended(), 0, "it goes on: not ended");
        work.take(
            &app,
            started(next, "copy 3 items to /mnt/server", 3),
            LOOKED_AT,
        );
        work.take(&app, finished(next, 3, None), LOOKED_AT);
        assert_eq!(work.spell_ended(), 1, "one action");

        // Retried by the user: the same.
        let (failed, retry) = (PlanId(4), PlanId(5));
        work.take(&app, started(failed, "move 2 items to Stick", 2), LOOKED_AT);
        work.take(&app, finished(failed, 0, Some("No space left")), LOOKED_AT);
        assert_eq!(work.spell_ended(), 2);
        work.take(
            &app,
            ExecEvent::Retried {
                plan: failed,
                next: retry,
                title: "move 2 items to Stick".into(),
            },
            LOOKED_AT,
        );
        assert_eq!(work.spell_ended(), 1, "retried: it waits, not ended");
        work.take(&app, started(retry, "move 2 items to Stick", 2), LOOKED_AT);
        work.take(&app, finished(retry, 2, None), LOOKED_AT);
        assert_eq!(work.spell_ended(), 2);
    }

    /// A task (a print) that ends counts whole, as a plan does, so that the
    /// bar never falls as it ends; one stopped before it went anywhere
    /// leaves the count.
    #[test]
    fn a_task_that_ends_counts_whole() {
        let (_tmp, app) = app();
        let mut work = Work::default();
        // A plan runs, holding the spell open.
        work.take(&app, started(PlanId(1), "copy 9 items", 9), LOOKED_AT);
        let print = app
            .tasks
            .start("Print “it's a.pdf”", skwir_core::CancelToken::new());
        work.settle(&app);
        print.went(0.5);
        drop(print);
        // Ended, its end not settled yet: counted as it will be.
        assert_eq!(work.under_way(&app).ended, 2, "the plan pending, the print");
        work.settle(&app);
        assert_eq!(work.spell_ended(), 1);
        // Stopped before it went anywhere: nothing ended.
        let cancel = skwir_core::CancelToken::new();
        let quit = app.tasks.start("Print “b.pdf”", cancel.clone());
        work.settle(&app);
        cancel.cancel();
        work.settle(&app);
        drop(quit);
        work.settle(&app);
        assert_eq!(work.spell_ended(), 1);
    }

    /// What runs is followed from its start to its answer: the window's
    /// and the program's alike, whoever takes it in. Done and kept by the
    /// journal, the History holds it, and `Earlier` does not.
    #[test]
    fn a_plan_is_followed_to_its_answer() {
        let (_tmp, app) = app();
        let mut work = Work::default();
        let plan = PlanId(7);
        assert!(matches!(
            work.take(&app, started(plan, "rename 2 items", 2), LOOKED_AT),
            Taken::Nothing
        ));
        assert_eq!(work.running.len(), 1);
        assert!(matches!(
            work.take(&app, renamed(plan, None), LOOKED_AT),
            Taken::Step(_)
        ));
        let Taken::Ended {
            plan: ended,
            done,
            answer: Some(answer),
            ..
        } = work.take(&app, finished(plan, 2, None), LOOKED_AT)
        else {
            panic!("a plan it knew ends with its answer");
        };
        assert_eq!(ended, plan);
        assert!(done);
        assert!(!answer.text.is_empty());
        assert!(work.running.is_empty());
        assert!(work.earlier.is_empty(), "the History holds it");
        assert!(work.stopped.is_none(), "it went through");

        // One it never saw start still says what it made.
        assert!(matches!(
            work.take(&app, finished(PlanId(8), 1, None), LOOKED_AT),
            Taken::Ended { answer: None, .. }
        ));
    }

    /// `Earlier` keeps what the History does not hold: an action an error
    /// stopped, one cancelled, one done that nothing undoes; one done and
    /// undone by the journal is the History's alone.
    #[test]
    fn earlier_keeps_what_the_history_does_not_hold() {
        let (_tmp, app) = app();
        let mut work = Work::default();
        let (failed, halted, for_good, undoable) = (PlanId(31), PlanId(32), PlanId(33), PlanId(34));
        for (plan, title) in [
            (failed, "copy it's a.txt to Backup"),
            (halted, "copy it's b.txt to Backup"),
            (for_good, "delete it's c.txt for good"),
            (undoable, "rename it's d.txt"),
        ] {
            work.take(&app, started(plan, title, 1), LOOKED_AT);
        }
        work.take(&app, finished(failed, 0, Some("No space left")), LOOKED_AT);
        work.take(&app, cancelled(halted, 1, 4), LOOKED_AT);
        let mut gone = finished(for_good, 1, None);
        if let ExecEvent::Finished { undoable, .. } = &mut gone {
            *undoable = false;
        }
        work.take(&app, gone, LOOKED_AT);
        work.take(&app, finished(undoable, 1, None), LOOKED_AT);
        let kept: Vec<Option<PlanId>> = work.earlier.iter().map(|e| e.plan).collect();
        assert_eq!(kept, [Some(for_good), Some(halted), Some(failed)]);
    }

    /// An action an error stopped stays in the strip only if it was shown
    /// there as a card: without a window, every one running is.
    #[test]
    fn a_plan_stopped_by_an_error_stays_when_it_was_a_card() {
        let (_tmp, app) = app();
        let mut work = Work::default();
        let (shown, quick) = (PlanId(1), PlanId(2));
        work.take(&app, started(shown, "copy 3 items to Backup", 3), LOOKED_AT);
        work.take(&app, started(quick, "rename 2 items", 2), LOOKED_AT);
        work.carded.insert(shown);
        work.take(&app, renamed(quick, Some("Permission denied")), LOOKED_AT);
        work.take(
            &app,
            finished(quick, 0, Some("Permission denied")),
            LOOKED_AT,
        );
        assert!(work.stopped.is_none(), "never a card: its answer says it");
        work.take(&app, finished(shown, 1, Some("No space left")), LOOKED_AT);
        let stopped = work.stopped.as_ref().expect("a card: it stays");
        assert_eq!(stopped.plan, shown);
        assert_eq!(stopped.title, "copy 3 items to Backup");
        assert_eq!(work.earlier.len(), 2, "both answers kept, newest first");
        assert_eq!(work.earlier[0].plan, Some(shown));
    }

    /// Without a window, an action an error stops stays though it was
    /// never shown as a card (started and stopped between two syncs): it
    /// keeps the program, and its Retry, for the window.
    #[test]
    fn without_a_window_a_plan_stopped_by_an_error_stays() {
        let (_tmp, app) = app();
        let mut work = Work::default();
        let unwatched = Watched {
            seen: Seen::NoWindow,
            ..LOOKED_AT
        };
        let plan = PlanId(9);
        work.take(&app, started(plan, "copy 3 items to Stick", 3), unwatched);
        work.take(&app, finished(plan, 0, Some("No such device")), unwatched);
        let stopped = work.stopped.as_ref().expect("kept without a window");
        assert_eq!(stopped.plan, plan);
    }

    /// Paused and resumed follow the plan; a plan its server stopped is
    /// waited for until it goes on by itself.
    #[test]
    fn a_pause_and_a_wait_for_a_server_are_followed() {
        let (_tmp, app) = app();
        let mut work = Work::default();
        let plan = PlanId(3);
        work.take(&app, started(plan, "copy to /mnt/server", 4), LOOKED_AT);
        work.take(&app, ExecEvent::Paused { plan }, LOOKED_AT);
        assert!(work.running[0].paused);
        work.take(&app, ExecEvent::Resumed { plan }, LOOKED_AT);
        assert!(!work.running[0].paused);
        assert!(matches!(
            work.take(&app, ExecEvent::WaitsForServer { plan }, LOOKED_AT),
            Taken::WaitsForServer(waits) if waits == plan
        ));
        assert!(work.waits_for_server());
        let next = PlanId(4);
        work.take(
            &app,
            ExecEvent::GoesOn {
                plan,
                next,
                title: "copy to /mnt/server".into(),
            },
            LOOKED_AT,
        );
        assert!(!work.waits_for_server(), "it goes on");
        work.take(&app, started(next, "copy to /mnt/server", 4), LOOKED_AT);
        assert!(work.running.iter().any(|r| r.plan == next && r.going_on));
    }
}
