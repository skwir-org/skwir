//! A running Skwir takes later launches over (ADR-115, ADR-122).
//!
//! Skwir's windows are one program's (ADR-122): a later launch hands the
//! running Skwir what it was asked to open and the desktop's token to bring
//! a window forward ([`hand_over`], before anything else of the program
//! starts), then exits, and the running Skwir opens a window for it (or a
//! tab), or, finishing what runs without a window (ADR-115), its windows
//! again. Only to a Skwir of the same build, reading the same settings and
//! state, on the same display ([`Instance`]): another is a program of its
//! own, so a window started with other folders (a test's, another user's
//! profile) never opens in this one. [`keep_apart`] makes this launch a
//! program of its own (`--new-instance`).
//!
//! - Linux: a name on the session bus, `org.skwir.skwir.I<key>`, held while
//!   taking over, its object `/org/skwir/skwir` serving
//!   `org.skwir.skwir.Finishing.Open(ay location, ay cwd, s token,
//!   aay select, b properties)` on
//!   the program's one connection (`crate::bus`), and served again
//!   there when the session's bus is started again. A path travels as its
//!   bytes; an empty location or token is none. Once the name is held, a
//!   marker, `$XDG_RUNTIME_DIR/skwir/running-<key>`, holds the running
//!   Skwir's pid: a launch where none runs stats it and does nothing
//!   more (no connection, no thread). Where it is there, the launch asks
//!   on a connection of its own, given up on after two seconds, as the
//!   shared one waits for good.
//! - Windows: a named pipe of the user's in this session,
//!   `\\.\pipe\org.skwir.skwir.running-<key>-<session>-<SID>`, its first
//!   instance made by the Skwir that takes over: only the user opens it,
//!   and no other machine reaches it (`crate::windows::pipe`). A launch
//!   lets that Skwir bring its window forward
//!   (`AllowSetForegroundWindow`), as Windows has no token to hand; its
//!   message is told beside the code that reads it (`frame`).
//! - macOS: `LaunchServices` brings the running application forward instead
//!   of starting another, and the UI follows gpui's `on_reopen` and
//!   `on_open_urls`: nothing is handed over here, and a launch from a
//!   terminal is a program of its own.
//!
//! A launch is handed over only once the running Skwir answered. One that
//! does not answer within two seconds (stopped, or gone and its marker
//! left after a crash) is given up on, and the launch goes on as a process
//! of its own; a marker that is not this Skwir's is never removed.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(any(target_os = "linux", windows))]
use std::time::Duration;

/// Which Skwir a launch is for (ADR-122): one of the same build, reading
/// the same settings and state, on the same display. Its key names the bus
/// name, the marker and the pipe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instance {
    key: String,
}

impl Instance {
    /// The instance `parts` make (each one's bytes, in order): a hash,
    /// written as 16 hexadecimal digits.
    #[must_use]
    pub fn of(parts: &[&std::ffi::OsStr]) -> Instance {
        // FNV-1a, 64 bits: the same for every build on every system.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for part in parts {
            for byte in part.as_encoded_bytes().iter().chain([&0xff]) {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        }
        Instance {
            key: format!("{hash:016x}"),
        }
    }

    /// This program's: its build (`process::build`), the settings and
    /// state files it reads, the display it draws on (Wayland's and
    /// X11's, as they are set).
    #[must_use]
    pub fn here() -> Instance {
        let build = crate::process::build().unwrap_or_default();
        let settings = skwir_core::settings::Settings::default_path();
        let state = skwir_core::settings::Settings::default_state_path().unwrap_or_default();
        let wayland = std::env::var_os("WAYLAND_DISPLAY").unwrap_or_default();
        let x11 = std::env::var_os("DISPLAY").unwrap_or_default();
        Instance::of(&[
            build.as_ref(),
            settings.as_os_str(),
            state.as_os_str(),
            &wayland,
            &x11,
        ])
    }

    /// Its key: 16 hexadecimal digits.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }
}

/// This launch is a program of its own (`--new-instance`): it hands
/// itself to no one, and takes no later launch over.
pub fn keep_apart() {
    APART.store(true, std::sync::atomic::Ordering::Release);
}

/// Set by [`keep_apart`].
static APART: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn apart() -> bool {
    APART.load(std::sync::atomic::Ordering::Acquire)
}

/// What a later launch asks of the Skwir it hands itself to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Opening {
    /// The launch's first argument as given: a folder (perhaps relative
    /// to `cwd`), or a location a plugin opens (`trash:`); `None` to open
    /// what the window had.
    pub location: Option<OsString>,
    /// The launch's working folder, which a relative `location` is read
    /// against.
    pub cwd: PathBuf,
    /// The desktop's token to bring the window forward: Wayland's
    /// `XDG_ACTIVATION_TOKEN`, X11's `DESKTOP_STARTUP_ID`.
    pub token: Option<String>,
    /// The names in `location` to select once it is listed: the files
    /// another program asks shown in their folder (ADR-140); none for a
    /// launch.
    pub select: Vec<OsString>,
    /// With them, Properties shown (`ShowItemProperties`).
    pub properties: bool,
}

/// What a launch viewing files (`skwir view`, `skw`, ADR-151) asks the
/// running Skwir of its instance: a viewer's window showing them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Viewing {
    /// The files, each a whole path, in the order given; none for the
    /// system's dialog to ask which.
    pub files: Vec<PathBuf>,
    /// The one of them standing for standard input: removed once the
    /// window showing it closes. Handed over, always the launch's own
    /// spool ([`spool_file`]): what serves launches refuses any other.
    pub piped: Option<PathBuf>,
    /// The desktop's token to bring the window forward ([`launch_token`]).
    pub token: Option<String>,
}

/// Where launches viewing standard input (`skw -`, ADR-151, 7) keep what
/// it gives: `view/` in skwir's cache, a folder for each launch.
#[must_use]
pub fn spools() -> PathBuf {
    skwir_core::cache::Cache::default_root().join("view")
}

/// The folder the launch `pid` keeps standard input in: `view/<pid>`,
/// made the user's alone (`folders::private`).
#[must_use]
pub fn spool_folder(pid: u32) -> PathBuf {
    spools().join(pid.to_string())
}

/// The file the launch `pid` keeps standard input in, `stdin`, or
/// `stdin.<extension>` once its type is known: the one name a Skwir taking
/// a viewing over removes as its window closes ([`Viewing::piped`]).
#[must_use]
pub fn spool_file(pid: u32, extension: Option<&str>) -> PathBuf {
    spool_folder(pid).join(match extension.filter(|extension| !extension.is_empty()) {
        Some(extension) => format!("{SPOOLED}.{extension}"),
        None => SPOOLED.to_owned(),
    })
}

/// What a spool's file is named, before its extension.
const SPOOLED: &str = "stdin";

/// Remove what stood for standard input, and its launch's folder once
/// empty; one already gone is no matter.
pub fn let_go_of_spool(piped: &std::path::Path) {
    match std::fs::remove_file(piped) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => tracing::debug!("what standard input gave stays: {error}"),
    }
    if let Some(folder) = piped.parent() {
        let _ = std::fs::remove_dir(folder);
    }
}

/// Whether `piped`, as a launch hands it over, is that launch's spool,
/// which the Skwir showing it may remove: one of `files`, exactly (byte for
/// byte) `<spools>/<pid>/stdin` or `stdin.<extension>`, `pid` the process
/// that asked (as the bus or the pipe says, never as it says itself), its
/// folder a real folder and the file a regular file, both the user's, no
/// link in either. Anything else would let a program that may talk to
/// Skwir (a sandboxed one allowed `org.freedesktop.FileManager1`, served on
/// the same connection) have any file of the user's removed.
#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
fn spooled_by(
    piped: &std::path::Path,
    files: &[PathBuf],
    spools: &std::path::Path,
    pid: u32,
) -> bool {
    let Some(name) = piped.file_name() else {
        return false;
    };
    let named = name.to_str().is_some_and(|name| {
        name == SPOOLED
            || name
                .strip_prefix(SPOOLED)
                .and_then(|rest| rest.strip_prefix('.'))
                .is_some_and(|extension| !extension.is_empty())
    });
    let folder = spools.join(pid.to_string());
    // Byte for byte as the launch makes it: no step back, no `.`, no other
    // spelling of the same place.
    let exact = folder.join(name).as_os_str() == piped.as_os_str();
    if !named
        || !exact
        || !files
            .iter()
            .any(|file| file.as_os_str() == piped.as_os_str())
    {
        return false;
    }
    let (Ok(held), Ok(file)) = (
        std::fs::symlink_metadata(&folder),
        std::fs::symlink_metadata(piped),
    ) else {
        return false;
    };
    #[cfg(unix)]
    let users = {
        use std::os::unix::fs::MetadataExt as _;
        let user = rustix::process::getuid().as_raw();
        held.uid() == user && file.uid() == user
    };
    #[cfg(not(unix))]
    let users = true;
    held.is_dir() && file.is_file() && users
}

/// The viewings whose windows closed while their launches wait, until
/// each launch went (its answer read) or was let go of: what the
/// program's end waits for ([`wait_told`]).
struct Telling {
    count: parking_lot::Mutex<usize>,
    none: parking_lot::Condvar,
}

static TELLING: Telling = Telling {
    count: parking_lot::const_mutex(0),
    none: parking_lot::Condvar::new(),
};

impl Telling {
    fn add(&self) {
        *self.count.lock() += 1;
    }

    fn done(&self) {
        let mut count = self.count.lock();
        *count = count.saturating_sub(1);
        if *count == 0 {
            self.none.notify_all();
        }
    }
}

/// Wait, two seconds at most, until each launch whose viewer's window
/// closed has read that it did (ADR-151, 1): the program ends with its last
/// viewer, and ending would take an answer not yet read along, its launch
/// then told the Skwir ended. At once when none waits: call it as the
/// program ends.
pub fn wait_told() {
    #[cfg(any(target_os = "linux", windows))]
    {
        let deadline = std::time::Instant::now() + PATIENCE;
        let mut count = TELLING.count.lock();
        while *count > 0 {
            if TELLING.none.wait_until(&mut count, deadline).timed_out() {
                tracing::debug!(waiting = *count, "launches not told their viewers closed");
                return;
            }
        }
    }
}

/// How a launch handing its files over ended ([`view_elsewhere`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewedElsewhere {
    /// No Skwir of this instance took them (none runs, none answered in
    /// time, or this system hands nothing over): the launch views them
    /// itself.
    NotTaken,
    /// Their window closed.
    Closed,
    /// The Skwir showing them ended before their window closed.
    Ended,
}

/// A viewing a launch handed over ([`view_elsewhere`]), for the window
/// that shows it: what it asks, whether its launch went, and how that
/// launch is told the window closed ([`Viewed::close`]). Every copy let go,
/// the launch is told it closed.
#[derive(Clone)]
pub struct Viewed {
    viewing: Arc<Viewing>,
    holder: Arc<ViewedHolder>,
}

impl std::fmt::Debug for Viewed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Viewed")
            .field("id", &self.holder.state.id)
            .field("viewing", &self.viewing)
            .finish_non_exhaustive()
    }
}

impl Viewed {
    /// A viewing of `viewing` that no launch waits for: for tests. What
    /// takes launches over makes the others.
    #[must_use]
    pub fn new(viewing: Viewing) -> Viewed {
        Viewed::made(viewing, false).0
    }

    /// A viewing of `viewing`; `followed` where a launch waits on it, which
    /// is told as its window closes ([`wait_told`]).
    fn made(viewing: Viewing, followed: bool) -> (Viewed, Arc<ViewedState>) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let state = Arc::new(ViewedState {
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            told: parking_lot::Mutex::new(ViewedTold {
                follow: if followed {
                    Follow::Waits
                } else {
                    Follow::None
                },
                ..ViewedTold::default()
            }),
            changed: parking_lot::Condvar::new(),
            #[cfg(target_os = "linux")]
            event: event_listener::Event::new(),
        });
        let viewed = Viewed {
            viewing: Arc::new(viewing),
            holder: Arc::new(ViewedHolder {
                state: Arc::clone(&state),
            }),
        };
        (viewed, state)
    }

    #[must_use]
    pub fn viewing(&self) -> &Viewing {
        &self.viewing
    }

    /// The window closed: its launch is told, and ends.
    pub fn close(&self) {
        self.holder.state.close();
    }

    /// Its window could not open (the system refused it): its launch is
    /// told nothing was shown, and views the files itself, as where no
    /// Skwir runs. Not for a dialog cancelled, which is [`Viewed::close`].
    pub fn not_shown(&self) {
        self.holder.state.told.lock().not_shown = true;
        self.holder.state.close();
    }

    /// Whether its launch went (its terminal's Ctrl+C, the terminal
    /// closed): the window then closes.
    #[must_use]
    pub fn is_gone(&self) -> bool {
        self.holder.state.told.lock().gone
    }

    /// Call `hook` once its launch goes, at once if it has; on the thread
    /// that is told, so it must return at once (a wake). Never once the
    /// window closed.
    pub fn when_gone(&self, hook: impl Fn() + Send + Sync + 'static) {
        let mut told = self.holder.state.told.lock();
        if told.closed {
            return;
        }
        if told.gone {
            drop(told);
            hook();
            return;
        }
        told.on_gone.push(Box::new(hook));
    }

    /// Its launch went: for tests, as what serves launches tells it.
    #[cfg(all(test, target_os = "linux"))]
    fn gone_for_test(&self) {
        self.holder.state.go();
    }
}

/// What every copy of a viewing holds: let go of by all, its window is
/// closed.
struct ViewedHolder {
    state: Arc<ViewedState>,
}

impl Drop for ViewedHolder {
    fn drop(&mut self) {
        self.state.close();
    }
}

/// Where the end of a viewing is told, shared by its window and what
/// serves its launch.
struct ViewedState {
    /// Its own number, for this program's life: what its launch waits on.
    id: u64,
    told: parking_lot::Mutex<ViewedTold>,
    // What serves a launch on the pipe waits on it (Windows).
    #[cfg_attr(not(windows), allow(dead_code))]
    changed: parking_lot::Condvar,
    // What serves a launch on the bus waits on it (Linux).
    #[cfg(target_os = "linux")]
    event: event_listener::Event,
}

#[derive(Default)]
struct ViewedTold {
    /// Its window closed.
    closed: bool,
    /// Its launch went.
    gone: bool,
    /// Its window could not open ([`Viewed::not_shown`]).
    not_shown: bool,
    follow: Follow,
    on_gone: Vec<Box<dyn Fn() + Send + Sync>>,
}

/// Whether a launch waits on a viewing, as what serves it follows it.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Follow {
    /// None does, or no longer ([`ViewedState::release`]).
    #[default]
    None,
    /// One waits on it.
    Waits,
    /// Its window closed while its launch waited, and that launch is not
    /// yet known to have read so: counted in [`TELLING`].
    Telling,
}

#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
impl ViewedState {
    fn close(&self) {
        {
            let mut told = self.told.lock();
            if told.closed {
                return;
            }
            told.closed = true;
            told.on_gone.clear();
            if told.follow == Follow::Waits && !told.gone {
                told.follow = Follow::Telling;
                TELLING.add();
            }
        }
        self.changed.notify_all();
        #[cfg(target_os = "linux")]
        self.event.notify(usize::MAX);
    }

    /// Its launch went: told to the window, unless it closed. Whether it
    /// was told: the window was still up (or yet to open), and nothing
    /// will remove what the launch kept of standard input but the Skwir.
    fn go(&self) -> bool {
        let hooks = {
            let mut told = self.told.lock();
            if told.closed || told.gone {
                return false;
            }
            told.gone = true;
            std::mem::take(&mut told.on_gone)
        };
        for hook in hooks {
            hook();
        }
        true
    }

    /// What serves its launch no longer follows it: its launch went, read
    /// that its window closed, or was let go of.
    fn release(&self) {
        let mut told = self.told.lock();
        if std::mem::take(&mut told.follow) == Follow::Telling {
            TELLING.done();
        }
    }

    #[cfg(target_os = "linux")]
    fn is_closed(&self) -> bool {
        self.told.lock().closed
    }

    fn is_not_shown(&self) -> bool {
        self.told.lock().not_shown
    }

    /// Once its window closes.
    #[cfg(target_os = "linux")]
    async fn closed(&self) {
        loop {
            let listener = self.event.listen();
            if self.is_closed() {
                return;
            }
            listener.await;
        }
    }

    /// Whether its window closed, waiting `within` at most for it.
    #[cfg(windows)]
    fn closed_within(&self, within: Duration) -> bool {
        let mut told = self.told.lock();
        if !told.closed {
            let _ = self.changed.wait_for(&mut told, within);
        }
        told.closed
    }
}

/// Held by what follows a viewing's launch: let go of however it ends, so
/// the program's end never waits for that launch ([`wait_told`]).
#[cfg(any(target_os = "linux", windows))]
struct Released(Arc<ViewedState>);

#[cfg(any(target_os = "linux", windows))]
impl Drop for Released {
    fn drop(&mut self) {
        self.0.release();
    }
}

/// How long a launch waits for the finishing Skwir's answer, and (on
/// Windows) the finishing Skwir for a launch to say what it asks: one
/// that takes longer is given up on.
#[cfg(any(target_os = "linux", windows))]
const PATIENCE: Duration = Duration::from_secs(2);

/// The token the desktop gave this launch, from its environment: Wayland's
/// `XDG_ACTIVATION_TOKEN`, else X11's `DESKTOP_STARTUP_ID`. Read only,
/// never removed. `None` on Windows and macOS, whose desktops give none.
#[must_use]
pub fn launch_token() -> Option<String> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        ["XDG_ACTIVATION_TOKEN", "DESKTOP_STARTUP_ID"]
            .into_iter()
            .find_map(|name| std::env::var(name).ok().filter(|token| !token.is_empty()))
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        None
    }
}

/// Hand `opening` to the running Skwir of this [`Instance`], if one is
/// there: whether it took it (this launch then ends). Called before
/// anything else of the program, so where none is it costs almost
/// nothing: a stat on Linux, a pipe not found on Windows; no bus
/// connection, no thread. A Skwir that does not answer within two seconds
/// is given up on (false). Always false on macOS and other systems, and
/// once [`keep_apart`] was asked.
pub fn hand_over(opening: &Opening) -> bool {
    if apart() {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        let instance = Instance::here();
        linux::marker(&instance).is_some_and(|marker| {
            linux::hand_over_at(
                &marker,
                &linux::name(&instance),
                linux::patient_session,
                opening,
            )
        })
    }
    #[cfg(windows)]
    {
        pipe::name(&Instance::here()).is_some_and(|name| pipe::hand_over_to(&name, opening))
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = opening;
        false
    }
}

/// Hand `viewing` to the running Skwir of this [`Instance`], as
/// [`hand_over`] hands a launch, and wait until the window showing it
/// closes: the launch stays in its terminal meanwhile, as a viewer there
/// does (ADR-151, 1), and where it ends first (its terminal's Ctrl+C, the
/// terminal closed) the window closes. Where none is, a stat (Linux) or a
/// pipe not found (Windows), nothing more. One that does not take it within
/// two seconds is given up on ([`ViewedElsewhere::NotTaken`]); taken, this
/// launch confirms it, and only then is it shown, so a Skwir answering
/// just too late shows nothing. [`ViewedElsewhere::NotTaken`] too where
/// that Skwir could not show it after all, or refused `piped` (not this
/// launch's spool, [`spool_file`]). Never taken on macOS and other
/// systems, nor once [`keep_apart`] was asked.
pub fn view_elsewhere(viewing: &Viewing) -> ViewedElsewhere {
    if apart() {
        return ViewedElsewhere::NotTaken;
    }
    #[cfg(target_os = "linux")]
    {
        let instance = Instance::here();
        linux::marker(&instance).map_or(ViewedElsewhere::NotTaken, |marker| {
            linux::view_at(
                &marker,
                &linux::name(&instance),
                linux::waiting_session,
                viewing,
            )
        })
    }
    #[cfg(windows)]
    {
        pipe::name(&Instance::here()).map_or(ViewedElsewhere::NotTaken, |name| {
            pipe::view_through(&name, viewing)
        })
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = viewing;
        ViewedElsewhere::NotTaken
    }
}

