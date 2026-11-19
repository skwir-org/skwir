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

use std::collections::{HashMap, HashSet};

use gpui::{App, AppContext, Global, WindowHandle};
use parking_lot::Mutex;
use skwir_core::Location;
use skwir_core::op::PlanId;
use skwir_core::workspace::{PaneId, WindowId, WindowTabs};
use skwir_platform::chooser::{Answer, Choosing};
use skwir_platform::desktop::{Asked, Presence, Standing};
use skwir_platform::handover::{NotTaken, Opening, TakingOver};

use skwir_core::words::Words;

use crate::activity::{Under, summary, words};
use crate::keymap;
use crate::shell::Shell;
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
    /// This launch lost the race to take launches over, and was handed
    /// over to the one that won (`take_over`): set by its worker.
    handed_away: Arc<AtomicBool>,
    /// What follows the user's choice of who shows files for other
    /// programs (ADR-140), while launches are taken over: set by the
    /// worker that took them over, for its round, let go with them.
    following: Arc<Mutex<Option<skwir_platform::file_manager::Following>>>,
    /// Started by the session bus to show files for another program,
    /// without a window (`--file-manager`, ADR-140): set, by a worker,
    /// once nothing is to come (asked nothing within `ASKED_WITHIN`, or
    /// skwir no longer the default for folders); the program then ends at
    /// the next wake unless a window opened or something was asked.
    windowless: Option<Arc<AtomicBool>>,
    /// The launches handed over, for the next wake.
    openings: Openings,
    /// Other programs' dialogs handed in (ADR-141), for the next wake.
    choosings: Arc<Mutex<Vec<Choosing>>>,
    /// The windows of the core asleep: those that were as a dialog's
    /// window opened with none of the user's own up, and one kept as it
    /// left (`left`, the last of the user's own): they stay asleep until
    /// one is asked (`show`), as while no window is up.
    asleep: HashSet<WindowId>,
    /// What a window that closed held and no other has taken yet: the next
    /// `deliver` hands it on (`hand_on`).
    handed: Inbox,
    /// A signal asked the program to end.
    ending: Arc<AtomicBool>,
    /// The presence shown, while no window is up.
    shown: Option<Presence>,
    /// What windows were opened in this run.
    opened: Opened,
}

/// What windows were opened in a run: one was (one that cannot open
/// after is no reason to end), and whether one of the user's own was:
/// until then nothing is recorded for the next start, which a program
/// started for another program's dialog would otherwise replace with its
/// window asleep (ADR-141).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Opened {
    Never,
    /// Only windows answering other programs' dialogs.
    Dialogs,
    Own,
}

impl Global for Finishing {}

/// What this launch opens (`start`): `initial`, in the active pane, and
/// `asked`, what it was asked to open, should another Skwir of this
/// instance prove to hold later launches (`take_over`).
pub(crate) struct Launch {
    pub(crate) initial: Option<Location>,
    pub(crate) asked: Option<Opening>,
    /// Started by the session bus without a window, and why.
    pub(crate) windowless: Option<Windowless>,
}

/// Why the session bus started the program without a window: a request
/// opens its window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Windowless {
    /// To show files for another program (`--file-manager`, ADR-140, 5).
    FileManager,
    /// To answer another program's dialog (`--portal`, ADR-141, 3).
    Portal,
}

/// Set as a program started without a window ends asked nothing
/// (`ended_unasked`), until a window opens.
static ENDED_UNASKED: AtomicBool = AtomicBool::new(false);

/// Whether this program, started by the session bus without a window
/// (`--file-manager`), ended with none opened: the binary then ends with
/// a status that is not 0, which dbus-daemon would read as a program gone
/// to the background and wait for, the asking program with it (ADR-140,
/// 5). Read once the UI has returned.
pub fn ended_unasked() -> bool {
    ENDED_UNASKED.load(Ordering::Acquire)
}

/// How long a program the session bus started without a window waits for
/// the request it was started for (ADR-140, 5): the bus holds it until
/// the name is taken, so it comes within moments; past this, the program
/// ends.
const ASKED_WITHIN: std::time::Duration = std::time::Duration::from_secs(30);

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
    launch: Launch,
) {
    let Launch {
        initial,
        asked,
        windowless,
    } = launch;
    // The program ends when nothing keeps it (`closed`), not with its last
    // window.
    cx.set_quit_mode(gpui::QuitMode::Explicit);
    let ending = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    end_on_signals(&app, &ending);
    let work = Rc::new(RefCell::new(Work::new(&app)));
    let prepared = Arc::clone(&app);
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
        handed_away: Arc::default(),
        following: Arc::default(),
        windowless: windowless.is_some().then(Arc::default),
        openings,
        choosings: Arc::default(),
        asleep: HashSet::new(),
        handed: Inbox::default(),
        ending,
        shown: None,
        opened: Opened::Never,
    });
    // Later launches of this Skwir (ADR-122) come here, for its life. Asked
    // first: a launch that started with this one finds this one's marker
    // sooner, and one that lost to it finds out before it shows anything
    // (`handed_away`).
    take_over(cx.global::<Finishing>(), asked, windowless);
    prepare(&prepared, cx);
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            closed(cx);
        }
    })
    .detach();
    listen(cx, wakes);
    if windowless.is_some() {
        // The request opens the window (`woken`); one that does not come
        // ends the program.
        end_unless_asked(cx.global::<Finishing>());
    } else if !handed_away(cx) {
        show(cx, initial, None, false);
    }
}

