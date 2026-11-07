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
//! The program takes later launches over for its whole life (ADR-122), not
//! only while it finishes: with windows up, a launch opens a window on what
//! it asks (or a tab, `settings::LAUNCH_OPENS_KEY`), as one program's.
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

use std::collections::HashMap;

use gpui::{App, AppContext, Global, WindowHandle};
use parking_lot::Mutex;
use skwir_core::Location;
use skwir_core::op::PlanId;
use skwir_core::workspace::{PaneId, WindowId};
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
    /// The windows up, each the core's window it draws (ADR-122), in the
    /// order they opened: a window leaves the list the moment it leaves.
    windows: Vec<(WindowId, WindowHandle<Shell>)>,
    /// The core's active window as the program last brought it forward:
    /// one a command made active since is brought forward.
    raised: Option<WindowId>,
    /// The window each plan it ran was asked from: what the plan says
    /// goes there (its toast, what it made), else to the active window.
    plans: HashMap<PlanId, WindowId>,
    /// The window that started playing last, while it is up: one plays at
    /// a time, what another played paused (`claim_player`).
    pub(crate) player: Option<WindowId>,
    /// What one window carries to another of a pane moved there with its
    /// tab: taken by the window that shows it next (`Shell::sync_tabs`).
    pub(crate) carried: HashMap<PaneId, crate::shell::Carried>,
    /// When the windows were last recorded for the next start, and whether
    /// a record waits for a timer (`record_windows`).
    recording: Recording,
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
    openings: Openings,
    /// A signal asked the program to end.
    ending: Arc<AtomicBool>,
    /// The presence shown, while no window is up.
    shown: Option<Presence>,
    /// A window was opened once: one that cannot open after is no reason
    /// to end.
    opened: bool,
}

impl Global for Finishing {}