/// [`hand_over`] for a launch that was refused taking later launches over
/// ([`NotTaken::Elsewhere`]): the Skwir that holds them answers it. Two
/// launches at once both find no one there and go on, the one that loses
/// finding out here; it is handed over rather than stay a second program
/// (ADR-122). A marker the other has yet to write is no reason not to ask.
/// Waits up to two seconds for the other's answer: call it on a worker.
#[must_use]
pub fn hand_over_to_holder(opening: &Opening) -> bool {
    if apart() {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        linux::ask_holder(
            &linux::name(&Instance::here()),
            linux::patient_session,
            opening,
        )
    }
    #[cfg(windows)]
    {
        pipe::name(&Instance::here()).is_some_and(|name| pipe::hand_over_to(&name, opening))
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = opening;
        false
    }
}

/// For a program the bus started to show files for other programs
/// (`--file-manager`, ADR-140) that finds another Skwir of its instance
/// taking launches over ([`NotTaken::Elsewhere`]): take the request that
/// started it, which the bus holds until someone has the name
/// `org.freedesktop.FileManager1`, hand its launches to that Skwir
/// ([`hand_over_to_holder`]), and let the name go. Waits up to `wait` for
/// the request: call it on a worker. Whether a request came and was
/// handed whole. Always false on other systems, and once [`keep_apart`]
/// was asked.
#[must_use]
pub fn answer_for_holder(wait: std::time::Duration) -> bool {
    if apart() {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        match crate::bus::session() {
            Ok(bus) => linux::answer_for_holder_on(&bus, wait, hand_over_to_holder),
            Err(error) => {
                tracing::debug!("no session bus: {error}");
                false
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = wait;
        false
    }
}

/// [`answer_for_holder`] for other programs' dialogs (`--portal`,
/// ADR-141, 3): the request the bus started this program for, and what
/// else comes while it is answered, asked of the Skwir of this instance
/// that takes launches over (`Finishing.Choose`), each answered as it
/// answers, the asking program's Close passed on. Waits up to `wait` for
/// a request, then until each is answered (the user deciding): call it
/// on a worker. Whether a request came. Always false on other systems.
#[must_use]
pub fn answer_chooser_for_holder(wait: std::time::Duration) -> bool {
    if apart() {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        match crate::bus::session() {
            Ok(bus) => crate::chooser::linux::answer_for_holder_on(
                &bus,
                wait,
                &linux::name(&Instance::here()),
            ),
            Err(error) => {
                tracing::debug!("no session bus: {error}");
                false
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = wait;
        false
    }
}

/// Why later launches are not taken over here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotTaken {
    /// Another Skwir of this instance takes them: this one, if it is a
    /// launch, is theirs ([`hand_over_to_holder`]).
    Elsewhere,
    /// It cannot be: no session bus or runtime folder (Linux), this
    /// Skwir does already, on macOS and other systems, once [`keep_apart`]
    /// was asked.
    Cannot,
}

/// While the returned value is held, later launches are handed here:
/// `taken` is called with each, on a thread of the module's (the session
/// bus's on Linux, the pipe's on Windows), before the launch is answered;
/// it must return at once, make no call on the session bus, and never
/// drop the [`TakingOver`] (which waits for it). [`NotTaken`] says why
/// not.
///
/// Waits for the bus's answer to the name asked (Linux): call it on a
/// worker.
pub fn take_over(taken: impl Fn(Opening) + Send + Sync + 'static) -> Result<TakingOver, NotTaken> {
    take_over_gate(Gate::new(taken))
}

/// [`take_over`], other programs' open and save dialogs taken too and
/// handed to `chose` (ADR-141, 2): on Linux the portal's back end is
/// served beside, its name asked in the queue behind a skwir the bus
/// started. And the files a launch views (`skw`, ADR-151, 1), handed to
/// `viewed` (Linux and Windows). Each is called as `taken` is, and must
/// return at once.
pub fn take_over_all(
    taken: impl Fn(Opening) + Send + Sync + 'static,
    chose: impl Fn(crate::chooser::Choosing) + Send + Sync + 'static,
    viewed: impl Fn(Viewed) + Send + Sync + 'static,
) -> Result<TakingOver, NotTaken> {
    take_over_gate(Gate::taking(
        Box::new(taken),
        Some(Box::new(chose)),
        Some(Box::new(viewed)),
    ))
}

fn take_over_gate(gate: Arc<Gate>) -> Result<TakingOver, NotTaken> {
    if apart() {
        return Err(NotTaken::Cannot);
    }
    #[cfg(target_os = "linux")]
    {
        let instance = Instance::here();
        linux::take_over_on(
            crate::bus::session,
            linux::marker(&instance).ok_or(NotTaken::Cannot)?,
            linux::name(&instance),
            gate,
        )
    }
    #[cfg(windows)]
    {
        pipe::take_over_on(pipe::name(&Instance::here()).ok_or(NotTaken::Cannot)?, gate)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        drop(gate);
        Err(NotTaken::Cannot)
    }
}

/// Taking later launches over: dropped, it stops (and takes back what it
/// set up: the name, the object and our marker on Linux; the pipe on
/// Windows). Once the drop returns, `taken` is not called again, and a
/// launch it did not answer goes on as a process of its own; one taken
/// just before is still to be opened.
pub struct TakingOver {
    gate: Arc<Gate>,
    #[cfg(target_os = "linux")]
    held: Option<linux::Held>,
    #[cfg(windows)]
    held: Option<pipe::Held>,
    #[cfg(target_os = "linux")]
    answer: Arc<linux::Answer>,
}

/// Whether the Skwir that takes launches over answers other programs'
/// requests to show folders and files too (ADR-140): a handle of
/// [`TakingOver::answering`], kept by what follows the user's choice.
/// What it answers is handed in as launches are, through the same
/// `taken`. Only on Linux: elsewhere it answers nothing.
#[derive(Clone)]
pub struct Answering {
    #[cfg(target_os = "linux")]
    answer: Arc<linux::Answer>,
}

impl Answering {
    /// Answer, or no longer: on Linux the name
    /// `org.freedesktop.FileManager1` asked (waiting behind a holder that
    /// does not let go) or let go, again on each connection the bus is
    /// given on. Waits for the bus: call it on a worker. Once taking over
    /// stopped, nothing.
    pub fn set(&self, answer: bool) {
        #[cfg(target_os = "linux")]
        self.answer.set(answer);
        #[cfg(not(target_os = "linux"))]
        let _ = answer;
    }
}

impl TakingOver {
    /// What says whether this Skwir answers other programs' requests to
    /// show folders and files ([`Answering`]): not at first.
    #[must_use]
    pub fn answering(&self) -> Answering {
        Answering {
            #[cfg(target_os = "linux")]
            answer: Arc::clone(&self.answer),
        }
    }

    /// Stop taking launches over, unless `waiting` says one already was
    /// (asked with no launch being taken meanwhile): whether it stopped.
    /// Stopped, launches are refused as after [`TakingOver::stop_aside`],
    /// which still takes down what serves them. Not stopped, nothing
    /// changed and taking over goes on: a program about to end looks here
    /// so that no launch handed over at the last moment is lost.
    pub fn close_unless(&self, waiting: impl FnOnce() -> bool) -> bool {
        self.gate.close_unless(waiting)
    }

    /// Stop as a drop does, what serves launches taken down on a thread of
    /// its own (the name, the object and our marker on Linux, a call on
    /// the bus; the pipe on Windows, up to [`PATIENCE`]): the window's
    /// thread does not wait for it. Once this returns, `taken` is not
    /// called again, and a launch meanwhile goes on as a process of its
    /// own.
    pub fn stop_aside(self) {
        self.gate.close();
        #[cfg(any(target_os = "linux", windows))]
        {
            let mut this = self;
            if let Some(held) = this.held.take() {
                // Without a thread, taken down here (the closure dropped).
                let spawned = std::thread::Builder::new()
                    .name("skwir-let-go".into())
                    .spawn(move || drop(held));
                if let Err(error) = spawned {
                    tracing::debug!("what took launches over is let go here: {error}");
                }
            }
        }
    }
}

impl Drop for TakingOver {
    fn drop(&mut self) {
        // Closed first: a launch being taken is done before this goes on,
        // and none is taken after, whatever the bus or the pipe still
        // delivers while what serves it stops (`held`, dropped next).
        self.gate.close();
    }
}

/// What a launch handed over is given to, until closed.
#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
struct Gate(parking_lot::Mutex<Option<Taker>>);

/// What takes what the gate passes: launches, other programs' dialogs
/// where they are taken (ADR-141), and the files launches view (ADR-151).
struct Taker {
    opening: Box<dyn Fn(Opening) + Send + Sync>,
    // Dialogs come only through the portal (Linux, ADR-141, 11).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    choosing: Option<Choose>,
    #[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
    viewing: Option<View>,
}

type Choose = Box<dyn Fn(crate::chooser::Choosing) + Send + Sync>;
type View = Box<dyn Fn(Viewed) + Send + Sync>;

#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
impl Gate {
    fn new(taken: impl Fn(Opening) + Send + Sync + 'static) -> Arc<Gate> {
        Gate::taking(Box::new(taken), None, None)
    }

    /// A gate that passes other programs' dialogs to `chose` too.
    #[cfg(all(test, target_os = "linux"))]
    fn choosing(
        taken: impl Fn(Opening) + Send + Sync + 'static,
        chose: impl Fn(crate::chooser::Choosing) + Send + Sync + 'static,
    ) -> Arc<Gate> {
        Gate::taking(Box::new(taken), Some(Box::new(chose)), None)
    }

    /// A gate that passes the files launches view to `viewed` too.
    #[cfg(all(test, any(target_os = "linux", windows)))]
    fn viewing(
        taken: impl Fn(Opening) + Send + Sync + 'static,
        viewed: impl Fn(Viewed) + Send + Sync + 'static,
    ) -> Arc<Gate> {
        Gate::taking(Box::new(taken), None, Some(Box::new(viewed)))
    }

    fn taking(
        opening: Box<dyn Fn(Opening) + Send + Sync>,
        choosing: Option<Choose>,
        viewing: Option<View>,
    ) -> Arc<Gate> {
        Arc::new(Gate(parking_lot::Mutex::new(Some(Taker {
            opening,
            choosing,
            viewing,
        }))))
    }

    /// Whether it passes dialogs: what serves them is set up then alone.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn takes_choosing(&self) -> bool {
        self.0
            .lock()
            .as_ref()
            .is_some_and(|taker| taker.choosing.is_some())
    }

    /// Whether it still passes the files launches view: asked before one is
    /// taken, which is passed only once its launch confirms it.
    fn takes_viewing(&self) -> bool {
        self.0
            .lock()
            .as_ref()
            .is_some_and(|taker| taker.viewing.is_some())
    }

    /// Give `opening` to the taker if it still takes: whether it took it.
    /// Held meanwhile, so [`Gate::close`] waits for it.
    fn pass(&self, opening: Opening) -> bool {
        let held = self.0.lock();
        let Some(taker) = held.as_ref() else {
            return false;
        };
        (taker.opening)(opening);
        true
    }

    /// Give a dialog to the taker if it still takes dialogs: whether it
    /// took it. Held meanwhile, as [`Gate::pass`].
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn pass_choosing(&self, choosing: crate::chooser::Choosing) -> bool {
        let held = self.0.lock();
        let Some(choose) = held.as_ref().and_then(|taker| taker.choosing.as_ref()) else {
            return false;
        };
        choose(choosing);
        true
    }

    /// Give the files a launch views to the taker if it still takes them:
    /// whether it took them. Held meanwhile, as [`Gate::pass`].
    fn pass_viewing(&self, viewed: Viewed) -> bool {
        let held = self.0.lock();
        let Some(view) = held.as_ref().and_then(|taker| taker.viewing.as_ref()) else {
            return false;
        };
        view(viewed);
        true
    }

    fn close(&self) {
        self.0.lock().take();
    }

    /// Close, unless `waiting` holds, asked with no launch being passed:
    /// whether it closed.
    fn close_unless(&self, waiting: impl FnOnce() -> bool) -> bool {
        let mut held = self.0.lock();
        if waiting() {
            return false;
        }
        held.take();
        true
    }
}

#[cfg(target_os = "linux")]
pub(crate) mod linux {
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use skwir_core::CancelToken;
    use zbus::blocking::Connection;
    use zbus::fdo::{RequestNameFlags, RequestNameReply};

    use super::{Gate, Instance, NotTaken, Opening, PATIENCE, TakingOver};

    /// What the bus name of an instance begins with.
    pub(super) const NAME: &str = "org.skwir.skwir";
    /// The name and object other programs ask to show folders and files
    /// (ADR-140), as Nautilus and Dolphin serve them.
    pub(super) const FILE_MANAGER: &str = "org.freedesktop.FileManager1";
    const FILE_MANAGER_PATH: &str = "/org/freedesktop/FileManager1";
    pub(crate) const PATH: &str = "/org/skwir/skwir";
    /// As [`Finishing`] is declared.
    pub(crate) const INTERFACE: &str = "org.skwir.skwir.Finishing";
    /// The most a marker holds: a pid and a line's end.
    const MARKER_BOUND: u64 = 32;

    /// Where the running Skwir of `instance` says so: in skwir's runtime
    /// folder.
    pub(super) fn marker(instance: &Instance) -> Option<PathBuf> {
        skwir_core::folders::runtime()
            .map(|folder| folder.join(format!("running-{}", instance.key())))
    }

    /// The bus name the running Skwir of `instance` holds: an element of
    /// a name never begins with a digit.
    pub(super) fn name(instance: &Instance) -> String {
        format!("{NAME}.I{}", instance.key())
    }

    /// A session connection of its own, whose calls are given up on
    /// after [`PATIENCE`]: the shared one has none, and a finishing Skwir
    /// that stopped answering would hold the launch for good.
    pub(super) fn patient_session() -> zbus::Result<Connection> {
        zbus::blocking::connection::Builder::session()?
            .method_timeout(PATIENCE)
            .build()
    }

    /// [`super::hand_over`] to the Skwir `marker` says finishes, on the
    /// bus `connect` connects to, made only when the marker is there.
    pub(super) fn hand_over_at(
        marker: &Path,
        name: &str,
        connect: impl FnOnce() -> zbus::Result<Connection>,
        opening: &Opening,
    ) -> bool {
        // Where no Skwir finishes: a stat, and nothing more.
        if std::fs::symlink_metadata(marker).is_err() {
            return false;
        }
        ask_holder(name, connect, opening)
    }

    /// Hand `opening` to the Skwir holding `name`, on the bus `connect`
    /// connects to: whether it took it.
    pub(super) fn ask_holder(
        name: &str,
        connect: impl FnOnce() -> zbus::Result<Connection>,
        opening: &Opening,
    ) -> bool {
        let ask = || -> zbus::Result<bool> {
            let bus = connect()?;
            // Asked first, so a name no one holds is not started by the
            // bus (were a service file to name it), and a stale marker
            // costs no failed call.
            let dbus = zbus::blocking::fdo::DBusProxy::new(&bus)?;
            if !dbus.name_has_owner(name.try_into()?)? {
                return Ok(false);
            }
            let location = opening
                .location
                .as_deref()
                .map_or(&[][..], OsStr::as_encoded_bytes);
            let cwd = opening.cwd.as_os_str().as_encoded_bytes();
            let token = opening.token.as_deref().unwrap_or_default();
            let select: Vec<&[u8]> = opening
                .select
                .iter()
                .map(|name| name.as_encoded_bytes())
                .collect();
            bus.call_method(
                Some(name),
                PATH,
                Some(INTERFACE),
                "Open",
                &(location, cwd, token, select, opening.properties),
            )?;
            Ok(true)
        };
        match ask() {
            Ok(taken) => taken,
            Err(error) => {
                tracing::debug!("this launch is not handed over: {error}");
                false
            }
        }
    }

    /// A launch's own connection for viewing files ([`super::view_elsewhere`]),
    /// made within the launch's patience (`view_at`): its calls are never
    /// given up on, as a viewer's window may stay for hours; asking is given
    /// up on after [`PATIENCE`] by the launch itself, connecting included.
    pub(super) fn waiting_session() -> zbus::Result<zbus::connection::Builder<'static>> {
        zbus::connection::Builder::session()
    }

    /// The error a launch's `View` or `Viewed` is answered when nothing is
    /// shown, nor will be: the launch views its files itself.
    pub(super) const NOT_TAKEN: &str = "org.skwir.skwir.Error.NotTaken";

    /// What `Finishing` answers a launch when it does not show what it
    /// views.
    #[derive(zbus::DBusError, Debug)]
    #[zbus(prefix = "org.skwir.skwir.Error")]
    enum Refused {
        #[zbus(error)]
        ZBus(zbus::Error),
        /// Nothing was shown, nor will be ([`NOT_TAKEN`]).
        NotTaken(String),
        /// Not answered (Skwir no longer takes launches over, a viewing it
        /// does not know): the Skwir showing it ended, for the launch.
        Failed(String),
    }

    impl From<zbus::fdo::Error> for Refused {
        fn from(error: zbus::fdo::Error) -> Refused {
            Refused::ZBus(error.into())
        }
    }

    /// [`super::view_elsewhere`] to the Skwir `marker` says finishes, as
    /// [`hand_over_at`], on the bus `connect` builds a connection to (made
    /// only when the marker is there). Taken in two steps, so that only the
    /// launch decides whether it is: `View` asked of the one that holds
    /// `name` within [`PATIENCE`] (connecting included), answered with the
    /// viewing's number and nothing shown; then `Viewed`, whose first asking
    /// confirms it (shown from then on, this launch no longer giving up),
    /// answered once its window closes, asked again should the bus give up
    /// on it while that Skwir still holds the name. A launch that gave up
    /// confirms nothing, and the Skwir lets its viewing go unseen.
    pub(super) fn view_at(
        marker: &Path,
        name: &str,
        connect: impl FnOnce() -> zbus::Result<zbus::connection::Builder<'static>>,
        viewing: &super::Viewing,
    ) -> super::ViewedElsewhere {
        use super::ViewedElsewhere;
        if std::fs::symlink_metadata(marker).is_err() {
            return ViewedElsewhere::NotTaken;
        }
        async_io::block_on(async {
            let asked = async {
                // Within the patience too: a bus that takes the socket and
                // never says hello holds this launch no longer than that.
                let bus = connect()?.build().await?;
                let dbus = zbus::fdo::DBusProxy::new(&bus).await?;
                let name = zbus::names::BusName::try_from(name)?;
                // Asked first, so a name no one holds is not started by
                // the bus, and a stale marker costs no failed call.
                if !dbus.name_has_owner(name.clone()).await? {
                    return Ok(None);
                }
                let holder = dbus.get_name_owner(name).await?;
                let files: Vec<&[u8]> = viewing
                    .files
                    .iter()
                    .map(|file| file.as_os_str().as_encoded_bytes())
                    .collect();
                let piped = viewing
                    .piped
                    .as_deref()
                    .map_or(&[][..], |piped| piped.as_os_str().as_encoded_bytes());
                let token = viewing.token.as_deref().unwrap_or_default();
                let reply = bus
                    .call_method(
                        Some(holder.as_str()),
                        PATH,
                        Some(INTERFACE),
                        "View",
                        &(files, piped, token),
                    )
                    .await?;
                let id: u64 = reply.body().deserialize()?;
                Ok::<_, zbus::Error>(Some((bus, holder, id, dbus)))
            };
            let late = async {
                async_io::Timer::after(PATIENCE).await;
                Err(zbus::Error::Failure("no answer".into()))
            };
            // Given up on, the answer that comes later is never read, and
            // this connection goes as this returns: the Skwir, which shows
            // nothing unconfirmed, lets the viewing go.
            let (bus, holder, id, dbus) = match futures_lite::future::or(asked, late).await {
                Ok(Some(taken)) => taken,
                Ok(None) => return ViewedElsewhere::NotTaken,
                Err(error) => {
                    tracing::debug!("these files are not handed over: {error}");
                    return ViewedElsewhere::NotTaken;
                }
            };
            loop {
                let waited = bus
                    .call_method(
                        Some(holder.as_str()),
                        PATH,
                        Some(INTERFACE),
                        "Viewed",
                        &(id,),
                    )
                    .await;
                let Err(error) = waited else {
                    return ViewedElsewhere::Closed;
                };
                let named = |names: &[&str]| {
                    matches!(&error, zbus::Error::MethodError(name, ..)
                        if names.contains(&name.as_str()))
                };
                // Not shown, nor to be: viewed here.
                if named(&[NOT_TAKEN]) {
                    tracing::debug!("these files were not taken: {error}");
                    return ViewedElsewhere::NotTaken;
                }
                // A bus that gives up on a call waiting long (a reply's
                // time limit) while the Skwir is there: asked again. One
                // gone answers no more.
                let gave_up = named(&[
                    "org.freedesktop.DBus.Error.NoReply",
                    "org.freedesktop.DBus.Error.Timeout",
                ]);
                let there = dbus
                    .name_has_owner(zbus::names::BusName::from(holder.as_ref()))
                    .await
                    .unwrap_or(false);
                if !(gave_up && there) {
                    tracing::debug!("the Skwir viewing these files ended: {error}");
                    return ViewedElsewhere::Ended;
                }
            }
        })
    }

    /// A viewing a launch asked, by its number: what its launch's `Viewed`
    /// finds.
    enum Handed {
        /// Asked (`View`), its launch yet to confirm it (its first
        /// `Viewed`): nothing shown, and nothing ever unless confirmed
        /// within [`PATIENCE`].
        Asked(super::Viewed),
        /// Confirmed, and passed to its window; once that closed, told so
        /// to each `Viewed` until its launch goes.
        Shown(Arc<super::ViewedState>),
        /// Not shown: not confirmed in time, or no longer taken. Its
        /// launch, should it confirm, views the files itself.
        NotTaken,
    }

    /// The viewings launches asked, by their number, until their launches
    /// go.
    type Viewings = Arc<parking_lot::Mutex<std::collections::HashMap<u64, Handed>>>;

    fn not_taken() -> Refused {
        Refused::NotTaken("these files are not shown here".into())
    }

    /// Once the launch the stream follows goes, or the stream ends (its
    /// connection gone).
    async fn launch_went(went: &mut zbus::fdo::NameOwnerChangedStream) {
        use futures_lite::StreamExt as _;
        while let Some(changed) = went.next().await {
            if changed.args().is_ok_and(|args| args.new_owner().is_none()) {
                return;
            }
        }
    }

    /// What follows viewing `id`'s launch, from its `View` on: unconfirmed
    /// within [`PATIENCE`], let go of unseen; confirmed, until its window
    /// closes (then until its launch read so, [`PATIENCE`] at most) or its
    /// launch goes first, which closes the window, and once the window let
    /// go removes what stood for standard input (`piped`, checked when
    /// asked): no launch is left to.
    async fn follow(
        id: u64,
        state: Arc<super::ViewedState>,
        piped: Option<PathBuf>,
        mut went: zbus::fdo::NameOwnerChangedStream,
        there: bool,
        viewings: Viewings,
        stopped: Arc<Stopped>,
    ) {
        use futures_lite::future::or;
        let _released = super::Released(Arc::clone(&state));
        // Each change taken as it comes: the stream never holds the
        // connection's other users.
        let mut gone = !there;
        if !gone {
            gone = or(
                async {
                    launch_went(&mut went).await;
                    true
                },
                async {
                    async_io::Timer::after(PATIENCE).await;
                    false
                },
            )
            .await;
        }
        let unconfirmed = {
            let mut held = viewings.lock();
            match held.get(&id) {
                Some(Handed::Shown(_)) => None,
                // Its launch gave up, and views the files itself, or went:
                // what it kept of standard input is its own.
                Some(_) if gone => held.remove(&id),
                Some(_) => held.insert(id, Handed::NotTaken),
                None => return,
            }
        };
        if unconfirmed.is_some() {
            return;
        }
        if !gone {
            enum Ended {
                Gone,
                Closed,
                Stopped,
            }
            let ended = or(
                async {
                    launch_went(&mut went).await;
                    Ended::Gone
                },
                or(
                    async {
                        state.closed().await;
                        Ended::Closed
                    },
                    async {
                        stopped.wait().await;
                        Ended::Stopped
                    },
                ),
            )
            .await;
            match ended {
                Ended::Gone => {}
                Ended::Closed => {
                    // Told: until its launch read so and went, or a while.
                    let went_now = or(
                        async {
                            launch_went(&mut went).await;
                            true
                        },
                        async {
                            async_io::Timer::after(PATIENCE).await;
                            false
                        },
                    )
                    .await;
                    if went_now {
                        viewings.lock().remove(&id);
                    }
                    return;
                }
                // What is asked again is answered by what is kept.
                Ended::Stopped => return,
            }
        }
        if state.go() {
            state.closed().await;
            if let Some(piped) = &piped {
                super::let_go_of_spool(piped);
            }
        }
        viewings.lock().remove(&id);
    }

    /// What a later launch calls.
    struct Finishing {
        gate: Arc<Gate>,
        viewings: Viewings,
        stopped: Arc<Stopped>,
    }

    impl Finishing {
        fn new(gate: &Arc<Gate>, stopped: &Arc<Stopped>) -> Finishing {
            Finishing {
                gate: Arc::clone(gate),
                viewings: Viewings::default(),
                stopped: Arc::clone(stopped),
            }
        }
    }

    /// Set once what serves launches on a connection stops (`Served`):
    /// what waits there for a viewing's window answers then, and lets go
    /// of the connection, which its launch would otherwise wait on for
    /// good.
    #[derive(Default)]
    pub(super) struct Stopped {
        stopped: std::sync::atomic::AtomicBool,
        told: event_listener::Event,
    }

    impl Stopped {
        fn stop(&self) {
            self.stopped
                .store(true, std::sync::atomic::Ordering::Release);
            self.told.notify(usize::MAX);
        }

        fn is_stopped(&self) -> bool {
            self.stopped.load(std::sync::atomic::Ordering::Acquire)
        }

        async fn wait(&self) {
            loop {
                let listener = self.told.listen();
                if self.is_stopped() {
                    return;
                }
                listener.await;
            }
        }
    }

    #[zbus::interface(name = "org.skwir.skwir.Finishing")]
    impl Finishing {
        /// View `files` (each a path's bytes; none for the system's dialog
        /// to ask which), `piped` the one standing for standard input
        /// (empty for none), the window brought forward with `token` (empty
        /// for none): answered with the viewing's number, nothing shown
        /// until its launch confirms it by asking `Viewed` (within
        /// [`PATIENCE`], else let go of). Refused, nothing to be shown
        /// ([`NOT_TAKEN`]), where Skwir no longer takes them, or where
        /// `piped` is not the caller's own spool (`spooled_by`): the
        /// window removes it as it closes, so a path anyone names would be
        /// a file anyone may have removed. Its launch followed: gone (its
        /// terminal's Ctrl+C), the window closes.
        async fn view(
            &self,
            #[zbus(header)] header: zbus::message::Header<'_>,
            #[zbus(connection)] bus: &zbus::Connection,
            files: Vec<Vec<u8>>,
            piped: Vec<u8>,
            token: String,
        ) -> Result<u64, Refused> {
            let Some(launch) = header.sender().map(zbus::names::UniqueName::to_owned) else {
                return Err(Refused::Failed("a viewing from no one".into()));
            };
            if !self.gate.takes_viewing() {
                return Err(not_taken());
            }
            let viewing = super::Viewing {
                files: files
                    .into_iter()
                    .map(|file| PathBuf::from(skwir_core::os_path::from_bytes(file)))
                    .collect(),
                piped: (!piped.is_empty())
                    .then(|| PathBuf::from(skwir_core::os_path::from_bytes(piped))),
                token: (!token.is_empty()).then_some(token),
            };
            let dbus = zbus::fdo::DBusProxy::new(bus).await?;
            if let Some(piped) = &viewing.piped {
                // Who asked, as the bus says: through a sandbox's proxy,
                // the proxy, whose folder the sandbox cannot make.
                let pid = dbus
                    .get_connection_unix_process_id(zbus::names::BusName::from(launch.as_ref()))
                    .await?;
                if !super::spooled_by(piped, &viewing.files, &super::spools(), pid) {
                    tracing::debug!("refused: what stands for standard input is not the launch's");
                    return Err(Refused::NotTaken(
                        "what stands for standard input is not this launch's".into(),
                    ));
                }
            }
            // Its launch followed from before it is answered: one that goes
            // meanwhile is let go of all the same.
            let went = dbus
                .receive_name_owner_changed_with_args(&[(0, launch.as_str())])
                .await?;
            let piped = viewing.piped.clone();
            let (viewed, state) = super::Viewed::made(viewing, true);
            let id = state.id;
            self.viewings.lock().insert(id, Handed::Asked(viewed));
            // An answer that fails is no launch gone: the stream follows it
            // still, and unconfirmed it is let go of anyway.
            let there = dbus
                .name_has_owner(zbus::names::BusName::from(launch.as_ref()))
                .await
                .unwrap_or(true);
            let (viewings, stopped) = (Arc::clone(&self.viewings), Arc::clone(&self.stopped));
            bus.executor()
                .spawn(
                    follow(id, state, piped, went, there, viewings, stopped),
                    "skwir-viewed",
                )
                .detach();
            Ok(id)
        }

        /// Asked first, confirms viewing `id`, which is shown from then on,
        /// or refused ([`NOT_TAKEN`]) where it was let go of unconfirmed or
        /// Skwir no longer takes them. Answered once its window closes, at
        /// once if it has, even once this Skwir no longer takes launches
        /// over; [`NOT_TAKEN`] for a window that could not open; refused
        /// once this Skwir stopped with the window up.
        async fn viewed(&self, id: u64) -> Result<(), Refused> {
            let (state, asked) = {
                let mut viewings = self.viewings.lock();
                match viewings.get(&id) {
                    Some(Handed::Asked(viewed)) => {
                        let state = Arc::clone(&viewed.holder.state);
                        let asked = viewings.insert(id, Handed::Shown(Arc::clone(&state)));
                        (state, asked)
                    }
                    Some(Handed::Shown(state)) => (Arc::clone(state), None),
                    Some(Handed::NotTaken) => return Err(not_taken()),
                    None => return Err(Refused::Failed("no such viewing here".into())),
                }
            };
            // Confirmed now: shown, if still taken (no lock held meanwhile
            // but the gate's).
            if let Some(Handed::Asked(viewed)) = asked
                && !self.gate.pass_viewing(viewed)
            {
                self.viewings.lock().insert(id, Handed::NotTaken);
                return Err(not_taken());
            }
            let closed = async {
                state.closed().await;
                true
            };
            let stopped = async {
                self.stopped.wait().await;
                false
            };
            if !futures_lite::future::or(closed, stopped).await {
                return Err(Refused::Failed(
                    "Skwir no longer takes launches over".into(),
                ));
            }
            if state.is_not_shown() {
                return Err(not_taken());
            }
            Ok(())
        }

        /// Answer another program's dialog a skwir the bus started took
        /// (`chooser`): the request as the portal gave it (`method`: 0
        /// open, 1 save, 2 save files), its dialog closable at `handle`
        /// here meanwhile; answered once the user decides (ADR-141, 3).
        #[allow(clippy::too_many_arguments)] // the portal's request, whole
        async fn choose(
            &self,
            #[zbus(object_server)] server: &zbus::ObjectServer,
            method: u32,
            handle: zbus::zvariant::OwnedObjectPath,
            app_id: String,
            parent_window: String,
            title: String,
            options: crate::chooser::linux::Options,
        ) -> (u32, crate::chooser::linux::Results) {
            let raw = crate::chooser::linux::Raw {
                method: crate::chooser::linux::Method::of(method),
                app_id,
                parent_window,
                title,
                options,
            };
            let gate = Arc::clone(&self.gate);
            crate::chooser::linux::ask(server, handle, raw, move |choosing| {
                gate.pass_choosing(choosing)
            })
            .await
        }

        /// Open `location` (its bytes; empty for what the window had),
        /// read against `cwd`, the window brought forward with `token`
        /// (empty for none), the names `select` (bytes each) selected
        /// there once listed, with `properties` shown: answered once
        /// taken.
        fn open(
            &self,
            location: Vec<u8>,
            cwd: Vec<u8>,
            token: String,
            select: Vec<Vec<u8>>,
            properties: bool,
        ) -> zbus::fdo::Result<()> {
            let opening = Opening {
                location: (!location.is_empty()).then(|| skwir_core::os_path::from_bytes(location)),
                cwd: PathBuf::from(skwir_core::os_path::from_bytes(cwd)),
                token: (!token.is_empty()).then_some(token),
                select: select
                    .into_iter()
                    .map(skwir_core::os_path::from_bytes)
                    .collect(),
                properties,
            };
            if self.gate.pass(opening) {
                Ok(())
            } else {
                Err(zbus::fdo::Error::Failed(
                    "Skwir no longer takes launches over".into(),
                ))
            }
        }
    }

    /// What other programs call to show folders and files (ADR-140):
    /// each request is launches, handed in through the gate.
    struct FileManager(Arc<Gate>);

    // zbus hands a method its arguments owned, as it read them.
    #[allow(clippy::needless_pass_by_value)]
    #[zbus::interface(name = "org.freedesktop.FileManager1")]
    impl FileManager {
        /// Open each folder of `uris`, the window brought forward with
        /// `startup_id`.
        fn show_folders(&self, uris: Vec<String>, startup_id: String) -> zbus::fdo::Result<()> {
            self.asked(crate::file_manager::Shown::Folders, &uris, startup_id)
        }

        /// Open the folder of each item of `uris`, its items selected.
        fn show_items(&self, uris: Vec<String>, startup_id: String) -> zbus::fdo::Result<()> {
            self.asked(crate::file_manager::Shown::Items, &uris, startup_id)
        }

        /// As `ShowItems`, Properties shown.
        fn show_item_properties(
            &self,
            uris: Vec<String>,
            startup_id: String,
        ) -> zbus::fdo::Result<()> {
            self.asked(crate::file_manager::Shown::Properties, &uris, startup_id)
        }
    }

    impl FileManager {
        /// `uris` shown as `shown` asks, as launches: answered once each is
        /// handed in, as Nautilus's and Dolphin's answer at once. Refused
        /// when none is a local file, or Skwir no longer takes them, so the
        /// asking program does what it does when no one answers.
        fn asked(
            &self,
            shown: crate::file_manager::Shown,
            uris: &[String],
            startup_id: String,
        ) -> zbus::fdo::Result<()> {
            let token = (!startup_id.is_empty()).then_some(startup_id);
            let openings = crate::file_manager::openings(shown, uris, token);
            if openings.is_empty() {
                return Err(zbus::fdo::Error::InvalidArgs(
                    "Skwir shows local files alone".into(),
                ));
            }
            for opening in openings {
                if !self.0.pass(opening) {
                    return Err(zbus::fdo::Error::Failed(
                        "Skwir no longer shows files for other programs".into(),
                    ));
                }
            }
            Ok(())
        }
    }

    /// Whether the program answers other programs (ADR-140), and where:
    /// shared by the thread that serves on each connection and by what
    /// follows the user's choice ([`super::Answering`]).
    pub(super) struct Answer {
        gate: Arc<Gate>,
        state: parking_lot::Mutex<Answered>,
    }

    #[derive(Default)]
    struct Answered {
        /// Asked to answer.
        wanted: bool,
        /// The connection taken over on, while there is one.
        bus: Option<Connection>,
        /// The object served and the name asked there.
        serving: bool,
    }

    impl Answer {
        pub(super) fn new(gate: Arc<Gate>) -> Answer {
            Answer {
                gate,
                state: parking_lot::Mutex::default(),
            }
        }

        pub(super) fn set(&self, wanted: bool) {
            let mut state = self.state.lock();
            state.wanted = wanted;
            self.apply(&mut state);
        }

        /// Taking over is served on `bus`: answered there too if wanted.
        fn connected(&self, bus: &Connection) {
            let mut state = self.state.lock();
            state.bus = Some(bus.clone());
            state.serving = false;
            self.apply(&mut state);
        }

        /// No longer served on the connection it was (ended, or taking
        /// over stopped): what was asked there let go.
        fn disconnected(&self) {
            let mut state = self.state.lock();
            if state.serving
                && let Some(bus) = &state.bus
            {
                let_go_of_file_manager(bus);
            }
            state.bus = None;
            state.serving = false;
        }

        fn apply(&self, state: &mut Answered) {
            let Some(bus) = state.bus.clone() else {
                return;
            };
            if state.wanted && !state.serving {
                state.serving = serve_file_manager(&bus, &self.gate);
            } else if !state.wanted && state.serving {
                let_go_of_file_manager(&bus);
                state.serving = false;
            }
        }
    }

    /// Serve `/org/freedesktop/FileManager1` on `bus` and ask its name,
    /// taking it from a holder that lets go (another Skwir), else waiting
    /// behind it, and letting it go to one asked after: whether it is
    /// served.
    pub(super) fn serve_file_manager(bus: &Connection, gate: &Arc<Gate>) -> bool {
        let objects = bus.object_server();
        match objects.at(FILE_MANAGER_PATH, FileManager(Arc::clone(gate))) {
            Ok(_) => {}
            Err(error) => {
                tracing::debug!("not showing files for other programs: {error}");
                return false;
            }
        }
        let flags = RequestNameFlags::ReplaceExisting | RequestNameFlags::AllowReplacement;
        match bus.request_name_with_flags(FILE_MANAGER, flags) {
            Ok(reply) => {
                tracing::info!(?reply, "showing files for other programs");
                true
            }
            Err(error) => {
                tracing::debug!("not showing files for other programs: {error}");
                let _ = objects.remove::<FileManager, _>(FILE_MANAGER_PATH);
                false
            }
        }
    }

    /// The name let go and the object taken down, on `bus`.
    pub(super) fn let_go_of_file_manager(bus: &Connection) {
        release_name(bus, FILE_MANAGER);
        let _ = bus
            .object_server()
            .remove::<FileManager, _>(FILE_MANAGER_PATH);
        tracing::info!("no longer showing files for other programs");
    }

    /// Let go of `name` on `bus`, asked of the bus itself too: zbus forgets
    /// a name asked with `AllowReplacement` once told it was lost, and its
    /// `release_name` then sends nothing, while the bus has put this
    /// connection back in the name's queue and hands it back once its new
    /// owner goes, to hold it serving nothing.
    pub(crate) fn release_name(bus: &Connection, name: &str) {
        let _ = bus.release_name(name);
        if let Ok(proxy) = zbus::blocking::fdo::DBusProxy::new(bus)
            && let Ok(name) = zbus::names::WellKnownName::try_from(name)
        {
            let _ = proxy.release_name(name);
        }
    }

    /// [`super::answer_for_holder`] on `bus`, each launch handed by `hand`.
    pub(super) fn answer_for_holder_on(
        bus: &Connection,
        wait: std::time::Duration,
        hand: impl Fn(&Opening) -> bool,
    ) -> bool {
        let (told, asked) = std::sync::mpsc::channel::<Opening>();
        let gate = Gate::new(move |opening| {
            let _ = told.send(opening);
        });
        if !serve_file_manager(bus, &gate) {
            return false;
        }
        let mut openings = Vec::new();
        if let Ok(first) = asked.recv_timeout(wait) {
            openings.push(first);
            // The rest of that request, handed in as one call: what comes
            // within a moment of it.
            while let Ok(more) = asked.recv_timeout(std::time::Duration::from_millis(50)) {
                openings.push(more);
            }
        }
        gate.close();
        let_go_of_file_manager(bus);
        // Each handed, whatever the one before did.
        let refused = openings.iter().filter(|opening| !hand(opening)).count();
        !openings.is_empty() && refused == 0
    }

    /// The thread that serves on each connection the bus is given on
    /// (`bus::keep`): stopped and waited for when dropped.
    pub(super) struct Held {
        stop: CancelToken,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Drop for Held {
        fn drop(&mut self) {
            self.stop.cancel();
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    /// [`super::take_over`] on the bus `connect` connects to (again each
    /// time it ends), its marker `marker`: once the first connection is
    /// served, or `None`.
    #[cfg(test)]
    pub(super) fn take_over_with<E: std::fmt::Display>(
        connect: impl FnMut() -> Result<Connection, E> + Send + 'static,
        marker: PathBuf,
        name: String,
        taken: impl Fn(Opening) + Send + Sync + 'static,
    ) -> Result<TakingOver, NotTaken> {
        take_over_on(connect, marker, name, Gate::new(taken))
    }

    /// [`take_over_with`] with what passes what is handed (launches, and
    /// other programs' dialogs where it takes them).
    pub(super) fn take_over_on<E: std::fmt::Display>(
        connect: impl FnMut() -> Result<Connection, E> + Send + 'static,
        marker: PathBuf,
        name: String,
        gate: Arc<Gate>,
    ) -> Result<TakingOver, NotTaken> {
        let answer = Arc::new(Answer::new(Arc::clone(&gate)));
        let stop = CancelToken::new();
        let (first, answered) = std::sync::mpsc::sync_channel(1);
        let thread = {
            let (gate, answer, stop) = (Arc::clone(&gate), Arc::clone(&answer), stop.clone());
            std::thread::Builder::new()
                .name("skwir-finishing".into())
                .spawn(move || {
                    let mut first = Some(first);
                    let mut served: Option<Served> = None;
                    crate::bus::keep(connect, &stop, |bus| {
                        // What the bus that ended had is let go of first
                        // (its marker, ours, written again once served).
                        drop(served.take());
                        let on = Served::on(bus, &gate, &answer, &marker, &name);
                        if let Some(first) = first.take() {
                            let _ = first.send(on.as_ref().map(|_| ()).map_err(|not| *not));
                        }
                        served = on.ok();
                    });
                    drop(served);
                })
        };
        let thread = thread
            .inspect_err(|error| tracing::debug!("not taking launches over: {error}"))
            .map_err(|_| NotTaken::Cannot)?;
        let taking = TakingOver {
            gate,
            held: Some(Held {
                stop,
                thread: Some(thread),
            }),
            answer,
        };
        // Not served (dropped: stopped and waited for), or no bus (the
        // thread ended without a word).
        answered
            .recv()
            .unwrap_or(Err(NotTaken::Cannot))
            .map(|()| taking)
    }

    /// The object and the name served on one connection, which the
    /// program shares (`bus::session`), and the marker written: all taken
    /// back when dropped.
    struct Served {
        bus: Connection,
        marker: PathBuf,
        name: String,
        /// Other programs answered here too while it is wanted.
        answer: Arc<Answer>,
        /// What waits for viewings' windows here, told it stopped.
        stopped: Arc<Stopped>,
    }

    impl Served {
        /// Served on `bus`, or why not: another Skwir holds the name
        /// ([`NotTaken::Elsewhere`]); this connection serves it already (a
        /// second [`super::take_over`] of this Skwir), no marker can be
        /// written ([`NotTaken::Cannot`]).
        fn on(
            bus: &Connection,
            gate: &Arc<Gate>,
            answer: &Arc<Answer>,
            marker: &Path,
            name: &str,
        ) -> Result<Served, NotTaken> {
            let objects = bus.object_server();
            let stopped = Arc::<Stopped>::default();
            match objects.at(PATH, Finishing::new(gate, &stopped)) {
                Ok(true) => {}
                Ok(false) => {
                    tracing::debug!("this Skwir takes later launches over already");
                    return Err(NotTaken::Cannot);
                }
                Err(error) => {
                    tracing::debug!("not taking launches over: {error}");
                    return Err(NotTaken::Cannot);
                }
            }
            let named = bus.request_name_with_flags(name, RequestNameFlags::DoNotQueue.into());
            if !matches!(named, Ok(RequestNameReply::PrimaryOwner)) {
                tracing::debug!(?named, "another Skwir takes later launches over");
                let _ = objects.remove::<Finishing, _>(PATH);
                // Refused: this library says so as an error, the bus as a
                // reply.
                let taken = matches!(
                    named,
                    Err(zbus::Error::NameTaken) | Ok(RequestNameReply::Exists)
                );
                return Err(if taken {
                    NotTaken::Elsewhere
                } else {
                    NotTaken::Cannot
                });
            }
            let served = Served {
                bus: bus.clone(),
                marker: marker.to_owned(),
                name: name.to_owned(),
                answer: Arc::clone(answer),
                stopped,
            };
            if let Err(error) = write_marker(marker) {
                // Dropped: neither the name nor the object kept.
                tracing::debug!("not taking launches over: {}: {error}", marker.display());
                return Err(NotTaken::Cannot);
            }
            answer.connected(bus);
            // Other programs' dialogs, where taken (ADR-141, 2): the name
            // is skwir's own, asked behind a skwir the bus started.
            if gate.takes_choosing() {
                let gate = Arc::clone(gate);
                crate::chooser::linux::serve(
                    bus,
                    Arc::new(move |choosing| gate.pass_choosing(choosing)),
                );
            }
            Ok(served)
        }
    }

    impl Drop for Served {
        fn drop(&mut self) {
            // What waits for viewings' windows answers: their launches end,
            // not waiting for good on a connection that stays for them.
            self.stopped.stop();
            // Other programs first, then the name: a launch that finds
            // the marker meanwhile finds no one holds it.
            self.answer.disconnected();
            crate::chooser::linux::let_go(&self.bus);
            let _ = self.bus.release_name(self.name.as_str());
            let _ = self.bus.object_server().remove::<Finishing, _>(PATH);
            let_go_of_marker(&self.marker);
        }
    }

    /// Our pid in `marker`, which takes the place of what was there whole.
    fn write_marker(marker: &Path) -> std::io::Result<()> {
        let folder = marker
            .parent()
            .ok_or_else(|| std::io::Error::other("a marker in no folder"))?;
        // The folder agents and scripts listen in too: made the user's
        // alone, as everything there is (`folders::private`).
        skwir_core::folders::private(folder)?;
        let mut staged = tempfile::NamedTempFile::new_in(folder)?;
        std::io::Write::write_all(&mut staged, format!("{}\n", std::process::id()).as_bytes())?;
        skwir_core::fs::file::take_place(staged, marker)
    }

    /// Remove `marker` if it is ours: another Skwir's (one that took
    /// launches over after this one) is its own.
    fn let_go_of_marker(marker: &Path) {
        let pid = skwir_core::fs::file::read_regular_text(marker, MARKER_BOUND)
            .ok()
            .and_then(|text| text.trim().parse::<u32>().ok());
        if pid == Some(std::process::id()) {
            let _ = std::fs::remove_file(marker);
        }
    }
}

#[cfg(windows)]
mod pipe {
    // This module is the system's calls on a pipe, which take raw
    // handles: each `unsafe` block says why its call is sound, and
    // nothing unsafe leaves the module.
    #![allow(unsafe_code)]

    use std::ffi::{OsStr, OsString};
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::windows::io::{AsRawHandle, OwnedHandle};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_PIPE_BUSY, HANDLE};
    use windows_sys::Win32::Security::TOKEN_QUERY;
    use windows_sys::Win32::Storage::FileSystem::SECURITY_IDENTIFICATION;
    use windows_sys::Win32::System::Pipes::{
        GetNamedPipeClientProcessId, GetNamedPipeServerProcessId, GetNamedPipeServerSessionId,
        PeekNamedPipe,
    };
    use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

    use super::{Gate, NotTaken, Opening, PATIENCE, TakingOver, frame};

    /// How often a wait on the pipe looks again: a wait lasts at most
    /// [`PATIENCE`], and only while a launch is handed over.
    const LOOK: Duration = Duration::from_millis(5);

    /// The pipe of this user in this session: another user's, or this
    /// user's in another session (a second sign-in, a remote desktop),
    /// is not this one.
    pub(super) fn name(instance: &super::Instance) -> Option<OsString> {
        let sid = crate::windows::user_sid()?;
        let mut session = 0u32;
        // SAFETY: this process's id, and an out value.
        if unsafe { ProcessIdToSessionId(std::process::id(), &raw mut session) } == 0 {
            return None;
        }
        let key = instance.key();
        Some(format!(r"\\.\pipe\org.skwir.skwir.running-{key}-{session}-{sid}").into())
    }

    /// The pipe opened as a client that lets the server know who it is,
    /// never act as it; one busy (between two instances) is asked again
    /// until `deadline`. `None` at once where there is no pipe.
    fn connect(pipe: &OsStr, deadline: Instant) -> Option<File> {
        use std::os::windows::fs::OpenOptionsExt;
        loop {
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .security_qos_flags(SECURITY_IDENTIFICATION)
                .open(pipe)
            {
                Ok(file) => return Some(file),
                Err(error)
                    if error.raw_os_error() == i32::try_from(ERROR_PIPE_BUSY).ok()
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(LOOK);
                }
                Err(_) => return None,
            }
        }
    }

    /// [`super::hand_over`] through `pipe`.
    pub(super) fn hand_over_to(pipe: &OsStr, opening: &Opening) -> bool {
        // Larger than a pipe's buffer: not handed over (never so for a
        // path Windows opens).
        frame::encode(&frame::Message::Open(frame::Fields::of(opening)))
            .and_then(|message| ask(pipe, &message))
            .is_some()
    }

    /// [`super::view_elsewhere`] through `pipe`: once taken within
    /// [`PATIENCE`], confirmed (`CONFIRMED`), which shows the files from
    /// then on, this launch no longer giving up; then the pipe held until
    /// the Skwir says the window closed, that nothing was shown, or goes. A
    /// launch that gave up confirms nothing, and nothing is shown.
    pub(super) fn view_through(pipe: &OsStr, viewing: &super::Viewing) -> super::ViewedElsewhere {
        use super::ViewedElsewhere;
        let Some(mut connected) = frame::encode(&frame::Message::View(frame::Viewing::of(viewing)))
            .and_then(|message| ask(pipe, &message))
        else {
            return ViewedElsewhere::NotTaken;
        };
        // Not written, the Skwir reads no confirmation: nothing is shown.
        if connected.write_all(&[frame::CONFIRMED]).is_err() {
            return ViewedElsewhere::NotTaken;
        }
        // Only this thread uses its end: a read that waits for the one
        // byte the Skwir writes as the window closes, or for its end gone.
        let mut answer = [0u8];
        match connected.read_exact(&mut answer) {
            Ok(()) if answer[0] == frame::CLOSED => ViewedElsewhere::Closed,
            Ok(()) if answer[0] == frame::NOT_TAKEN => ViewedElsewhere::NotTaken,
            _ => ViewedElsewhere::Ended,
        }
    }

    /// Say `message` to the Skwir serving `pipe`: its end, once it
    /// answered taken within [`PATIENCE`].
    fn ask(pipe: &OsStr, message: &[u8]) -> Option<File> {
        let deadline = Instant::now() + PATIENCE;
        let mut connected = connect(pipe, deadline)?;
        // Anyone may make a pipe of a name not taken: one another user
        // made first would be told what this launch opens, answer it
        // taken, and keep Skwir from starting. Nothing is said, and no
        // window let forward, before the pipe is known to be this user's.
        let Some(server) = our_server(&connected) else {
            tracing::debug!("the pipe of that name is not this user's: not handed over");
            return None;
        };
        // SAFETY: a process id, nothing else. Allowed or not, the window
        // still opens.
        unsafe { AllowSetForegroundWindow(server) };
        // Within the pipe's buffer: written at once, read or not.
        connected.write_all(message).ok()?;
        let mut answer = [0u8];
        (matches!(wait(&connected, 1, deadline, None), Waited::Ready)
            && connected.read_exact(&mut answer).is_ok()
            && answer[0] == frame::TAKEN)
            .then_some(connected)
    }

    /// The process serving `connected`, where it is this user's in this
    /// session. Who runs it is asked, not whose the pipe is (its owner):
    /// Wine makes that the user's primary group, Windows the
    /// Administrators group for a user running elevated, while the
    /// process's own user is the user in both; another user's process
    /// cannot be opened to ask.
    pub(super) fn our_server(connected: &File) -> Option<u32> {
        let (mut ours, mut theirs, mut server) = (0u32, 0u32, 0u32);
        // SAFETY: this process's id, the pipe just opened, and out values.
        let known = unsafe {
            ProcessIdToSessionId(std::process::id(), &raw mut ours) != 0
                && GetNamedPipeServerSessionId(connected.as_raw_handle(), &raw mut theirs) != 0
                && GetNamedPipeServerProcessId(connected.as_raw_handle(), &raw mut server) != 0
        };
        let user = crate::windows::user_sid();
        (known && theirs == ours && user.is_some() && user_of(server) == user).then_some(server)
    }

    /// The user the process `pid` runs as, by their SID; `None` where it
    /// cannot be opened to ask (another user's, or none).
    pub(super) fn user_of(pid: u32) -> Option<String> {
        // SAFETY: a process id; the handle, if any, closed below.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return None;
        }
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: the process just opened, and an out pointer.
        let opened = unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) } != 0;
        // SAFETY: the process's handle, closed once.
        unsafe { CloseHandle(process) };
        if !opened {
            return None;
        }
        let user = crate::windows::token_sid(token);
        // SAFETY: the token just opened, closed once.
        unsafe { CloseHandle(token) };
        user
    }

    enum Waited {
        Ready,
        Closed,
        GaveUp,
    }

    /// Wait until `need` bytes can be read of `pipe` without waiting, it
    /// is closed at the other end, `deadline` passes or `stop` is set.
    fn wait(pipe: &File, need: usize, deadline: Instant, stop: Option<&AtomicBool>) -> Waited {
        loop {
            let mut there = 0u32;
            // SAFETY: an open pipe; no buffer (of size 0), and an out
            // value for the bytes there.
            let ok = unsafe {
                PeekNamedPipe(
                    pipe.as_raw_handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &raw mut there,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Waited::Closed;
            }
            if usize::try_from(there).is_ok_and(|there| there >= need) {
                return Waited::Ready;
            }
            if Instant::now() >= deadline || stop.is_some_and(|stop| stop.load(Ordering::Acquire)) {
                return Waited::GaveUp;
            }
            std::thread::sleep(LOOK);
        }
    }

    /// The thread that serves the pipe: stopped when dropped, waited for
    /// [`PATIENCE`] at most.
    pub(super) struct Held {
        pipe: OsString,
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Drop for Held {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            let Some(thread) = self.thread.take() else {
                return;
            };
            // Woken from its wait for a launch by one of ours, which it
            // then lets go; one that says nothing is let go of at the
            // next look.
            let deadline = Instant::now() + PATIENCE;
            while !thread.is_finished() && Instant::now() < deadline {
                drop(connect(&self.pipe, Instant::now()));
                std::thread::sleep(LOOK);
            }
            if thread.is_finished() {
                let _ = thread.join();
            } else {
                tracing::debug!("the pipe's thread is left to end by itself");
            }
        }
    }

    /// [`super::take_over`] on `pipe`: `None` where its first instance is
    /// another's.
    #[cfg(test)]
    pub(super) fn take_over_at(
        pipe: OsString,
        taken: impl Fn(Opening) + Send + Sync + 'static,
    ) -> Result<TakingOver, NotTaken> {
        take_over_on(pipe, Gate::new(taken))
    }

    /// [`take_over_at`] with what passes what is handed: no dialog is
    /// handed on Windows (ADR-141, 11).
    pub(super) fn take_over_on(pipe: OsString, gate: Arc<Gate>) -> Result<TakingOver, NotTaken> {
        // Its first instance: refused where any is there, made by
        // another Skwir (or anyone): ERROR_ACCESS_DENIED.
        const ACCESS_DENIED: i32 = 5;
        let first = crate::windows::pipe::instance(&pipe, true)
            .inspect_err(|error| tracing::debug!("not taking launches over: {error}"))
            .map_err(|error| {
                if error.raw_os_error() == Some(ACCESS_DENIED) {
                    NotTaken::Elsewhere
                } else {
                    NotTaken::Cannot
                }
            })?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (pipe, gate, stop) = (pipe.clone(), Arc::clone(&gate), Arc::clone(&stop));
            std::thread::Builder::new()
                .name("skwir-finishing".into())
                .spawn(move || serve(&pipe, first, &gate, &stop))
                .inspect_err(|error| tracing::debug!("not taking launches over: {error}"))
                .map_err(|_| NotTaken::Cannot)?
        };
        Ok(TakingOver {
            gate,
            held: Some(Held {
                pipe,
                stop,
                thread: Some(thread),
            }),
        })
    }

    /// Answer each launch that connects, one at a time, until `stop`.
    fn serve(pipe: &OsStr, mut waiting: OwnedHandle, gate: &Arc<Gate>, stop: &Arc<AtomicBool>) {
        loop {
            let connected = match crate::windows::pipe::accept(waiting) {
                Ok(connected) => connected,
                Err(error) => {
                    tracing::debug!("no longer taking launches over: {error}");
                    return;
                }
            };
            if stop.load(Ordering::Acquire) {
                return;
            }
            // The next instance before this one is answered: the pipe
            // stays this Skwir's (no one makes it first meanwhile), and a
            // launch finds it busy a moment, never gone.
            let next = crate::windows::pipe::instance(pipe, false);
            answer(connected, gate, stop);
            match next {
                Ok(next) => waiting = next,
                Err(error) => {
                    tracing::debug!("no longer taking launches over: {error}");
                    return;
                }
            }
        }
    }

    /// Read a launch's message from `connected`, give it to `gate`, and
    /// answer it taken; given up on after [`PATIENCE`], or at `stop`. A
    /// viewing keeps its connection, on a thread of its own, until its
    /// launch confirms it, then until its window closes (`show_viewing`).
    fn answer(mut connected: File, gate: &Arc<Gate>, stop: &Arc<AtomicBool>) {
        let deadline = Instant::now() + PATIENCE;
        let mut length = [0u8; 4];
        if !matches!(wait(&connected, 4, deadline, Some(stop)), Waited::Ready)
            || connected.read_exact(&mut length).is_err()
        {
            return;
        }
        let Some(length) = usize::try_from(u32::from_le_bytes(length))
            .ok()
            .filter(|n| *n <= frame::LARGEST)
        else {
            return;
        };
        let mut body = vec![0; length];
        if !matches!(
            wait(&connected, length, deadline, Some(stop)),
            Waited::Ready
        ) || connected.read_exact(&mut body).is_err()
        {
            return;
        }
        match frame::decode(&body) {
            Some(frame::Message::Open(fields)) => {
                if !gate.pass(fields.into_opening())
                    || connected.write_all(&[frame::TAKEN]).is_err()
                {
                    return;
                }
                // The answer read before the connection goes, which would
                // take it along: until the launch lets go (never more to
                // read).
                let _ = wait(&connected, usize::MAX, deadline, Some(stop));
            }
            Some(frame::Message::View(viewing)) => {
                let viewing = viewing.into_viewing();
                // What stands for standard input is removed as its window
                // closes: the launch's own spool alone (`spooled_by`), by
                // the process the pipe says connected. Anything else is
                // not taken: the pipe closes unanswered.
                if let Some(piped) = &viewing.piped {
                    let mut client = 0u32;
                    // SAFETY: the pipe just connected, and an out value.
                    let known = unsafe {
                        GetNamedPipeClientProcessId(connected.as_raw_handle(), &raw mut client)
                    } != 0;
                    if !known || !super::spooled_by(piped, &viewing.files, &super::spools(), client)
                    {
                        tracing::debug!(
                            "refused: what stands for standard input is not the launch's"
                        );
                        return;
                    }
                }
                if !gate.takes_viewing() || connected.write_all(&[frame::TAKEN]).is_err() {
                    return;
                }
                // The serve thread is not held while the launch confirms.
                let (gate, stop) = (Arc::clone(gate), Arc::clone(stop));
                let followed = std::thread::Builder::new()
                    .name("skwir-viewed".into())
                    .spawn(move || show_viewing(&connected, viewing, &gate, &stop));
                if let Err(error) = followed {
                    tracing::debug!("a viewing's launch is not followed: {error}");
                }
            }
            None => {}
        }
    }

    /// A viewing taken on `connected`: shown once its launch confirms it
    /// (`CONFIRMED`) within [`PATIENCE`], then followed until its window
    /// closes (`follow_viewing`). Not confirmed in time, or no longer taken,
    /// nothing is shown, and the launch is told so (`NOT_TAKEN`) should it
    /// still read: it views the files itself.
    fn show_viewing(mut connected: &File, viewing: super::Viewing, gate: &Gate, stop: &AtomicBool) {
        let mut confirmed = [0u8];
        let in_time = matches!(
            wait(connected, 1, Instant::now() + PATIENCE, Some(stop)),
            Waited::Ready
        ) && connected.read_exact(&mut confirmed).is_ok()
            && confirmed[0] == frame::CONFIRMED;
        if !in_time {
            not_taken(connected);
            return;
        }
        let piped = viewing.piped.clone();
        let (viewed, state) = super::Viewed::made(viewing, true);
        let _released = super::Released(Arc::clone(&state));
        if !gate.pass_viewing(viewed) {
            not_taken(connected);
            return;
        }
        follow_viewing(connected, &state, piped.as_deref(), stop);
    }

    /// Tell a launch that nothing was shown, read before the connection
    /// goes; one gone already is told nothing.
    fn not_taken(mut connected: &File) {
        if connected.write_all(&[frame::NOT_TAKEN]).is_ok() {
            let _ = wait(connected, usize::MAX, Instant::now() + PATIENCE, None);
        }
    }

    /// How often a viewing's thread looks whether its launch went: a
    /// synchronous pipe's read would hold the write that says the window
    /// closed, so its end is looked at rather than read, while the window
    /// is up alone.
    const LOOK_VIEWED: Duration = Duration::from_millis(250);

    /// Until the window of `state` closes, told then to its launch on
    /// `connected` (`CLOSED`, or `NOT_TAKEN` for a window that could not
    /// open); or its launch goes first, told to the window, and once the
    /// window let go, what stood for standard input (`piped`) removed: no
    /// launch is left to. Taking over stopped (`stop`) with the window up,
    /// the launch is let go of, told the Skwir ended.
    fn follow_viewing(
        mut connected: &File,
        state: &super::ViewedState,
        piped: Option<&std::path::Path>,
        stop: &AtomicBool,
    ) {
        loop {
            // Looked at first: a window closed as taking over stops is told.
            let stopped = stop.load(Ordering::Acquire);
            if state.closed_within(if stopped { Duration::ZERO } else { LOOK_VIEWED }) {
                let said = if state.is_not_shown() {
                    frame::NOT_TAKEN
                } else {
                    frame::CLOSED
                };
                if connected.write_all(&[said]).is_ok() {
                    // Read before the connection goes, which would take it
                    // along.
                    let _ = wait(connected, usize::MAX, Instant::now() + PATIENCE, None);
                }
                return;
            }
            if stopped {
                return;
            }
            if matches!(
                wait(connected, usize::MAX, Instant::now(), None),
                Waited::Closed
            ) {
                if state.go() {
                    while !state.closed_within(PATIENCE) {}
                    if let Some(piped) = piped {
                        super::let_go_of_spool(piped);
                    }
                }
                return;
            }
        }
    }
}

