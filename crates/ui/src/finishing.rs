//! What runs outlives the window (ADR-115).
//!
//! Closing the last window stops nothing. The window goes at once, and the
//! program keeps what runs and what was said of it (`Work`): it takes the
//! executor's events in as the window did, tells the desktop, and shows
//! that it is still at work, its presence (an icon in the tray or the
//! notification area, else a notice that stays; on macOS its Dock icon). It
//! ends by itself once nothing keeps it (`keeps`). Meanwhile its panes
//! sleep (`App::suspend_panes`): nothing is listed or watched.
//!
//! The window opens again from the presence, a notice, the Dock, or a
//! later launch, which hands itself over (`skwir_platform::handover`) rather
//! than start a Skwir that knows nothing of what this one finishes. It
//! takes back what runs, `Earlier` and the stopped strip with it.
//!
//! Quit asks while something is under way (`Shell::quit`): finish without
//! the window, or stop now. Logging out or shutting down is held off
//! meanwhile (`Work::tell`); on macOS the system asks first
//! (`may_quit`); a signal to end (Unix) stops what runs and quits, as Stop
//! and quit does.
//!
//! One loop takes the core's wakes for the program's life (`listen`): the
//! window's sync while one is up, this module's while none is. `Work` is
//! the program's for its life (`Finishing::work`): the window's shell
//! shares it and never moves it, so nothing it took can go with a window
//! that closed, nor with one that failed to open; a shell that has left
//! takes nothing more (`Shell::leave`).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use gpui::{App, AppContext, Global, WindowHandle};
use parking_lot::Mutex;
use skwir_core::Location;
use skwir_platform::desktop::{Asked, Presence, Standing};
use skwir_platform::handover::{Opening, TakingOver};

use crate::activity::{Under, summary, words};
use crate::format;
use crate::keymap;
use crate::shell::Shell;
use crate::toast::capitalized;
use crate::work::{Watched, Work};

/// How long the program waits, as it ends, for the desktop to be told the
/// last of what it was asked (the last notice).
const TOLD_WITHIN: Duration = Duration::from_secs(2);

/// The program's own state across its windows.
pub(crate) struct Finishing {
    app: Arc<skwir_core::App>,
    keymap: keymap::Keymap,
    started: Instant,
    /// The window up, if any: none from the moment it leaves.
    window: Option<WindowHandle<Shell>>,
    /// What runs and what was said of it, for the program's life: the
    /// window's shell shares it while one is up.
    work: Rc<RefCell<Work>>,
    /// Asked to stop and quit: the window closing ends the program.
    stopping: bool,
    /// The program ends: the system asking again is told yes.
    quitting: bool,
    /// Later launches taken over, while no window is up: set by a worker
    /// (`take_over` waits for the bus), kept only for the round it was
    /// asked in (`round`).
    taking: Arc<Mutex<Option<TakingOver>>>,
    /// Counts the times taking over started or stopped.
    round: Arc<AtomicU64>,
    /// The launches handed over, for the next wake.
    openings: Arc<Mutex<Vec<Opening>>>,
    /// A signal asked the program to end.
    ending: Arc<AtomicBool>,
    /// The presence shown, while no window is up.
    shown: Option<Presence>,
    /// A window was opened once: one that cannot open after is no reason
    /// to end.
    opened: bool,
}

impl Global for Finishing {}

/// Start the program's life: its state, the loop taking the core's wakes,
/// what ends it, then its window, showing `initial` (else the tabs left
/// open).
pub(crate) fn start(
    cx: &mut App,
    app: Arc<skwir_core::App>,
    keymap: keymap::Keymap,
    started: Instant,
    wakes: flume::Receiver<()>,
    initial: Option<Location>,
) {
    // The program ends when nothing keeps it (`closed`), not with its last
    // window.
    cx.set_quit_mode(gpui::QuitMode::Explicit);
    let ending = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    end_on_signals(&app, &ending);
    let work = Rc::new(RefCell::new(Work::new(&app)));
    cx.set_global(Finishing {
        app,
        keymap,
        started,
        window: None,
        work,
        stopping: false,
        quitting: false,
        taking: Arc::default(),
        round: Arc::default(),
        openings: Arc::default(),
        ending,
        shown: None,
        opened: false,
    });
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            closed(cx);
        }
    })
    .detach();
    listen(cx, wakes);
    open(cx, initial, None, false);
}

