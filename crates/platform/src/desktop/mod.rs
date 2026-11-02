//! What Skwir tells the desktop while long actions run (copies, moves): a
//! notice when one ends, stops or waits for the user while the window is
//! not looked at, its progress on the program's entry in the taskbar or
//! the dock, and the machine kept from going to sleep by itself meanwhile.
//! While the program has no window (ADR-115: its last window closed while
//! actions run, the process staying until they end), a presence shows that
//! it is still at work and lets the user get back or stop it; and the
//! session's end (logout, shutdown) is held off while actions run, with
//! their words.
//!
//! Every call returns at once: it is handed to one thread of this
//! module's, started when first asked, which speaks to the desktop in the
//! order asked, as its servers may answer slowly. What the desktop lacks
//! (no notification server, no logind, no tray, a Windows id nobody
//! registered) is logged and otherwise ignored: Skwir works the same
//! without any of it. What the desktop asks back (a notice clicked, the
//! presence's menu) is told through the callback given to [`Desktop::new`].
//!
//! - Linux: notices through the desktop's notification server
//!   (`org.freedesktop.Notifications`; version 1.2 of its specification
//!   for the activation token) on the program's session bus (`bus`); the
//!   progress as the Unity launcher entry's `Update` signal, which
//!   Plasma's task manager and docks such as Ubuntu's show (GNOME's own
//!   dash shows none); sleep held off by the portal's `Inhibit` where a
//!   session manager is on the bus (GNOME's session and KDE's `PowerDevil`
//!   keep their idle from suspending for it, the user's own Suspend let
//!   through), else by logind's, its descriptor kept meanwhile. The
//!   presence is an item in the tray (KDE's
//!   `StatusNotifierItem`, its menu as `com.canonical.dbusmenu`) where a
//!   host shows one, else a notice that stays, as GNOME Files' does. The
//!   session's end is held off by the portal's `Inhibit` (logout), which
//!   GNOME's session lists in its dialogs, and by logind's shutdown
//!   inhibitor where no session manager shows a dialog of its own.
//! - Windows: toasts, `ITaskbarList3`'s progress on the window given, a
//!   power request. A toast is shown only for an `AppUserModelID` the
//!   system knows: the installer registers Skwir's as unpackaged programs
//!   do (`HKCU\Software\Classes\AppUserModelId\org.skwir.skwir`, its
//!   `DisplayName`); elsewhere (the zip, Wine) none shows, and a refusal
//!   is logged. The process is given no id of its own: the installer's
//!   shortcut has none to match (wixl cannot give it one), and a pinned
//!   shortcut's button would part from the window's. The presence is an
//!   icon in the notification area, and the session's end is held off by
//!   `ShutdownBlockReasonCreate` with `WM_QUERYENDSESSION` answered no,
//!   both from a hidden window on a thread of their own. Compiled only
//!   from Linux, and tried under Wine, which has no toasts and stubs the
//!   taskbar and power requests.
//! - macOS: the user notification center, the dock tile's badge, an
//!   activity of the process's. No presence: a program without windows
//!   stays in the Dock, whose badge already shows the progress, and the UI
//!   follows the Dock's click. Nothing held here at the session's end: the
//!   UI answers `applicationShouldTerminate:` itself.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Select, Sender};

#[cfg(any(target_os = "linux", windows, test))]
mod icon;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(all(test, target_os = "linux"))]
mod linux_tests;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod other;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
use other as imp;
#[cfg(windows)]
use windows as imp;

/// What a notice is about, for the desktop to file it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An action that came to its end.
    Finished,
    /// An action that stopped before its end.
    Stopped,
    /// An action that waits for the user.
    Waiting,
}

/// A notice, as the desktop shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub body: String,
    pub kind: Kind,
}

/// A notice clicked: the key it was shown with, and the token the desktop
/// gave with the click to bring the window forward (Wayland's
/// xdg-activation, notification spec 1.2's `ActivationToken`), if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clicked {
    pub key: u64,
    pub token: Option<String>,
}

/// What the desktop asked of the program, told through the callback given
/// to [`Desktop::new`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    /// A notice clicked.
    Notice(Clicked),
    /// The window wanted again: the presence clicked, or its Open; with
    /// the token the desktop gave with it (Wayland's xdg-activation), if
    /// any.
    Open { token: Option<String> },
    /// The presence's menu: pause every action.
    PauseAll,
    /// The presence's menu: let every paused action go on.
    GoOn,
    /// The presence's menu: stop every action, after which the program
    /// ends.
    StopAll,
}

/// How what runs stands, as the presence shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Standing {
    Running,
    Paused,
    /// Waiting for a server that stopped answering.
    Waiting,
    /// An error stopped it.
    Stopped,
    /// It waits for an answer from the user.
    Asks,
}