/// A launch's message on the pipe (Windows), and its answer. All numbers
/// little-endian:
///
/// - `[u32 length][body]`, the body at most `LARGEST` bytes;
/// - the body `[u8 VERSION][u8 kind]` and what the kind says, nothing
///   after: `OPEN`'s `[location][cwd][token][u32 count][names…][u8
///   properties]`, `VIEW`'s `[token][u32 count][files…][piped]`;
/// - each field `[u32 length in bytes][bytes]`, `u32::MAX` alone for none
///   (the folder, a name, a file are never none): paths' and names' UTF-16
///   code units (so a lone surrogate, which a Windows name may hold, goes
///   across whole), the token's UTF-8.
///
/// The answer is one byte, `TAKEN`, once the launch is taken; anything
/// else (the pipe closed first) is a launch not taken. A viewing is taken
/// in two steps, so that only its launch decides whether it is: once it
/// read `TAKEN`, the launch writes `CONFIRMED`, which the Skwir waits for
/// within its patience before it shows anything (a launch that gave up
/// writes nothing, and nothing is shown). Its connection then stays until
/// its window closes, which `CLOSED` says, or `NOT_TAKEN`: nothing was
/// shown (confirmed too late, no longer taken, a window that could not
/// open), and the launch views the files itself. A message of another
/// version is not read, and the launch goes on as a process of its own: a
/// later Skwir launched while an earlier one finishes.
#[cfg(any(windows, test))]
#[cfg_attr(not(windows), allow(dead_code))]
mod frame {
    /// What the body starts with: 3 since a launch may view files
    /// (ADR-151), 4 since a viewing is confirmed. Both ends are one build:
    /// no other is read.
    pub(super) const VERSION: u8 = 4;
    /// The kinds of message: a launch opening a folder, one viewing files.
    pub(super) const OPEN: u8 = 0;
    pub(super) const VIEW: u8 = 1;
    /// The answer: taken.
    pub(super) const TAKEN: u8 = 1;
    /// What a viewing's connection says last: its window closed.
    pub(super) const CLOSED: u8 = 2;
    /// What a viewing's launch says once it read `TAKEN`: it waits for the
    /// window, and no longer gives up.
    pub(super) const CONFIRMED: u8 = 3;
    /// What a viewing's connection says last when nothing was shown.
    pub(super) const NOT_TAKEN: u8 = 4;
    /// The most a body holds: within a pipe's buffer, so it is written at
    /// once, and above any path Windows opens.
    pub(super) const LARGEST: usize = 60 * 1024;
    /// A field's length saying it is none.
    const NONE: u32 = u32::MAX;