/// Start the program's life: what it reads once for every window, its
/// state, the loop taking the core's wakes, what ends it, then a window
/// for each of the core's (the windows left, restored, else one), the
/// active pane showing `initial`.
pub(crate) fn start(
    cx: &mut App,
    app: Arc<skwir_core::App>,
    keymap: keymap::Keymap,
    started: Instant,
    wakes: flume::Receiver<()>,
    openings: Openings,
    initial: Option<Location>,
) {
    // The program ends when nothing keeps it (`closed`), not with its last
    // window.
    cx.set_quit_mode(gpui::QuitMode::Explicit);
    let ending = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    end_on_signals(&app, &ending);
    let work = Rc::new(RefCell::new(Work::new(&app)));
    prepare(&app, cx);
    cx.set_global(Finishing {
        app,
        keymap,
        started,
        windows: Vec::new(),
        raised: None,
        plans: HashMap::new(),
        player: None,
        carried: HashMap::new(),
        recording: Recording::default(),
        work,
        stopping: false,
        quitting: false,
        taking: Arc::default(),
        round: Arc::default(),
        openings,
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
    let state = cx.global::<Finishing>();
    // The tab left stays drawn until the tab shown is ready (`tabs.rs`).
    state.app.hold_tabs(true);
    show(cx, initial, None, false);
    // Later launches of this Skwir (ADR-122) come here, for its life.
    take_over(cx.global::<Finishing>());
}

/// What the program reads once, whatever windows it opens: the settings the
/// window declares, the themes and fonts offered, owner names.
fn prepare(app: &Arc<skwir_core::App>, cx: &mut App) {
    crate::settings::declare(app);
    // The themes the user added to bat and the fonts installed, on a
    // worker: then a script or an agent can choose one before the
    // settings ever open.
    skwir_platform::highlight::added::keep_in(app.cache.namespace("grammars"));
    // The WebAssembly grammars the tree-sitter helper compiles, kept in the
    // cache rather than compiled at each start (ADR-110).
    skwir_platform::tree_sitter::keep_compiled_in(
        app.cache.namespace("wasmtime").folder("tree-sitter"),
    );
    crate::settings::list_code_themes(app);
    crate::settings::list_fonts(app, cx.text_system().clone());
    crate::session::declare(app);
    // Owner and group names for Properties: two small files, but a read can
    // block, so not here.
    let wake = app.waker_handle();
    app.scheduler.spawn(
        skwir_core::scheduler::Priority::Background,
        "user-names",
        app.lifetime().child(),
        {
            let cache = app.cache.root().to_owned();
            move || {
                skwir_platform::users::load();
                // Members a killed process extracted for its previews.
                crate::preview::sweep_members(&cache);
                wake();
            }
        },
    );
}

/// Show the program's windows, none being up: one for each of the core's
/// (those left, restored at start or asleep since the last closed), the
/// active one last so that it has the focus; the active pane showing
/// `initial`; the tabs each shows opened. `token` brings the window
/// forward; `activity` opens Activity in it.
fn show(cx: &mut App, initial: Option<Location>, token: Option<String>, activity: bool) {
    let state = cx.global::<Finishing>();
    let app = Arc::clone(&state.app);
    let (windows, active) = {
        let ws = app.workspace.read();
        let ids: Vec<WindowId> = ws.windows().iter().map(|w| w.id).collect();
        (ids, ws.active_window_id())
    };
    // A folder asked for is shown alone, as the window last left was split.
    let split = initial.is_some();
    let mut order: Vec<WindowId> = windows.into_iter().filter(|w| *w != active).collect();
    order.push(active);
    let mut any = false;
    for id in order {
        let mine = id == active;
        any |= open_window(
            cx,
            id,
            split && mine,
            token.clone().filter(|_| mine),
            activity && mine,
        );
    }
    if !any {
        // Still without one: the presence kept, the program ended if
        // nothing keeps it.
        if cx.global::<Finishing>().opened {
            sync(cx);
        }
        return;
    }
    if let Some(location) = initial {
        let pane = app.workspace.read().active_id();
        if let Err(error) = app.navigate(pane, &location) {
            say_in_active(cx, format!("{error:#}"));
        }
    }
    // Tabs restored, or asleep while no window was up, open the one each
    // window shows, now that the listings have a window to wake.
    if let Err(error) = app.resume_tabs() {
        say_in_active(cx, format!("{error:#}"));
    }
    // Scripts and agents reach the windows from now on: what they ask
    // comes after what they start on.
    app.set_window_shown();
    cx.global_mut::<Finishing>().raised = Some(active);
}

/// Say `text` as an error in the active window's status.
fn say_in_active(cx: &mut App, text: String) {
    if let Some(window) = active_handle(cx) {
        let _ = window.update(cx, |shell, _, cx| shell.set_status(text, true, cx));
    }
}

/// The window the core says is active, else the one opened last.
pub(crate) fn active_handle(cx: &App) -> Option<WindowHandle<Shell>> {
    let state = cx.global::<Finishing>();
    let active = state.app.workspace.read().active_window_id();
    state
        .windows
        .iter()
        .find(|(id, _)| *id == active)
        .or(state.windows.last())
        .map(|(_, handle)| *handle)
}

/// How many windows are up.
pub(crate) fn window_count(cx: &App) -> usize {
    cx.try_global::<Finishing>()
        .map_or(0, |state| state.windows.len())
}

/// The windows up other than `id`.
pub(crate) fn others(cx: &App, id: WindowId) -> Vec<WindowHandle<Shell>> {
    cx.global::<Finishing>()
        .windows
        .iter()
        .filter(|(other, _)| *other != id)
        .map(|(_, handle)| *handle)
        .collect()
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
    // What woke it, taken before anything is applied: a wake that comes
    // while the windows sync is for the next pass.
    let app = Arc::clone(&state.app);
    let woken = app.take_woken();
    let openings = std::mem::take(&mut *state.openings.lock());
    for opening in openings {
        reopen(cx, Some(opening), false);
    }
    let state = cx.global::<Finishing>();
    if state.windows.is_empty() {
        // No window since one was up: the program takes it in.
        if state.opened {
            sync(cx);
        }
        return false;
    }
    follow_windows(cx);
    deliver(cx);
    let windows: Vec<(WindowId, WindowHandle<Shell>)> = cx.global::<Finishing>().windows.clone();
    // What a pane moved to another window carries there, given before any
    // window follows the tabs, taken by the one that shows it.
    for (_, window) in &windows {
        let _ = window.update(cx, |shell, _, cx| shell.give_away_moved(cx));
    }
    // Every window takes in what the core sent; the windows it concerns
    // draw (ADR-122): all of them for a wake that says nothing, the window
    // a wake names, and the windows of the panes that woke or whose
    // listings moved, wherever their tabs are now. A film playing in one
    // draws that one alone.
    let mut backlog = false;
    let mut panes = woken.panes;
    for (_, window) in &windows {
        let _ = window.update(cx, |shell, window, cx| {
            shell.sync(window, cx);
            backlog |= shell.backlog();
            panes.extend(shell.take_pumped());
        });
    }
    let concerned: std::collections::HashSet<WindowId> = {
        let ws = app.workspace.read();
        panes
            .iter()
            .filter_map(|pane| ws.window_of(*pane))
            .chain(woken.windows.iter().copied())
            .collect()
    };
    for (id, window) in windows {
        if woken.all || concerned.contains(&id) {
            let _ = window.update(cx, |_, _, cx| cx.notify());
        }
    }
    cx.global_mut::<Finishing>().carried.clear();
    record_windows(cx);
    backlog
}

/// What panes moved to another window carry there (`Shell::give_away_moved`).
pub(crate) fn carry(cx: &mut App, moved: Vec<(PaneId, crate::shell::Carried)>) {
    if !moved.is_empty() && cx.has_global::<Finishing>() {
        cx.global_mut::<Finishing>().carried.extend(moved);
    }
}

/// What of `panes` came from another window carries (`carry`).
pub(crate) fn take_carried(
    cx: &mut App,
    panes: &[PaneId],
) -> HashMap<PaneId, crate::shell::Carried> {
    if !cx.has_global::<Finishing>() {
        return HashMap::new();
    }
    let carried = &mut cx.global_mut::<Finishing>().carried;
    panes
        .iter()
        .filter_map(|id| carried.remove(id).map(|c| (*id, c)))
        .collect()
}

/// The windows follow the core's (ADR-122): one it made (`window.new`, a
/// tab moved out, a window reopened) opens; one it closed (`window.close`,
/// its last tab closed) goes; one a command made active comes forward.
fn follow_windows(cx: &mut App) {
    let state = cx.global::<Finishing>();
    let (core, active) = {
        let ws = state.app.workspace.read();
        let ids: Vec<WindowId> = ws.windows().iter().map(|w| w.id).collect();
        (ids, ws.active_window_id())
    };
    let gone: Vec<WindowHandle<Shell>> = state
        .windows
        .iter()
        .filter(|(id, _)| !core.contains(id))
        .map(|(_, handle)| *handle)
        .collect();
    let new: Vec<WindowId> = core
        .iter()
        .copied()
        .filter(|id| !state.windows.iter().any(|(up, _)| up == id))
        .collect();
    for window in gone {
        let _ = window.update(cx, |shell, window, cx| {
            shell.leave(window, cx);
            window.remove_window();
        });
    }
    for id in new {
        open_window(cx, id, false, None, false);
    }
    let state = cx.global::<Finishing>();
    if state.raised != Some(active)
        && let Some((_, window)) = state.windows.iter().find(|(id, _)| *id == active)
    {
        let window = *window;
        cx.global_mut::<Finishing>().raised = Some(active);
        let _ = window.update(cx, |_, window, _| window.activate_window());
    }
}

/// The launches handed over (and, on macOS, what the Finder opens), for
/// the next wake.
pub(crate) type Openings = Arc<Mutex<Vec<Opening>>>;

/// What a window takes in at its next sync, the program having taken it
/// from the core (`deliver`): each window what its own asked, the active
/// one the rest.
#[derive(Default)]
pub(crate) struct Inbox {
    /// What the plans it ran did (`Work::take`).
    pub(crate) taken: Vec<crate::work::Taken>,
    pub(crate) notifications: Vec<skwir_core::app::Notification>,
    /// Its commands asked from elsewhere (an agent, a script).
    pub(crate) front: Vec<skwir_core::command::Invocation>,
    pub(crate) proposals: Vec<skwir_core::op::Plan>,
    pub(crate) questions: Vec<skwir_core::app::Question>,
    pub(crate) clashes: Vec<skwir_core::app::ClashQuestion>,
    pub(crate) secrets: Vec<skwir_core::app::SecretQuestion>,
    pub(crate) forms: Vec<skwir_core::form::Asked>,
}

/// Take in what the core sent and asked, once for the program, and hand
/// each window its part: what a plan did to the window that ran it (else
/// the active one), a window's command to the window of the pane it names,
/// the rest to the active window. Then tell the desktop, and do what it
/// asked.
fn deliver(cx: &mut App) {
    let state = cx.global_mut::<Finishing>();
    let app = Arc::clone(&state.app);
    let active = app.workspace.read().active_window_id();
    let mut inboxes: HashMap<WindowId, Inbox> = HashMap::new();
    let watched = watched(cx);
    let state = cx.global_mut::<Finishing>();
    let shared = Rc::clone(&state.work);
    let (asked, wake) = {
        let mut work = state.work.borrow_mut();
        let events = app.executor.events().clone();
        while let Ok(event) = events.try_recv() {
            let taken = work.take(&app, event, watched);
            let plan = match &taken {
                crate::work::Taken::Nothing => continue,
                crate::work::Taken::Step(progress) => progress.plan,
                crate::work::Taken::WaitsForServer(plan)
                | crate::work::Taken::Ended { plan, .. } => *plan,
            };
            let window = if matches!(taken, crate::work::Taken::Ended { .. }) {
                state.plans.remove(&plan)
            } else {
                state.plans.get(&plan).copied()
            };
            let window = window
                .filter(|w| state.windows.iter().any(|(up, _)| up == w))
                .unwrap_or(active);
            inboxes.entry(window).or_default().taken.push(taken);
        }
        work.sync_stopped(&app);
        work.settle(&app);
        // What joined is shown and announced as its time comes, the
        // program woken then (ADR-114, amended 2026-11-03).
        let wake = work.watch(&app, Instant::now());
        let under = work.under_way(&app);
        work.tell(&app, &under, watched);
        (work.take_asked(), wake)
    };
    if let Some(at) = wake {
        crate::work::wake_after(&shared, &app, at, cx);
    }
    let inbox = inboxes.entry(active).or_default();
    inbox.notifications = app.take_notifications();
    inbox.proposals = app.take_proposals();
    inbox.questions = app.take_questions();
    inbox.clashes = app.take_clash_questions();
    inbox.secrets = app.take_secret_questions();
    inbox.forms = app.take_forms();
    for request in app.take_front_requests() {
        // Asked for a pane: the window that shows it.
        let window = request
            .args
            .get("pane")
            .and_then(|pane| serde_json::from_value::<PaneId>(pane.clone()).ok())
            .and_then(|pane| app.workspace.read().window_of(pane))
            .unwrap_or(active);
        inboxes.entry(window).or_default().front.push(request);
    }
    let windows = cx.global::<Finishing>().windows.clone();
    for (id, window) in windows {
        if let Some(inbox) = inboxes.remove(&id) {
            let _ = window.update(cx, |shell, _, _| shell.inbox.take_in(inbox));
        }
    }
    answer_desktop(cx, asked);
}

impl Inbox {
    /// Add what `more` holds after what this holds.
    pub(crate) fn take_in(&mut self, more: Inbox) {
        self.taken.extend(more.taken);
        self.notifications.extend(more.notifications);
        self.front.extend(more.front);
        self.proposals.extend(more.proposals);
        self.questions.extend(more.questions);
        self.clashes.extend(more.clashes);
        self.secrets.extend(more.secrets);
        self.forms.extend(more.forms);
    }
}

/// How the desktop is told while windows are up: looked at while one of
/// them has the focus.
fn watched(cx: &App) -> Watched {
    let state = cx.global::<Finishing>();
    let looked_at = state.windows.iter().any(|(_, handle)| {
        handle
            .read(cx)
            .is_ok_and(crate::shell::Shell::is_window_active)
    });
    let switch = |key: &str| state.app.settings.get::<bool>(key).unwrap_or(true);
    Watched {
        seen: if looked_at {
            crate::work::Seen::LookedAt
        } else {
            crate::work::Seen::Away
        },
        notify: switch(crate::settings::NOTIFY_DESKTOP_KEY),
        keep_awake: switch(crate::settings::KEEP_AWAKE_KEY),
    }
}

/// What the desktop asked while windows are up: a notice or the presence
/// clicked brings the active window forward, with the token the desktop
/// gave where it gave one, and opens Activity there; its menu pauses, lets
/// go on or stops what runs.
fn answer_desktop(cx: &mut App, asked: Vec<Asked>) {
    let mut token = None;
    let mut shown = false;
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
                let state = cx.global::<Finishing>();
                act(&state.app, &state.work.borrow(), &asked);
            }
            Asked::StopAll => {
                if let Some(window) = active_handle(cx) {
                    let _ = window.update(cx, Shell::stop_and_quit);
                }
                return;
            }
        }
    }
    if shown && let Some(window) = active_handle(cx) {
        let _ = window.update(cx, |shell, window, cx| {
            window.activate_window_with_token(token.as_deref());
            if !shell.activity_open() && !shell.activity_tray_shown() {
                shell.toggle_activity(&serde_json::json!({}), window, cx);
            }
        });
    }
}

