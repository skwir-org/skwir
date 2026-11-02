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
//!   dash shows none); sleep held off by logind's `Inhibit`, its
//!   descriptor kept meanwhile. The presence is an item in the tray (KDE's
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
    /// time.
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
    /// held (the first why asked is kept while held); `None`: let go.
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
    /// while held, `why` shown where the system shows one (a new why takes
    /// the last one's place); `None`: let go. Nothing on macOS, where the
    /// UI answers the system's question itself.
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

/// Tell the desktop what is asked, in order, the progress and the presence
/// paced, until the [`Desktop`] is dropped: then take back the presence,
/// let go of the session's end and the machine's sleep, and hide the
/// progress.
fn run(setup: &Setup, asked: &Receiver<Ask>) {
    let mut desktop = imp::Backend::new(setup);
    // What the desktop changed on its own (a tray come or gone), told by
    // the backend's own threads.
    let wakes = desktop.wakes();
    let mut progress = Paced::new(PACE);
    let mut presence = Paced::new(PRESENCE_PACE);
    let mut awake = false;
    let mut session: Option<String> = None;
    loop {
        let due = [progress.due(), presence.due(), desktop.due()]
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
                Ok(Ask::Notify(key, notice)) => desktop.notify(key, &notice),
                Ok(Ask::Withdraw(key)) => desktop.withdraw(key),
                Ok(Ask::Progress(asked)) => progress.want(imp::shown(asked)),
                Ok(Ask::Presence(asked)) => presence.want(asked),
                Ok(Ask::KeepAwake(Some(why))) if !awake => {
                    awake = true;
                    desktop.hold(&why);
                }
                Ok(Ask::KeepAwake(None)) if awake => {
                    awake = false;
                    desktop.let_go();
                }
                // Held already with its first why, or let go already.
                Ok(Ask::KeepAwake(_)) => {}
                Ok(Ask::HoldSession(why)) if why != session => {
                    desktop.hold_session(why.as_deref());
                    session = why;
                }
                // Held already with this why, or let go already.
                Ok(Ask::HoldSession(_)) => {}
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
        if progress.take(now) {
            desktop.progress(progress.sent);
        }
        if presence.take(now) {
            desktop.presence(presence.sent.as_ref());
        }
        desktop.tick(now);
    }
    if presence.sent.is_some() {
        desktop.presence(None);
    }
    if session.is_some() {
        desktop.hold_session(None);
    }
    if progress.sent.is_some() {
        desktop.progress(None);
    }
    if awake {
        desktop.let_go();
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
    wanted: Option<T>,
    sent: Option<T>,
    /// When one was last told.
    last: Option<Instant>,
}

impl<T: Clone + PartialEq> Paced<T> {
    fn new(pace: Duration) -> Paced<T> {
        Paced {
            pace,
            wanted: None,
            sent: None,
            last: None,
        }
    }

    fn want(&mut self, wanted: Option<T>) {
        self.wanted = wanted;
    }

    /// When the next is to be told, if anything differs from what was.
    fn due(&self) -> Option<Instant> {
        (self.wanted != self.sent)
            .then(|| self.last.map_or_else(Instant::now, |last| last + self.pace))
    }

    /// Whether something is to be told at `now`: then `sent` is what.
    fn take(&mut self, now: Instant) -> bool {
        if self.wanted == self.sent || self.last.is_some_and(|last| now < last + self.pace) {
            return false;
        }
        self.sent.clone_from(&self.wanted);
        self.last = Some(now);
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