    /// What a message says.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(super) enum Message {
        Open(Fields),
        View(Viewing),
    }

    /// What a launch opening a folder carries.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(super) struct Fields {
        pub(super) location: Option<Vec<u16>>,
        pub(super) cwd: Vec<u16>,
        pub(super) token: Option<String>,
        pub(super) select: Vec<Vec<u16>>,
        pub(super) properties: bool,
    }

    /// What a launch viewing files carries.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(super) struct Viewing {
        pub(super) token: Option<String>,
        pub(super) files: Vec<Vec<u16>>,
        pub(super) piped: Option<Vec<u16>>,
    }

    #[cfg(windows)]
    impl Fields {
        pub(super) fn of(opening: &super::Opening) -> Fields {
            use std::os::windows::ffi::OsStrExt;
            Fields {
                location: opening.location.as_ref().map(|l| l.encode_wide().collect()),
                cwd: opening.cwd.as_os_str().encode_wide().collect(),
                token: opening.token.clone(),
                select: opening
                    .select
                    .iter()
                    .map(|name| name.encode_wide().collect())
                    .collect(),
                properties: opening.properties,
            }
        }

        pub(super) fn into_opening(self) -> super::Opening {
            use std::os::windows::ffi::OsStringExt;
            super::Opening {
                location: self.location.map(|l| std::ffi::OsString::from_wide(&l)),
                cwd: std::ffi::OsString::from_wide(&self.cwd).into(),
                token: self.token,
                select: self
                    .select
                    .iter()
                    .map(|name| std::ffi::OsString::from_wide(name))
                    .collect(),
                properties: self.properties,
            }
        }
    }

    #[cfg(windows)]
    impl Viewing {
        pub(super) fn of(viewing: &super::Viewing) -> Viewing {
            use std::os::windows::ffi::OsStrExt;
            Viewing {
                token: viewing.token.clone(),
                files: viewing
                    .files
                    .iter()
                    .map(|file| file.as_os_str().encode_wide().collect())
                    .collect(),
                piped: viewing
                    .piped
                    .as_ref()
                    .map(|piped| piped.as_os_str().encode_wide().collect()),
            }
        }

        pub(super) fn into_viewing(self) -> super::Viewing {
            use std::os::windows::ffi::OsStringExt;
            super::Viewing {
                files: self
                    .files
                    .iter()
                    .map(|file| std::ffi::OsString::from_wide(file).into())
                    .collect(),
                piped: self
                    .piped
                    .map(|piped| std::ffi::OsString::from_wide(&piped).into()),
                token: self.token,
            }
        }
    }

    /// The message saying `message`, its length first; `None` when its
    /// body would be larger than [`LARGEST`].
    pub(super) fn encode(message: &Message) -> Option<Vec<u8>> {
        let mut body = vec![VERSION];
        match message {
            Message::Open(fields) => {
                body.push(OPEN);
                units(&mut body, fields.location.as_deref());
                units(&mut body, Some(&fields.cwd));
                field(&mut body, fields.token.as_deref().map(str::as_bytes));
                list(&mut body, &fields.select);
                body.push(u8::from(fields.properties));
            }
            Message::View(viewing) => {
                body.push(VIEW);
                field(&mut body, viewing.token.as_deref().map(str::as_bytes));
                list(&mut body, &viewing.files);
                units(&mut body, viewing.piped.as_deref());
            }
        }
        if body.len() > LARGEST {
            return None;
        }
        let mut said = u32::try_from(body.len()).ok()?.to_le_bytes().to_vec();
        said.extend(body);
        Some(said)
    }

    /// How many, then each; one too many for a `u32` makes the body too
    /// large.
    fn list(into: &mut Vec<u8>, each: &[Vec<u16>]) {
        into.extend(u32::try_from(each.len()).unwrap_or(NONE).to_le_bytes());
        for one in each {
            units(into, Some(one));
        }
    }

    fn units(into: &mut Vec<u8>, units: Option<&[u16]>) {
        let bytes: Option<Vec<u8>> =
            units.map(|units| units.iter().flat_map(|unit| unit.to_le_bytes()).collect());
        field(into, bytes.as_deref());
    }

    /// A field longer than a `u32` says makes the body larger than
    /// [`LARGEST`]: refused whole.
    fn field(into: &mut Vec<u8>, bytes: Option<&[u8]>) {
        match bytes {
            Some(bytes) => {
                into.extend(u32::try_from(bytes.len()).unwrap_or(NONE).to_le_bytes());
                into.extend_from_slice(bytes);
            }
            None => into.extend(NONE.to_le_bytes()),
        }
    }

    /// What a message's body (its length taken off) says; `None` when it
    /// is not one of [`VERSION`], of no kind known, cut short, or more.
    pub(super) fn decode(body: &[u8]) -> Option<Message> {
        let (&version, rest) = body.split_first()?;
        if version != VERSION {
            return None;
        }
        let (&kind, mut rest) = rest.split_first()?;
        let message = match kind {
            OPEN => {
                let location = match take(&mut rest)? {
                    Field::Bytes(bytes) => Some(units_of(bytes)?),
                    Field::None => None,
                };
                let Field::Bytes(cwd) = take(&mut rest)? else {
                    return None;
                };
                let cwd = units_of(cwd)?;
                let Token(token) = token(&mut rest)?;
                let select = taken_list(&mut rest)?;
                let (&properties, after) = rest.split_first()?;
                rest = after;
                let properties = match properties {
                    0 => false,
                    1 => true,
                    _ => return None,
                };
                Message::Open(Fields {
                    location,
                    cwd,
                    token,
                    select,
                    properties,
                })
            }
            VIEW => {
                let Token(token) = token(&mut rest)?;
                let files = taken_list(&mut rest)?;
                let piped = match take(&mut rest)? {
                    Field::Bytes(bytes) => Some(units_of(bytes)?),
                    Field::None => None,
                };
                Message::View(Viewing {
                    token,
                    files,
                    piped,
                })
            }
            _ => return None,
        };
        rest.is_empty().then_some(message)
    }

    /// A token; `None` when it is not UTF-8 or cut short.
    fn token(rest: &mut &[u8]) -> Option<Token> {
        Some(match take(rest)? {
            Field::Bytes(bytes) => Token(Some(String::from_utf8(bytes.to_vec()).ok()?)),
            Field::None => Token(None),
        })
    }

    /// A token read, which may be none.
    struct Token(Option<String>);

    /// A count and as many paths or names, none of them none.
    fn taken_list(rest: &mut &[u8]) -> Option<Vec<Vec<u16>>> {
        let (count, after) = rest.split_first_chunk::<4>()?;
        *rest = after;
        let count = usize::try_from(u32::from_le_bytes(*count)).ok()?;
        // Each takes four bytes at least: a count larger than what is
        // left is refused before anything is kept for it.
        if count > rest.len() / 4 {
            return None;
        }
        let mut each = Vec::with_capacity(count);
        for _ in 0..count {
            let Field::Bytes(one) = take(rest)? else {
                return None;
            };
            each.push(units_of(one)?);
        }
        Some(each)
    }

    /// A field as read.
    enum Field<'a> {
        None,
        Bytes(&'a [u8]),
    }

    /// The next field of `rest`; `None` when it is cut short.
    fn take<'a>(rest: &mut &'a [u8]) -> Option<Field<'a>> {
        let (length, after) = rest.split_first_chunk::<4>()?;
        let length = u32::from_le_bytes(*length);
        if length == NONE {
            *rest = after;
            return Some(Field::None);
        }
        let field = after.get(..usize::try_from(length).ok()?)?;
        *rest = &after[field.len()..];
        Some(Field::Bytes(field))
    }

    fn units_of(bytes: &[u8]) -> Option<Vec<u16>> {
        let (pairs, odd) = bytes.as_chunks::<2>();
        odd.is_empty()
            .then(|| pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect())
    }
}