/// When the windows were recorded last (`record_windows`).
#[derive(Default)]
struct Recording {
    at: Option<Instant>,
    /// A timer records them again once `RECORD_EVERY` passed.
    timer: bool,
}

/// How long the windows wait to be recorded again for the next start: as
/// they change, never more often.
const RECORD_EVERY: Duration = Duration::from_secs(2);

/// Record every window's tabs and shape for the next start, as they
/// change (ADR-122): Quit keeps every window, a window closed alone is
/// forgotten, a crash loses none. At most every `RECORD_EVERY`: a change
/// within it is recorded when it ends, by a timer. Each wake asks, and
/// what changes with no wake (a window moved) asks too.
pub(crate) fn record_windows(cx: &mut App) {
    let state = cx.global_mut::<Finishing>();
    let now = Instant::now();
    if let Some(at) = state.recording.at
        && now.duration_since(at) < RECORD_EVERY
    {
        if !state.recording.timer {
            state.recording.timer = true;
            let wait = RECORD_EVERY.saturating_sub(now.duration_since(at));
            cx.spawn(async move |cx| {
                cx.background_executor().timer(wait).await;
                cx.update(|cx| {
                    cx.global_mut::<Finishing>().recording.timer = false;
                    record_windows(cx);
                });
            })
            .detach();
        }
        return;
    }
    state.recording.at = Some(now);
    let app = Arc::clone(&state.app);
    if skwir_core::tabs::record(&app) {
        let weak = app.weak();
        app.scheduler.spawn(
            skwir_core::scheduler::Priority::Background,
            "ui-windows-save",
            app.lifetime().child(),
            move || {
                if let Some(app) = weak.upgrade()
                    && let Err(error) = app.settings.save()
                {
                    tracing::warn!("cannot remember the windows: {error:#}");
                }
            },
        );
    }
}

