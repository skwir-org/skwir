//! What Skwir tells the desktop while long actions run (copies, moves): a
//! notice when one ends, stops or waits for the user while the window is
//! not looked at, its progress on the program's entry in the taskbar or
//! the dock, and the machine kept from going to sleep by itself meanwhile.
//!
//! Every call returns at once: it is handed to one thread of this
//! module's, started when first asked, which speaks to the desktop in the
//! order asked, as its servers may answer slowly. What the desktop lacks
//! (no notification server, no logind, a Windows id nobody registered) is
//! logged and otherwise ignored: Skwir works the same without any of it.
//!
//! - Linux: notices through the desktop's notification server
//!   (`org.freedesktop.Notifications`; version 1.2 of its specification
//!   for the activation token) on the program's session bus (`bus`); the
//!   progress as the Unity launcher entry's `Update` signal, which
//!   Plasma's task manager and docks such as Ubuntu's show (GNOME's own
//!   dash shows none); sleep held off by logind's `Inhibit`, its
//!   descriptor kept meanwhile.
//! - Windows: toasts, `ITaskbarList3`'s progress on the window given, a
//!   power request. A toast is shown only for an `AppUserModelID` the
//!   system knows: the installer registers Skwir's as unpackaged programs
//!   do (`HKCU\Software\Classes\AppUserModelId\org.skwir.skwir`, its
//!   `DisplayName`); elsewhere (the zip, Wine) none shows, and a refusal
//!   is logged. The process is given no id of its own: the installer's
//!   shortcut has none to match (wixl cannot give it one), and a pinned
//!   shortcut's button would part from the window's. Compiled only from
//!   Linux, and tried under Wine, which has no toasts and stubs the
//!   taskbar and power requests.
//! - macOS: the user notification center, the dock tile's badge, an
//!   activity of the process's.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};

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

/// What a caller asked, in order, for the module's thread.
enum Ask {
    Notify(u64, Notice),
    /// Take back the notice shown, or only the one with this key.
    Withdraw(Option<u64>),
    Progress(Option<Progress>),
    KeepAwake(Option<String>),
    /// The window whose taskbar button shows the progress changed.
    #[cfg(windows)]
    Window,
}

type OnClick = Arc<dyn Fn(Clicked) + Send + Sync>;

/// What the module's thread is made with.
#[derive(Clone)]
struct Setup {
    /// The desktop file's name, the Windows `AppUserModelID`.
    app_id: String,
    app_name: String,
    clicked: OnClick,
    /// The connections asked for: the program's own, or a test's.
    #[cfg(target_os = "linux")]
    buses: linux::Buses,
    /// The notice shown, shared with the thread that follows the server.
    #[cfg(target_os = "linux")]
    showing: Arc<parking_lot::Mutex<linux::Showing>>,
    /// The window given (`Desktop::set_window`), 0 before.
    #[cfg(windows)]
    window: Arc<std::sync::atomic::AtomicIsize>,
}

/// What Skwir tells the desktop while long actions run. Dropped, it lets
/// go of the machine's sleep and hides the progress; notices already
/// shown stay.
pub struct Desktop {
    setup: Setup,
    /// The module's thread, started when first asked: `None` when it
    /// could not start.
    asks: OnceLock<Option<Sender<Ask>>>,
}

// Held by the UI and handed to its threads.
const _: fn() = || {
    fn shared<T: Send + Sync>() {}
    shared::<Desktop>();
};