/// Started without a window: once `ASKED_WITHIN` has passed, the program
/// ends at the next wake unless a request came (`woken`).
fn end_unless_asked(state: &Finishing) {
    let Some(over) = state.windowless.clone() else {
        return;
    };
    let wake = state.app.waker_handle();
    let started = std::thread::Builder::new()
        .name("skwir-asked-within".into())
        .spawn(move || {
            std::thread::sleep(ASKED_WITHIN);
            over.store(true, Ordering::Release);
            wake();
        });
    if let Err(error) = started {
        tracing::debug!("a program started without a window waits for good: {error}");
    }
}

/// This launch was handed over to another Skwir of this instance
/// (`take_over`), which opens what it was asked: it ends, as a launch that
/// found that Skwir at once would have, rather than stay a second program
/// (ADR-122). Whether it does.
fn handed_away(cx: &mut App) -> bool {
    let state = cx.global::<Finishing>();
    if !state.handed_away.swap(false, Ordering::AcqRel) {
        return false;
    }
    if under_way(&state.app) {
        tracing::info!("another Skwir took this launch over, and this one is at work: it stays");
        return false;
    }
    tracing::info!("another Skwir took this launch over");
    quit(cx);
    true
}

/// What the program reads once, whatever windows it opens: the settings the
/// window declares, the themes and fonts offered, owner names.
fn prepare(app: &Arc<skwir_core::App>, cx: &mut App) {
    crate::settings::declare(app);
    // A translation Automatic speaks before it is reviewed, said once: what
    // was told is a setting of the state file, declared just above.
    crate::drafted::tell(app);
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
/// The active pane, when it was taken to `initial`.
fn show(
    cx: &mut App,
    initial: Option<Location>,
    token: Option<String>,
    activity: bool,
) -> Option<PaneId> {
    let state = cx.global_mut::<Finishing>();
    let app = Arc::clone(&state.app);
    // What slept while dialogs' windows alone were up wakes now; those
    // that are up stay as they are.
    state.asleep.clear();
    let up: HashSet<WindowId> = state.windows.iter().map(|(id, _)| *id).collect();
    let (windows, active) = {
        let ws = app.workspace.read();
        let ids: Vec<WindowId> = ws
            .windows()
            .iter()
            .filter(|w| !w.chooses() && !up.contains(&w.id))
            .map(|w| w.id)
            .collect();
        let active = ws.active_window_id();
        let active = if ids.contains(&active) {
            active
        } else {
            ids.last().copied().unwrap_or(active)
        };
        (ids, active)
    };
    // A folder asked for is shown alone, as the window last left was split.
    let split = initial.is_some();
    let mut order: Vec<WindowId> = windows.iter().copied().filter(|w| *w != active).collect();
    if windows.contains(&active) {
        order.push(active);
    }
    let mut any = false;
    for id in order {
        let mine = id == active;
        any |= open_window(
            cx,
            id,
            split && mine,
            token.clone().filter(|_| mine),
            activity && mine,
            None,
        );
    }
    if !any {
        // Still without one: the presence kept, the program ended if
        // nothing keeps it.
        if cx.global::<Finishing>().opened != Opened::Never {
            sync(cx);
        }
        return None;
    }
    // The tab left stays drawn until the tab shown is ready (`tabs.rs`),
    // each time windows come back: none being up, the panes slept
    // (`App::suspend_panes`, which lets go of it).
    app.hold_tabs(true);
    let mut shown = None;
    if let Some(location) = initial {
        let pane = {
            let ws = app.workspace.read();
            ws.window(active)
                .map_or_else(|| ws.active_id(), |w| w.shown().active)
        };
        match app.navigate(pane, &location) {
            Ok(()) => shown = Some(pane),
            Err(error) => say_in_active(cx, Words::of_error(error)),
        }
    }
    // Tabs restored, or asleep while no window was up, open the one each
    // window shows, now that the listings have a window to wake.
    if let Err(error) = app.resume_tabs() {
        say_in_active(cx, Words::of_error(error));
    }
    // Scripts and agents reach the windows from now on: what they ask
    // comes after what they start on.
    app.set_window_shown();
    cx.global_mut::<Finishing>().raised = Some(active);
    shown
}

/// Say `text` as an error in the status of the front window of the
/// user's own, never another program's dialog (ADR-141, 8).
fn say_in_active(cx: &mut App, text: Words) {
    // What it says may name a path, never traced: unsaid while no window
    // of the user's own is up.
    if let Some(window) = ordinary_handle(cx) {
        let _ = window.update(cx, |shell, _, cx| shell.set_status(text, true, cx));
    }
}

/// What takes what names no window: the core's active window while it is
/// up, else the one opened last. The core's may not be up: it was just made
/// and has yet to open, or could not (`open_window`).
fn front_of<H>(windows: &[(WindowId, H)], active: WindowId) -> Option<&(WindowId, H)> {
    windows
        .iter()
        .find(|(id, _)| *id == active)
        .or(windows.last())
}

/// The window that takes what names no window (`front_of`).
pub(crate) fn active_handle(cx: &App) -> Option<WindowHandle<Shell>> {
    let state = cx.global::<Finishing>();
    let active = state.app.workspace.read().active_window_id();
    front_of(&state.windows, active).map(|(_, handle)| *handle)
}

/// How many windows are up.
pub(crate) fn window_count(cx: &App) -> usize {
    cx.try_global::<Finishing>()
        .map_or(0, |state| state.windows.len())
}

/// How many windows of the user's own are up: what "the last window"
/// counts (ADR-115, ADR-122), never those answering other programs'
/// dialogs (ADR-141).
pub(crate) fn own_window_count(cx: &App) -> usize {
    cx.try_global::<Finishing>()
        .map_or(0, |state| own_windows(state).len())
}

/// Window `id` of the core, made for the user while no window of theirs
/// is up, sleeps until one is asked, as the others (`show`).
pub(crate) fn sleeps(cx: &mut App, id: WindowId) {
    cx.global_mut::<Finishing>().asleep.insert(id);
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
    if handed_away(cx) {
        return false;
    }
    let state = cx.global::<Finishing>();
    // What woke it, taken before anything is applied: a wake that comes
    // while the windows sync is for the next pass.
    let app = Arc::clone(&state.app);
    let woken = app.take_woken();
    let openings = std::mem::take(&mut *state.openings.lock());
    let choosings = std::mem::take(&mut *state.choosings.lock());
    for opening in openings {
        reopen(cx, Some(opening), false);
    }
    for choosing in choosings {
        open_chooser(cx, choosing);
    }
    let state = cx.global::<Finishing>();
    if state.windows.is_empty() {
        // No window since one was up: the program takes it in.
        if state.opened != Opened::Never {
            sync(cx);
        } else if state
            .windowless
            .as_ref()
            .is_some_and(|over| over.load(Ordering::Acquire))
        {
            // Started without a window, and nothing was asked of it.
            tracing::info!("started to show files for another program, and asked nothing");
            ENDED_UNASKED.store(true, Ordering::Release);
            end_unless_handed(cx);
        }
        return false;
    }
    // What a pane moved to another window carries there, given before any
    // window follows the tabs, one opened for it included (`Shell::new`).
    let windows: Vec<(WindowId, WindowHandle<Shell>)> = cx.global::<Finishing>().windows.clone();
    for (_, window) in &windows {
        let _ = window.update(cx, |shell, _, cx| shell.give_away_moved(cx));
    }
    follow_windows(cx);
    if cx.global::<Finishing>().windows.is_empty() {
        // None left and none could open: the program takes it in.
        sync(cx);
        return false;
    }
    deliver(cx);
    let windows: Vec<(WindowId, WindowHandle<Shell>)> = cx.global::<Finishing>().windows.clone();
    // The listings are pumped once, for every window (a slice a wake:
    // `PUMP_BUDGET`, whatever the number of windows). Then the windows the
    // wake concerns take in what the core sent and draw (ADR-122): all of
    // them for a wake that says nothing, the window a wake names, the
    // windows of the panes that woke or whose listings moved, wherever
    // their tabs are now, and any that has something waiting or whose tabs
    // moved. A film playing in one syncs and draws that one alone; another
    // window's own draw syncs it (`render`).
    let report = app.pump_report();
    let mut panes = woken.panes;
    panes.extend(report.panes_changed.iter().copied());
    for (_, window) in &windows {
        let _ = window.update(cx, |shell, _, _| panes.extend(shell.take_pumped()));
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
        let is_concerned = woken.all || concerned.contains(&id);
        let _ = window.update(cx, |shell, window, cx| {
            if is_concerned || shell.has_to_sync() {
                shell.sync_pumped(report.applied, window, cx);
            }
            if is_concerned {
                cx.notify();
            }
        });
    }
    let backlog = report.backlog;
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
    // What sleeps while dialogs' windows alone are up stays asleep.
    let new: Vec<WindowId> = core
        .iter()
        .copied()
        .filter(|id| !state.windows.iter().any(|(up, _)| up == id) && !state.asleep.contains(id))
        .collect();
    for window in gone {
        let _ = window.update(cx, |shell, window, cx| {
            shell.leave(window, cx);
            window.remove_window();
        });
    }
    for id in new {
        open_window(cx, id, false, None, false, None);
    }
    let state = cx.global_mut::<Finishing>();
    if state.raised == Some(active) {
        return;
    }
    // What the window brought forward last closed: the core's next active
    // window was chosen by that alone, and the compositor gives the focus
    // where it goes (`observe_window_activation` tells which): a command
    // made none active, and none is pulled forward.
    if state.raised.is_some_and(|raised| !core.contains(&raised)) {
        state.raised = Some(active);
        return;
    }
    if let Some((_, window)) = state.windows.iter().find(|(id, _)| *id == active) {
        let window = *window;
        state.raised = Some(active);
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
/// the rest to the active window. "The active window" is one that is up
/// (`front_of`): what is taken from the core's queues is never left in the
/// inbox of a window that is not. Then tell the desktop, and do what it
/// asked.
fn deliver(cx: &mut App) {
    let state = cx.global_mut::<Finishing>();
    let app = Arc::clone(&state.app);
    let active = app.workspace.read().active_window_id();
    // What asks the user an answer, and what is asked from elsewhere, go
    // to a window of the user's own, never to one answering another
    // program's dialog (ADR-141, 8): while those alone are up, they wait
    // in the core as with no window at all.
    let own = own_windows(state);
    let dialogs: HashSet<WindowId> = state
        .windows
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| !own.iter().any(|(mine, _)| mine == id))
        .collect();
    let own_front = front_of(&own, active).map(|(id, _)| *id);
    let Some(front) = own_front.or_else(|| front_of(&state.windows, active).map(|(id, _)| *id))
    else {
        return;
    };
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
                | crate::work::Taken::Replays(plan)
                | crate::work::Taken::Ended { plan, .. } => *plan,
            };
            let window = if matches!(taken, crate::work::Taken::Ended { .. }) {
                state.plans.remove(&plan)
            } else {
                state.plans.get(&plan).copied()
            };
            let window = window
                .filter(|w| state.windows.iter().any(|(up, _)| up == w))
                .unwrap_or(front);
            inboxes.entry(window).or_default().taken.push(taken);
        }
        work.sync_stopped(&app);
        work.settle(&app);
        // What joined is shown and announced as its time comes, and
        // unfolds Activity where it was asked once it lasted, the program
        // woken then (ADR-114, amended 2026-11-03 and 2026-11-07).
        let askers = crate::work::Askers {
            ran: &state.plans,
            front,
            unfold_after: unfold_after(&app),
        };
        let wake = work.watch(&app, Instant::now(), Some(askers));
        let under = work.under_way(&app);
        work.tell(&app, &under, watched);
        (work.take_asked(), wake)
    };
    if let Some(at) = wake {
        crate::work::wake_after(&shared, &app, at, cx);
    }
    let inbox = inboxes.entry(front).or_default();
    inbox.notifications = app.take_notifications();
    if let Some(front) = own_front {
        let inbox = inboxes.entry(front).or_default();
        inbox.proposals = app.take_proposals();
        inbox.questions = app.take_questions();
        inbox.clashes = app.take_clash_questions();
        inbox.secrets = app.take_secret_questions();
        inbox.forms = app.take_forms();
        let handed = std::mem::take(&mut cx.global_mut::<Finishing>().handed);
        inbox.take_in(handed);
        for request in app.take_front_requests() {
            // Asked for a pane: the window that shows it, never one
            // answering another program's dialog.
            let named = request
                .args
                .get("pane")
                .and_then(|pane| serde_json::from_value::<PaneId>(pane.clone()).ok())
                .and_then(|pane| app.workspace.read().window_of(pane));
            let mine = |w: &WindowId| own.iter().any(|(up, _)| up == w);
            if named.is_some_and(|w| dialogs.contains(&w)) {
                tracing::debug!(name = %request.name, "not run: asked of another program's dialog");
                continue;
            }
            let window = named.filter(mine).unwrap_or(front);
            inboxes.entry(window).or_default().front.push(request);
        }
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
    /// Whether it holds nothing.
    pub(crate) fn is_empty(&self) -> bool {
        self.taken.is_empty()
            && self.notifications.is_empty()
            && self.front.is_empty()
            && self.proposals.is_empty()
            && self.questions.is_empty()
            && self.clashes.is_empty()
            && self.secrets.is_empty()
            && self.forms.is_empty()
    }

    /// Answer Cancel to all it asks: no window is left to ask it.
    fn decline(self, app: &skwir_core::App) {
        for plan in self.proposals {
            app.dismissed(plan.id);
        }
        for question in self.questions {
            app.answer(question.id, false);
        }
        for clashes in self.clashes {
            app.answer_clashes(clashes.id, None);
        }
        for secret in self.secrets {
            app.answer_secret(secret.id, None);
        }
        for form in self.forms {
            app.answer_form(form.id, None);
        }
    }

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

/// After how long an action a window asked opens Activity there
/// (`ui.activity_unfold_ms`).
fn unfold_after(app: &skwir_core::App) -> std::time::Duration {
    let ms = app
        .settings
        .get::<i64>(crate::settings::ACTIVITY_UNFOLD_KEY)
        .unwrap_or(crate::settings::ACTIVITY_UNFOLD_DEFAULT);
    std::time::Duration::from_millis(u64::try_from(ms).unwrap_or(0))
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
            if !shell.activity_open() {
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
    if state.opened != Opened::Own {
        return;
    }
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

/// A window came to the front: what follows the default for folders asks
/// it again on a worker, for the system's lists, which no watch tells
/// (ADR-140). A reading under way is that one.
pub(crate) fn looked_at(cx: &mut App) {
    if !cx.has_global::<Finishing>() {
        return;
    }
    let state = cx.global::<Finishing>();
    let again = state
        .following
        .lock()
        .as_ref()
        .map(skwir_platform::file_manager::Following::again);
    let Some(again) = again else {
        return;
    };
    state.app.scheduler.spawn(
        skwir_core::scheduler::Priority::Background,
        "file-manager.again",
        state.app.lifetime().child(),
        move || again.check(),
    );
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
    // Kept in the core (the last of the user's own, ADR-115): asleep until
    // one is asked (`show`), never opened again by the next wake while
    // dialogs' windows are up.
    if state.app.workspace.read().window(id).is_some() {
        state.asleep.insert(id);
    }
    if state.player == Some(id) {
        state.player = None;
    }
    if state.windows.is_empty() {
        state.work.borrow_mut().stopped = None;
    }
}

/// What a window that left held (`Shell::take_held`) goes to the next
/// window up, at the next wake (ADR-122): a copy's question is not answered
/// Cancel because the window it was shown in closed. With none left, it is
/// declined.
pub(crate) fn hand_on(cx: &mut App, held: Inbox) {
    let state = cx.global_mut::<Finishing>();
    state.handed.take_in(held);
    // The windows answering other programs' dialogs ask nothing of the
    // user's (ADR-141, 8): with none of the user's own left, it is
    // declined as when the last window closes.
    if own_windows(state).is_empty() {
        let held = std::mem::take(&mut state.handed);
        held.decline(&state.app);
    } else {
        state.app.wake();
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
    let held = std::mem::take(&mut state.handed);
    held.decline(&state.app);
    if state.quitting {
        return;
    }
    let app = Arc::clone(&state.app);
    if state.opened == Opened::Own {
        crate::remember_location(&app);
    }
    if state.stopping {
        quit(cx);
        return;
    }
    if !keeps(&app, &state.work.borrow()) {
        end_unless_handed(cx);
        return;
    }
    tracing::info!("the window closed: finishing what runs without it");
    app.suspend_panes();
    sync(cx);
}

/// Take later launches over, on a worker: on Linux it waits for the bus.
/// What it gets is kept only if taking over was not stopped meanwhile.
/// `launch` is what this program was started to open, at start: if another
/// Skwir of this instance holds later launches (two launches at once, both
/// finding no one there), it is handed over there, and this one ends
/// (`handed_away`).
///
/// Taking them over, it shows files for other programs too while skwir
/// is the default for folders (`file_manager::follow`, ADR-140). Started by
/// the session bus without a window (`windowless`), it takes the request
/// that started it to the Skwir that holds launches, if one does; else,
/// skwir proving no longer the default, nothing is to come
/// (`Finishing::windowless`). Other programs' dialogs come with launches
/// (ADR-141, 2); started by the bus for one, that request is taken to the
/// Skwir that holds launches the same way.
fn take_over(state: &Finishing, launch: Option<Opening>, windowless: Option<Windowless>) {
    let round = state.round.fetch_add(1, Ordering::AcqRel) + 1;
    let (openings, wake) = (Arc::clone(&state.openings), state.app.waker_handle());
    let (choosings, wake_chose) = (Arc::clone(&state.choosings), wake.clone());
    let (taking, rounds) = (Arc::clone(&state.taking), Arc::clone(&state.round));
    let (handed_away, wake_handed) = (Arc::clone(&state.handed_away), wake.clone());
    let following = Arc::clone(&state.following);
    let over = state.windowless.clone().filter(|_| windowless.is_some());
    let started = std::thread::Builder::new()
        .name("skwir-take-over".into())
        .spawn(move || {
            let taken = skwir_platform::handover::take_over_choosing(
                move |opening| {
                    openings.lock().push(opening);
                    wake();
                },
                move |choosing| {
                    choosings.lock().push(choosing);
                    wake_chose();
                },
            );
            if matches!(taken, Err(NotTaken::Elsewhere)) {
                let handed = match (launch, windowless) {
                    (Some(launch), _) => skwir_platform::handover::hand_over_to_holder(&launch),
                    (None, Some(Windowless::FileManager)) => {
                        skwir_platform::handover::answer_for_holder(ASKED_WITHIN)
                    }
                    (None, Some(Windowless::Portal)) => {
                        skwir_platform::handover::answer_chooser_for_holder(ASKED_WITHIN)
                    }
                    (None, None) => false,
                };
                if handed {
                    handed_away.store(true, Ordering::Release);
                    wake_handed();
                }
            }
            // Followed while launches are taken over, by the program that
            // takes them: none, nothing to answer for.
            let followed = taken
                .as_ref()
                .ok()
                .map(|taking| skwir_platform::file_manager::follow(&taking.answering()));
            // Started for a dialog, only its wait ends it (`end_unless_asked`).
            if let Some(over) = over
                && windowless == Some(Windowless::FileManager)
                && !followed.as_ref().is_some_and(|(chosen, _)| *chosen)
            {
                over.store(true, Ordering::Release);
                wake_handed();
            }
            let mut taking = taking.lock();
            if rounds.load(Ordering::Acquire) == round {
                *taking = taken.ok();
                *following.lock() = followed.map(|(_, following)| following);
            }
        });
    if let Err(error) = started {
        tracing::debug!("later launches start a Skwir of their own: {error}");
    }
}

/// Nothing keeps the program, which ends, unless a launch was handed over
/// just before. Asked with no launch being taken meanwhile
/// (`TakingOver::close_unless`): a launch answered "taken" is never lost
/// with the program. One waits: the window opens for it, and the program
/// goes on, taking later launches over as it did (ADR-122, 5).
fn end_unless_handed(cx: &mut App) {
    let state = cx.global::<Finishing>();
    let (openings, choosings) = (Arc::clone(&state.openings), Arc::clone(&state.choosings));
    let goes_on = state.taking.lock().as_ref().is_some_and(|taking| {
        !taking.close_unless(|| !openings.lock().is_empty() || !choosings.lock().is_empty())
    });
    if !goes_on {
        stop_taking_over(state);
    }
    let handed = openings.lock().pop();
    let chose = std::mem::take(&mut *choosings.lock());
    if handed.is_none() && chose.is_empty() {
        quit(cx);
        return;
    }
    if let Some(opening) = handed {
        reopen(cx, Some(opening), false);
    }
    for choosing in chose {
        open_chooser(cx, choosing);
    }
    if !goes_on {
        // Stopped before this one was seen (no gate to ask yet, or a
        // launch the last moment before): taken over anew.
        take_over(cx.global::<Finishing>(), None, None);
    }
}

/// Stop taking later launches over: once this returns, none is handed
/// here; one handed just before waits in `openings`. What served them is
/// taken down on a thread of its own: the bus or the pipe is not waited
/// for here.
fn stop_taking_over(state: &Finishing) {
    state.round.fetch_add(1, Ordering::AcqRel);
    // What follows the choice first: it answers no more once taking over
    // stops, and asks nothing of what is let go.
    *state.following.lock() = None;
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
        // (`work::wake_after`); nothing is tinted nor unfolds here.
        let wake = work.watch(&app, std::time::Instant::now(), None);
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
            token,
            ..Opening::default()
        };
        reopen(cx, Some(opening), true);
    } else if !keeps {
        end_unless_handed(cx);
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
        ([(running, _)], []) => running.title.heading(),
        // A task's title begins a sentence already (`task::Running`).
        ([], [task]) => task.title.clone(),
        ([], []) => Words::default(),
        _ => skwir_core::words!(
            finishing_actions = "{ $count ->
                [one] { $count } action
               *[other] { $count } actions
            }",
            count = under.count(),
            note = "What the desktop shows of what runs while no window is up: how many \
                    actions."
        ),
    };
    let stopped = work.stopped.as_ref().map(|s| s.title.heading());
    let (standing, about) = if asks {
        (
            Standing::Asks,
            skwir_core::words!(
                finishing_about_asks = "Open Skwir to answer",
                note = "What the desktop shows while a question waits and no window is up."
            ),
        )
    } else if let Some(summary) = &summary {
        if summary.shares.is_empty() {
            // Nothing runs yet: what is asked waits its turn.
            (
                Standing::Running,
                skwir_core::words!(
                    finishing_about_waiting = "Waiting its turn",
                    note = "What the desktop shows while an action waits its turn and no \
                            window is up."
                ),
            )
        } else if summary.paused {
            (
                Standing::Paused,
                skwir_core::words!(
                    finishing_about_paused = "{ $what }: paused",
                    what = what,
                    note = "What the desktop shows while what runs is paused and no window \
                            is up: the action by its title, capitalised (Copy 3 items to \
                            Docs), or how many (2 actions)."
                ),
            )
        } else {
            (Standing::Running, what)
        }
    } else if work.waits_for_server() {
        (
            Standing::Waiting,
            skwir_core::words!(
                finishing_about_waits_for_server = "Waiting for its server",
                note = "What the desktop shows while an action waits for its server to answer \
                        again and no window is up."
            ),
        )
    } else if let Some(stopped) = stopped {
        (
            Standing::Stopped,
            skwir_core::words!(
                finishing_about_stopped = "{ $title }: stopped",
                title = stopped,
                note = "What the desktop shows of an action an error stopped while no window \
                        is up: its title, capitalised (Copy 3 items to Docs)."
            ),
        )
    } else {
        (
            Standing::Running,
            skwir_core::words!(
                finishing_about = "Finishing",
                note = "What the desktop shows while Skwir finishes without its window."
            ),
        )
    };
    let title = match (&summary, standing) {
        // What is left of the spell to finish: what runs and what waits.
        (Some(summary), _) => skwir_core::words!(
            finishing_title_count = "Finishing { $count ->
                [one] { $count } action
               *[other] { $count } actions
            }",
            count = summary.of - summary.ended,
            note = "The title of what the desktop shows while Skwir finishes without its \
                    window: how many actions it has left, running or waiting."
        ),
        (None, Standing::Asks) => skwir_core::words!(
            finishing_title_asks = "An action needs an answer",
            note = "The title of what the desktop shows while a question waits and no window \
                    is up."
        ),
        (None, Standing::Waiting) => skwir_core::words!(
            finishing_title_waits_for_server = "Waiting for the server",
            note = "The title of what the desktop shows while an action waits for its server \
                    and no window is up."
        ),
        (None, Standing::Stopped) => skwir_core::words!(
            finishing_title_stopped = "An action stopped",
            note = "The title of what the desktop shows of an action an error stopped while no \
                    window is up."
        ),
        _ => skwir_core::words!(
            finishing_title = "Finishing",
            note = "The title of what the desktop shows while Skwir finishes without its \
                    window."
        ),
    }
    .say();
    let said = summary.as_ref().map_or_else(
        || about.say(),
        |summary| {
            let [said, _, _] = words(under, summary);
            said.say()
        },
    );
    let about = about.say();
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
    let (initial, token, launched, to_select) = match opening {
        Some(opening) => (
            opening
                .location
                .map(|arg| crate::location_of(&app, &arg, &opening.cwd)),
            opening.token,
            // A notice clicked, the presence asked: no launch.
            !activity,
            ToSelect {
                select: opening.select,
                properties: opening.properties,
            },
        ),
        None => (None, None, false, ToSelect::default()),
    };
    // A launch goes to a window of the user's own, never to another
    // program's dialog (ADR-141); with none up, those asleep wake, else
    // one opens.
    let window = ordinary_handle(cx);
    if window.is_none() && asleep_windows(cx) {
        if let Some(pane) = show(cx, initial, token, activity) {
            let window = ordinary_handle(cx);
            to_select.shown_in(cx, &app, window, pane);
        }
        return;
    }
    if let Some(window) = window {
        reopen_in(cx, &app, window, initial, token, launched, to_select);
        return;
    }
    let location = initial.unwrap_or_else(|| {
        let home = skwir_core::os_path::home().unwrap_or_else(|| std::path::PathBuf::from("/"));
        Location::from_path(skwir_core::settings::startup_folder(&app.settings, &home))
    });
    open_new(cx, &app, location, token, to_select);
}