/// Wake-up, not polling: the core signals, the window's shell applies what
/// it sent and gpui draws (or, with no window up, this module takes it
/// in). Applying is done here, not left to the draw that `notify` asks
/// for: a window the compositor is not showing (a workspace that is not
/// on screen) is never asked to draw, and what the core sent (a media
/// key's request, a track that ended) must not wait until it is. `render`
/// syncs too, for what the user did since.
fn listen(cx: &mut App, wakes: flume::Receiver<()>) {
    cx.spawn(async move |cx| {
        loop {
            if wakes.recv_async().await.is_err() {
                break;
            }
            loop {
                while wakes.try_recv().is_ok() {}
                let backlog = cx.update(woken);
                if !backlog {
                    break;
                }
                // A listing is still being applied, a slice per sync
                // (`PUMP_BUDGET`): give the event loop a turn, so the
                // window draws what has arrived, then go on. The wait must
                // go through the platform's loop: a zero timer is a ready
                // task in gpui, and yields to nothing.
                cx.background_executor()
                    .timer(Duration::from_millis(1))
                    .await;
            }
        }
    })
    .detach();
}

/// The core woke the program: whether a listing is still being applied.
fn woken(cx: &mut App) -> bool {
    let state = cx.global::<Finishing>();
    if state.quitting {
        return false;
    }
    if state.ending.load(Ordering::Acquire) {
        end(cx);
        return false;
    }
    let openings = std::mem::take(&mut *state.openings.lock());
    for opening in openings {
        reopen(cx, Some(opening), false);
    }
    let state = cx.global::<Finishing>();
    match state.window {
        Some(window) => window
            .update(cx, |shell, window, cx| {
                shell.sync(window, cx);
                cx.notify();
                shell.backlog()
            })
            .unwrap_or(false),
        // No window since one was up: the program takes it in.
        None if state.opened => {
            sync(cx);
            false
        }
        None => false,
    }
}

/// A signal asked the program to end: what runs stops, as Stop and quit
/// does, the window (if up) closing first, as it does then: it is not
/// left frozen on screen while plans take back what they were in the
/// middle of, and its tabs are remembered (`closed`).
fn end(cx: &mut App) {
    let window = cx.global::<Finishing>().window;
    let closed = window.is_some_and(|window| window.update(cx, Shell::stop_and_quit).is_ok());
    if !closed {
        quit(cx);
    }
}

/// The window is about to close (`Shell::leave`): from now on the
/// program takes in what the core sends, the window's shell taking
/// nothing more. An action stopped before is let go: the window showed it.
pub(crate) fn left(cx: &mut App) {
    let state = cx.global_mut::<Finishing>();
    state.window = None;
    state.work.borrow_mut().stopped = None;
}

/// Stop and quit was asked: the window closing ends the program, the
/// plans stopping as it does (`lib.rs`, `wind_down`).
pub(crate) fn stopping(cx: &mut App) {
    cx.global_mut::<Finishing>().stopping = true;
}

/// Whether something runs, waits its turn, or waits for its server to go
/// on by itself: what Quit asks about. Asked of the executor at once: a
/// plan handed from waiting for its server to its turn is never missed
/// between two questions.
pub(crate) fn under_way(app: &skwir_core::App) -> bool {
    app.executor.busy_or_waiting() || !app.tasks.running().is_empty()
}