/// What shows that Skwir is still at work while it has no window.
#[derive(Clone, Debug, PartialEq)]
pub struct Presence {
    /// "Finishing 2 actions".
    pub title: String,
    /// The footer strip's words, which change often: "Copy 3 items to
    /// Backup · 42 % · about 3 min left".
    pub said: String,
    /// What it is about, without figures, for a notice that stays: "Copy
    /// 3 items to Backup", "Waiting for its server", "Needs an answer".
    pub about: String,
    /// How far, 0 to 1; `None` draws no share (waiting, asking).
    pub fraction: Option<f32>,
    pub standing: Standing,
    /// The menu's pausing entry: `Some(true)` "Pause all", `Some(false)`
    /// "Go on", `None` neither.
    pub pause: Option<bool>,
}

/// How the progress is going.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Normal,
    Paused,
    Error,
}

/// The progress shown on the program's entry: `fraction` from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Progress {
    pub fraction: f32,
    pub state: State,
}

/// The least time between two progresses told: about four a second, the
/// last one asked always told.
const PACE: Duration = Duration::from_millis(250);

/// The least time between two presences told: its words change with each
/// percent, and nobody reads a tray's tooltip four times a second. A
/// presence taken back, or the first after a quiet second, is told at
/// once.
const PRESENCE_PACE: Duration = Duration::from_secs(1);

/// The least time between two notices told: a burst of ends (a queue of
/// short actions finishing while no one looks) tells its last, which says
/// what all of them did, rather than each in turn, each shown anew by the
/// desktop. The first after a quiet while is told at once.
const NOTICE_PACE: Duration = Duration::from_secs(2);

/// How long the machine's sleep and the session's end stay held once let
/// go: the next action of a queue, asked for at once, finds them held, and
/// nothing lapses between two actions.
const LINGER: Duration = Duration::from_secs(1);

/// What a caller asked, in order, for the module's thread.
enum Ask {
    Notify(u64, Notice),
    /// Take back the notice shown, or only the one with this key.
    Withdraw(Option<u64>),
    Progress(Option<Progress>),
    KeepAwake(Option<String>),
    Presence(Option<Presence>),
    HoldSession(Option<String>),
    /// The window whose taskbar button shows the progress changed.
    #[cfg(windows)]
    Window,
}

type OnAsked = Arc<dyn Fn(Asked) + Send + Sync>;

/// What the module's thread is made with.
#[derive(Clone)]
struct Setup {
    /// The desktop file's name, the Windows `AppUserModelID`.
    app_id: String,
    app_name: String,
    /// The program's icon, as SVG: the presence's is drawn from it.
    icon: &'static [u8],
    asked: OnAsked,
    /// The connections asked for: the program's own, or a test's.
    #[cfg(target_os = "linux")]
    buses: linux::Buses,
    /// The notices shown, shared with the thread that follows the server.
    #[cfg(target_os = "linux")]
    showing: Arc<parking_lot::Mutex<linux::Showing>>,
    /// The window given (`Desktop::set_window`), 0 before.
    #[cfg(windows)]
    window: Arc<std::sync::atomic::AtomicIsize>,
}

/// The module's thread, once started.
struct Started {
    asks: Sender<Ask>,
    /// Disconnected once the thread has ended, what it holds let go of.
    ended: Receiver<()>,
}

/// What Skwir tells the desktop while long actions run. Dropped, it lets
/// go of the machine's sleep and of the session's end, hides the progress
/// and takes back the presence; notices already shown stay.
pub struct Desktop {
    setup: Setup,
    /// The module's thread, started when first asked: `None` when it
    /// could not start.
    started: OnceLock<Option<Started>>,
}

// Held by the UI and handed to its threads.
const _: fn() = || {
    fn shared<T: Send + Sync>() {}
    shared::<Desktop>();
};

impl Desktop {
    /// `app_id` "org.skwir.skwir" (the desktop file's name, the Windows
    /// `AppUserModelID` toasts are shown for), `app_name` "skwir",
    /// `icon_svg` the program's icon, which the presence's is drawn from.
    /// Starts no thread and opens no connection until first asked: a run
    /// that never asks never touches a bus. `asked` is called on a thread
    /// of the module's (on Windows, of the system's), and must return at
    /// once. On Linux and Windows a click only tells: bringing the window
    /// forward is the caller's, with the token on Wayland. macOS brings the
    /// program forward itself on a click of a notice and tells nothing.
    ///
    /// Starting is asking something to be shown or held: taking back,
    /// hiding or letting go while nothing was asked starts nothing.
    pub fn new(
        app_id: &str,
        app_name: &str,
        icon_svg: &'static [u8],
        asked: impl Fn(Asked) + Send + Sync + 'static,
    ) -> Desktop {
        Desktop::with(Setup {
            app_id: app_id.to_owned(),
            app_name: app_name.to_owned(),
            icon: icon_svg,
            asked: Arc::new(asked),
            #[cfg(target_os = "linux")]
            buses: linux::Buses::the_programs(),
            #[cfg(target_os = "linux")]
            showing: Arc::default(),
            #[cfg(windows)]
            window: Arc::default(),
        })
    }

