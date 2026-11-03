//! What runs and what was said of it (ADR-114, ADR-115), kept apart from
//! the window that shows it, so that it outlives the window.
//!
//! While a window is up, the shell holds it: the executor's events taken
//! in (`Work::take`), the strip, Activity's cards and `Earlier` read from
//! it, the desktop told. When the last window closes while something is
//! left, the program keeps it and goes on taking the events without a
//! window (`finishing.rs`); a window opened again takes it back, `Earlier`
//! and the stopped strip with it.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use skwir_core::App;
use skwir_core::op::{Change, ExecEvent, PlanId, RunKind};
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
    /// The actions of this spell of work that ended, each as one the strip
    /// counts (ADR-114, amended 2026-11-03): from an action started with
    /// nothing under way until nothing runs, waits or goes on by itself
    /// (`Work::settle`). Counted whole, so the bar never goes back as one
    /// ends.
    pub(crate) spell_ended: usize,
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

    /// What runs and is shown as a card: a plan of a single step ends
    /// before anyone could read a bar, unless the step moves bytes (one
    /// large copy).
    pub(crate) fn under_way(&self, app: &App) -> Under {
        let plans = self
            .running
            .iter()
            .filter_map(|running| {
                let transfer = app.executor.transfer(running.plan)?;
                (running.total > 1 || transfer.total > 0).then(|| (running.clone(), transfer))
            })
            .collect();
        Under {
            plans,
            tasks: app.tasks.running(),
            ended: self.spell_ended,
            waiting: app.executor.waiting().iter().filter(|w| w.weighs()).count(),
        }
    }

    /// The spell of work ends once nothing runs, waits or goes on by
    /// itself, its plans' ends taken in: what the strip counts begins
    /// again (`spell_ended`). Asked after each sync's events, as the
    /// executor wakes the window once it is idle.
    pub(crate) fn settle(&mut self, app: &App) {
        if self.spell_ended > 0
            && self.running.is_empty()
            && app.tasks.running().is_empty()
            && !app.executor.busy_or_waiting()
        {
            self.spell_ended = 0;
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
            } => {
                let going_on = self.going_on.remove(&plan);
                self.running
                    .push(Running::started(plan, title, run, total, items, going_on));
                Taken::Nothing
            }
            ExecEvent::GoesOn { plan, next, .. } => {
                self.going_on.insert(next);
                self.servers.remove(&plan);
                // What the desktop was told of its wait is taken back.
                if let Some(desktop) = &self.desktop {
                    desktop.withdraw_key(plan.0);
                }
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
                error,
                cancelled,
                undoable,
                items,
                said,
                moved,
                ran,
                ..
            } => {
                let done = error.is_none() && !cancelled;
                let Some(index) = self.running.iter().position(|r| r.plan == plan) else {
                    return Taken::Ended {
                        plan,
                        done,
                        items,
                        answer: None,
                    };
                };
                let made = items.clone();
                let running = self.running.remove(index);
                // Counted as the strip counts it while it ran or waited:
                // several steps, or bytes moved (`Waiting::weighs`).
                if running.total > 1 || moved > 0 {
                    self.spell_ended += 1;
                }
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
                    },
                );
                let actions = toast::finished_actions(
                    plan,
                    run,
                    undoable && applied > 0,
                    run == RunKind::Apply && app.executor.can_retry(plan),
                    run == RunKind::Undo && app.executor.can_redo(),
                );
                // Without a window every action is as a card: one started
                // and stopped between two syncs keeps its Retry too.
                let carded = watched.seen == Seen::NoWindow;
                self.ended(
                    app,
                    plan,
                    &title,
                    (&text, tone, &actions),
                    stopped_by_error,
                    carded,
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
    /// kept in `Earlier`; one an error stopped stays in the strip, if it
    /// was shown there (or `carded`, as every one is without a window).
    fn ended(
        &mut self,
        app: &App,
        plan: PlanId,
        title: &str,
        answer: (&str, Tone, &[Action]),
        stopped_by_error: bool,
        carded: bool,
    ) {
        let (text, tone, actions) = answer;
        let carded = self.carded.remove(&plan) || carded;
        crate::activity::keep(&mut self.earlier, Ended::of_plan(plan, text, tone, actions));
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
        let progress = match &summary {
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

    fn started(plan: PlanId, title: &str, total: usize) -> ExecEvent {
        ExecEvent::Started {
            plan,
            title: title.into(),
            total,
            run: RunKind::Apply,
            items: None,
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
        }
    }

    /// The strip's spell of work counts each action that ended as the
    /// strip counted it while it ran (several steps, or bytes moved), and
    /// begins again once nothing runs or waits (ADR-114, amended
    /// 2026-11-03): a single rename between two is no action of its own.
    #[test]
    fn the_spell_counts_what_ended_until_nothing_is_under_way() {
        let (_tmp, app) = app();
        let mut work = Work::default();
        let (first, quick, copy) = (PlanId(11), PlanId(12), PlanId(13));
        work.take(&app, started(first, "rename 2 items", 2), LOOKED_AT);
        work.take(&app, finished(first, 2, None), LOOKED_AT);
        assert_eq!(work.spell_ended, 1);
        work.take(&app, started(quick, "rename it's a.txt", 1), LOOKED_AT);
        work.take(&app, finished(quick, 1, None), LOOKED_AT);
        assert_eq!(work.spell_ended, 1, "a single rename counts for nothing");
        work.take(
            &app,
            started(copy, "copy it's a.txt to Backup", 1),
            LOOKED_AT,
        );
        let moved = match finished(copy, 1, None) {
            ExecEvent::Finished {
                plan,
                applied,
                skipped,
                error,
                cancelled,
                undoable,
                items,
                said,
                ran,
                ..
            } => ExecEvent::Finished {
                plan,
                applied,
                skipped,
                error,
                cancelled,
                undoable,
                items,
                said,
                moved: 4096,
                ran,
            },
            other => other,
        };
        // Still running: the spell goes on.
        work.settle(&app);
        assert_eq!(work.spell_ended, 1);
        work.take(&app, moved, LOOKED_AT);
        assert_eq!(work.spell_ended, 2, "one file copied, its bytes moved");
        assert_eq!(work.under_way(&app).ended, 2);
        // Nothing runs, waits or goes on by itself: it begins again.
        work.settle(&app);
        assert_eq!(work.spell_ended, 0);
    }

    /// What runs is followed from its start to its answer, which `Earlier`
    /// keeps: the window's and the program's alike, whoever takes it in.
    #[test]
    fn a_plan_is_followed_to_its_answer_kept_in_earlier() {
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
        assert!(work.running.is_empty());
        assert_eq!(work.earlier.len(), 1);
        assert_eq!(work.earlier[0].text, answer.text);
        assert!(work.stopped.is_none(), "it went through");

        // One it never saw start still says what it made.
        assert!(matches!(
            work.take(&app, finished(PlanId(8), 1, None), LOOKED_AT),
            Taken::Ended { answer: None, .. }
        ));
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