/// Window `id` started playing: the player is its own, and what another
/// window plays pauses where it is (one plays in the program, ADR-122).
pub(crate) fn claim_player(cx: &mut App, id: WindowId) {
    if !cx.has_global::<Finishing>() {
        return;
    }
    let state = cx.global_mut::<Finishing>();
    let before = state.player.replace(id);
    let Some(before) = before.filter(|before| *before != id) else {
        return;
    };
    let Some(window) = state
        .windows
        .iter()
        .find(|(up, _)| *up == before)
        .map(|(_, handle)| *handle)
    else {
        return;
    };
    // Not within the window that claims: the other is updated after.
    cx.defer(move |cx| {
        let _ = window.update(cx, |shell, _, cx| shell.pause_playing(cx));
    });
}

/// Whether window `id` takes the desktop's requests to the player and
/// tells it what plays: the window that played last, else the one in use.
pub(crate) fn steers_player(cx: &App, id: WindowId) -> bool {
    let Some(state) = cx.try_global::<Finishing>() else {
        return true;
    };
    match state.player {
        Some(playing) if state.windows.iter().any(|(up, _)| *up == playing) => playing == id,
        _ => state.app.workspace.read().active_window_id() == id,
    }
}

/// The window other than `id` whose Settings dialog is open, if one is:
/// there is one dialog in the program.
pub(crate) fn settings_elsewhere(cx: &App, id: WindowId) -> Option<WindowHandle<Shell>> {
    let state = cx.try_global::<Finishing>()?;
    state
        .windows
        .iter()
        .filter(|(up, _)| *up != id)
        .map(|(_, handle)| *handle)
        .find(|handle| handle.read(cx).is_ok_and(Shell::settings_open))
}