    fn with(setup: Setup) -> Desktop {
        Desktop {
            setup,
            started: OnceLock::new(),
        }
    }

    /// Show `notice`, known by `key`, in place of the one shown: one at a
    /// time, at most one every two seconds, the last asked always shown.
    pub fn notify(&self, key: u64, notice: Notice) {
        self.ask(Ask::Notify(key, notice));
    }

    /// Take back the notice shown, if any.
    pub fn withdraw(&self) {
        self.ask(Ask::Withdraw(None));
    }

    /// Take back the notice shown only if it is the one shown with `key`.
    pub fn withdraw_key(&self, key: u64) {
        self.ask(Ask::Withdraw(Some(key)));
    }

    /// The progress on the program's taskbar or dock entry; `None` hides
    /// it.
    pub fn progress(&self, progress: Option<Progress>) {
        self.ask(Ask::Progress(progress));
    }

    /// `Some(why)`: keep the machine from going to sleep by itself while
    /// held (the first why asked is kept while held); `None`: let go, a
    /// second later unless held again meanwhile.
    pub fn keep_awake(&self, why: Option<String>) {
        self.ask(Ask::KeepAwake(why));
    }

    /// Show `presence` (in place of the one shown), or take it back
    /// (`None`): what shows that the program is still at work while it has
    /// no window. Told at most once a second, the last asked always told.
    /// Nothing on macOS, where the Dock already shows it.
    pub fn presence(&self, presence: Option<Presence>) {
        self.ask(Ask::Presence(presence));
    }

    /// `Some(why)`: hold off the session's end (logout, shutdown, restart)
    /// while held, `why` shown where the system shows one (the first why
    /// asked is kept while held: on Linux each would be a new inhibitor);
    /// `None`: let go, a second later unless held again meanwhile. Nothing
    /// on macOS, where the UI answers the system's question itself.
    pub fn hold_session(&self, why: Option<String>) {
        self.ask(Ask::HoldSession(why));
    }

    /// Windows only: the window whose taskbar button shows the progress.
    #[cfg(windows)]
    pub fn set_window(&self, hwnd: isize) {
        self.setup
            .window
            .store(hwnd, std::sync::atomic::Ordering::Relaxed);
        if self.started.get().is_some() {
            self.ask(Ask::Window);
        }
    }

    /// Wait at most `within` for what was asked to be told (the last
    /// notice, before the program ends), and what is held let go of, then
    /// end the module's thread. What is not done by then is left to finish
    /// by itself, as when this is dropped.
    pub fn finish(self, within: Duration) {
        let Some(Some(Started { asks, ended })) = self.started.into_inner() else {
            return;
        };
        drop(asks);
        if ended.recv_timeout(within) == Err(RecvTimeoutError::Timeout) {
            tracing::debug!(
                "the desktop was not told everything within {} ms",
                within.as_millis()
            );
        }
    }

    fn ask(&self, ask: Ask) {
        // What only undoes needs no thread while nothing was done.
        let undoes = matches!(
            ask,
            Ask::Withdraw(_)
                | Ask::Progress(None)
                | Ask::KeepAwake(None)
                | Ask::Presence(None)
                | Ask::HoldSession(None)
        );
        if undoes && self.started.get().is_none() {
            return;
        }
        if let Some(started) = self.started.get_or_init(|| start(self.setup.clone())) {
            // The thread ends only once this is dropped.
            let _ = started.asks.send(ask);
        }
    }
}

/// The module's thread, which ends once its sender is dropped.
fn start(setup: Setup) -> Option<Started> {
    let (asks, asked) = crossbeam_channel::unbounded();
    let (ends, ended) = crossbeam_channel::bounded::<()>(0);
    let started = std::thread::Builder::new()
        .name("skwir-desktop".into())
        .spawn(move || {
            run(&setup, &asked);
            drop(ends);
        });
    match started {
        Ok(_) => Some(Started { asks, ended }),
        Err(error) => {
            tracing::debug!("the desktop is told nothing: {error}");
            None
        }
    }
}