/// Whether the program stays without a window: something under way, an
/// action an error stopped since the window closed (its Retry), a
/// question waiting for the window to ask it.
fn keeps(app: &skwir_core::App, work: &Work) -> bool {
    under_way(app) || work.stopped.is_some() || app.waits_for_answer()
}

/// The last window closed: the program ends, unless something keeps it,
/// which it then finishes without a window.
fn closed(cx: &mut App) {
    let state = cx.global_mut::<Finishing>();
    // Gone without leaving (`left`): what it showed is let go all the same.
    if state.window.take().is_some() {
        state.work.borrow_mut().stopped = None;
    }
    if state.quitting {
        return;
    }
    let app = Arc::clone(&state.app);
    crate::remember_location(&app);
    if state.stopping || !keeps(&app, &state.work.borrow()) {
        quit(cx);
        return;
    }
    tracing::info!("the window closed: finishing what runs without it");
    app.suspend_panes();
    take_over(state);
    sync(cx);
}

/// Take later launches over, on a worker: on Linux it waits for the bus.
/// What it gets is kept only if taking over was not stopped meanwhile.
fn take_over(state: &Finishing) {
    let round = state.round.fetch_add(1, Ordering::AcqRel) + 1;
    let (openings, wake) = (Arc::clone(&state.openings), state.app.waker_handle());
    let (taking, rounds) = (Arc::clone(&state.taking), Arc::clone(&state.round));
    let started = std::thread::Builder::new()
        .name("skwir-take-over".into())
        .spawn(move || {
            let taken = skwir_platform::handover::take_over(move |opening| {
                openings.lock().push(opening);
                wake();
            });
            let mut taking = taking.lock();
            if rounds.load(Ordering::Acquire) == round {
                *taking = taken;
            }
        });
    if let Err(error) = started {
        tracing::debug!("later launches start a Skwir of their own: {error}");
    }
}

/// Stop taking later launches over: once this returns, none is handed
/// here; one handed just before waits in `openings`. What served them is
/// taken down on a thread of its own: the bus or the pipe is not waited
/// for here.
fn stop_taking_over(state: &Finishing) {
    state.round.fetch_add(1, Ordering::AcqRel);
    let taking = state.taking.lock().take();
    if let Some(taking) = taking {
        taking.stop_aside();
    }
}

/// No window is up: take in what the core sent, tell the desktop, show
/// the presence, do what it was asked; end once nothing keeps the program.
fn sync(cx: &mut App) {
    let state = cx.global_mut::<Finishing>();
    let app = Arc::clone(&state.app);
    let (keeps, asked) = {
        let mut work = state.work.borrow_mut();
        let watched = Watched::windowless(&app);
        let events = app.executor.events().clone();
        while let Ok(event) = events.try_recv() {
            work.take(&app, event, watched);
        }
        let under = work.under_way(&app);
        work.card(&under);
        work.sync_stopped(&app);
        work.tell(&app, &under, watched);
        let asks = app.waits_for_answer();
        work.tell_asks(asks, watched);
        let asked = work.take_asked();
        for asked in &asked {
            if matches!(asked, Asked::PauseAll | Asked::GoOn) {
                act(&app, &work, asked);
            }
        }
        let keeps = keeps(&app, &work);
        let presence = keeps.then(|| presence(&work, &under, asks));
        if state.shown != presence {
            work.present(presence.clone());
            state.shown = presence;
        }
        (keeps, asked)
    };
    if asked.iter().any(|asked| matches!(asked, Asked::StopAll)) {
        quit(cx);
        return;
    }
    // The window wanted again: the last token given brings it forward.
    let token = asked.iter().rev().find_map(|asked| match asked {
        Asked::Notice(clicked) => Some(clicked.token.clone()),
        Asked::Open { token } => Some(token.clone()),
        _ => None,
    });
    if let Some(token) = token {
        let opening = Opening {
            location: None,
            cwd: std::path::PathBuf::new(),
            token,
        };
        reopen(cx, Some(opening), true);
    } else if !keeps {
        // A launch handed over just before taking over stopped opens the
        // window rather than end with the program.
        let state = cx.global::<Finishing>();
        stop_taking_over(state);
        let handed = state.openings.lock().pop();
        match handed {
            Some(opening) => reopen(cx, Some(opening), false),
            None => quit(cx),
        }
    }
}