#[cfg(test)]
mod frame_tests {
    use super::frame::{
        CLOSED, CONFIRMED, Fields, LARGEST, Message, NOT_TAKEN, OPEN, TAKEN, VERSION, VIEW,
        Viewing, decode, encode,
    };

    fn body(message: &[u8]) -> &[u8] {
        let (length, body) = message.split_first_chunk::<4>().unwrap();
        assert_eq!(
            usize::try_from(u32::from_le_bytes(*length)).unwrap(),
            body.len()
        );
        body
    }

    fn every() -> Fields {
        Fields {
            // `C:\Dossier d'été\` with a lone surrogate, as a Windows
            // name may hold.
            location: Some(
                "C:\\Dossier d'été\\"
                    .encode_utf16()
                    .chain([0xD800, u16::from(b'x')])
                    .collect(),
            ),
            cwd: r"C:\Users\someone".encode_utf16().collect(),
            token: Some("token 1".into()),
            // Two names to select, one with a lone surrogate, and an
            // empty one.
            select: vec![
                "it's a draft.txt".encode_utf16().collect(),
                [u16::from(b'a'), 0xDC00].to_vec(),
                Vec::new(),
            ],
            properties: true,
        }
    }

    #[test]
    fn a_message_says_what_it_carries() {
        for fields in [
            every(),
            Fields {
                location: None,
                token: None,
                ..every()
            },
            Fields {
                location: Some(Vec::new()),
                cwd: Vec::new(),
                token: Some(String::new()),
                select: Vec::new(),
                properties: false,
            },
        ] {
            let message = Message::Open(fields);
            let said = encode(&message).unwrap();
            assert_eq!(decode(body(&said)), Some(message));
        }
        assert_ne!(TAKEN, 0, "an answer is told apart from nothing");
        let said = [TAKEN, CLOSED, CONFIRMED, NOT_TAKEN];
        for (at, one) in said.iter().enumerate() {
            assert!(
                said[at + 1..].iter().all(|other| other != one),
                "what a viewing's connection says is told apart"
            );
        }
    }

    fn viewing() -> Viewing {
        Viewing {
            token: Some("token 2".into()),
            // Two files, one with a lone surrogate.
            files: vec![
                r"C:\Users\someone\it's a note.md".encode_utf16().collect(),
                [u16::from(b'C'), u16::from(b':'), 0xD800].to_vec(),
            ],
            piped: Some(
                r"C:\Users\someone\AppData\stdin.png"
                    .encode_utf16()
                    .collect(),
            ),
        }
    }

    /// A viewing says its files, the one piped and the token, or none of
    /// them (the system's dialog asks which); cut short, of a kind not
    /// known, or naming a file that is none, it is refused.
    #[test]
    fn a_viewing_says_what_it_carries() {
        let bare = Viewing {
            token: None,
            files: Vec::new(),
            piped: None,
        };
        for viewing in [viewing(), bare.clone()] {
            let message = Message::View(viewing);
            let said = encode(&message).unwrap();
            assert_eq!(body(&said)[1], VIEW);
            assert_eq!(decode(body(&said)), Some(message));
        }
        let said = encode(&Message::View(viewing())).unwrap();
        let whole = body(&said);
        for end in 0..whole.len() {
            assert_eq!(decode(&whole[..end]), None, "cut at {end}");
        }
        let mut other = whole.to_vec();
        other[1] = VIEW + 1;
        assert_eq!(decode(&other), None);
        // No token, then one file that is none, nothing piped.
        let mut none = vec![VERSION, VIEW];
        none.extend(u32::MAX.to_le_bytes());
        none.extend(1u32.to_le_bytes());
        none.extend(u32::MAX.to_le_bytes());
        none.extend(u32::MAX.to_le_bytes());
        assert_eq!(decode(&none), None);
        let mut nothing = vec![VERSION, VIEW];
        nothing.extend(u32::MAX.to_le_bytes());
        nothing.extend(0u32.to_le_bytes());
        nothing.extend(u32::MAX.to_le_bytes());
        assert_eq!(decode(&nothing), Some(Message::View(bare)));
    }