/// A launch shown in `window`, the user's own up: brought forward, its
/// location opened in a tab of it or a window of its own as the setting
/// says.
fn reopen_in(
    cx: &mut App,
    app: &Arc<skwir_core::App>,
    window: WindowHandle<Shell>,
    initial: Option<Location>,
    token: Option<String>,
    launched: bool,
    to_select: ToSelect,
) {
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
            match shell.app.new_tab_as_kept(Some(pane), location, None, true) {
                Ok((_, pane)) => {
                    shell.app.reveal_names(pane, to_select.select);
                    if to_select.properties {
                        shell.show_properties(window, cx);
                    }
                }
                Err(error) => shell.set_status(Words::of_error(error), true, cx),
            }
            window.activate_window_with_token(token.as_deref());
        });
        return;
    }
    let location = initial.unwrap_or_else(|| {
        let home = skwir_core::os_path::home().unwrap_or_else(|| std::path::PathBuf::from("/"));
        Location::from_path(skwir_core::settings::startup_folder(&app.settings, &home))
    });
    open_new(cx, app, location, token, to_select);
}

/// A new window of the user's own, on `location`.
fn open_new(
    cx: &mut App,
    app: &Arc<skwir_core::App>,
    location: Location,
    token: Option<String>,
    to_select: ToSelect,
) {
    match app.open_window(location) {
        Ok((id, _, pane)) => {
            open_window(cx, id, false, token, false, None);
            cx.global_mut::<Finishing>().raised = Some(id);
            let window = cx
                .global::<Finishing>()
                .windows
                .iter()
                .find(|(up, _)| *up == id)
                .map(|(_, handle)| *handle);
            to_select.shown_in(cx, app, window, pane);
        }
        Err(error) => say_in_active(cx, Words::of_error(error)),
    }
}