/// The presence's menu: pause what runs, or let it go on.
pub(crate) fn act(app: &skwir_core::App, work: &Work, asked: &Asked) {
    let pause = matches!(asked, Asked::PauseAll);
    for running in &work.running {
        if pause {
            app.executor.pause(running.plan);
        } else {
            app.executor.resume(running.plan);
        }
    }
}

/// What shows that the program is still at work: what runs, as the strip
/// says it, or what keeps it otherwise.
fn presence(work: &Work, under: &Under, asks: bool) -> Presence {
    let summary = summary(under);
    let what = match (under.plans.as_slice(), under.tasks.as_slice()) {
        ([(running, _)], []) => capitalized(&running.title),
        ([], [task]) => capitalized(&task.title),
        ([], []) => String::new(),
        _ => format::count(under.count(), "action"),
    };
    let stopped = work.stopped.as_ref().map(|s| capitalized(&s.title));
    let (standing, about) = if asks {
        (Standing::Asks, "Open Skwir to answer".to_owned())
    } else if let Some(summary) = summary {
        if summary.paused {
            (Standing::Paused, format!("{what}: paused"))
        } else {
            (Standing::Running, what.clone())
        }
    } else if work.waits_for_server() {
        (Standing::Waiting, "Waiting for its server".to_owned())
    } else if let Some(stopped) = &stopped {
        (Standing::Stopped, format!("{stopped}: stopped"))
    } else {
        (Standing::Running, "Finishing".to_owned())
    };
    let title = match standing {
        _ if !under.is_empty() => format!("Finishing {}", format::count(under.count(), "action")),
        Standing::Asks => "An action needs an answer".to_owned(),
        Standing::Waiting => "Waiting for the server".to_owned(),
        Standing::Stopped => "An action stopped".to_owned(),
        _ => "Finishing".to_owned(),
    };
    let said = summary.map_or_else(
        || about.clone(),
        |summary| {
            let [said, _, _] = words(under, &summary);
            said
        },
    );
    Presence {
        title,
        said,
        about,
        fraction: summary.map(|summary| summary.fraction),
        standing,
        pause: summary
            .filter(|summary| !summary.stopping)
            .map(|summary| !summary.paused),
    }
}

/// Open the window again: one asked by the presence or a notice
/// (`activity`: Activity opens with it), a later launch handed over (its
/// folder shown), the Dock clicked. One up already comes forward.
pub(crate) fn reopen(cx: &mut App, opening: Option<Opening>, activity: bool) {
    let state = cx.global::<Finishing>();
    let app = Arc::clone(&state.app);
    let (initial, token) = match opening {
        Some(opening) => (
            opening
                .location
                .map(|arg| crate::location_of(&app, &arg, &opening.cwd)),
            opening.token,
        ),
        None => (None, None),
    };
    if let Some(window) = state.window {
        let _ = window.update(cx, |shell, window, cx| {
            window.activate_window_with_token(token.as_deref());
            if let Some(location) = initial
                && let Err(error) = shell.app.navigate(shell.pane, &location)
            {
                shell.set_status(format!("{error:#}"), true, cx);
            }
        });
        return;
    }
    open(cx, initial, token, activity);
}