/// Tell the desktop what is asked, in order, the progress, the presence and
/// the notices paced, what is held let go of a moment late, until the
/// [`Desktop`] is dropped: then tell the last notice, take back the
/// presence, let go of the session's end and the machine's sleep, and hide
/// the progress.
fn run(setup: &Setup, asked: &Receiver<Ask>) {
    let mut desktop = imp::Backend::new(setup);
    // What the desktop changed on its own (a tray come or gone), told by
    // the backend's own threads.
    let wakes = desktop.wakes();
    let mut progress = Paced::new(PACE);
    let mut presence = Paced::new(PRESENCE_PACE).taken_back_at_once();
    let mut notices = Notices::default();
    let mut awake = Held::default();
    let mut session = Held::default();
    loop {
        let due = [
            progress.due(),
            presence.due(),
            notices.due(),
            awake.due(),
            session.due(),
            desktop.due(),
        ]
        .into_iter()
        .flatten()
        .min();
        let mut select = Select::new();
        let on_asked = select.recv(asked);
        select.recv(&wakes);
        let ready = match due {
            Some(due) => select.select_deadline(due).ok(),
            None => Some(select.select()),
        };
        match ready {
            Some(ready) if ready.index() == on_asked => match ready.recv(asked) {
                Ok(Ask::Notify(key, notice)) => notices.want(key, notice),
                Ok(Ask::Withdraw(key)) => match notices.withdraw(key) {
                    TakeBack::Nothing => {}
                    TakeBack::Any => desktop.withdraw(None),
                    TakeBack::Key(key) => desktop.withdraw(Some(key)),
                },
                Ok(Ask::Progress(asked)) => progress.want(imp::shown(asked)),
                Ok(Ask::Presence(asked)) => presence.want(asked),
                Ok(Ask::KeepAwake(why)) => {
                    if let Some(why) = awake.ask(why, Instant::now()) {
                        desktop.hold(&why);
                    }
                }
                Ok(Ask::HoldSession(why)) => {
                    if let Some(why) = session.ask(why, Instant::now()) {
                        desktop.hold_session(Some(&why));
                    }
                }
                #[cfg(windows)]
                Ok(Ask::Window) => desktop.window(),
                Err(_) => break,
            },
            Some(ready) => {
                let _ = ready.recv(&wakes);
                desktop.woken();
            }
            None => {}
        }
        let now = Instant::now();
        if let Some((key, notice)) = notices.take(now) {
            desktop.notify(key, &notice);
        }
        if progress.take(now) {
            desktop.progress(progress.sent);
        }
        if presence.take(now) {
            desktop.presence(presence.sent.as_ref());
        }
        if awake.take(now) {
            desktop.let_go();
        }
        if session.take(now) {
            desktop.hold_session(None);
        }
        desktop.tick(now);
    }
    if let Some((key, notice)) = notices.waiting.take() {
        desktop.notify(key, &notice);
    }
    if presence.sent.is_some() {
        desktop.presence(None);
    }
    if session.why.is_some() {
        desktop.hold_session(None);
    }
    if progress.sent.is_some() {
        desktop.progress(None);
    }
    if awake.why.is_some() {
        desktop.let_go();
    }
}

/// What the desktop is to take back of the notice it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TakeBack {
    Nothing,
    /// Whichever it shows.
    Any,
    /// Only the one shown with this key.
    Key(u64),
}

/// The notice asked, told at most once in [`NOTICE_PACE`]: one waiting is
/// the one shown, as far as the caller knows, and replaced by the next.
#[derive(Default)]
struct Notices {
    /// The notice asked and not told yet, and its key.
    waiting: Option<(u64, Notice)>,
    /// When one was last told.
    last: Option<Instant>,
}

impl Notices {
    fn want(&mut self, key: u64, notice: Notice) {
        self.waiting = Some((key, notice));
    }

    /// When the one waiting is to be told.
    fn due(&self) -> Option<Instant> {
        self.waiting.as_ref().map(|_| {
            self.last
                .map_or_else(Instant::now, |last| last + NOTICE_PACE)
        })
    }

    /// The notice to tell at `now`, if one waits and its time came.
    fn take(&mut self, now: Instant) -> Option<(u64, Notice)> {
        if self.last.is_some_and(|last| now < last + NOTICE_PACE) {
            return None;
        }
        let taken = self.waiting.take()?;
        self.last = Some(now);
        Some(taken)
    }

    /// The notice taken back (any, or only the one with `key`): what the
    /// desktop is to take back of what it shows. One waiting stands for the
    /// one shown, which it replaces: taken back with it, the desktop takes
    /// back whatever it shows; with another key, nothing.
    fn withdraw(&mut self, key: Option<u64>) -> TakeBack {
        match (&self.waiting, key) {
            (None, None) => TakeBack::Any,
            (None, Some(key)) => TakeBack::Key(key),
            (Some((waiting, _)), Some(key)) if *waiting != key => TakeBack::Nothing,
            (Some(_), _) => {
                self.waiting = None;
                TakeBack::Any
            }
        }
    }
}

/// What is held while asked (the machine kept awake, the session's end
/// held off), with the reason first asked: one asked later while held is
/// not told, as each would be a new inhibitor on Linux; let go only once
/// [`LINGER`] passed without its being asked again.
#[derive(Default)]
struct Held {
    /// The reason it is held for, while held.
    why: Option<String>,
    /// When it is let go of, unless asked again before.
    until: Option<Instant>,
}