    #[test]
    fn what_is_not_a_message_is_refused() {
        let message = encode(&Message::Open(every())).unwrap();
        let whole = body(&message);
        // Cut short anywhere.
        for end in 0..whole.len() {
            assert_eq!(decode(&whole[..end]), None, "cut at {end}");
        }
        // More after it.
        let mut more = whole.to_vec();
        more.push(0);
        assert_eq!(decode(&more), None);
        // Another version.
        let mut other = whole.to_vec();
        other[0] = VERSION + 1;
        assert_eq!(decode(&other), None);
        // A path of an odd number of bytes, a folder that is none, a
        // token not UTF-8.
        let field = |bytes: Option<&[u8]>| {
            let mut out = Vec::new();
            match bytes {
                Some(bytes) => {
                    out.extend(u32::try_from(bytes.len()).unwrap().to_le_bytes());
                    out.extend_from_slice(bytes);
                }
                None => out.extend(u32::MAX.to_le_bytes()),
            }
            out
        };
        // Three fields, then no names and no Properties.
        let made = |fields: [Option<&[u8]>; 3]| {
            let mut body = vec![VERSION, OPEN];
            for f in fields {
                body.extend(field(f));
            }
            body.extend(0u32.to_le_bytes());
            body.push(0);
            body
        };
        assert!(decode(&made([None, Some(b"a\0"), None])).is_some());
        // More names counted than the body holds; a name that is none;
        // Properties neither shown nor not.
        let mut counted = made([None, Some(b"a\0"), None]);
        let at = counted.len() - 5;
        counted[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode(&counted), None);
        let mut nameless = made([None, Some(b"a\0"), None]);
        nameless.truncate(nameless.len() - 5);
        nameless.extend(1u32.to_le_bytes());
        nameless.extend(field(None));
        nameless.push(0);
        assert_eq!(decode(&nameless), None);
        let mut neither = made([None, Some(b"a\0"), None]);
        *neither.last_mut().unwrap() = 2;
        assert_eq!(decode(&neither), None);
        assert_eq!(decode(&made([Some(b"abc"), Some(b"a\0"), None])), None);
        assert_eq!(decode(&made([None, None, None])), None);
        assert_eq!(decode(&made([None, Some(b"a\0"), Some(b"\xff")])), None);
    }

    #[test]
    fn a_message_larger_than_the_pipe_takes_is_not_made() {
        let at = |units: usize| Fields {
            location: None,
            cwd: vec![u16::from(b'a'); units],
            token: None,
            select: Vec::new(),
            properties: false,
        };
        // The version, the kind, three lengths, the folder, no names
        // counted, no Properties.
        let around = (LARGEST - 2 - 12 - 4 - 1) / 2;
        assert!(encode(&Message::Open(at(around))).is_some());
        assert_eq!(encode(&Message::Open(at(around + 1))), None);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::linux::{ask_holder, hand_over_at, take_over_with};
    use super::*;
    use std::io::BufRead;
    use std::os::unix::ffi::OsStringExt;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;
    use zbus::blocking::Connection;

    /// A bus daemon of the test's own, never the session's: stopped when
    /// dropped.
    struct Daemon {
        child: std::process::Child,
        address: String,
    }

    impl Drop for Daemon {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// A folder of the test's own under the tests' cache.
    fn folder() -> tempfile::TempDir {
        let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new()
            .prefix("handover-")
            .tempdir_in(base)
            .unwrap()
    }

    /// A daemon in `folder` listening at `listen` (`unix:dir=` for any
    /// socket, `unix:path=` for one started again at the same address).
    /// `None` without `dbus-daemon`.
    fn daemon(folder: &Path, listen: &str) -> Option<Daemon> {
        let config = folder.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>{listen}</listen>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#
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
        })
    }