/// Another program's dialog (ADR-141): a window of its own, on where it
/// starts (`chooser::start_folder`). The windows asleep, with none of the
/// user's own up, stay asleep. One already answered (closed meanwhile)
/// opens nothing.
fn open_chooser(cx: &mut App, choosing: Choosing) {
    if choosing.is_settled() {
        return;
    }
    let state = cx.global_mut::<Finishing>();
    let app = Arc::clone(&state.app);
    let up: HashSet<WindowId> = state.windows.iter().map(|(id, _)| *id).collect();
    {
        let ws = app.workspace.read();
        let own_up = up
            .iter()
            .any(|id| ws.window(*id).is_some_and(|w| !w.chooses()));
        if !own_up {
            state.asleep.extend(
                ws.windows()
                    .iter()
                    .map(|w| w.id)
                    .filter(|id| !up.contains(id)),
            );
        }
    }
    let folder = crate::chooser::start_folder(&app, choosing.asked());
    match app.open_window(Location::from_path(folder)) {
        Ok((id, _, _)) => {
            app.workspace.write().set_chooses(id, true);
            if !open_window(cx, id, false, None, false, Some(choosing.clone())) {
                choosing.answer(Answer::Failed);
            }
        }
        Err(error) => {
            tracing::warn!("another program's dialog has no window: {error:#}");
            choosing.answer(Answer::Failed);
        }
    }
}