impl Held {
    /// `why` asked at `now` (`None`: let go): the reason to hold with now,
    /// if it was not held.
    fn ask(&mut self, why: Option<String>, now: Instant) -> Option<String> {
        let Some(why) = why else {
            if self.why.is_some() && self.until.is_none() {
                self.until = Some(now + LINGER);
            }
            return None;
        };
        self.until = None;
        if self.why.is_some() {
            return None;
        }
        self.why = Some(why.clone());
        Some(why)
    }

    fn due(&self) -> Option<Instant> {
        self.until
    }

    /// Whether to let go at `now`.
    fn take(&mut self, now: Instant) -> bool {
        if self.until.is_none_or(|until| now < until) {
            return false;
        }
        self.until = None;
        self.why = None;
        true
    }
}

/// The progress as a system shows it: `None` hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shown {
    /// Thousandths done.
    permille: u16,
    state: State,
}

/// Thousandths done, rounded down to a multiple of `step` (1 for
/// thousandths, 10 for whole percents): a whole one shown only once done.
fn permille(fraction: f32, step: u16) -> u16 {
    // NaN casts to 0.
    let permille = (fraction.clamp(0.0, 1.0) * 1000.0) as u16;
    permille - permille % step
}

/// What a system is told of something that changes often (the progress,
/// the presence): a change at once, then none until `pace` has passed
/// since, when the last asked is told.
struct Paced<T> {
    pace: Duration,
    /// Whether `None` (taken back) is told at once, never paced.
    taken_back_at_once: bool,
    wanted: Option<T>,
    sent: Option<T>,
    /// When one was last told.
    last: Option<Instant>,
}

impl<T: Clone + PartialEq> Paced<T> {
    fn new(pace: Duration) -> Paced<T> {
        Paced {
            pace,
            taken_back_at_once: false,
            wanted: None,
            sent: None,
            last: None,
        }
    }

    /// `None` told at once: the presence, which offers what the window
    /// that took its place offers.
    fn taken_back_at_once(self) -> Paced<T> {
        Paced {
            taken_back_at_once: true,
            ..self
        }
    }

    fn want(&mut self, wanted: Option<T>) {
        self.wanted = wanted;
    }

    /// When the next can be told, paced.
    fn next(&self) -> Option<Instant> {
        if self.taken_back_at_once && self.wanted.is_none() {
            return None;
        }
        self.last.map(|last| last + self.pace)
    }

    /// When the next is to be told, if anything differs from what was.
    fn due(&self) -> Option<Instant> {
        (self.wanted != self.sent).then(|| self.next().unwrap_or_else(Instant::now))
    }

    /// Whether something is to be told at `now`: then `sent` is what.
    fn take(&mut self, now: Instant) -> bool {
        if self.wanted == self.sent || self.next().is_some_and(|next| now < next) {
            return false;
        }
        self.sent.clone_from(&self.wanted);
        self.last = Some(now);
        true
    }
}

/// When something is told again after a change the system makes its own a
/// moment later, from the change: Windows' taskbar button, which Explorer
/// makes once the window shows, and again when it starts again. Calls on
/// the button wait for it (`TaskbarButtonCreated`, told to the window's
/// procedure, which is gpui's): one before it is lost.
#[cfg(any(windows, test))]
const AGAIN: [Duration; 3] = [
    Duration::from_millis(250),
    Duration::from_secs(1),
    Duration::from_secs(3),
];

/// What is told again at [`AGAIN`]'s times after a change.
#[cfg(any(windows, test))]
#[derive(Default)]
struct Again {
    /// When the change was, while some time is left.
    from: Option<Instant>,
    /// The next of [`AGAIN`]'s times.
    step: usize,
}

#[cfg(any(windows, test))]
impl Again {
    /// The change was `now`: told again from it.
    fn start(&mut self, now: Instant) {
        self.from = Some(now);
        self.step = 0;
    }

    fn due(&self) -> Option<Instant> {
        Some(self.from? + *AGAIN.get(self.step)?)
    }

    /// Whether to tell again at `now`.
    fn take(&mut self, now: Instant) -> bool {
        if self.due().is_none_or(|due| now < due) {
            return false;
        }
        self.step += 1;
        if self.step == AGAIN.len() {
            self.from = None;
        }
        true
    }
}

/// `text` as markup and XML hold it: `&`, `<` and `>` escaped, and the
/// control characters XML refuses even escaped dropped (a tab and line
/// ends kept), as one would refuse the whole notice.
#[cfg(any(target_os = "linux", windows, test))]
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\t' | '\n' | '\r' => escaped.push(character),
            character if character.is_ascii_control() && character != '\u{7f}' => {}
            character => escaped.push(character),
        }
    }
    escaped
}