/// Open the window, showing `initial` (else the tabs left open), sharing
/// what runs, which the program finished without one. One that cannot
/// open leaves the program as it was, without a window.
fn open(cx: &mut App, initial: Option<Location>, token: Option<String>, activity: bool) {
    let state = cx.global_mut::<Finishing>();
    // A later launch starts its own Skwir again.
    stop_taking_over(state);
    let (app, keymap, started) = (Arc::clone(&state.app), state.keymap.clone(), state.started);
    let work = Rc::clone(&state.work);
    let bounds = gpui::Bounds::centered(None, gpui::size(gpui::px(1200.), gpui::px(800.)), cx);
    let opened = cx.open_window(
        gpui::WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some("skwir".into()),
                ..Default::default()
            }),
            app_id: Some(skwir_core::APP_ID.into()),
            ..Default::default()
        },
        |window, cx| cx.new(|cx| Shell::new(app, initial, started, keymap, work, window, cx)),
    );
    let window = match opened {
        Ok(window) => window,
        // At start, nothing can go on without it.
        Err(error) if !cx.global::<Finishing>().opened => panic!("open window: {error:#}"),
        Err(error) => {
            // Still without one: later launches taken over again, the
            // presence kept, the program ended if nothing keeps it.
            tracing::error!("the window could not open again: {error:#}");
            take_over(cx.global::<Finishing>());
            sync(cx);
            return;
        }
    };
    tracing::debug!(ms = started.elapsed().as_millis() as u64, "window opened");
    let state = cx.global_mut::<Finishing>();
    state.window = Some(window);
    state.opened = true;
    if state.shown.take().is_some() {
        state.work.borrow().present(None);
    }
    let _ = window.update(cx, |shell, window, cx| {
        window.focus(&shell.ui().focus, cx);
        cx.activate(true);
        if token.is_some() {
            window.activate_window_with_token(token.as_deref());
        }
        // Opened for what runs: Activity shows it, once the strip it opens
        // above is laid out.
        if activity {
            cx.on_next_frame(window, |shell, window, cx| {
                if !shell.activity_open() && !shell.activity_tray_shown() {
                    shell.toggle_activity(&serde_json::json!({}), window, cx);
                }
            });
        }
    });
}

/// macOS asks whether the program may quit (Quit from the Dock, logging
/// out, restarting): yes when nothing is under way; else no, and the
/// window, opened again if need be, asks the user as Quit does. A log out
/// is then cancelled, the system saying by which program.
pub(crate) fn may_quit(cx: &mut App) -> bool {
    let Some(state) = cx.try_global::<Finishing>() else {
        return true;
    };
    if state.quitting || state.stopping {
        return true;
    }
    let busy = under_way(&state.app);
    if !busy {
        cx.global_mut::<Finishing>().quitting = true;
        return true;
    }
    if cx.global::<Finishing>().window.is_none() {
        open(cx, None, None, false);
    }
    if let Some(window) = cx.global::<Finishing>().window {
        let _ = window.update(cx, |shell, window, cx| {
            window.activate_window();
            shell.quit(window, cx);
        });
    }
    false
}

/// End the program: what the desktop shows of it taken back and the last
/// notice told, then gpui's quit, whose handler stops what still runs
/// (`lib.rs`, `wind_down`).
fn quit(cx: &mut App) {
    let state = cx.global_mut::<Finishing>();
    state.quitting = true;
    // Aside as well: the thread lets go of the marker as the plans stop,
    // and a bus that does not answer cannot hold the program's end.
    stop_taking_over(state);
    {
        let mut work = state.work.borrow_mut();
        if state.shown.take().is_some() {
            work.present(None);
        }
        work.finish(TOLD_WITHIN);
    }
    let (decoded, skipped, bytes) = state.app.thumbnails.stats();
    tracing::info!(decoded, skipped, cached_bytes = bytes, "thumbnail stats");
    cx.quit();
}