    /// Connections to `address`, a new one at each call, as the program
    /// makes the session's (`bus::session`).
    fn connector(address: &str) -> impl FnMut() -> zbus::Result<Connection> + Send + 'static {
        let address = address.to_owned();
        move || zbus::blocking::connection::Builder::address(address.as_str())?.build()
    }

    /// A launch's own connection to `address`, as `patient_session`.
    fn patient(address: &str) -> impl FnOnce() -> zbus::Result<Connection> {
        let address = address.to_owned();
        move || {
            zbus::blocking::connection::Builder::address(address.as_str())?
                .method_timeout(PATIENCE)
                .build()
        }
    }

    /// The bus name the tests' Skwir holds, as an instance's is
    /// (`linux::name`).
    const HELD: &str = "org.skwir.skwir.Itest";

    type Told = Arc<parking_lot::Mutex<Vec<Opening>>>;

    fn taker() -> (Told, impl Fn(Opening) + Send + Sync + 'static) {
        let told = Told::default();
        let into = Arc::clone(&told);
        (told, move |opening| into.lock().push(opening))
    }

    fn held(address: &str) -> bool {
        let bus = connector(address)().unwrap();
        zbus::blocking::fdo::DBusProxy::new(&bus)
            .unwrap()
            .name_has_owner(HELD.try_into().unwrap())
            .unwrap()
    }

    fn opening() -> Opening {
        Opening {
            // Not UTF-8, with an apostrophe and a space.
            location: Some(OsString::from_vec(b"/srv/Dossier d'\xe9t\xe9".to_vec())),
            cwd: PathBuf::from(OsString::from_vec(b"/srv/caf\xe9".to_vec())),
            token: Some("activation-1".into()),
            // Names to select: one not UTF-8, one with an apostrophe.
            select: vec![
                OsString::from_vec(b"r\xe9sum\xe9.txt".to_vec()),
                OsString::from("it's a draft.txt"),
            ],
            properties: true,
        }
    }

    fn until(what: &str, done: &dyn Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "{what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    macro_rules! bus_or_skip {
        ($folder:expr, $listen:expr) => {
            match daemon($folder, $listen) {
                Some(daemon) => daemon,
                None => {
                    eprintln!("skipped: dbus-daemon is not installed");
                    return;
                }
            }
        };
    }

    /// A launch is handed only to a Skwir of its own instance: another
    /// one's name, even behind the same marker, takes nothing.
    #[test]
    fn a_launch_goes_to_its_own_instance_alone() {
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let marker = folder.path().join("skwir").join("running-test");
        let (told, taken) = taker();
        let taking =
            take_over_with(connector(&bus.address), marker.clone(), HELD.into(), taken).unwrap();
        // The marker's folder, where agents and scripts listen too, made
        // the user's alone by the marker that came first.
        {
            use std::os::unix::fs::PermissionsExt as _;
            let made = std::fs::metadata(marker.parent().unwrap()).unwrap();
            assert_eq!(made.permissions().mode() & 0o777, 0o700);
        }
        let other = "org.skwir.skwir.Iother";
        assert!(!hand_over_at(
            &marker,
            other,
            patient(&bus.address),
            &opening()
        ));
        assert!(told.lock().is_empty());
        assert!(hand_over_at(
            &marker,
            HELD,
            patient(&bus.address),
            &opening()
        ));
        assert_eq!(told.lock().len(), 1);
        drop(taking);
    }

    /// Where no Skwir finishes, a launch makes no connection.
    #[test]
    fn without_a_marker_a_launch_connects_to_nothing() {
        let folder = folder();
        let connected = AtomicBool::new(false);
        let handed = hand_over_at(
            &folder.path().join("skwir").join("finishing"),
            HELD,
            || {
                connected.store(true, Ordering::SeqCst);
                Err(zbus::Error::Unsupported)
            },
            &opening(),
        );
        assert!(!handed);
        assert!(!connected.load(Ordering::SeqCst), "no connection made");
    }

    /// What a launch asks arrives whole (a path not UTF-8, a token, no
    /// location), taken before the launch is answered; the marker holds
    /// our pid while taking over.
    #[test]
    fn a_launch_is_handed_over_exactly() {
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let marker = folder.path().join("skwir").join("finishing");
        let (told, taken) = taker();
        let taking =
            take_over_with(connector(&bus.address), marker.clone(), HELD.into(), taken).unwrap();
        assert!(held(&bus.address));
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap().trim(),
            std::process::id().to_string()
        );
        let first = opening();
        assert!(hand_over_at(&marker, HELD, patient(&bus.address), &first));
        assert_eq!(
            *told.lock(),
            std::slice::from_ref(&first),
            "taken before answered"
        );
        let bare = Opening {
            location: None,
            cwd: PathBuf::from("/"),
            ..Opening::default()
        };
        assert!(hand_over_at(&marker, HELD, patient(&bus.address), &bare));
        assert_eq!(*told.lock(), [first, bare]);
        drop(taking);
    }

    /// Connections to `address` as `connector` makes them, each also kept
    /// in `kept` until the test ends: as the program's shared one stays up
    /// once taking over stops, so what a launch is told is what was said,
    /// never the connection going.
    fn kept_connector(
        address: &str,
        kept: &Arc<parking_lot::Mutex<Vec<Connection>>>,
    ) -> impl FnMut() -> zbus::Result<Connection> + Send + 'static {
        let (mut connect, kept) = (connector(address), Arc::clone(kept));
        move || {
            let bus = connect()?;
            kept.lock().push(bus.clone());
            Ok(bus)
        }
    }

    /// A launch's own connection to `address`, as `waiting_session`.
    fn waiting(
        address: &str,
    ) -> impl FnOnce() -> zbus::Result<zbus::connection::Builder<'static>> + Send + 'static {
        let address = address.to_owned();
        move || zbus::connection::Builder::address(address.as_str())
    }

    /// What a launch of this process keeps of standard input, as `skw -`
    /// does: `stdin.<extension>` in its spool.
    fn spool(extension: &str) -> PathBuf {
        let pid = std::process::id();
        skwir_core::folders::private(&super::spool_folder(pid)).unwrap();
        let file = super::spool_file(pid, Some(extension));
        std::fs::write(&file, b"piped").unwrap();
        file
    }

    /// A Skwir taking launches and viewings over on `bus`, the viewings
    /// handed in kept in the returned list; its connections kept too.
    fn viewing_skwir(
        bus: &Daemon,
        marker: &Path,
    ) -> (
        TakingOver,
        Arc<parking_lot::Mutex<Vec<Viewed>>>,
        Arc<parking_lot::Mutex<Vec<Connection>>>,
    ) {
        let viewed = Arc::new(parking_lot::Mutex::new(Vec::<Viewed>::new()));
        let into = Arc::clone(&viewed);
        let gate = Gate::viewing(|_| {}, move |handed| into.lock().push(handed));
        let shared = Arc::<parking_lot::Mutex<Vec<Connection>>>::default();
        let taking = super::linux::take_over_on(
            kept_connector(&bus.address, &shared),
            marker.to_owned(),
            HELD.into(),
            gate,
        )
        .unwrap();
        (taking, viewed, shared)
    }

    /// `View` asked as a launch asks it, on `launch`: the viewing's
    /// number, or the error's name.
    fn ask_view(launch: &Connection, files: &[&Path], piped: Option<&Path>) -> Result<u64, String> {
        let files: Vec<&[u8]> = files
            .iter()
            .map(|file| file.as_os_str().as_encoded_bytes())
            .collect();
        let piped = piped.map_or(&[][..], |piped| piped.as_os_str().as_encoded_bytes());
        match launch.call_method(
            Some(HELD),
            super::linux::PATH,
            Some(super::linux::INTERFACE),
            "View",
            &(files, piped, ""),
        ) {
            Ok(reply) => Ok(reply.body().deserialize().unwrap()),
            Err(zbus::Error::MethodError(name, ..)) => Err(name.as_str().to_owned()),
            Err(error) => panic!("View: {error}"),
        }
    }

    /// `Viewed` asked for `id` on `launch`, its answer not waited for: a
    /// launch confirming its viewing.
    fn confirm(launch: &Connection, id: u64) {
        let asked = zbus::Message::method_call(super::linux::PATH, "Viewed")
            .unwrap()
            .destination(HELD)
            .unwrap()
            .interface(super::linux::INTERFACE)
            .unwrap()
            .build(&(id,))
            .unwrap();
        launch.send(&asked).unwrap();
    }

    /// What a launch views arrives whole (a path not UTF-8, the file piped,
    /// a token), and the launch waits until its window closes, launches
    /// taken meanwhile; the program's end then waits for no one. One
    /// waiting while the Skwir stops taking launches over is told it ended,
    /// by that Skwir: its connection stays.
    #[test]
    fn a_viewing_waits_for_its_window() {
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let marker = folder.path().join("skwir").join("finishing");
        let (taking, viewed, _shared) = viewing_skwir(&bus, &marker);
        let piped = spool("png");
        let asked = Viewing {
            files: vec![
                PathBuf::from(OsString::from_vec(b"/srv/caf\xe9/it's a note.md".to_vec())),
                piped.clone(),
            ],
            piped: Some(piped.clone()),
            token: Some("activation-2".into()),
        };
        let launch = {
            let (address, marker, asked) = (bus.address.clone(), marker.clone(), asked.clone());
            std::thread::spawn(move || {
                super::linux::view_at(&marker, HELD, waiting(&address), &asked)
            })
        };
        until("the viewing handed in", &|| !viewed.lock().is_empty());
        let first = viewed.lock()[0].clone();
        assert_eq!(first.viewing(), &asked);
        assert!(!first.is_gone());
        // A launch opening a folder is taken while the viewing waits.
        assert!(hand_over_at(
            &marker,
            HELD,
            patient(&bus.address),
            &opening()
        ));
        std::thread::sleep(Duration::from_millis(200));
        assert!(!launch.is_finished(), "the launch waits for its window");
        first.close();
        assert_eq!(launch.join().unwrap(), ViewedElsewhere::Closed);
        let started = Instant::now();
        super::wait_told();
        assert!(started.elapsed() < PATIENCE, "the launch read it and went");
        assert!(piped.exists(), "its launch, there, removes what it kept");

        // The Skwir stops while a launch waits.
        let launch = {
            let (address, marker, asked) = (bus.address.clone(), marker.clone(), asked.clone());
            std::thread::spawn(move || {
                super::linux::view_at(&marker, HELD, waiting(&address), &asked)
            })
        };
        until("the second viewing handed in", &|| viewed.lock().len() == 2);
        drop(taking);
        assert_eq!(launch.join().unwrap(), ViewedElsewhere::Ended);
        // None holds launches: nothing is taken, and the launch views them
        // itself.
        std::fs::write(&marker, "1\n").unwrap();
        assert_eq!(
            super::linux::view_at(&marker, HELD, waiting(&bus.address), &asked),
            ViewedElsewhere::NotTaken
        );
        super::let_go_of_spool(&piped);
    }

    /// A launch that confirms its viewing and goes (its terminal's Ctrl+C)
    /// is told to its window, and once the window let go, what it kept of
    /// standard input is removed: no launch is left to.
    #[test]
    fn a_launch_that_goes_closes_its_window_and_its_spool_goes() {
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let marker = folder.path().join("skwir").join("finishing");
        let (taking, viewed, _shared) = viewing_skwir(&bus, &marker);
        let piped = spool("txt");
        let gone = Arc::new(AtomicBool::new(false));
        {
            let launch = connector(&bus.address)().unwrap();
            let id = ask_view(&launch, &[piped.as_path()], Some(piped.as_path())).unwrap();
            confirm(&launch, id);
            until("the viewing handed in", &|| viewed.lock().len() == 1);
            let handed = viewed.lock()[0].clone();
            assert_eq!(handed.viewing().piped.as_deref(), Some(piped.as_path()));
            let told = Arc::clone(&gone);
            handed.when_gone(move || told.store(true, Ordering::SeqCst));
            assert!(!gone.load(Ordering::SeqCst));
        }
        until("the launch gone is told", &|| gone.load(Ordering::SeqCst));
        assert!(viewed.lock()[0].is_gone());
        std::thread::sleep(Duration::from_millis(100));
        assert!(piped.exists(), "kept while its window shows it");
        viewed.lock().clear();
        until("what it kept removed", &|| !piped.exists());
        assert!(
            !super::spool_folder(std::process::id()).exists(),
            "and its folder"
        );
        drop(taking);
    }

    /// Nothing is shown until its launch confirms it: a launch that gave up
    /// (here, one that never confirms, then goes) shows nothing, and one
    /// that confirms past the patience is told it was not taken, and views
    /// the files itself. Asked of a Skwir that no longer takes viewings, it
    /// is not taken either.
    #[test]
    fn a_viewing_not_confirmed_is_never_shown() {
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let marker = folder.path().join("skwir").join("finishing");
        let (taking, viewed, _shared) = viewing_skwir(&bus, &marker);
        {
            let launch = connector(&bus.address)().unwrap();
            ask_view(&launch, &[], None).unwrap();
        }
        let late = connector(&bus.address)().unwrap();
        let id = ask_view(&late, &[], None).unwrap();
        std::thread::sleep(PATIENCE + Duration::from_millis(500));
        let answered = late.call_method(
            Some(HELD),
            super::linux::PATH,
            Some(super::linux::INTERFACE),
            "Viewed",
            &(id,),
        );
        assert!(
            matches!(&answered, Err(zbus::Error::MethodError(name, ..))
                if name.as_str() == super::linux::NOT_TAKEN),
            "{answered:?}"
        );
        assert!(viewed.lock().is_empty(), "nothing shown");
        // Confirmed in time, a window that could not open is not taken
        // either: the launch views the files itself.
        let asked = Viewing::default();
        let launch = {
            let (address, marker) = (bus.address.clone(), marker.clone());
            std::thread::spawn(move || {
                super::linux::view_at(&marker, HELD, waiting(&address), &asked)
            })
        };
        until("the viewing handed in", &|| viewed.lock().len() == 1);
        viewed.lock()[0].not_shown();
        assert_eq!(launch.join().unwrap(), ViewedElsewhere::NotTaken);
        // No longer taking them: refused at once.
        assert!(taking.close_unless(|| false));
        let started = Instant::now();
        assert_eq!(
            super::linux::view_at(&marker, HELD, waiting(&bus.address), &Viewing::default()),
            ViewedElsewhere::NotTaken
        );
        assert!(started.elapsed() < PATIENCE, "not waited out");
        drop(taking);
    }

    /// What stands for standard input is taken only as the caller's own
    /// spool: a path elsewhere, another process's spool, or one not among
    /// the files, is refused, nothing shown.
    #[test]
    fn a_viewing_names_its_own_spool_alone() {
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let marker = folder.path().join("skwir").join("finishing");
        let (taking, viewed, _shared) = viewing_skwir(&bus, &marker);
        let launch = connector(&bus.address)().unwrap();
        // A file of the user's elsewhere.
        let elsewhere = folder.path().join("a thesis.odt");
        std::fs::write(&elsewhere, b"not to be removed").unwrap();
        // Another process's spool: no process has the largest pid.
        let other = super::spool_folder(u32::MAX);
        skwir_core::folders::private(&other).unwrap();
        let others = super::spool_file(u32::MAX, None);
        std::fs::write(&others, b"piped").unwrap();
        let own = spool("md");
        for (files, piped) in [
            (vec![elsewhere.as_path()], elsewhere.as_path()),
            (vec![others.as_path()], others.as_path()),
            (vec![elsewhere.as_path()], own.as_path()),
        ] {
            assert_eq!(
                ask_view(&launch, &files, Some(piped)),
                Err(super::linux::NOT_TAKEN.to_owned()),
                "{piped:?}"
            );
        }
        assert!(elsewhere.exists() && others.exists());
        // Its own, among the files: taken.
        let id = ask_view(&launch, &[own.as_path()], Some(own.as_path())).unwrap();
        confirm(&launch, id);
        until("the viewing handed in", &|| viewed.lock().len() == 1);
        assert_eq!(
            viewed.lock()[0].viewing().piped.as_deref(),
            Some(own.as_path())
        );
        viewed.lock()[0].close();
        drop(taking);
        super::let_go_of_spool(&own);
        super::let_go_of_spool(&others);
    }

    /// The rule a spool keeps (`spooled_by`): the launch's own folder, a
    /// regular file named `stdin` or `stdin.<extension>`, among the files,
    /// no link, no step back.
    #[test]
    fn a_spool_is_the_launchs_own_file() {
        let root = folder();
        let spools = root.path().join("view");
        let folder = spools.join("42");
        skwir_core::folders::private(&folder).unwrap();
        let stdin = folder.join("stdin");
        let png = folder.join("stdin.png");
        std::fs::write(&stdin, b"x").unwrap();
        std::fs::write(&png, b"x").unwrap();
        let ok = |piped: &Path| super::spooled_by(piped, &[piped.to_owned()], &spools, 42);
        assert!(ok(&stdin) && ok(&png));
        assert!(
            !super::spooled_by(&png, std::slice::from_ref(&stdin), &spools, 42),
            "not among the files"
        );
        assert!(
            !super::spooled_by(&png, std::slice::from_ref(&png), &spools, 43),
            "another's"
        );
        // Named otherwise, a folder, a link, a step back.
        let named = |name: &str| folder.join(name);
        std::fs::write(named("stdin."), b"x").unwrap();
        std::fs::write(named("stdout"), b"x").unwrap();
        std::fs::create_dir(named("stdin.d")).unwrap();
        let outside = root.path().join("a thesis.odt");
        std::fs::write(&outside, b"x").unwrap();
        std::os::unix::fs::symlink(&outside, named("stdin.odt")).unwrap();
        for piped in [
            named("stdin."),
            named("stdout"),
            named("stdin.d"),
            named("stdin.odt"),
            named("nowhere/../stdin"),
            spools.join("42/./stdin.png"),
            root.path().join("stdin"),
        ] {
            assert!(!ok(&piped), "{piped:?}");
        }
        // A folder that is a link to elsewhere.
        let real = root.path().join("real");
        std::fs::create_dir(&real).unwrap();
        std::fs::write(real.join("stdin"), b"x").unwrap();
        std::os::unix::fs::symlink(&real, spools.join("43")).unwrap();
        assert!(!super::spooled_by(
            &spools.join("43/stdin"),
            &[spools.join("43/stdin")],
            &spools,
            43
        ));
        assert!(outside.exists() && real.join("stdin").exists());
    }

    /// A viewing whose copies are all let go is closed: its launch is told
    /// so, and its launch going is no more told.
    #[test]
    fn a_viewing_let_go_is_closed() {
        let viewed = Viewed::new(Viewing::default());
        let state = Arc::clone(&viewed.holder.state);
        let told = Arc::new(AtomicBool::new(false));
        let into = Arc::clone(&told);
        viewed.when_gone(move || into.store(true, Ordering::SeqCst));
        let copy = viewed.clone();
        drop(viewed);
        assert!(!state.is_closed(), "a copy holds it");
        drop(copy);
        assert!(state.is_closed());
        state.go();
        assert!(!told.load(Ordering::SeqCst), "closed first: never gone");
        // Gone first: told at once, to a hook given after too.
        let viewed = Viewed::new(Viewing::default());
        viewed.gone_for_test();
        let into = Arc::clone(&told);
        viewed.when_gone(move || into.store(true, Ordering::SeqCst));
        assert!(told.load(Ordering::SeqCst));
    }

    /// A marker left after a crash: no one answers, the launch goes on,
    /// and the marker, not ours, stays.
    #[test]
    fn a_marker_left_behind_hands_nothing_over_and_stays() {
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let marker = folder.path().join("finishing");
        std::fs::write(&marker, "4194304\n").unwrap();
        let started = Instant::now();
        assert!(!hand_over_at(
            &marker,
            HELD,
            patient(&bus.address),
            &opening()
        ));
        assert!(started.elapsed() < PATIENCE, "not waited out");
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "4194304\n");
    }

    /// Dropped, it lets go of the name and of its marker, and launches go
    /// on as processes of their own; a marker another Skwir wrote since
    /// is that Skwir's, and stays.
    #[test]
    fn dropped_it_lets_go_of_the_name_and_its_marker_alone() {
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let marker = folder.path().join("skwir").join("finishing");
        let (told, taken) = taker();
        let taking =
            take_over_with(connector(&bus.address), marker.clone(), HELD.into(), taken).unwrap();
        assert!(held(&bus.address));
        drop(taking);
        assert!(!held(&bus.address), "the name let go of");
        assert!(!marker.exists(), "the marker removed");
        assert!(!hand_over_at(
            &marker,
            HELD,
            patient(&bus.address),
            &opening()
        ));
        assert!(told.lock().is_empty());

        let (_, taken) = taker();
        let taking =
            take_over_with(connector(&bus.address), marker.clone(), HELD.into(), taken).unwrap();
        std::fs::write(&marker, "4194304\n").unwrap();
        drop(taking);
        assert!(!held(&bus.address));
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "4194304\n");
    }

    /// Stopped aside, it takes no launch from then on, and lets go of the
    /// name and of its marker on a thread of its own.
    #[test]
    fn stopped_aside_it_takes_nothing_and_lets_go_soon() {
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let marker = folder.path().join("skwir").join("finishing");
        let (told, taken) = taker();
        let taking =
            take_over_with(connector(&bus.address), marker.clone(), HELD.into(), taken).unwrap();
        taking.stop_aside();
        assert!(!hand_over_at(
            &marker,
            HELD,
            patient(&bus.address),
            &opening()
        ));
        assert!(told.lock().is_empty(), "nothing taken once stopped");
        until("the name let go of", &|| !held(&bus.address));
        until("the marker removed", &|| !marker.exists());
    }

    /// A launch that lost the race to take launches over hands itself to
    /// the one that won, even before that one's marker is written.
    #[test]
    fn a_launch_that_lost_the_name_is_taken_by_the_one_that_holds_it() {
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let marker = folder.path().join("skwir").join("finishing");
        let (told, taken) = taker();
        let first =
            take_over_with(connector(&bus.address), marker.clone(), HELD.into(), taken).unwrap();
        std::fs::remove_file(&marker).unwrap();
        assert!(
            !hand_over_at(&marker, HELD, patient(&bus.address), &opening()),
            "a launch finding no marker goes on"
        );
        assert!(ask_holder(HELD, patient(&bus.address), &opening()));
        assert_eq!(*told.lock(), vec![opening()]);
        drop(first);
    }

    /// A launch handed over just before the program ends keeps it: the
    /// program looks, and goes on taking launches over. With none waiting,
    /// it stops: the next launch is refused.
    #[test]
    fn a_launch_waiting_keeps_the_program_taking_launches_over() {
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let marker = folder.path().join("skwir").join("finishing");
        let (told, taken) = taker();
        let taking =
            take_over_with(connector(&bus.address), marker.clone(), HELD.into(), taken).unwrap();
        assert!(hand_over_at(
            &marker,
            HELD,
            patient(&bus.address),
            &opening()
        ));
        let waiting = || !told.lock().is_empty();
        assert!(!taking.close_unless(waiting), "a launch waits");
        told.lock().clear();
        assert!(hand_over_at(
            &marker,
            HELD,
            patient(&bus.address),
            &opening()
        ));
        assert_eq!(told.lock().len(), 1, "taking launches over goes on");
        told.lock().clear();
        assert!(taking.close_unless(waiting), "none waits");
        assert!(!hand_over_at(
            &marker,
            HELD,
            patient(&bus.address),
            &opening()
        ));
        assert!(told.lock().is_empty(), "refused once closed");
        drop(taking);
    }

    /// While one Skwir takes launches over, another cannot, on a
    /// connection of its own (another process) or on the same (this one
    /// asking twice), and the first goes on taking them; once it lets go,
    /// another can.
    #[test]
    fn one_skwir_takes_launches_over_at_a_time() {
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let marker = folder.path().join("skwir").join("finishing");
        let shared = connector(&bus.address)().unwrap();
        let (told, taken) = taker();
        let first = {
            let shared = shared.clone();
            take_over_with(
                move || Ok::<_, zbus::Error>(shared.clone()),
                marker.clone(),
                HELD.into(),
                taken,
            )
            .unwrap()
        };
        let (_, other) = taker();
        assert_eq!(
            take_over_with(connector(&bus.address), marker.clone(), HELD.into(), other).err(),
            Some(NotTaken::Elsewhere),
            "another Skwir holds the name"
        );
        let (_, again) = taker();
        let same = {
            let shared = shared.clone();
            move || Ok::<_, zbus::Error>(shared.clone())
        };
        assert_eq!(
            take_over_with(same, marker.clone(), HELD.into(), again).err(),
            Some(NotTaken::Cannot),
            "this Skwir does already"
        );
        assert!(hand_over_at(
            &marker,
            HELD,
            patient(&bus.address),
            &opening()
        ));
        assert_eq!(told.lock().len(), 1, "the first still takes them");
        drop(first);

        let (told, taken) = taker();
        let second =
            take_over_with(connector(&bus.address), marker.clone(), HELD.into(), taken).unwrap();
        assert!(hand_over_at(
            &marker,
            HELD,
            patient(&bus.address),
            &opening()
        ));
        assert_eq!(told.lock().len(), 1);
        drop(second);
    }

    /// The session's bus started again: launches are taken over there
    /// once it is back.
    #[test]
    fn launches_are_taken_over_again_once_the_bus_is_back() {
        let folder = folder();
        // Named without the daemon's guid, which the next has another of.
        let address = format!("unix:path={}", folder.path().join("bus").display());
        let first = bus_or_skip!(folder.path(), &address);
        let marker = folder.path().join("skwir").join("finishing");
        let (told, taken) = taker();
        let taking =
            take_over_with(connector(&address), marker.clone(), HELD.into(), taken).unwrap();
        assert!(hand_over_at(&marker, HELD, patient(&address), &opening()));

        drop(first);
        let _second = daemon(folder.path(), &address).unwrap();
        until("taken over on the new bus", &|| {
            hand_over_at(&marker, HELD, patient(&address), &opening())
        });
        assert_eq!(told.lock().len(), 2);
        drop(taking);
        assert!(!held(&address));
        assert!(!marker.exists());
    }

    /// Who holds `org.freedesktop.FileManager1` on the bus at `address`:
    /// its unique name.
    fn file_manager_owner(address: &str) -> Option<String> {
        let bus = connector(address)().unwrap();
        zbus::blocking::fdo::DBusProxy::new(&bus)
            .unwrap()
            .get_name_owner(super::linux::FILE_MANAGER.try_into().unwrap())
            .ok()
            .map(|owner| owner.to_string())
    }

    /// Another program's request, `method` with `uris`, on its own
    /// connection to `address`: whether it was answered without an error.
    fn ask_to_show(address: &str, method: &str, uris: &[&str], token: &str) -> bool {
        let bus = patient(address)().unwrap();
        bus.call_method(
            Some(super::linux::FILE_MANAGER),
            "/org/freedesktop/FileManager1",
            Some("org.freedesktop.FileManager1"),
            method,
            &(uris, token),
        )
        .is_ok()
    }

    /// While answering, another program's Show in Folder is launches
    /// taken as a launch is: a folder each, its items to select, the
    /// token with the last; no longer answering, the name is let go, and
    /// a request then finds no one (the asker opens the folder itself).
    #[test]
    fn a_request_to_show_files_is_launches() {
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let marker = folder.path().join("skwir").join("finishing");
        let (told, taken) = taker();
        let taking =
            take_over_with(connector(&bus.address), marker.clone(), HELD.into(), taken).unwrap();
        assert_eq!(
            file_manager_owner(&bus.address),
            None,
            "not answering at first"
        );
        taking.answering().set(true);
        assert!(file_manager_owner(&bus.address).is_some());
        assert!(ask_to_show(
            &bus.address,
            "ShowItems",
            &[
                "file:///srv/Field%20Guide/it's%20a%20draft.txt",
                "file:///srv/Maps/tr%C3%B6isi%C3%A8me.png",
            ],
            "token-1",
        ));
        let shown = told.lock().clone();
        assert_eq!(
            shown
                .iter()
                .map(|o| (
                    o.location.clone().unwrap(),
                    o.select.clone(),
                    o.token.clone()
                ))
                .collect::<Vec<_>>(),
            [
                (
                    "/srv/Field Guide".into(),
                    vec![OsString::from("it's a draft.txt")],
                    None
                ),
                (
                    "/srv/Maps".into(),
                    vec![OsString::from("tröisième.png")],
                    Some("token-1".into())
                ),
            ]
        );
        assert!(ask_to_show(
            &bus.address,
            "ShowItemProperties",
            &["file:///srv/Maps/x.png"],
            ""
        ));
        assert!(told.lock().last().unwrap().properties);
        assert!(
            !ask_to_show(&bus.address, "ShowFolders", &["sftp://host/srv"], ""),
            "nothing local: refused, so the asker does what it does alone"
        );
        taking.answering().set(false);
        assert_eq!(file_manager_owner(&bus.address), None);
        assert!(!ask_to_show(&bus.address, "ShowFolders", &["/srv"], ""));
        assert_eq!(told.lock().len(), 3);
        drop(taking);
    }

    /// A holder that does not let go (Dolphin's daemon) answers until it
    /// ends; then the Skwir that waited in the queue does.
    #[test]
    fn a_holder_that_does_not_let_go_answers_until_it_ends() {
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let holder = connector(&bus.address)().unwrap();
        holder
            .request_name_with_flags(
                super::linux::FILE_MANAGER,
                zbus::fdo::RequestNameFlags::DoNotQueue.into(),
            )
            .unwrap();
        let marker = folder.path().join("skwir").join("finishing");
        let (_told, taken) = taker();
        let taking =
            take_over_with(connector(&bus.address), marker.clone(), HELD.into(), taken).unwrap();
        taking.answering().set(true);
        let holder_name = holder.unique_name().unwrap().to_string();
        assert_eq!(file_manager_owner(&bus.address), Some(holder_name.clone()));
        drop(holder);
        until("the Skwir that waited answers", &|| {
            file_manager_owner(&bus.address).is_some_and(|owner| owner != holder_name)
        });
        drop(taking);
        assert_eq!(file_manager_owner(&bus.address), None);
    }

    /// A program the bus started finds another Skwir taking launches over:
    /// the request that started it goes there whole, and the name is let
    /// go once handed.
    #[test]
    fn a_request_started_elsewhere_goes_to_the_skwir_that_runs() {
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let marker = folder.path().join("skwir").join("finishing");
        let (told, taken) = taker();
        let taking =
            take_over_with(connector(&bus.address), marker.clone(), HELD.into(), taken).unwrap();
        let started = connector(&bus.address)().unwrap();
        let address = bus.address.clone();
        let asker = std::thread::spawn(move || {
            until("the started program answers", &|| {
                file_manager_owner(&address).is_some()
            });
            ask_to_show(
                &address,
                "ShowItems",
                &["/srv/a/one.txt", "/srv/b/two.txt"],
                "token-2",
            )
        });
        let address = bus.address.clone();
        let handed = super::linux::answer_for_holder_on(&started, Duration::from_secs(10), |o| {
            ask_holder(HELD, patient(&address), o)
        });
        assert!(asker.join().unwrap(), "the asker was answered");
        assert!(handed);
        let told = told.lock().clone();
        assert_eq!(told.len(), 2);
        assert_eq!(told[1].token.as_deref(), Some("token-2"));
        assert_eq!(file_manager_owner(&bus.address), None, "let go once handed");
        drop(taking);
    }

    /// Who holds the file manager's name is followed: told once listening,
    /// then as a holder takes it and as it lets go, until stopped.
    #[test]
    fn the_holder_of_the_name_is_followed() {
        use std::sync::atomic::AtomicUsize;
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let told = Arc::new(AtomicUsize::new(0));
        let stop = skwir_core::CancelToken::new();
        let counted = Arc::clone(&told);
        crate::file_manager::linux::follow_shower_on(
            connector(&bus.address)().unwrap(),
            crate::file_manager::linux::owner_changes().unwrap(),
            &stop,
            move || {
                counted.fetch_add(1, Ordering::SeqCst);
            },
        );
        until("told once listening", &|| told.load(Ordering::SeqCst) >= 1);
        let holder = connector(&bus.address)().unwrap();
        holder.request_name(super::linux::FILE_MANAGER).unwrap();
        until("told as it is taken", &|| told.load(Ordering::SeqCst) >= 2);
        drop(holder);
        until("told as it is let go", &|| told.load(Ordering::SeqCst) >= 3);
        stop.cancel();
    }

    /// What the tests' Skwir is handed of other programs' dialogs.
    type Chosen = Arc<parking_lot::Mutex<Vec<crate::chooser::Choosing>>>;

    /// A Skwir taking launches over and dialogs, on the bus at `address`.
    fn choosing_skwir(address: &str, marker: &Path) -> (TakingOver, Chosen) {
        let chosen = Chosen::default();
        let into = Arc::clone(&chosen);
        let gate = super::Gate::choosing(|_| {}, move |choosing| into.lock().push(choosing));
        let taking =
            super::linux::take_over_on(connector(address), marker.to_owned(), HELD.into(), gate)
                .unwrap();
        (taking, chosen)
    }

    const REQUEST: &str = "/org/freedesktop/portal/desktop/request/1_42/t";

    /// Another program's dialog, `method` with `options`, as the portal
    /// asks the back end, on a connection of its own: its answer.
    fn ask_dialog(
        address: &str,
        method: &str,
        options: &crate::chooser::linux::Options,
    ) -> (u32, crate::chooser::linux::Results) {
        let bus = connector(address)().unwrap();
        let handle = zbus::zvariant::ObjectPath::try_from(REQUEST).unwrap();
        bus.call_method(
            Some(crate::chooser::BACK_END),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.impl.portal.FileChooser"),
            method,
            &(
                handle,
                "org.example.Editor",
                "x11:1a2b",
                "Open a map",
                options,
            ),
        )
        .unwrap()
        .body()
        .deserialize()
        .unwrap()
    }

    /// The first dialog handed in, once it is.
    fn first(chosen: &Chosen) -> crate::chooser::Choosing {
        until("the dialog handed in", &|| !chosen.lock().is_empty());
        chosen.lock()[0].clone()
    }

    /// While taking launches over, another program's dialog is handed in
    /// as data and answered as the user decides: the paths chosen as
    /// URIs; or cancelled, nothing chosen.
    #[test]
    fn another_programs_dialog_is_answered() {
        use crate::chooser::{Answer, Kind};
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let (taking, chosen) = choosing_skwir(&bus.address, &folder.path().join("marker"));
        let address = bus.address.clone();
        let asker = std::thread::spawn(move || {
            let options = crate::chooser::linux::Options {
                multiple: Some(true),
                current_folder: Some(b"/srv/Maps\0".to_vec()),
                ..Default::default()
            };
            ask_dialog(&address, "OpenFile", &options)
        });
        let choosing = first(&chosen);
        assert_eq!(
            choosing.asked().kind,
            Kind::Open {
                multiple: true,
                directory: false
            }
        );
        assert_eq!(choosing.asked().app_id, "org.example.Editor");
        assert_eq!(
            choosing.asked().folder.as_deref(),
            Some(Path::new("/srv/Maps"))
        );
        choosing.answer(Answer::Chosen {
            paths: vec![
                PathBuf::from("/srv/Maps/tröisième.png"),
                PathBuf::from("/srv/Maps/it's north.png"),
            ],
            choices: Vec::new(),
            filter: None,
        });
        let (response, results) = asker.join().unwrap();
        assert_eq!(response, 0);
        assert_eq!(
            results.uris.unwrap(),
            [
                "file:///srv/Maps/tr%C3%B6isi%C3%A8me.png",
                "file:///srv/Maps/it's%20north.png"
            ]
        );
        // Given to the program to write (the portal registers a sandboxed
        // program's documents by it).
        assert_eq!(results.writable, Some(true));

        chosen.lock().clear();
        let address = bus.address.clone();
        let asker = std::thread::spawn(move || {
            ask_dialog(
                &address,
                "SaveFile",
                &crate::chooser::linux::Options::default(),
            )
        });
        first(&chosen).answer(Answer::Cancelled);
        let (response, results) = asker.join().unwrap();
        assert_eq!(response, 1);
        assert!(results.uris.is_none() && results.writable.is_none());
        drop(taking);
    }

    /// The asking program gives up (the portal's `Request.Close`): the
    /// dialog is told so, and its request answered cancelled.
    #[test]
    fn a_dialog_its_program_closes_is_told_and_cancelled() {
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let (taking, chosen) = choosing_skwir(&bus.address, &folder.path().join("marker"));
        let address = bus.address.clone();
        let asker = std::thread::spawn(move || {
            ask_dialog(
                &address,
                "OpenFile",
                &crate::chooser::linux::Options::default(),
            )
        });
        let choosing = first(&chosen);
        let told = Arc::new(AtomicBool::new(false));
        let hook = Arc::clone(&told);
        choosing.when_closed(move || hook.store(true, Ordering::SeqCst));
        let closer = connector(&bus.address)().unwrap();
        closer
            .call_method(
                Some(crate::chooser::BACK_END),
                REQUEST,
                Some("org.freedesktop.impl.portal.Request"),
                "Close",
                &(),
            )
            .unwrap();
        let (response, _) = asker.join().unwrap();
        assert_eq!(response, 1);
        assert!(choosing.is_closed() && told.load(Ordering::SeqCst));
        drop(taking);
    }

    /// A Skwir the bus started for a dialog, finding another of its
    /// instance taking launches over, asks that one: the dialog answered
    /// there, its answer the asking program's; the name then goes to the
    /// Skwir that runs, which waited in the queue.
    #[test]
    fn a_dialog_started_elsewhere_is_answered_by_the_skwir_that_runs() {
        use crate::chooser::Answer;
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        // The started one holds the name first, as the bus started it.
        let started = connector(&bus.address)().unwrap();
        let held_by = started.unique_name().unwrap().to_string();
        let forwarding = std::thread::spawn(move || {
            crate::chooser::linux::answer_for_holder_on(&started, Duration::from_secs(10), HELD)
        });
        let owner = |address: &str| {
            let bus = connector(address)().unwrap();
            zbus::blocking::fdo::DBusProxy::new(&bus)
                .unwrap()
                .get_name_owner(crate::chooser::BACK_END.try_into().unwrap())
                .ok()
                .map(|owner| owner.to_string())
        };
        until("the started one holds the name", &|| {
            owner(&bus.address).as_deref() == Some(held_by.as_str())
        });
        let (taking, chosen) = choosing_skwir(&bus.address, &folder.path().join("marker"));
        let address = bus.address.clone();
        let asker = std::thread::spawn(move || {
            let options = crate::chooser::linux::Options {
                current_name: Some("north.png".into()),
                ..Default::default()
            };
            ask_dialog(&address, "SaveFile", &options)
        });
        let choosing = first(&chosen);
        assert_eq!(
            choosing.asked().kind,
            crate::chooser::Kind::Save {
                name: Some("north.png".into()),
                file: None
            }
        );
        choosing.answer(Answer::Chosen {
            paths: vec![PathBuf::from("/srv/Maps/north.png")],
            choices: Vec::new(),
            filter: None,
        });
        let (response, results) = asker.join().unwrap();
        assert_eq!(response, 0);
        assert_eq!(results.uris.unwrap(), ["file:///srv/Maps/north.png"]);
        assert!(forwarding.join().unwrap(), "a request came, and was handed");
        until("the Skwir that runs holds the name", &|| {
            owner(&bus.address).is_some_and(|owner| owner != held_by)
        });
        drop(taking);
    }

    /// A Skwir that served the file manager's name, lost it to another
    /// and was handed it back as the bus queued it, still lets it go
    /// once asked, though zbus forgot it when it was lost.
    #[test]
    fn a_name_handed_back_is_let_go() {
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let owner = || {
            let asker = connector(&bus.address)().unwrap();
            zbus::blocking::fdo::DBusProxy::new(&asker)
                .unwrap()
                .get_name_owner(super::linux::FILE_MANAGER.try_into().unwrap())
                .ok()
                .map(|owner| owner.to_string())
        };
        let first = connector(&bus.address)().unwrap();
        let held_by = first.unique_name().unwrap().to_string();
        assert!(super::linux::serve_file_manager(
            &first,
            &super::Gate::new(|_| {})
        ));
        until("served", &|| owner().as_deref() == Some(held_by.as_str()));
        // Another takes it (zbus's request_name replaces), then ends.
        let second = connector(&bus.address)().unwrap();
        second.request_name(super::linux::FILE_MANAGER).unwrap();
        until("taken", &|| owner().is_some_and(|o| o != held_by));
        second.close().unwrap();
        until("handed back", &|| {
            owner().as_deref() == Some(held_by.as_str())
        });
        super::linux::let_go_of_file_manager(&first);
        until("let go", &|| owner().is_none());
    }

    /// Asked nothing within its wait, it hands nothing and lets go.
    #[test]
    fn a_program_asked_nothing_hands_nothing() {
        let folder = folder();
        let listen = format!("unix:dir={}", folder.path().display());
        let bus = bus_or_skip!(folder.path(), &listen);
        let started = connector(&bus.address)().unwrap();
        let handed =
            super::linux::answer_for_holder_on(&started, Duration::from_millis(100), |_| true);
        assert!(!handed);
        assert_eq!(file_manager_owner(&bus.address), None);
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use std::os::windows::ffi::OsStringExt;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    /// A pipe no other test (nor a Skwir finishing) is given.
    fn test_pipe() -> OsString {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        format!(
            r"\\.\pipe\org.skwir.skwir.finishing-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )
        .into()
    }

    type Told = Arc<parking_lot::Mutex<Vec<Opening>>>;

    fn taker() -> (Told, impl Fn(Opening) + Send + Sync + 'static) {
        let told = Told::default();
        let into = Arc::clone(&told);
        (told, move |opening| into.lock().push(opening))
    }

    fn opening() -> Opening {
        Opening {
            // A lone surrogate, as a Windows name may hold, beside an
            // apostrophe and a space.
            location: Some(OsString::from_wide(
                &"C:\\Dossier d'été\\"
                    .encode_utf16()
                    .chain([0xD800, u16::from(b'x')])
                    .collect::<Vec<_>>(),
            )),
            cwd: PathBuf::from(r"C:\Users"),
            token: None,
            select: vec![OsString::from("it's a draft.txt")],
            properties: true,
        }
    }

    #[test]
    fn the_pipe_is_the_users_in_this_session() {
        let instance = Instance::of(&["test".as_ref()]);
        let name = pipe::name(&instance).unwrap().into_string().unwrap();
        let sid = crate::windows::user_sid().unwrap();
        assert!(name.starts_with(&format!(
            r"\\.\pipe\org.skwir.skwir.running-{}-",
            instance.key()
        )));
        assert!(name.ends_with(&format!("-{sid}")), "{name}");
    }

    /// What a launch asks arrives whole (a lone surrogate in a path, no
    /// token, then a token and no location), taken before it is
    /// answered.
    #[test]
    fn a_launch_is_handed_over_exactly() {
        let pipe = test_pipe();
        let (told, taken) = taker();
        let taking = pipe::take_over_at(pipe.clone(), taken).unwrap();
        let first = opening();
        assert!(pipe::hand_over_to(&pipe, &first));
        assert_eq!(
            *told.lock(),
            std::slice::from_ref(&first),
            "taken before answered"
        );
        let bare = Opening {
            location: None,
            cwd: PathBuf::from(r"D:\"),
            token: Some("token".into()),
            ..Opening::default()
        };
        assert!(pipe::hand_over_to(&pipe, &bare));
        assert_eq!(*told.lock(), [first, bare]);
        drop(taking);
    }

    fn until(what: &str, done: &dyn Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "{what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// A Skwir taking launches and viewings over on `pipe`, the viewings
    /// handed in kept in the returned list.
    fn viewing_skwir(pipe: &OsString) -> (TakingOver, Arc<parking_lot::Mutex<Vec<Viewed>>>) {
        let viewed = Arc::new(parking_lot::Mutex::new(Vec::<Viewed>::new()));
        let into = Arc::clone(&viewed);
        let gate = Gate::viewing(|_| {}, move |handed| into.lock().push(handed));
        (pipe::take_over_on(pipe.clone(), gate).unwrap(), viewed)
    }

    /// What a launch of this process keeps of standard input, as `skw -`
    /// does: `stdin.<extension>` in its spool.
    fn spool(extension: &str) -> PathBuf {
        let pid = std::process::id();
        skwir_core::folders::private(&super::spool_folder(pid)).unwrap();
        let file = super::spool_file(pid, Some(extension));
        std::fs::write(&file, b"piped").unwrap();
        file
    }

    /// `pipe` opened as a launch opens it, once free.
    fn open(pipe: &OsString) -> std::fs::File {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(pipe)
            {
                Ok(launch) => return launch,
                Err(error) => {
                    assert!(Instant::now() < deadline, "the pipe: {error}");
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }

    /// A launch's `VIEW` of `files` said on `launch`: the first byte
    /// answered, or none where the pipe closed unanswered.
    fn ask_view(launch: &mut std::fs::File, files: &[PathBuf], piped: Option<&Path>) -> Option<u8> {
        use std::io::{Read, Write};
        let viewing = super::Viewing {
            files: files.to_vec(),
            piped: piped.map(Path::to_path_buf),
            token: None,
        };
        let message = frame::encode(&frame::Message::View(frame::Viewing::of(&viewing))).unwrap();
        launch.write_all(&message).unwrap();
        let mut answer = [0u8];
        launch.read_exact(&mut answer).ok().map(|()| answer[0])
    }

    /// What a launch views arrives whole (a lone surrogate in a path, the
    /// file piped), and the launch waits until its window closes, launches
    /// taken meanwhile; the program's end then waits for no one. A launch
    /// that confirms and goes is told to its window, and once the window
    /// let go, what it kept of standard input is removed. One waiting while
    /// the Skwir stops taking launches over is told it ended.
    #[test]
    fn a_viewing_waits_for_its_window() {
        use std::io::Write;
        let pipe = test_pipe();
        let (taking, viewed) = viewing_skwir(&pipe);
        let piped = spool("png");
        let asked = Viewing {
            files: vec![
                PathBuf::from(OsString::from_wide(
                    &"C:\\Dossier d'été\\"
                        .encode_utf16()
                        .chain([0xD800])
                        .collect::<Vec<_>>(),
                )),
                piped.clone(),
            ],
            piped: Some(piped.clone()),
            token: None,
        };
        let launch = {
            let (pipe, asked) = (pipe.clone(), asked.clone());
            std::thread::spawn(move || pipe::view_through(&pipe, &asked))
        };
        until("the viewing handed in", &|| !viewed.lock().is_empty());
        let first = viewed.lock()[0].clone();
        assert_eq!(first.viewing(), &asked);
        assert!(pipe::hand_over_to(&pipe, &opening()), "a launch meanwhile");
        std::thread::sleep(Duration::from_millis(200));
        assert!(!launch.is_finished(), "the launch waits for its window");
        first.close();
        assert_eq!(launch.join().unwrap(), ViewedElsewhere::Closed);
        let started = Instant::now();
        super::wait_told();
        assert!(started.elapsed() < PATIENCE, "the launch read it and went");
        assert!(piped.exists(), "its launch, there, removes what it kept");

        // A launch that confirms and goes: its window is told.
        let gone = Arc::new(std::sync::atomic::AtomicBool::new(false));
        {
            let mut launch = open(&pipe);
            assert_eq!(
                ask_view(&mut launch, std::slice::from_ref(&piped), Some(&piped)),
                Some(frame::TAKEN)
            );
            launch.write_all(&[frame::CONFIRMED]).unwrap();
            until("the second viewing handed in", &|| viewed.lock().len() == 2);
            let second = viewed.lock()[1].clone();
            let told = Arc::clone(&gone);
            second.when_gone(move || told.store(true, Ordering::SeqCst));
        }
        until("the launch gone is told", &|| gone.load(Ordering::SeqCst));
        assert!(piped.exists(), "kept while its window shows it");
        viewed.lock().clear();
        until("what it kept removed", &|| !piped.exists());

        // The Skwir stops while a launch waits.
        let launch = {
            let pipe = pipe.clone();
            std::thread::spawn(move || pipe::view_through(&pipe, &Viewing::default()))
        };
        until("the third viewing handed in", &|| viewed.lock().len() == 1);
        drop(taking);
        assert_eq!(launch.join().unwrap(), ViewedElsewhere::Ended);
    }

    /// Nothing is shown until its launch confirms it: a launch that gave up
    /// (here, one that goes without confirming) shows nothing, and one that
    /// confirms past the patience is told it was not taken, and views the
    /// files itself; so too for a window that could not open. What stands
    /// for standard input must be the launch's own: another's is refused.
    #[test]
    fn a_viewing_not_confirmed_is_never_shown() {
        use std::io::{Read, Write};
        let pipe = test_pipe();
        let (taking, viewed) = viewing_skwir(&pipe);
        {
            let mut launch = open(&pipe);
            assert_eq!(ask_view(&mut launch, &[], None), Some(frame::TAKEN));
        }
        let mut late = open(&pipe);
        assert_eq!(ask_view(&mut late, &[], None), Some(frame::TAKEN));
        std::thread::sleep(PATIENCE + Duration::from_millis(500));
        let _ = late.write_all(&[frame::CONFIRMED]);
        let mut answer = [0u8];
        late.read_exact(&mut answer).unwrap();
        assert_eq!(answer[0], frame::NOT_TAKEN);
        drop(late);
        assert!(viewed.lock().is_empty(), "nothing shown");
        let launch = {
            let pipe = pipe.clone();
            std::thread::spawn(move || pipe::view_through(&pipe, &Viewing::default()))
        };
        until("the viewing handed in", &|| viewed.lock().len() == 1);
        viewed.lock()[0].not_shown();
        assert_eq!(launch.join().unwrap(), ViewedElsewhere::NotTaken);
        // Another process's spool: no process has the largest id.
        skwir_core::folders::private(&super::spool_folder(u32::MAX)).unwrap();
        let others = super::spool_file(u32::MAX, None);
        std::fs::write(&others, b"piped").unwrap();
        let mut launch = open(&pipe);
        assert_eq!(
            ask_view(&mut launch, std::slice::from_ref(&others), Some(&others)),
            None
        );
        assert!(others.exists());
        super::let_go_of_spool(&others);
        drop(taking);
    }

    /// Where no Skwir finishes, a launch goes on at once.
    #[test]
    fn without_a_finishing_skwir_a_launch_goes_on_at_once() {
        let started = Instant::now();
        assert!(!pipe::hand_over_to(&test_pipe(), &opening()));
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    /// Dropped, it ends at once, and the pipe goes with it: a launch then
    /// goes on as a process of its own.
    #[test]
    fn dropped_it_ends_at_once_and_the_pipe_goes() {
        let pipe = test_pipe();
        let (told, taken) = taker();
        let taking = pipe::take_over_at(pipe.clone(), taken).unwrap();
        let started = Instant::now();
        drop(taking);
        assert!(started.elapsed() < PATIENCE, "{:?}", started.elapsed());
        assert!(!pipe::hand_over_to(&pipe, &opening()));
        assert!(told.lock().is_empty());
        // Taken over again by the next.
        let (told, taken) = taker();
        let taking = pipe::take_over_at(pipe.clone(), taken).unwrap();
        assert!(pipe::hand_over_to(&pipe, &opening()));
        assert_eq!(told.lock().len(), 1);
        drop(taking);
    }

    /// A launch that connects and says nothing holds neither the drop nor
    /// the thread past its patience.
    #[test]
    fn a_launch_that_says_nothing_holds_nothing() {
        let pipe = test_pipe();
        let (_, taken) = taker();
        let taking = pipe::take_over_at(pipe.clone(), taken).unwrap();
        let silent = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&pipe)
            .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let started = Instant::now();
        drop(taking);
        assert!(started.elapsed() < PATIENCE, "{:?}", started.elapsed());
        drop(silent);
    }

    /// A launch hands itself only to a pipe served by a process of this
    /// user: one whose process cannot be opened to ask (another user's,
    /// here one that is not there) has no user, and this one is the
    /// user's. (Another user's pipe cannot be made in a test.)
    #[test]
    fn only_a_process_of_this_user_is_handed_launches() {
        let user = crate::windows::user_sid();
        assert!(user.is_some());
        assert_eq!(pipe::user_of(std::process::id()), user);
        assert_eq!(pipe::user_of(u32::MAX - 3), None);
        // The pipe a Skwir taking over serves is found to be its own.
        let pipe = test_pipe();
        let (_, taken) = taker();
        let taking = pipe::take_over_at(pipe.clone(), taken).unwrap();
        let connected = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&pipe)
            .unwrap();
        assert_eq!(pipe::our_server(&connected), Some(std::process::id()));
        drop(connected);
        drop(taking);
    }

    /// While one Skwir takes launches over, another cannot.
    #[test]
    fn one_skwir_takes_launches_over_at_a_time() {
        if skwir_core::system::under_wine("FILE_FLAG_FIRST_PIPE_INSTANCE") {
            return;
        }
        let pipe = test_pipe();
        let (told, taken) = taker();
        let first = pipe::take_over_at(pipe.clone(), taken).unwrap();
        let (_, other) = taker();
        assert_eq!(
            pipe::take_over_at(pipe.clone(), other).err(),
            Some(super::NotTaken::Elsewhere)
        );
        assert!(pipe::hand_over_to(&pipe, &opening()));
        assert_eq!(told.lock().len(), 1);
        drop(first);
    }
}

#[cfg(test)]
mod instance_tests {
    use super::Instance;

    /// An instance is its parts, each in its place: the same parts the
    /// same key on every build, other parts or the same in another order
    /// another key; a key a bus name can end with.
    #[test]
    fn an_instance_is_its_build_files_and_display() {
        let of =
            |parts: &[&str]| Instance::of(&parts.iter().map(AsRef::as_ref).collect::<Vec<_>>());
        let here = of(&[
            "0.3.1 abc",
            "/x/settings.toml",
            "/x/state.toml",
            "wayland-1",
            ":0",
        ]);
        assert_eq!(here.key().len(), 16);
        assert!(here.key().chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(
            here,
            of(&[
                "0.3.1 abc",
                "/x/settings.toml",
                "/x/state.toml",
                "wayland-1",
                ":0"
            ])
        );
        let test = of(&["0.3.1 abc", "/t/settings.toml", "/t/state.toml", "", ":95"]);
        assert_ne!(here, test, "other folders, another display");
        assert_ne!(of(&["ab", "c"]), of(&["a", "bc"]), "parts kept apart");
        assert_eq!(of(&[]).key(), "cbf29ce484222325");
    }
}