/// `title` and `said` on two lines, as UTF-16 within `room` units and a
/// terminating zero (Windows' tip of a notification area's icon): what
/// does not fit cut at a character's end, with an ellipsis.
#[cfg(any(windows, test))]
fn tip(title: &str, said: &str, room: usize) -> Vec<u16> {
    let text = if said.is_empty() {
        title.to_owned()
    } else {
        format!("{title}\n{said}")
    };
    let mut units: Vec<u16> = text.encode_utf16().collect();
    // Room for the zero, then for the ellipsis.
    let room = room.saturating_sub(1);
    if units.len() > room {
        let mut cut = room.saturating_sub(1);
        // Never between a surrogate pair's halves.
        if cut > 0 && (0xD800..0xDC00).contains(&units[cut - 1]) {
            cut -= 1;
        }
        units.truncate(cut);
        units.push(0x2026);
    }
    units.push(0);
    units
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(permille: u16) -> Shown {
        Shown {
            permille,
            state: State::Normal,
        }
    }

    /// A change is told at once, then none until the pace has passed, when
    /// the last asked is; what was told is not told again.
    #[test]
    fn progress_is_told_at_a_pace_ending_with_the_last_asked() {
        let mut paced = Paced::new(PACE);
        let start = Instant::now();
        assert_eq!(paced.due(), None, "nothing shown, nothing asked");
        paced.want(None);
        assert!(!paced.take(start));

        paced.want(Some(shown(100)));
        assert!(paced.take(start), "the first at once");
        assert_eq!(paced.sent, Some(shown(100)));
        paced.want(Some(shown(200)));
        paced.want(Some(shown(300)));
        assert_eq!(paced.due(), Some(start + PACE));
        assert!(!paced.take(start + PACE / 2), "within the pace");
        assert!(paced.take(start + PACE));
        assert_eq!(paced.sent, Some(shown(300)), "the last asked");
        assert_eq!(paced.due(), None);

        // Asked back to what was told: nothing to tell.
        paced.want(Some(shown(400)));
        paced.want(Some(shown(300)));
        assert_eq!(paced.due(), None);
        assert!(!paced.take(start + PACE * 3));

        paced.want(None);
        assert!(paced.take(start + PACE * 3));
        assert_eq!(paced.sent, None, "hidden");
    }

    /// The presence is paced, but taken back at once: the window that
    /// takes its place offers what it offered.
    #[test]
    fn a_presence_is_taken_back_at_once() {
        let mut paced = Paced::new(PRESENCE_PACE).taken_back_at_once();
        let start = Instant::now();
        paced.want(Some(shown(100)));
        assert!(paced.take(start));
        paced.want(Some(shown(200)));
        assert!(!paced.take(start + PRESENCE_PACE / 2), "paced");
        paced.want(None);
        assert!(paced.due().is_some_and(|due| due <= Instant::now()));
        assert!(paced.take(start + PRESENCE_PACE / 2), "at once");
        assert_eq!(paced.sent, None);
        // Shown again: paced from the taking back.
        paced.want(Some(shown(300)));
        assert!(!paced.take(start + PRESENCE_PACE));
        assert!(paced.take(start + PRESENCE_PACE * 3 / 2));
    }

    fn notice(title: &str) -> Notice {
        Notice {
            title: title.into(),
            body: String::new(),
            kind: Kind::Finished,
        }
    }

    /// A burst of notices tells the first at once and then only the last,
    /// a pace later.
    #[test]
    fn a_burst_of_notices_tells_its_last() {
        let mut notices = Notices::default();
        let start = Instant::now();
        assert_eq!(notices.due(), None);
        notices.want(1, notice("Finished"));
        assert_eq!(notices.take(start).map(|(key, _)| key), Some(1), "at once");
        for key in 2..=20 {
            notices.want(key, notice(&format!("{key} actions finished")));
        }
        assert_eq!(notices.due(), Some(start + NOTICE_PACE));
        assert!(notices.take(start + NOTICE_PACE / 2).is_none(), "paced");
        let (key, last) = notices.take(start + NOTICE_PACE).unwrap();
        assert_eq!((key, last.title.as_str()), (20, "20 actions finished"));
        assert_eq!(notices.due(), None, "nothing waits");
    }

    /// One waiting stands for the one shown, which it replaces: taken back
    /// by its key, the desktop takes back what it shows; by another key,
    /// nothing; with none waiting, as asked.
    #[test]
    fn a_notice_waiting_is_taken_back_as_the_one_shown() {
        let mut notices = Notices::default();
        let start = Instant::now();
        notices.want(1, notice("Finished"));
        notices.take(start);
        assert_eq!(notices.withdraw(Some(1)), TakeBack::Key(1), "none waits");
        notices.want(2, notice("2 actions finished"));
        assert_eq!(
            notices.withdraw(Some(1)),
            TakeBack::Nothing,
            "the one shown was replaced"
        );
        assert!(notices.waiting.is_some());
        assert_eq!(
            notices.withdraw(Some(2)),
            TakeBack::Any,
            "taken back with it"
        );
        assert!(notices.waiting.is_none());
        notices.want(3, notice("Stopped"));
        assert_eq!(notices.withdraw(None), TakeBack::Any);
        assert_eq!(notices.due(), None);
    }

    /// Held with the first reason asked; let go of a moment late, unless
    /// asked again meanwhile, which tells nothing.
    #[test]
    fn what_is_held_keeps_its_first_reason_and_lingers() {
        let mut held = Held::default();
        let start = Instant::now();
        assert_eq!(held.ask(None, start), None, "nothing held");
        assert_eq!(held.due(), None);
        assert_eq!(held.ask(Some("Copy".into()), start), Some("Copy".into()));
        assert_eq!(held.ask(Some("Move".into()), start), None, "the first kept");
        assert_eq!(held.ask(None, start), None);
        assert_eq!(held.due(), Some(start + LINGER));
        assert!(!held.take(start + LINGER / 2), "lingers");
        // The queue's next action: still held, nothing told.
        assert_eq!(held.ask(Some("Move".into()), start + LINGER / 2), None);
        assert_eq!(held.due(), None);
        assert!(!held.take(start + LINGER * 2));
        assert_eq!(held.why.as_deref(), Some("Copy"));

        held.ask(None, start + LINGER * 2);
        // Asked to let go again: the first time counts.
        held.ask(None, start + LINGER * 3);
        assert!(held.take(start + LINGER * 3), "let go");
        assert_eq!(held.why, None);
        assert_eq!(
            held.ask(Some("Move".into()), start),
            Some("Move".into()),
            "anew"
        );
    }

    #[test]
    fn thousandths_are_rounded_down_and_bounded() {
        assert_eq!(permille(0.4567, 1), 456);
        assert_eq!(permille(0.4567, 10), 450);
        assert_eq!(permille(0.999, 10), 990, "whole only once done");
        assert_eq!(permille(1.0, 10), 1000);
        assert_eq!(permille(1.5, 1), 1000);
        assert_eq!(permille(-0.5, 1), 0);
        assert_eq!(permille(f32::NAN, 1), 0);
    }

    #[test]
    fn markup_and_xml_characters_are_escaped() {
        assert_eq!(
            escape("Tom & Jerry's <copy> \"done\""),
            "Tom &amp; Jerry's &lt;copy&gt; \"done\""
        );
        assert_eq!(escape("café ☕"), "café ☕");
        assert_eq!(
            escape("a\u{1}b\tc\nd\u{1b}e"),
            "ab\tc\nde",
            "what XML refuses dropped"
        );
    }

    /// Told again at each of its times after a change, then no more; a
    /// change meanwhile starts over.
    #[test]
    fn again_is_told_at_its_times_then_done() {
        let mut again = Again::default();
        let start = Instant::now();
        assert_eq!(again.due(), None);
        assert!(!again.take(start));
        again.start(start);
        for after in AGAIN {
            assert_eq!(again.due(), Some(start + after));
            assert!(!again.take(start + after / 2), "not yet");
            assert!(again.take(start + after));
        }
        assert_eq!(again.due(), None, "done");
        again.start(start + AGAIN[2]);
        assert_eq!(again.due(), Some(start + AGAIN[2] + AGAIN[0]), "anew");
    }

    /// The tip fits its room with its zero, cut at a character's end.
    #[test]
    fn a_tip_is_cut_to_fit_whole_characters() {
        let text = |units: &[u16]| String::from_utf16(&units[..units.len() - 1]).unwrap();
        let short = tip("Finishing 2 actions", "42 %", 128);
        assert_eq!(text(&short), "Finishing 2 actions\n42 %");
        assert_eq!(short.last(), Some(&0));
        assert_eq!(text(&tip("Finishing", "", 128)), "Finishing");

        let long = tip("Copy", &"é".repeat(300), 128);
        assert_eq!(long.len(), 128, "room and zero");
        assert!(text(&long).ends_with('…'));

        // A character outside the basic plane never cut in two: each is
        // a pair, the cut falls between pairs or before one.
        for room in 8..16 {
            let cut = tip("😀😀😀😀😀😀😀😀😀😀", "", room);
            assert!(cut.len() <= room);
            assert!(String::from_utf16(&cut[..cut.len() - 1]).is_ok(), "{room}");
        }
    }

    /// The Windows backend, driven where Wine answers: a power request held
    /// and let go; the taskbar and toasts asked, which Wine may lack, only
    /// logged then.
    #[cfg(windows)]
    #[test]
    fn windows_backend_holds_and_lets_go_and_survives_what_is_missing() {
        let setup = windows_setup(Arc::new(|_| {}));
        let mut backend = imp::Backend::new(&setup);
        backend.hold("Copying");
        assert!(backend.holds(), "a power request is held");
        backend.let_go();
        assert!(!backend.holds(), "let go");
        backend.notify(
            1,
            &Notice {
                title: "Copied".into(),
                body: "a & b".into(),
                kind: Kind::Finished,
            },
        );
        backend.withdraw(Some(2));
        backend.withdraw(None);
        #[allow(unsafe_code)]
        // SAFETY: a call without arguments, answering the desktop's window.
        let desktop =
            unsafe { ::windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow() }.0 as isize;
        setup
            .window
            .store(desktop, std::sync::atomic::Ordering::Relaxed);
        backend.progress(imp::shown(Some(Progress {
            fraction: 0.5,
            state: State::Paused,
        })));
        // A window given: told again a moment later, as its button comes
        // after it, then no more.
        backend.window();
        while let Some(due) = backend.tells_again() {
            backend.tick(due);
        }
        backend.progress(None);
        drop(backend);
    }

    #[cfg(windows)]
    fn windows_setup(asked: OnAsked) -> Setup {
        Setup {
            // An id nobody registered: no toast is shown, on Windows either.
            app_id: "org.skwir.skwir.test".into(),
            app_name: "skwir".into(),
            icon: include_bytes!("../../../ui/assets/org.skwir.skwir.svg"),
            asked,
            window: Arc::default(),
        }
    }

    /// The hidden window answers the session's end: no while held, yes
    /// once let go; a click on the presence's icon is told as Open; the
    /// presence shown and taken back where there is no notification area
    /// (Wine without a display) without hanging, and the window's thread
    /// ended with the backend.
    #[cfg(windows)]
    #[test]
    fn windows_hidden_window_holds_the_session_and_tells_clicks() {
        use ::windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
        use ::windows::Win32::System::Shutdown::ShutdownBlockReasonQuery;
        use ::windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_QUERYENDSESSION};
        use ::windows::core::PWSTR;

        let asked = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let setup = windows_setup({
            let asked = Arc::clone(&asked);
            Arc::new(move |what| asked.lock().push(what))
        });
        let mut backend = imp::Backend::new(&setup);
        backend.hold_session(Some("Copy 3 items to Backup"));
        let Some(window) = backend.hidden_window() else {
            assert!(
                skwir_core::system::under_wine("a window without a display"),
                "the hidden window was made"
            );
            return;
        };
        let window = HWND(window as *mut core::ffi::c_void);
        let ends = || {
            #[allow(unsafe_code)]
            // SAFETY: a message the window's procedure answers, sent to the
            // window this backend made, alive until it is dropped.
            unsafe {
                SendMessageW(window, WM_QUERYENDSESSION, Some(WPARAM(0)), Some(LPARAM(0)))
            }
        };
        assert_eq!(ends(), LRESULT(0), "held: the session may not end");
        // The reason Windows shows, set by the window's thread once told.
        let reason = || {
            let mut buffer = [0u16; 256];
            let mut length = buffer.len() as u32;
            #[allow(unsafe_code)]
            // SAFETY: a buffer of the length given, written within it.
            unsafe {
                ShutdownBlockReasonQuery(window, Some(PWSTR(buffer.as_mut_ptr())), &raw mut length)
            }
            .map(|()| {
                let length = (length as usize).min(buffer.len());
                String::from_utf16_lossy(&buffer[..length])
                    .trim_end_matches('\0')
                    .to_owned()
            })
        };
        // Wine sets none (`ShutdownBlockReasonCreate` a stub), and lacks
        // the call that reads it: called, the process ends.
        if !skwir_core::system::under_wine("ShutdownBlockReasonQuery") {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match reason() {
                    Ok(reason) if reason == "Copy 3 items to Backup" => break,
                    other => assert!(Instant::now() < deadline, "the reason set: {other:?}"),
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        backend.hold_session(None);
        assert_eq!(ends(), LRESULT(1), "let go: it may");

        let presence = Presence {
            title: "Finishing 2 actions".into(),
            said: "Copy 3 items to Backup · 42 %".into(),
            about: "Copy 3 items to Backup".into(),
            fraction: Some(0.42),
            standing: Standing::Running,
            pause: Some(true),
        };
        backend.presence(Some(&presence));
        // A notification area started with the desktop may come a moment
        // after: it then tells every window (`TaskbarCreated`).
        let deadline = Instant::now() + Duration::from_secs(3);
        while !imp::added(window) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        if !imp::added(window) {
            assert!(
                skwir_core::system::under_wine("a notification area"),
                "the icon added"
            );
        }
        imp::click(window);
        assert_eq!(asked.lock().as_slice(), [Asked::Open { token: None }]);
        backend.presence(None);
        let start = Instant::now();
        drop(backend);
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "the window's thread ended with the backend"
        );
    }
}