/// SIGTERM (a log out, a shutdown going on anyway), SIGINT and SIGHUP stop
/// what runs and end the program, as Stop and quit does: what a copy was in
/// the middle of is taken back rather than left for the next start to
/// sweep. Asked twice, the program ends at once.
#[cfg(unix)]
fn end_on_signals(app: &skwir_core::App, ending: &Arc<AtomicBool>) {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    let mut signals = match signal_hook::iterator::Signals::new([SIGTERM, SIGINT, SIGHUP]) {
        Ok(signals) => signals,
        Err(error) => {
            tracing::debug!("signals end the program at once: {error}");
            return;
        }
    };
    let (wake, ending) = (app.waker_handle(), Arc::clone(ending));
    let started = std::thread::Builder::new()
        .name("skwir-signals".into())
        .spawn(move || {
            for signal in signals.forever() {
                if ending.swap(true, Ordering::AcqRel) {
                    std::process::exit(128 + signal);
                }
                tracing::info!(signal, "asked to end: stopping what runs");
                wake();
            }
        });
    if let Err(error) = started {
        tracing::debug!("signals end the program at once: {error}");
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use skwir_core::op::{PlanId, RunKind, Transfer};
    use skwir_platform::desktop::Standing;

    use super::presence;
    use crate::activity::{Stopped, Under};
    use crate::toast::Running;
    use crate::work::Work;

    fn copying(plan: u64, title: &str, paused: bool) -> (Running, Transfer) {
        let mut running = Running::for_test(PlanId(plan), title, RunKind::Apply, 3);
        running.paused = paused;
        let transfer = Transfer {
            done: 1 << 30,
            planned: Some(4 << 30),
            left: Some(Duration::from_secs(170)),
            ..Transfer::default()
        };
        (running, transfer)
    }

    /// Without a window, the presence says what runs as the strip does,
    /// and offers what the menu can do with it.
    #[test]
    fn the_presence_says_what_runs_as_the_strip_does() {
        let work = Work::default();
        let one = Under {
            plans: vec![copying(1, "copy 3 items to Backup", false)],
            tasks: Vec::new(),
        };
        let shown = presence(&work, &one, false);
        assert_eq!(shown.title, "Finishing 1 action");
        assert_eq!(shown.about, "Copy 3 items to Backup");
        assert_eq!(
            shown.said,
            "Copy 3 items to Backup · 25 % · about 3 min left"
        );
        assert_eq!(shown.standing, Standing::Running);
        assert_eq!(shown.pause, Some(true), "Pause all");
        assert!((shown.fraction.unwrap() - 0.25).abs() < 1e-6);

        let paused = Under {
            plans: vec![
                copying(1, "copy 3 items to Backup", true),
                copying(2, "copy 1 item to Stick", true),
            ],
            tasks: Vec::new(),
        };
        let shown = presence(&work, &paused, false);
        assert_eq!(shown.title, "Finishing 2 actions");
        assert_eq!(shown.about, "2 actions: paused");
        assert_eq!(shown.standing, Standing::Paused);
        assert_eq!(shown.pause, Some(false), "Go on");
    }

    /// What keeps the program with nothing running says so: a question
    /// first, then an action stopped since the window closed.
    #[test]
    fn the_presence_says_why_it_stays_with_nothing_running() {
        let mut work = Work::default();
        let nothing = Under {
            plans: Vec::new(),
            tasks: Vec::new(),
        };
        work.stopped = Some(Stopped::new(PlanId(5), "copy 3 items to Backup", true));
        let shown = presence(&work, &nothing, false);
        assert_eq!(shown.standing, Standing::Stopped);
        assert_eq!(shown.title, "An action stopped");
        assert_eq!(shown.about, "Copy 3 items to Backup: stopped");
        assert_eq!(shown.fraction, None);
        assert_eq!(shown.pause, None, "nothing to pause");

        let shown = presence(&work, &nothing, true);
        assert_eq!(shown.standing, Standing::Asks);
        assert_eq!(shown.title, "An action needs an answer");
        assert_eq!(shown.about, "Open Skwir to answer");
    }
}