/// Window `id` came forward by the user's hand: the program has nothing
/// more to bring forward.
pub(crate) fn raised(cx: &mut App, id: WindowId) {
    if cx.has_global::<Finishing>() {
        cx.global_mut::<Finishing>().raised = Some(id);
    }
}

/// A plan the window `window` asked for: what it does goes there.
pub(crate) fn ran(cx: &mut App, plan: PlanId, window: WindowId) {
    if cx.has_global::<Finishing>() {
        cx.global_mut::<Finishing>().plans.insert(plan, window);
    }
}

/// A signal asked the program to end: what runs stops, as Stop and quit
/// does, the windows (if up) closing first, as they do then: none is left
/// frozen on screen while plans take back what they were in the middle
/// of, and their tabs are remembered (`closed`).
fn end(cx: &mut App) {
    let closed =
        active_handle(cx).is_some_and(|window| window.update(cx, Shell::stop_and_quit).is_ok());
    if !closed {
        quit(cx);
    }
}

/// Window `id` is about to close (`Shell::leave`): from now on its shell
/// takes nothing more. The last one left, the program takes in what the
/// core sends, an action stopped before let go: the window showed it.
pub(crate) fn left(cx: &mut App, id: WindowId) {
    let state = cx.global_mut::<Finishing>();
    state.windows.retain(|(up, _)| *up != id);
    if state.player == Some(id) {
        state.player = None;
    }
    if state.windows.is_empty() {
        state.work.borrow_mut().stopped = None;
    }
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
    // Gone without leaving (`left`): what they showed is let go all the same.
    if !state.windows.is_empty() {
        state.windows.clear();
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
    let shared = Rc::clone(&state.work);
    let (keeps, asked, wake) = {
        let mut work = state.work.borrow_mut();
        let watched = Watched::windowless(&app);
        let events = app.executor.events().clone();
        while let Ok(event) = events.try_recv() {
            work.take(&app, event, watched);
        }
        work.settle(&app);
        // What joined counts as its time comes, the program woken then
        // (`work::wake_after`); nothing is tinted here.
        let wake = work.watch(&app, std::time::Instant::now());
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
        (keeps, asked, wake)
    };
    if let Some(at) = wake.filter(|_| keeps) {
        crate::work::wake_after(&shared, &app, at, cx);
    }
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
    } else if let Some(summary) = &summary {
        if summary.shares.is_empty() {
            // Nothing runs yet: what is asked waits its turn.
            (Standing::Running, "Waiting its turn".to_owned())
        } else if summary.paused {
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
    let title = match (&summary, standing) {
        // What is left of the spell to finish: what runs and what waits.
        (Some(summary), _) => format!(
            "Finishing {}",
            format::count(summary.of - summary.ended, "action")
        ),
        (None, Standing::Asks) => "An action needs an answer".to_owned(),
        (None, Standing::Waiting) => "Waiting for the server".to_owned(),
        (None, Standing::Stopped) => "An action stopped".to_owned(),
        _ => "Finishing".to_owned(),
    };
    let said = summary.as_ref().map_or_else(
        || about.clone(),
        |summary| {
            let [said, _, _] = words(under, summary);
            said
        },
    );
    Presence {
        title,
        said,
        about,
        fraction: summary
            .as_ref()
            .filter(|summary| summary.has_share())
            .map(|summary| summary.fraction),
        standing,
        // Pause and Go on reach plans: offered while one runs, not stopping.
        pause: summary
            .filter(|summary| !summary.stopping && !under.plans.is_empty())
            .map(|summary| !summary.paused),
    }
}

/// Open the windows again, or one more: asked by the presence or a notice
/// (`activity`: Activity opens with it), the Dock clicked, or a later
/// launch handed over. With no window up, the windows come back, the
/// active pane showing the launch's folder. With windows up, a launch
/// opens a window on its folder (the startup folder when it gave none),
/// or, as `LAUNCH_OPENS_KEY` says, a tab of the window in use, a bare
/// launch then bringing that window forward (ADR-122); anything else
/// brings the window in use forward.
pub(crate) fn reopen(cx: &mut App, opening: Option<Opening>, activity: bool) {
    let state = cx.global::<Finishing>();
    let app = Arc::clone(&state.app);
    let (initial, token, launched) = match opening {
        Some(opening) => (
            opening
                .location
                .map(|arg| crate::location_of(&app, &arg, &opening.cwd)),
            opening.token,
            // A notice clicked, the presence asked: no launch.
            !activity,
        ),
        None => (None, None, false),
    };
    let Some(window) = active_handle(cx) else {
        show(cx, initial, token, activity);
        return;
    };
    let in_tab = app
        .settings
        .get::<String>(crate::settings::LAUNCH_OPENS_KEY)
        .is_some_and(|opens| opens == crate::settings::LAUNCH_IN_TAB);
    if !launched || (in_tab && initial.is_none()) {
        let _ = window.update(cx, |_, window, _| {
            window.activate_window_with_token(token.as_deref());
        });
        return;
    }
    if in_tab && let Some(location) = initial {
        // What another program asks is shown as it is, with no pane's lens.
        let _ = window.update(cx, |shell, window, cx| {
            let pane = shell.window_pane();
            if let Err(error) = shell.app.new_tab_as_kept(Some(pane), location, None, true) {
                shell.set_status(format!("{error:#}"), true, cx);
            }
            window.activate_window_with_token(token.as_deref());
        });
        return;
    }
    let location = initial.unwrap_or_else(|| {
        let home = skwir_core::os_path::home().unwrap_or_else(|| std::path::PathBuf::from("/"));
        Location::from_path(skwir_core::settings::startup_folder(&app.settings, &home))
    });
    match app.open_window(location) {
        Ok((id, _, _)) => {
            open_window(cx, id, false, token, false);
            cx.global_mut::<Finishing>().raised = Some(id);
        }
        Err(error) => say_in_active(cx, format!("{error:#}")),
    }
}

/// How far a window opened beside another is put from it, where the
/// system lets a window say where it goes.
const CASCADE: f32 = 32.;

/// Open a window drawing the core's window `id`, shaped and placed as it
/// was left (restored, reopened), else as the window it was made from,
/// else as the window used last (`Placement::opening`); `split` splits it
/// as that was. What runs is shared: the program finished it without a
/// window. Whether it opened.
fn open_window(
    cx: &mut App,
    id: WindowId,
    split: bool,
    token: Option<String>,
    activity: bool,
) -> bool {
    let state = cx.global::<Finishing>();
    let (app, keymap) = (Arc::clone(&state.app), state.keymap.clone());
    // The first window is timed from the process's start, one opened again
    // from now: the program may have run for hours without one.
    let first_frame = if state.opened {
        crate::shell::FirstFrame {
            from: Instant::now(),
            after: "opening",
        }
    } else {
        crate::shell::FirstFrame {
            from: state.started,
            after: "start",
        }
    };
    let work = Rc::clone(&state.work);
    let (own, from) = {
        let ws = app.workspace.read();
        let window = ws.window(id);
        (
            window.and_then(|w| w.shape().cloned()),
            window.and_then(skwir_core::workspace::WindowTabs::made_from),
        )
    };
    let source = from.and_then(|from| {
        state
            .windows
            .iter()
            .find(|(up, _)| *up == from)
            .map(|(_, handle)| *handle)
    });
    // Its own shape, where it was (restored, reopened; a step from the
    // others when it never said where); else the window it comes from, set
    // a step from it; else the window used last, a step from it while
    // windows are up, where it was when none is.
    let up = !state.windows.is_empty();
    let (shape, cascade) = match own.and_then(crate::session::from_value) {
        Some(own) => {
            let placed = own
                .window
                .as_ref()
                .is_some_and(|window| window.at.is_some());
            (Some(own), up && !placed)
        }
        None => match source.and_then(|h| h.read(cx).ok().map(Shell::session)) {
            Some(source) => (Some(source), true),
            None => (crate::session::restore(&app), up),
        },
    };
    let placement = shape
        .as_ref()
        .and_then(|shape| shape.window.clone())
        .unwrap_or(crate::session::Placement {
            width: 1200.,
            height: 800.,
            maximized: false,
            at: None,
        });
    let (displays, primary) = crate::session::Screen::all(cx);
    let opening = placement.opening(
        if cascade { CASCADE } else { 0. },
        &displays,
        primary.as_ref(),
    );
    let start = crate::shell::Start {
        shape,
        split,
        first_frame,
        placed: opening.asked,
    };
    let opened = cx.open_window(
        gpui::WindowOptions {
            window_bounds: Some(opening.bounds),
            display_id: opening.display,
            titlebar: Some(gpui::TitlebarOptions {
                title: Some("skwir".into()),
                ..Default::default()
            }),
            app_id: Some(skwir_core::APP_ID.into()),
            ..Default::default()
        },
        |window, cx| cx.new(|cx| Shell::new(app, id, start, keymap, work, window, cx)),
    );
    let window = match opened {
        Ok(window) => window,
        // At start, nothing can go on without it.
        Err(error) if !cx.global::<Finishing>().opened => panic!("open window: {error:#}"),
        Err(error) => {
            tracing::error!("a window could not open: {error:#}");
            return false;
        }
    };
    tracing::debug!(
        ms = first_frame.from.elapsed().as_millis() as u64,
        after = first_frame.after,
        "window opened"
    );
    let state = cx.global_mut::<Finishing>();
    state.windows.push((id, window));
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
    true
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
    if cx.global::<Finishing>().windows.is_empty() {
        show(cx, None, None, false);
    }
    if let Some(window) = active_handle(cx) {
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
            ended: 0,
            waiting: 0,
            next: None,
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
            ended: 0,
            waiting: 0,
            next: None,
        };
        let shown = presence(&work, &paused, false);
        assert_eq!(shown.title, "Finishing 2 actions");
        assert_eq!(shown.about, "2 actions: paused");
        assert_eq!(shown.standing, Standing::Paused);
        assert_eq!(shown.pause, Some(false), "Go on");
    }

    /// While only something waits its turn, the presence says so, draws no
    /// share until something of the spell ended, and offers no Pause:
    /// there is nothing to pause.
    #[test]
    fn the_presence_says_what_waits_with_nothing_running() {
        let work = Work::default();
        let first = Under {
            plans: Vec::new(),
            tasks: Vec::new(),
            ended: 0,
            waiting: 2,
            next: None,
        };
        let shown = presence(&work, &first, false);
        assert_eq!(shown.title, "Finishing 2 actions");
        assert_eq!(shown.about, "Waiting its turn");
        assert_eq!(shown.said, "2 waiting");
        assert_eq!(shown.standing, Standing::Running);
        assert_eq!(shown.fraction, None, "no share yet");
        assert_eq!(shown.pause, None, "nothing to pause");

        let between = Under {
            ended: 1,
            waiting: 1,
            next: None,
            ..first
        };
        let shown = presence(&work, &between, false);
        assert_eq!(shown.title, "Finishing 1 action");
        assert_eq!(shown.said, "1 of 2 · 50 % · 1 waiting");
        assert!((shown.fraction.unwrap() - 0.5).abs() < 1e-6);
        assert_eq!(shown.pause, None);
    }

    /// What keeps the program with nothing running says so: a question
    /// first, then an action stopped since the window closed.
    #[test]
    fn the_presence_says_why_it_stays_with_nothing_running() {
        let mut work = Work::default();
        let nothing = Under {
            plans: Vec::new(),
            tasks: Vec::new(),
            ended: 0,
            waiting: 0,
            next: None,
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