impl Desktop {
    /// `app_id` "org.skwir.skwir" (the desktop file's name, the Windows
    /// `AppUserModelID` toasts are shown for), `app_name` "skwir". Starts no thread and opens no
    /// connection until first asked: a run that never asks never touches a
    /// bus. `clicked` is called on a thread of the module's (on Windows,
    /// of the system's), and must return at once. On Linux and Windows a
    /// click only tells: bringing the window forward is the caller's, with
    /// the token on Wayland. macOS brings the program forward itself and
    /// tells nothing.
    ///
    /// Starting is asking something to be shown or held: taking back,
    /// hiding or letting go while nothing was asked starts nothing.
    pub fn new(
        app_id: &str,
        app_name: &str,
        clicked: impl Fn(Clicked) + Send + Sync + 'static,
    ) -> Desktop {
        Desktop::with(Setup {
            app_id: app_id.to_owned(),
            app_name: app_name.to_owned(),
            clicked: Arc::new(clicked),
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
            asks: OnceLock::new(),
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

    /// Windows only: the window whose taskbar button shows the progress.
    #[cfg(windows)]
    pub fn set_window(&self, hwnd: isize) {
        self.setup
            .window
            .store(hwnd, std::sync::atomic::Ordering::Relaxed);
        if self.asks.get().is_some() {
            self.ask(Ask::Window);
        }
    }

    fn ask(&self, ask: Ask) {
        // What only undoes needs no thread while nothing was done.
        let undoes = matches!(
            ask,
            Ask::Withdraw(_) | Ask::Progress(None) | Ask::KeepAwake(None)
        );
        if undoes && self.asks.get().is_none() {
            return;
        }
        if let Some(asks) = self.asks.get_or_init(|| start(self.setup.clone())) {
            // The thread ends only once this is dropped.
            let _ = asks.send(ask);
        }
    }
}

/// The module's thread, which ends once its sender is dropped.
fn start(setup: Setup) -> Option<Sender<Ask>> {
    let (asks, asked) = crossbeam_channel::unbounded();
    let started = std::thread::Builder::new()
        .name("skwir-desktop".into())
        .spawn(move || run(&setup, &asked));
    match started {
        Ok(_) => Some(asks),
        Err(error) => {
            tracing::debug!("the desktop is told nothing: {error}");
            None
        }
    }
}

/// Tell the desktop what is asked, in order, the progress paced, until the
/// [`Desktop`] is dropped: then let go of the machine's sleep and hide the
/// progress.
fn run(setup: &Setup, asked: &Receiver<Ask>) {
    let mut desktop = imp::Backend::new(setup);
    let mut progress = Paced::default();
    let mut awake = false;
    loop {
        let ask = match progress.due() {
            Some(due) => match asked.recv_deadline(due) {
                Ok(ask) => Some(ask),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match asked.recv() {
                Ok(ask) => Some(ask),
                Err(_) => break,
            },
        };
        match ask {
            Some(Ask::Notify(key, notice)) => desktop.notify(key, &notice),
            Some(Ask::Withdraw(key)) => desktop.withdraw(key),
            Some(Ask::Progress(asked)) => progress.want(imp::shown(asked)),
            Some(Ask::KeepAwake(Some(why))) if !awake => {
                awake = true;
                desktop.hold(&why);
            }
            Some(Ask::KeepAwake(None)) if awake => {
                awake = false;
                desktop.let_go();
            }
            // Held already with its first why, or let go already.
            Some(Ask::KeepAwake(_)) => {}
            #[cfg(windows)]
            Some(Ask::Window) => desktop.window(),
            None => {}
        }
        if progress.take(Instant::now()) {
            desktop.progress(progress.sent);
        }
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

/// The progress a system is told: a change at once, then none until
/// [`PACE`] has passed since, when the last asked is told.
#[derive(Default)]
struct Paced {
    wanted: Option<Shown>,
    sent: Option<Shown>,
    /// When one was last told.
    last: Option<Instant>,
}

impl Paced {
    fn want(&mut self, shown: Option<Shown>) {
        self.wanted = shown;
    }

    /// When the next is to be told, if anything differs from what was.
    fn due(&self) -> Option<Instant> {
        (self.wanted != self.sent).then(|| self.last.map_or_else(Instant::now, |last| last + PACE))
    }

    /// Whether something is to be told at `now`: then `sent` is what.
    fn take(&mut self, now: Instant) -> bool {
        if self.wanted == self.sent || self.last.is_some_and(|last| now < last + PACE) {
            return false;
        }
        self.sent = self.wanted;
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

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
use other as imp;
#[cfg(windows)]
use windows as imp;

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashMap;
    use std::os::fd::OwnedFd;
    use std::sync::Arc;
    use std::time::Duration;

    use parking_lot::Mutex;
    use skwir_core::CancelToken;
    use zbus::blocking::Connection;
    use zbus::zvariant::Value;

    use super::{Clicked, Kind, Notice, OnClick, Progress, Setup, Shown, State};

    /// The notification server: its name, object and interface.
    const NOTICES: (&str, &str, &str) = (
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    );
    /// logind's manager on the system bus.
    const LOGIN: (&str, &str, &str) = (
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    );
    const LAUNCHER_ENTRY: &str = "com.canonical.Unity.LauncherEntry";
    /// The action a click on the notice itself invokes.
    const DEFAULT: &str = "default";
    const ACTIONS: [&str; 2] = [DEFAULT, "Show"];
    /// How long a server is waited for: the session's connection is
    /// shared and has no timeout of its own, and one that hangs would
    /// hold every later ask.
    const ANSWER: Duration = Duration::from_secs(5);
    /// How long the server's signals are waited to be listened to before
    /// the first notice, so no click on it is missed.
    const LISTENING: Duration = Duration::from_secs(2);

    pub(super) type Connect = Arc<dyn Fn() -> zbus::Result<Connection> + Send + Sync>;

    /// Where the session's and the system's connections come from.
    #[derive(Clone)]
    pub(super) struct Buses {
        /// The session's: the program's own (`bus::session`), as its
        /// notification server may tell its signals to the connection that
        /// showed the notice alone.
        pub(super) session: Connect,
        /// The system's, made for a call and closed after it.
        pub(super) system: Connect,
    }

    impl Buses {
        pub(super) fn the_programs() -> Buses {
            Buses {
                session: Arc::new(crate::bus::session),
                system: Arc::new(Connection::system),
            }
        }
    }

    /// The notice shown, as the server knows it.
    #[derive(Default)]
    pub(super) struct Showing {
        /// The server's id of it, and the key it was shown with.
        pub(super) shown: Option<(u32, u64)>,
        /// The token the server gave last, for the notice of that id.
        token: Option<(u32, String)>,
    }

    impl Showing {
        fn is(&self, id: u32) -> bool {
            self.shown.is_some_and(|(shown, _)| shown == id)
        }
    }

    pub(super) struct Backend {
        app_id: String,
        app_name: String,
        clicked: OnClick,
        buses: Buses,
        showing: Arc<Mutex<Showing>>,
        /// The program's entry, as the launcher's signal names it.
        entry: String,
        /// The object the launcher's signal is said from.
        path: String,
        /// The thread following the server's signals, once a notice was
        /// shown, until this ends.
        following: Option<CancelToken>,
        /// logind's descriptor, while the machine is kept awake.
        inhibitor: Option<OwnedFd>,
        /// Whether the last progress could not be told: said once, until
        /// one can.
        untold: bool,
    }

    /// The progress as the launcher shows it: whole percents, none for an
    /// error (it has no such state); paused shown as it was.
    pub(super) fn shown(progress: Option<Progress>) -> Option<Shown> {
        progress
            .filter(|progress| progress.state != State::Error)
            .map(|progress| Shown {
                permille: super::permille(progress.fraction, 10),
                state: State::Normal,
            })
    }

    impl Backend {
        pub(super) fn new(setup: &Setup) -> Backend {
            let path = setup
                .app_id
                .split('.')
                .filter(|part| !part.is_empty())
                .map(|part| {
                    part.chars()
                        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                        .collect::<String>()
                })
                .fold(String::new(), |path, part| path + "/" + &part);
            Backend {
                app_id: setup.app_id.clone(),
                app_name: setup.app_name.clone(),
                clicked: Arc::clone(&setup.clicked),
                buses: setup.buses.clone(),
                showing: Arc::clone(&setup.showing),
                entry: format!("application://{}.desktop", setup.app_id),
                path: path + "/LauncherEntry",
                following: None,
                inhibitor: None,
                untold: false,
            }
        }

        pub(super) fn notify(&mut self, key: u64, notice: &Notice) {
            let bus = match (self.buses.session)() {
                Ok(bus) => bus,
                Err(error) => {
                    tracing::debug!("the notice was not shown: no session bus: {error}");
                    return;
                }
            };
            self.follow();
            // Escaped only for a server that reads markup: another shows
            // the body as written.
            let markup = call(&bus, NOTICES, "GetCapabilities", &())
                .and_then(|reply| reply.body().deserialize::<Vec<String>>())
                .is_ok_and(|capabilities| capabilities.iter().any(|c| c == "body-markup"));
            let body = if markup {
                super::escape(&notice.body)
            } else {
                notice.body.clone()
            };
            let replaces = self.showing.lock().shown.map_or(0, |(id, _)| id);
            let category = match notice.kind {
                Kind::Finished => "transfer.complete",
                Kind::Stopped => "transfer.error",
                Kind::Waiting => "transfer",
            };
            let hints = HashMap::from([
                ("category", Value::from(category)),
                ("desktop-entry", Value::from(self.app_id.as_str())),
                ("urgency", Value::U8(1)),
            ]);
            let shown = call(
                &bus,
                NOTICES,
                "Notify",
                &(
                    self.app_name.as_str(),
                    replaces,
                    self.app_id.as_str(),
                    notice.title.as_str(),
                    body.as_str(),
                    ACTIONS.as_slice(),
                    hints,
                    -1i32,
                ),
            )
            .and_then(|reply| reply.body().deserialize::<u32>());
            match shown {
                Ok(id) => self.showing.lock().shown = Some((id, key)),
                Err(error) => tracing::debug!("the notice was not shown: {error}"),
            }
        }

        pub(super) fn withdraw(&mut self, key: Option<u64>) {
            let id = {
                let mut showing = self.showing.lock();
                match showing.shown {
                    Some((id, shown)) if key.is_none_or(|key| key == shown) => {
                        showing.shown = None;
                        id
                    }
                    _ => return,
                }
            };
            let closed = (self.buses.session)()
                .and_then(|bus| call(&bus, NOTICES, "CloseNotification", &id));
            if let Err(error) = closed {
                tracing::debug!("the notice was not taken back: {error}");
            }
        }

        pub(super) fn progress(&mut self, shown: Option<Shown>) {
            let progress = shown.map_or(0.0, |shown| f64::from(shown.permille) / 1000.0);
            let properties = HashMap::from([
                ("progress", Value::F64(progress)),
                ("progress-visible", Value::Bool(shown.is_some())),
            ]);
            let told = (self.buses.session)().and_then(|bus| {
                bus.emit_signal(
                    None::<&str>,
                    self.path.as_str(),
                    LAUNCHER_ENTRY,
                    "Update",
                    &(self.entry.as_str(), properties),
                )
            });
            match told {
                Ok(()) => self.untold = false,
                Err(error) => {
                    if !std::mem::replace(&mut self.untold, true) {
                        tracing::debug!(
                            "the progress is not shown on the program's entry: {error}"
                        );
                    }
                }
            }
        }

        /// Ask logind to hold off sleep, on a system connection made for
        /// the call: the descriptor it answers outlives the connection.
        pub(super) fn hold(&mut self, why: &str) {
            let held = (self.buses.system)()
                .and_then(|bus| {
                    call(
                        &bus,
                        LOGIN,
                        "Inhibit",
                        &("sleep", self.app_name.as_str(), why, "block"),
                    )
                })
                .and_then(|reply| reply.body().deserialize::<zbus::zvariant::OwnedFd>());
            match held {
                Ok(fd) => self.inhibitor = Some(fd.into()),
                Err(error) => tracing::debug!("the machine may sleep meanwhile: {error}"),
            }
        }

        pub(super) fn let_go(&mut self) {
            self.inhibitor = None;
        }

        /// Follow the server's signals on a thread of their own, for the
        /// program's life (`bus::keep`): started with the first notice,
        /// which waits until they are listened to.
        fn follow(&mut self) {
            if self.following.is_some() {
                return;
            }
            let stop = CancelToken::new();
            self.following = Some(stop.clone());
            let (ready, listening) = crossbeam_channel::bounded::<()>(1);
            let connect = Arc::clone(&self.buses.session);
            let showing = Arc::clone(&self.showing);
            let clicked = Arc::clone(&self.clicked);
            let started = std::thread::Builder::new()
                .name("skwir-notices".into())
                .spawn(move || {
                    let rule = match rule() {
                        Ok(rule) => rule,
                        Err(error) => {
                            tracing::debug!("clicks on notices are not followed: {error}");
                            return;
                        }
                    };
                    let mut again = false;
                    crate::bus::keep(
                        || connect(),
                        &stop,
                        |bus| {
                            if std::mem::replace(&mut again, true) {
                                // The bus started again, and its server with
                                // it: what was shown went with them.
                                *showing.lock() = Showing::default();
                            }
                            let ready = ready.clone();
                            crate::bus::listen(
                                bus,
                                vec![rule.clone()],
                                &stop,
                                move || {
                                    let _ = ready.try_send(());
                                },
                                |message| told(message, &showing, &clicked),
                            );
                        },
                    );
                });
            match started {
                Ok(_) => {
                    let _ = listening.recv_timeout(LISTENING);
                }
                Err(error) => tracing::debug!("clicks on notices are not followed: {error}"),
            }
        }
    }

    impl Drop for Backend {
        fn drop(&mut self) {
            if let Some(following) = self.following.take() {
                following.cancel();
            }
        }
    }

    /// The server's signals: a notice clicked, the token given with the
    /// click, a notice closed.
    fn rule() -> zbus::Result<zbus::MatchRule<'static>> {
        Ok(zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(NOTICES.2)?
            .path(NOTICES.1)?
            .build())
    }

    /// A signal of the server's, taken at once: a click on the notice
    /// shown told with the token given just before it (spec 1.2), the
    /// notice closed forgotten. Others' notices are none of this one's.
    fn told(message: &zbus::Message, showing: &Mutex<Showing>, clicked: &OnClick) {
        let header = message.header();
        let Some(member) = header.member() else {
            return;
        };
        let body = message.body();
        match member.as_str() {
            "ActivationToken" => {
                let Ok((id, token)) = body.deserialize::<(u32, String)>() else {
                    return;
                };
                let mut showing = showing.lock();
                if showing.is(id) {
                    showing.token = Some((id, token));
                }
            }
            "ActionInvoked" => {
                let Ok((id, action)) = body.deserialize::<(u32, String)>() else {
                    return;
                };
                let click = {
                    let mut showing = showing.lock();
                    match showing.shown {
                        Some((shown, key)) if shown == id && action == DEFAULT => {
                            let token = showing
                                .token
                                .take()
                                .filter(|(of, _)| *of == id)
                                .map(|(_, token)| token);
                            Some(Clicked { key, token })
                        }
                        _ => None,
                    }
                };
                if let Some(click) = click {
                    clicked(click);
                }
            }
            "NotificationClosed" => {
                let Ok((id, _reason)) = body.deserialize::<(u32, u32)>() else {
                    return;
                };
                let mut showing = showing.lock();
                if showing.is(id) {
                    *showing = Showing::default();
                }
            }
            _ => {}
        }
    }

    /// Call `method` of `(name, path, interface)` on `bus`, waiting at most
    /// [`ANSWER`]: the reply given up on, zbus drops it when it comes.
    fn call<B>(
        bus: &Connection,
        (name, path, interface): (&str, &str, &str),
        method: &str,
        body: &B,
    ) -> zbus::Result<zbus::Message>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        let answered = bus
            .inner()
            .call_method(Some(name), path, Some(interface), method, body);
        zbus::block_on(futures_lite::future::or(answered, async {
            async_io::Timer::after(ANSWER).await;
            Err(zbus::Error::Failure(format!(
                "{name} did not answer {method} within {} s",
                ANSWER.as_secs()
            )))
        }))
    }
}

#[cfg(windows)]
mod windows {
    use std::cell::OnceCell;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicIsize, Ordering};

    use ::windows::Data::Xml::Dom::XmlDocument;
    use ::windows::Foundation::TypedEventHandler;
    use ::windows::UI::Notifications::{
        ToastNotification, ToastNotificationManager, ToastNotifier,
    };
    use ::windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
    use ::windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use ::windows::Win32::System::Power::{
        PowerClearRequest, PowerCreateRequest, PowerRequestSystemRequired, PowerSetRequest,
    };
    use ::windows::Win32::System::Threading::{
        POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0,
    };
    use ::windows::Win32::UI::Shell::{
        ITaskbarList3, TBPF_ERROR, TBPF_NOPROGRESS, TBPF_NORMAL, TBPF_PAUSED, TBPFLAG, TaskbarList,
    };
    use ::windows::core::{HSTRING, IInspectable, PWSTR};

    use super::{Clicked, Notice, OnClick, Progress, Setup, Shown, State};

    /// The tag and group every notice is shown with: the next takes the
    /// last one's place, in the notification center too.
    const TAG: &str = "notice";
    const GROUP: &str = "skwir";

    /// The progress as the taskbar shows it: thousandths, and its state.
    pub(super) fn shown(progress: Option<Progress>) -> Option<Shown> {
        progress.map(|progress| Shown {
            permille: super::permille(progress.fraction, 1),
            state: progress.state,
        })
    }

    pub(super) struct Backend {
        app_id: HSTRING,
        clicked: OnClick,
        window: Arc<AtomicIsize>,
        /// Whether COM was started on this thread, to be ended with it.
        com: bool,
        /// The taskbar's list, made once when first needed.
        taskbar: OnceCell<Option<ITaskbarList3>>,
        /// The progress asked last, shown again on a window given after.
        shown: Option<Shown>,
        /// The toasts' notifier, made once when first needed: none for an
        /// id the system does not know (the zip, Wine).
        notifier: OnceCell<Option<ToastNotifier>>,
        /// The toast shown and its key.
        toast: Option<(u64, ToastNotification)>,
        /// The power request, while the machine is kept awake.
        request: Option<HANDLE>,
    }

    impl Backend {
        pub(super) fn new(setup: &Setup) -> Backend {
            // Not asked of the system here, but the toast's name, and
            // logind's on Linux.
            let _ = &setup.app_name;
            #[allow(unsafe_code)]
            // SAFETY: this thread's own COM, multithreaded as it pumps no
            // messages (WinRT and the taskbar's list work from there),
            // ended when this is dropped.
            let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
            Backend {
                app_id: HSTRING::from(setup.app_id.as_str()),
                clicked: Arc::clone(&setup.clicked),
                window: Arc::clone(&setup.window),
                com,
                taskbar: OnceCell::new(),
                shown: None,
                notifier: OnceCell::new(),
                toast: None,
                request: None,
            }
        }

        pub(super) fn notify(&mut self, key: u64, notice: &Notice) {
            let Some(notifier) = self.notifier() else {
                return;
            };
            let toast = match self.toast(key, notice) {
                Ok(toast) => toast,
                Err(error) => {
                    tracing::debug!("the notice was not shown: {error}");
                    return;
                }
            };
            if let Some((_, shown)) = self.toast.take() {
                let _ = notifier.Hide(&shown);
            }
            match notifier.Show(&toast) {
                Ok(()) => self.toast = Some((key, toast)),
                Err(error) => tracing::debug!("the notice was not shown: {error}"),
            }
        }

        fn notifier(&self) -> Option<ToastNotifier> {
            self.notifier
                .get_or_init(|| {
                    ToastNotificationManager::CreateToastNotifierWithId(&self.app_id)
                        .inspect_err(|error| tracing::debug!("notices are not shown: {error}"))
                        .ok()
                })
                .clone()
        }

        /// A toast of `notice`, its click told with `key`. Its kind is not
        /// said: Windows files every toast alike. Windows does not bring
        /// the window forward on a click: the caller does.
        fn toast(&self, key: u64, notice: &Notice) -> ::windows::core::Result<ToastNotification> {
            let xml = format!(
                r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual></toast>"#,
                super::escape(&notice.title),
                super::escape(&notice.body)
            );
            let document = XmlDocument::new()?;
            document.LoadXml(&HSTRING::from(xml))?;
            let toast = ToastNotification::CreateToastNotification(&document)?;
            toast.SetTag(&HSTRING::from(TAG))?;
            toast.SetGroup(&HSTRING::from(GROUP))?;
            let clicked = Arc::clone(&self.clicked);
            toast.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(
                move |_, _| {
                    clicked(Clicked { key, token: None });
                    Ok(())
                },
            ))?;
            Ok(toast)
        }

        pub(super) fn withdraw(&mut self, key: Option<u64>) {
            let Some((_, toast)) = self
                .toast
                .take_if(|(shown, _)| key.is_none_or(|key| key == *shown))
            else {
                return;
            };
            if let Some(Some(notifier)) = self.notifier.get()
                && let Err(error) = notifier.Hide(&toast)
            {
                tracing::debug!("the notice was not taken back: {error}");
            }
            // From the notification center too, where it went once off the
            // screen.
            let removed = ToastNotificationManager::History().and_then(|history| {
                history.RemoveGroupedTagWithId(
                    &HSTRING::from(TAG),
                    &HSTRING::from(GROUP),
                    &self.app_id,
                )
            });
            if let Err(error) = removed {
                tracing::debug!("the notice was not taken back: {error}");
            }
        }

        pub(super) fn progress(&mut self, shown: Option<Shown>) {
            self.shown = shown;
            self.show_progress();
        }

        /// The window given changed: the progress shown on its button.
        pub(super) fn window(&mut self) {
            self.show_progress();
        }

        fn show_progress(&mut self) {
            let window = self.window.load(Ordering::Relaxed);
            if window == 0 {
                return;
            }
            let Some(taskbar) = self.taskbar() else {
                return;
            };
            let window = HWND(window as *mut core::ffi::c_void);
            #[allow(unsafe_code)]
            // SAFETY: the list's own calls, on a handle the caller gave: one
            // that is gone is refused, never followed.
            let shown = unsafe {
                match self.shown {
                    None => taskbar.SetProgressState(window, TBPF_NOPROGRESS),
                    Some(shown) => taskbar
                        .SetProgressValue(window, u64::from(shown.permille), 1000)
                        .and_then(|()| taskbar.SetProgressState(window, flag(shown.state))),
                }
            };
            // Refused too before the window's button is made.
            if let Err(error) = shown {
                tracing::debug!("the progress is not shown on the taskbar: {error}");
            }
        }

        fn taskbar(&self) -> Option<ITaskbarList3> {
            self.taskbar
                .get_or_init(|| {
                    if !self.com {
                        return None;
                    }
                    #[allow(unsafe_code)]
                    // SAFETY: COM started on this thread; the list made and
                    // started as its documentation asks, before any use.
                    let made = unsafe {
                        CoCreateInstance::<_, ITaskbarList3>(
                            &TaskbarList,
                            None,
                            CLSCTX_INPROC_SERVER,
                        )
                        .and_then(|taskbar| taskbar.HrInit().map(|()| taskbar))
                    };
                    made.inspect_err(|error| {
                        tracing::debug!("the progress is not shown on the taskbar: {error}");
                    })
                    .ok()
                })
                .clone()
        }

        pub(super) fn hold(&mut self, why: &str) {
            let mut reason: Vec<u16> = why.encode_utf16().chain([0]).collect();
            let context = REASON_CONTEXT {
                // POWER_REQUEST_CONTEXT_VERSION.
                Version: 0,
                Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
                Reason: REASON_CONTEXT_0 {
                    SimpleReasonString: PWSTR(reason.as_mut_ptr()),
                },
            };
            #[allow(unsafe_code)]
            // SAFETY: the context and its string live through the calls,
            // which copy them; the request made is set, or closed at once.
            let held = unsafe {
                PowerCreateRequest(&raw const context).and_then(|request| {
                    match PowerSetRequest(request, PowerRequestSystemRequired) {
                        Ok(()) => Ok(request),
                        Err(error) => {
                            let _ = CloseHandle(request);
                            Err(error)
                        }
                    }
                })
            };
            match held {
                Ok(request) => self.request = Some(request),
                Err(error) => tracing::debug!("the machine may sleep meanwhile: {error}"),
            }
        }

        /// Whether a power request is held.
        #[cfg(test)]
        pub(super) fn holds(&self) -> bool {
            self.request.is_some()
        }

        pub(super) fn let_go(&mut self) {
            if let Some(request) = self.request.take() {
                #[allow(unsafe_code)]
                // SAFETY: the request this made and set, cleared and closed
                // once.
                unsafe {
                    let _ = PowerClearRequest(request, PowerRequestSystemRequired);
                    let _ = CloseHandle(request);
                }
            }
        }
    }

    fn flag(state: State) -> TBPFLAG {
        match state {
            State::Normal => TBPF_NORMAL,
            State::Paused => TBPF_PAUSED,
            State::Error => TBPF_ERROR,
        }
    }

    impl Drop for Backend {
        fn drop(&mut self) {
            self.let_go();
            // COM's objects released before COM ends on this thread.
            self.taskbar.take();
            self.toast = None;
            self.notifier.take();
            if self.com {
                #[allow(unsafe_code)]
                // SAFETY: ends the COM this thread started, nothing of it
                // held any more.
                unsafe {
                    CoUninitialize();
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::cell::OnceCell;

    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, NSObjectProtocol, ProtocolObject};
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{
        NSActivityOptions, NSArray, NSBundle, NSError, NSProcessInfo, NSString,
    };
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
        UNUserNotificationCenter,
    };

    use super::{Notice, Progress, Setup, Shown, State};

    /// The progress as the dock's badge shows it: whole percents, none for
    /// an error; paused shown as it was.
    pub(super) fn shown(progress: Option<Progress>) -> Option<Shown> {
        progress
            .filter(|progress| progress.state != State::Error)
            .map(|progress| Shown {
                permille: super::permille(progress.fraction, 10),
                state: State::Normal,
            })
    }

    pub(super) struct Backend {
        app_id: String,
        /// The center, asked once: none outside an application bundle.
        center: OnceCell<Option<Retained<UNUserNotificationCenter>>>,
        /// The key of the notice shown.
        shown: Option<u64>,
        /// The activity holding off sleep, while kept awake.
        activity: Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
    }

    impl Backend {
        pub(super) fn new(setup: &Setup) -> Backend {
            // macOS brings the program forward itself on a click of its
            // notice: nothing is told. The center names the program by its
            // bundle.
            let _ = (&setup.clicked, &setup.app_name);
            Backend {
                app_id: setup.app_id.clone(),
                center: OnceCell::new(),
                shown: None,
                activity: None,
            }
        }

        fn identifier(&self, key: u64) -> Retained<NSString> {
            NSString::from_str(&format!("{}.notice.{key}", self.app_id))
        }

        fn center(&self) -> Option<Retained<UNUserNotificationCenter>> {
            self.center
                .get_or_init(|| {
                    // The center ends the program when it runs outside an
                    // application bundle (a build run from the tree).
                    if NSBundle::mainBundle().bundleIdentifier().is_none() {
                        tracing::debug!(
                            "notices are not shown: not run from an application bundle"
                        );
                        return None;
                    }
                    let center = UNUserNotificationCenter::currentNotificationCenter();
                    // Asked once: the system asks the user the first time.
                    let answered = RcBlock::new(|granted: Bool, _error: *mut NSError| {
                        if !granted.as_bool() {
                            tracing::debug!("notices are not shown: not allowed");
                        }
                    });
                    center.requestAuthorizationWithOptions_completionHandler(
                        UNAuthorizationOptions::Alert,
                        &answered,
                    );
                    Some(center)
                })
                .clone()
        }

        pub(super) fn notify(&mut self, key: u64, notice: &Notice) {
            let Some(center) = self.center() else {
                return;
            };
            if let Some(previous) = self.shown.replace(key).filter(|previous| *previous != key) {
                remove(&center, &self.identifier(previous));
            }
            let content = UNMutableNotificationContent::new();
            content.setTitle(&NSString::from_str(&notice.title));
            content.setBody(&NSString::from_str(&notice.body));
            let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                &self.identifier(key),
                &content,
                None,
            );
            let added = RcBlock::new(|error: *mut NSError| {
                if !error.is_null() {
                    tracing::debug!("the notice was not shown");
                }
            });
            center.addNotificationRequest_withCompletionHandler(&request, Some(&added));
        }

        pub(super) fn withdraw(&mut self, key: Option<u64>) {
            let Some(shown) = self
                .shown
                .take_if(|shown| key.is_none_or(|key| key == *shown))
            else {
                return;
            };
            if let Some(Some(center)) = self.center.get() {
                remove(center, &self.identifier(shown));
            }
        }

        /// The badge on the dock's tile, set on the main thread, where
        /// `AppKit` asks for it.
        #[allow(clippy::unused_self)] // Each system's backend is told alike.
        pub(super) fn progress(&mut self, shown: Option<Shown>) {
            let label = shown.map(|shown| format!("{}%", shown.permille / 10));
            dispatch2::DispatchQueue::main().exec_async(move || {
                let Some(main) = MainThreadMarker::new() else {
                    return;
                };
                let label = label.as_deref().map(NSString::from_str);
                NSApplication::sharedApplication(main)
                    .dockTile()
                    .setBadgeLabel(label.as_deref());
            });
        }

        pub(super) fn hold(&mut self, why: &str) {
            self.activity = Some(
                NSProcessInfo::processInfo().beginActivityWithOptions_reason(
                    NSActivityOptions::UserInitiated,
                    &NSString::from_str(why),
                ),
            );
        }

        pub(super) fn let_go(&mut self) {
            if let Some(activity) = self.activity.take() {
                #[allow(unsafe_code)]
                // SAFETY: the token `beginActivity` gave, ended once.
                unsafe {
                    NSProcessInfo::processInfo().endActivity(&activity);
                }
            }
        }
    }

    fn remove(center: &UNUserNotificationCenter, identifier: &NSString) {
        let identifiers = NSArray::from_slice(&[identifier]);
        center.removePendingNotificationRequestsWithIdentifiers(&identifiers);
        center.removeDeliveredNotificationsWithIdentifiers(&identifiers);
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod other {
    use super::{Notice, Progress, Setup, Shown};

    pub(super) fn shown(progress: Option<Progress>) -> Option<Shown> {
        let _ = progress;
        None
    }

    /// Nothing told: said once, when the module's thread starts.
    pub(super) struct Backend;

    impl Backend {
        pub(super) fn new(setup: &Setup) -> Backend {
            let _ = (&setup.app_id, &setup.app_name, &setup.clicked);
            tracing::debug!(
                "{}",
                skwir_core::system::not_yet("Telling the desktop of long actions")
            );
            Backend
        }

        pub(super) fn notify(&mut self, key: u64, notice: &Notice) {
            let _ = (key, notice);
        }

        pub(super) fn withdraw(&mut self, key: Option<u64>) {
            let _ = key;
        }

        pub(super) fn progress(&mut self, shown: Option<Shown>) {
            let _ = shown;
        }

        pub(super) fn hold(&mut self, why: &str) {
            let _ = why;
        }

        pub(super) fn let_go(&mut self) {}
    }
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
        let mut paced = Paced::default();
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

    /// The Windows backend, driven where Wine answers: a power request held
    /// and let go; the taskbar and toasts asked, which Wine may lack, only
    /// logged then.
    #[cfg(windows)]
    #[test]
    fn windows_backend_holds_and_lets_go_and_survives_what_is_missing() {
        let setup = Setup {
            // An id nobody registered: no toast is shown, on Windows either.
            app_id: "org.skwir.skwir.test".into(),
            app_name: "skwir".into(),
            clicked: Arc::new(|_| {}),
            window: Arc::default(),
        };
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
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::{BufRead, Read};
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicU32, Ordering};

    use parking_lot::Mutex;
    use skwir_core::CancelToken;
    use zbus::blocking::Connection;
    use zbus::zvariant::OwnedValue;

    /// A bus daemon of the test's own, in a folder of its own under the
    /// tests' cache: never the session's or the system's. Stopped when
    /// dropped.
    struct Daemon {
        child: std::process::Child,
        address: String,
        _folder: tempfile::TempDir,
    }

    impl Drop for Daemon {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// `None` without `dbus-daemon`.
    fn daemon() -> Option<Daemon> {
        let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let folder = tempfile::Builder::new()
            .prefix("desktop-")
            .tempdir_in(base)
            .unwrap();
        let config = folder.path().join("bus.conf");
        std::fs::write(
            &config,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir={}</listen>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#,
                folder.path().display()
            ),
        )
        .unwrap();
        let mut child = std::process::Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        let mut address = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        Some(Daemon {
            child,
            address: address.trim().to_owned(),
            _folder: folder,
        })
    }

    fn connect(address: &str) -> zbus::Result<Connection> {
        zbus::blocking::connection::Builder::address(address)?.build()
    }

    fn until(what: &str, done: &dyn Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "{what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[derive(Debug)]
    struct Notified {
        app_name: String,
        replaces: u32,
        icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        timeout: i32,
        id: u32,
    }

    impl Notified {
        fn hint(&self, key: &str) -> Option<&OwnedValue> {
            self.hints.get(key)
        }
    }

    /// What the stand-in servers were asked, and the ends they kept of
    /// the descriptors they handed out.
    #[derive(Default)]
    struct Seen {
        notified: Vec<Notified>,
        closed: Vec<u32>,
        inhibited: Vec<[String; 4]>,
        held: Vec<UnixStream>,
    }

    /// A notification server, as the specification has it.
    struct Notifications {
        seen: Arc<Mutex<Seen>>,
        next: AtomicU32,
    }

    #[zbus::interface(name = "org.freedesktop.Notifications")]
    #[allow(clippy::needless_pass_by_value)] // zbus hands its arguments by value.
    impl Notifications {
        #[allow(clippy::too_many_arguments)] // The specification's eight.
        fn notify(
            &self,
            app_name: String,
            replaces_id: u32,
            app_icon: String,
            summary: String,
            body: String,
            actions: Vec<String>,
            hints: HashMap<String, OwnedValue>,
            expire_timeout: i32,
        ) -> u32 {
            let id = if replaces_id == 0 {
                self.next.fetch_add(1, Ordering::SeqCst)
            } else {
                replaces_id
            };
            self.seen.lock().notified.push(Notified {
                app_name,
                replaces: replaces_id,
                icon: app_icon,
                summary,
                body,
                actions,
                hints,
                timeout: expire_timeout,
                id,
            });
            id
        }

        fn close_notification(&self, id: u32) {
            self.seen.lock().closed.push(id);
        }

        #[allow(clippy::unused_self)] // A method zbus serves.
        fn get_capabilities(&self) -> Vec<String> {
            ["actions", "body", "body-markup"]
                .map(String::from)
                .to_vec()
        }
    }

    /// logind's manager: an inhibitor is one end of a socket pair, the
    /// other kept here to see it closed.
    struct Login {
        seen: Arc<Mutex<Seen>>,
    }

    #[zbus::interface(name = "org.freedesktop.login1.Manager")]
    #[allow(clippy::needless_pass_by_value)] // zbus hands its arguments by value.
    impl Login {
        fn inhibit(
            &self,
            what: String,
            who: String,
            why: String,
            mode: String,
        ) -> zbus::zvariant::OwnedFd {
            let (kept, handed) = UnixStream::pair().unwrap();
            kept.set_nonblocking(true).unwrap();
            let mut seen = self.seen.lock();
            seen.inhibited.push([what, who, why, mode]);
            seen.held.push(kept);
            std::os::fd::OwnedFd::from(handed).into()
        }
    }

    /// Whether the other end of `kept` is still held open.
    fn open(kept: &UnixStream) -> bool {
        match (&*kept).read(&mut [0u8; 1]) {
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => true,
            Ok(0) => false,
            other => panic!("the inhibitor wrote: {other:?}"),
        }
    }

    /// A desktop told on a daemon of its own, its servers stood in for.
    struct Harness {
        /// First, so it ends before the daemon does.
        desktop: Option<Desktop>,
        clicks: Arc<Mutex<Vec<Clicked>>>,
        seen: Arc<Mutex<Seen>>,
        showing: Arc<Mutex<linux::Showing>>,
        /// The servers' connection, owning both names.
        server: Connection,
        daemon: Daemon,
    }

    impl Harness {
        fn desktop(&self) -> &Desktop {
            self.desktop.as_ref().unwrap()
        }

        fn notified(&self) -> usize {
            self.seen.lock().notified.len()
        }

        fn emit(&self, member: &str, body: &(impl serde::Serialize + zbus::zvariant::DynamicType)) {
            self.server
                .emit_signal(
                    None::<&str>,
                    "/org/freedesktop/Notifications",
                    "org.freedesktop.Notifications",
                    member,
                    body,
                )
                .unwrap();
        }

        /// The progresses told on the bus, from now: the entry's address,
        /// the progress and whether it shows.
        fn updates(&self) -> (Arc<Mutex<Vec<(String, f64, bool)>>>, CancelToken) {
            let told = Arc::new(Mutex::new(Vec::new()));
            let stop = CancelToken::new();
            let bus = connect(&self.daemon.address).unwrap();
            let rule = zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface("com.canonical.Unity.LauncherEntry")
                .and_then(|rule| rule.member("Update"))
                .unwrap()
                .build();
            let (ready, listening) = crossbeam_channel::bounded(1);
            {
                let (told, stop) = (Arc::clone(&told), stop.clone());
                std::thread::spawn(move || {
                    crate::bus::listen(
                        &bus,
                        vec![rule],
                        &stop,
                        move || ready.send(()).unwrap(),
                        |message| {
                            let (entry, properties) = message
                                .body()
                                .deserialize::<(String, HashMap<String, OwnedValue>)>()
                                .unwrap();
                            let progress = f64::try_from(&properties["progress"]).unwrap();
                            let visible = bool::try_from(&properties["progress-visible"]).unwrap();
                            told.lock().push((entry, progress, visible));
                        },
                    );
                });
            }
            listening.recv_timeout(Duration::from_secs(10)).unwrap();
            (told, stop)
        }
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            drop(self.desktop.take());
        }
    }

    fn harness() -> Option<Harness> {
        let Some(daemon) = daemon() else {
            eprintln!("skipped: dbus-daemon is not installed");
            return None;
        };
        let seen = Arc::new(Mutex::new(Seen::default()));
        let server = zbus::blocking::connection::Builder::address(daemon.address.as_str())
            .unwrap()
            .name("org.freedesktop.Notifications")
            .unwrap()
            .name("org.freedesktop.login1")
            .unwrap()
            .serve_at(
                "/org/freedesktop/Notifications",
                Notifications {
                    seen: Arc::clone(&seen),
                    next: AtomicU32::new(41),
                },
            )
            .unwrap()
            .serve_at(
                "/org/freedesktop/login1",
                Login {
                    seen: Arc::clone(&seen),
                },
            )
            .unwrap()
            .build()
            .unwrap();
        let client = connect(&daemon.address).unwrap();
        let address = daemon.address.clone();
        let clicks = Arc::new(Mutex::new(Vec::new()));
        let showing = Arc::new(Mutex::new(linux::Showing::default()));
        let desktop = Desktop::with(Setup {
            app_id: "org.skwir.skwir".into(),
            app_name: "skwir".into(),
            clicked: {
                let clicks = Arc::clone(&clicks);
                Arc::new(move |clicked| clicks.lock().push(clicked))
            },
            buses: linux::Buses {
                session: Arc::new(move || Ok(client.clone())),
                system: Arc::new(move || connect(&address)),
            },
            showing: Arc::clone(&showing),
        });
        Some(Harness {
            desktop: Some(desktop),
            clicks,
            seen,
            showing,
            server,
            daemon,
        })
    }

    fn notice(title: &str, body: &str, kind: Kind) -> Notice {
        Notice {
            title: title.into(),
            body: body.into(),
            kind,
        }
    }

    fn string(value: Option<&OwnedValue>) -> String {
        String::try_from(value.unwrap().try_clone().unwrap()).unwrap()
    }

    /// A notice is shown as the specification asks, the next in its place,
    /// and taken back by its key alone, once.
    #[test]
    fn notices_are_shown_in_one_place_and_taken_back() {
        let Some(harness) = harness() else { return };
        let desktop = harness.desktop();
        desktop.notify(1, notice("Copied", "Tom & Jerry's <files>", Kind::Finished));
        until("shown", &|| harness.notified() == 1);
        let first = {
            let seen = harness.seen.lock();
            let shown = &seen.notified[0];
            assert_eq!(shown.app_name, "skwir");
            assert_eq!(shown.replaces, 0);
            assert_eq!(shown.icon, "org.skwir.skwir");
            assert_eq!(shown.summary, "Copied");
            assert_eq!(
                shown.body, "Tom &amp; Jerry's &lt;files&gt;",
                "markup escaped"
            );
            assert_eq!(shown.actions, ["default", "Show"]);
            assert_eq!(string(shown.hint("category")), "transfer.complete");
            assert_eq!(string(shown.hint("desktop-entry")), "org.skwir.skwir");
            assert_eq!(u8::try_from(shown.hint("urgency").unwrap()).unwrap(), 1);
            assert_eq!(shown.timeout, -1);
            shown.id
        };

        desktop.notify(2, notice("Copy stopped", "No room", Kind::Stopped));
        until("shown in its place", &|| harness.notified() == 2);
        {
            let seen = harness.seen.lock();
            assert_eq!(seen.notified[1].replaces, first);
            assert_eq!(string(seen.notified[1].hint("category")), "transfer.error");
        }

        // Another key's: nothing taken back; its own: taken back.
        desktop.withdraw_key(1);
        desktop.withdraw_key(2);
        until("taken back", &|| !harness.seen.lock().closed.is_empty());
        // Nothing shown any more: nothing to take back, and the next is
        // shown anew. Asks are carried out in order: a close sent by
        // either would come before it.
        desktop.withdraw();
        desktop.notify(3, notice("Waiting", "Replace it?", Kind::Waiting));
        until("shown anew", &|| harness.notified() == 3);
        let seen = harness.seen.lock();
        assert_eq!(seen.closed, [first], "taken back once");
        assert_eq!(seen.notified[2].replaces, 0);
        assert_eq!(string(seen.notified[2].hint("category")), "transfer");
    }

    /// A click on the notice shown is told with its key and the token the
    /// server gave just before; another program's notice is not ours.
    #[test]
    fn a_click_on_the_notice_shown_is_told_with_its_token() {
        let Some(harness) = harness() else { return };
        harness
            .desktop()
            .notify(7, notice("Moved", "12 files", Kind::Finished));
        until("shown", &|| harness.notified() == 1);
        let id = harness.seen.lock().notified[0].id;

        harness.emit("ActivationToken", &(id, "token-7"));
        harness.emit("ActionInvoked", &(id, "default"));
        until("clicked", &|| harness.clicks.lock().len() == 1);
        assert_eq!(
            harness.clicks.lock()[0],
            Clicked {
                key: 7,
                token: Some("token-7".into())
            }
        );

        // Another program's notice, then ours again, without a token:
        // signals come in order, so a click told for the other would come
        // first.
        harness.emit("ActivationToken", &(id + 100, "token-other"));
        harness.emit("ActionInvoked", &(id + 100, "default"));
        harness.emit("ActionInvoked", &(id, "default"));
        until("clicked again", &|| harness.clicks.lock().len() == 2);
        assert_eq!(
            harness.clicks.lock()[1],
            Clicked {
                key: 7,
                token: None
            }
        );
    }

    /// A notice the server closed is forgotten: nothing taken back, and
    /// the next shown anew.
    #[test]
    fn a_notice_closed_by_the_server_is_not_taken_back() {
        let Some(harness) = harness() else { return };
        harness
            .desktop()
            .notify(5, notice("Copied", "3 files", Kind::Finished));
        until("shown", &|| harness.notified() == 1);
        let id = harness.seen.lock().notified[0].id;
        // Another's closed first: ours stays.
        harness.emit("NotificationClosed", &(id + 100, 2u32));
        harness.emit("NotificationClosed", &(id, 2u32));
        until("forgotten", &|| harness.showing.lock().shown.is_none());

        harness.desktop().withdraw();
        harness
            .desktop()
            .notify(6, notice("Copied", "4 files", Kind::Finished));
        until("shown anew", &|| harness.notified() == 2);
        let seen = harness.seen.lock();
        assert!(seen.closed.is_empty(), "nothing taken back");
        assert_eq!(seen.notified[1].replaces, 0);
    }

    /// Progress is told on the launcher's entry at a pace, the last asked
    /// told; what shows the same (a fraction of a percent, paused) is not
    /// told again; an error hides it.
    #[test]
    fn progress_is_told_paced_on_the_launcher_entry() {
        let Some(harness) = harness() else { return };
        let (told, stop) = harness.updates();
        let progress = |fraction, state| Some(Progress { fraction, state });
        for step in 0..=100u8 {
            harness
                .desktop()
                .progress(progress(f32::from(step) / 200.0, State::Normal));
        }
        until("the last asked told", &|| {
            told.lock()
                .last()
                .is_some_and(|last| (last.1 - 0.5).abs() < 1e-9)
        });
        {
            let told = told.lock();
            assert!(told.len() <= 3, "paced: {told:?}");
            for (entry, _, visible) in told.iter() {
                assert_eq!(entry, "application://org.skwir.skwir.desktop");
                assert!(visible);
            }
        }
        let before = told.lock().len();
        harness.desktop().progress(progress(0.504, State::Normal));
        harness.desktop().progress(progress(0.5, State::Paused));
        harness.desktop().progress(progress(0.5, State::Error));
        until("hidden", &|| told.lock().last().is_some_and(|last| !last.2));
        assert_eq!(told.lock().len(), before + 1, "only the change told");
        stop.cancel();
    }

    /// Kept awake, logind is asked once and its descriptor held until let
    /// go; dropped, the desktop lets go and hides the progress.
    #[test]
    fn kept_awake_holds_loginds_descriptor_until_let_go() {
        let Some(mut harness) = harness() else { return };
        let (told, stop) = harness.updates();
        let desktop = harness.desktop();
        desktop.keep_awake(Some("Copying 3 files".into()));
        desktop.keep_awake(Some("Moving".into()));
        // Asks are carried out in order: once this is told, both were.
        desktop.progress(Some(Progress {
            fraction: 0.25,
            state: State::Normal,
        }));
        until("told", &|| !told.lock().is_empty());
        {
            let seen = harness.seen.lock();
            assert_eq!(
                seen.inhibited,
                [["sleep", "skwir", "Copying 3 files", "block"].map(String::from)],
                "asked once, with the first why"
            );
            assert!(open(&seen.held[0]), "held");
        }

        desktop.keep_awake(None);
        until("let go", &|| !open(&harness.seen.lock().held[0]));
        desktop.keep_awake(Some("Copying again".into()));
        until("asked again", &|| harness.seen.lock().inhibited.len() == 2);
        assert_eq!(harness.seen.lock().inhibited[1][2], "Copying again");

        drop(harness.desktop.take());
        until("let go when dropped", &|| {
            let seen = harness.seen.lock();
            seen.held.len() == 2 && !open(&seen.held[1])
        });
        until("hidden when dropped", &|| {
            told.lock().last().is_some_and(|last| !last.2)
        });
        stop.cancel();
    }

    /// Nothing asked but what undoes: no thread, no bus touched.
    #[test]
    fn undoing_alone_starts_nothing() {
        let Some(harness) = harness() else { return };
        let desktop = harness.desktop();
        desktop.withdraw();
        desktop.withdraw_key(3);
        desktop.progress(None);
        desktop.keep_awake(None);
        assert!(desktop.asks.get().is_none());
    }
}