/// The front window up that is the user's own, not another program's
/// dialog (`front_of`).
fn ordinary_handle(cx: &App) -> Option<WindowHandle<Shell>> {
    let state = cx.global::<Finishing>();
    let active = state.app.workspace.read().active_window_id();
    front_of(&own_windows(state), active).map(|(_, handle)| *handle)
}

/// The windows up that are the user's own: all but those answering
/// another program's dialog (ADR-141), which take nothing that names no
/// window (what asks an answer, what is asked from elsewhere).
fn own_windows(state: &Finishing) -> Vec<(WindowId, WindowHandle<Shell>)> {
    let ws = state.app.workspace.read();
    state
        .windows
        .iter()
        .filter(|(id, _)| !ws.window(*id).is_some_and(WindowTabs::chooses))
        .copied()
        .collect()
}

/// Whether windows of the user's own sleep, none being up: `show` wakes
/// them.
fn asleep_windows(cx: &App) -> bool {
    let state = cx.global::<Finishing>();
    let ws = state.app.workspace.read();
    ws.windows()
        .iter()
        .any(|w| !w.chooses() && !state.windows.iter().any(|(up, _)| *up == w.id))
}

/// What a launch asks of its folder once shown: another program's files
/// selected there, Properties shown (ADR-140).
#[derive(Default)]
struct ToSelect {
    select: Vec<std::ffi::OsString>,
    properties: bool,
}

impl ToSelect {
    /// Asked of `pane`, in `window`: its names selected once listed.
    fn shown_in(
        self,
        cx: &mut App,
        app: &skwir_core::App,
        window: Option<WindowHandle<Shell>>,
        pane: PaneId,
    ) {
        app.reveal_names(pane, self.select);
        if self.properties
            && let Some(window) = window
        {
            let _ = window.update(cx, Shell::show_properties);
        }
    }
}

/// How far a window opened beside another is put from it, where the
/// system lets a window say where it goes.
const CASCADE: f32 = 32.;

/// The most a window answering another program's dialog takes at first,
/// at 100 %: a dialog's, beside the program it answers.
const DIALOG_WIDTH: f32 = 1000.;
const DIALOG_HEIGHT: f32 = 680.;

/// Open a window drawing the core's window `id`, shaped and placed as it
/// was left (restored, reopened), else as the window it was made from,
/// else as the window used last (`Placement::opening`); `split` splits it
/// as that was. What runs is shared: the program finished it without a
/// window. Whether it opened. A window answering another program's dialog
/// (`choosing`, ADR-141) is shaped as the last one, a dialog's size at
/// most, never maximized, the compositor placing it.
fn open_window(
    cx: &mut App,
    id: WindowId,
    split: bool,
    token: Option<String>,
    activity: bool,
    choosing: Option<Choosing>,
) -> bool {
    let state = cx.global::<Finishing>();
    let (app, keymap) = (Arc::clone(&state.app), state.keymap.clone());
    // The first window is timed from the process's start, one opened again
    // from now: the program may have run for hours without one.
    let first_frame = if state.opened == Opened::Never {
        crate::shell::FirstFrame {
            from: state.started,
            after: "start",
        }
    } else {
        crate::shell::FirstFrame {
            from: Instant::now(),
            after: "opening",
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
    let mut placement = shape
        .as_ref()
        .and_then(|shape| shape.window.clone())
        .unwrap_or(crate::session::Placement {
            width: 1200.,
            height: 800.,
            maximized: false,
            at: None,
        });
    if choosing.is_some() {
        placement = crate::session::Placement {
            width: placement.width.min(DIALOG_WIDTH),
            height: placement.height.min(DIALOG_HEIGHT),
            maximized: false,
            at: None,
        };
    }
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
    // The system's title bar, or none for skwir's own (ADR-138): what
    // `ui.title_bar` says, asked again as it moves (`Shell::sync`). On
    // macOS and Windows the window is made so (its title bar transparent,
    // or hidden), as asking once it is open would show the other a moment.
    let title_bar = crate::title_bar::TitleBar::read(
        app.settings
            .get::<String>(crate::settings::TITLE_BAR_KEY)
            .as_deref(),
    );
    let asks = title_bar.asks();
    let opened = cx.open_window(
        gpui::WindowOptions {
            window_bounds: Some(opening.bounds),
            display_id: opening.display,
            titlebar: Some(gpui::TitlebarOptions {
                title: Some("skwir".into()),
                appears_transparent: asks == gpui::WindowDecorations::Client,
                ..Default::default()
            }),
            app_id: Some(skwir_core::APP_ID.into()),
            window_decorations: Some(asks),
            // Over the program whose dialog it answers (ADR-141, 9).
            foreign_parent: choosing
                .as_ref()
                .and_then(|c| crate::chooser::foreign_parent(&c.asked().parent_window)),
            ..Default::default()
        },
        |window, cx| {
            cx.new(|cx| {
                let mut shell = Shell::new(app, id, start, keymap, work, window, cx);
                // What the window was made asking: not asked again at its
                // first sync, whose answer would wait on the first.
                shell.prefs.title_bar = title_bar;
                shell.title_bar.answered = title_bar;
                if let Some(choosing) = choosing.clone() {
                    shell.answer_dialog(choosing, window, cx);
                }
                shell
            })
        },
    );
    let window = match opened {
        Ok(window) => window,
        // At start, nothing can go on without it.
        Err(error) if cx.global::<Finishing>().opened == Opened::Never => {
            panic!("open window: {error:#}")
        }
        Err(error) => {
            tracing::error!("a window could not open: {error:#}");
            // Out of the core: it would stay the active window, and be
            // asked to open again at every wake (the last one stays).
            let app = Arc::clone(&cx.global::<Finishing>().app);
            if let Err(error) = app.close_window(id) {
                tracing::warn!("a window that did not open closes: {error:#}");
            }
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
    state.opened = state.opened.max(if choosing.is_some() {
        Opened::Dialogs
    } else {
        Opened::Own
    });
    // A request that came as the program ended unasked opened this: it
    // was asked after all.
    ENDED_UNASKED.store(false, Ordering::Release);
    if state.shown.take().is_some() {
        state.work.borrow().present(None);
    }
    let _ = window.update(cx, |shell, window, cx| {
        // Saving for another program, the name it is saved under takes the
        // keyboard; else the files.
        if !shell.chooser_focus(window, cx) {
            window.focus(&shell.ui().focus, cx);
        }
        cx.activate(true);
        if token.is_some() {
            window.activate_window_with_token(token.as_deref());
        }
        // Opened for what runs: Activity shows it, once the strip it opens
        // above is laid out.
        if activity {
            cx.on_next_frame(window, |shell, window, cx| {
                if !shell.activity_open() {
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
    // Asked in a window of the user's own, where one is up.
    if let Some(window) = ordinary_handle(cx).or_else(|| active_handle(cx)) {
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
    use skwir_core::words::Words;
    use std::time::Duration;

    use skwir_core::op::{PlanId, RunKind, Transfer};
    use skwir_core::workspace::WindowId;
    use skwir_platform::desktop::Standing;

    use super::{front_of, presence};
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

    /// What names no window goes to the active window while it is up, else
    /// to the one opened last: never to one that did not open.
    #[test]
    fn what_names_no_window_goes_to_one_that_is_up() {
        let up = [(WindowId(1), "first"), (WindowId(2), "second")];
        let front = |active| front_of(&up, active).map(|(_, name)| *name);
        assert_eq!(front(WindowId(1)), Some("first"));
        assert_eq!(
            front(WindowId(9)),
            Some("second"),
            "the active one is not up"
        );
        assert_eq!(front_of::<&str>(&[], WindowId(1)), None);
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
            "Copy 3 items to Backup · 25% · about 3\u{a0}min left"
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
        assert_eq!(shown.said, "1 of 2 · 50% · 1 waiting");
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
        work.stopped = Some(Stopped::new(
            PlanId(5),
            &Words::as_written("copy 3 items to Backup"),
            true,
        ));
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
