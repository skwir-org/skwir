//! A Skwir finishing without its window takes later launches over
//! (ADR-115).
//!
//! Closing Skwir's last window does not stop what it does (a copy, a
//! move): the window goes, and the process stays without one until its
//! actions end, then exits by itself. Skwir runs one process per window,
//! so a later launch would start a second Skwir that knows nothing of the
//! copy the first is finishing. While it finishes, the first takes later
//! launches over ([`take_over`]): a launch hands it what it was asked to
//! open and the desktop's token to bring a window forward ([`hand_over`],
//! before anything else of the program starts), then exits, and the
//! finishing Skwir opens its window again. Otherwise every launch is a
//! process of its own.
//!
//! - Linux: the name `org.skwir.skwir` on the session bus, held while
//!   taking over, its object `/org/skwir/skwir` serving
//!   `org.skwir.skwir.Finishing.Open(ay location, ay cwd, s token)` on
//!   the program's one connection (`crate::bus`), and served again
//!   there when the session's bus is started again. A path travels as its
//!   bytes; an empty location or token is none. Once the name is held, a
//!   marker, `$XDG_RUNTIME_DIR/skwir/finishing`, holds the finishing
//!   Skwir's pid: a launch where none finishes stats it and does nothing
//!   more (no connection, no thread). Where it is there, the launch asks
//!   on a connection of its own, given up on after two seconds, as the
//!   shared one waits for good.
//! - Windows: a named pipe of the user's in this session,
//!   `\\.\pipe\org.skwir.skwir.finishing-<session>-<SID>`, its first
//!   instance made by the Skwir that takes over: only the user opens it,
//!   and no other machine reaches it (`crate::windows::pipe`). A launch
//!   lets that Skwir bring its window forward
//!   (`AllowSetForegroundWindow`), as Windows has no token to hand; its
//!   message is told beside the code that reads it (`frame`).
//! - macOS: `LaunchServices` brings the running application forward instead
//!   of starting another, and the UI follows gpui's `on_reopen`: nothing
//!   is handed over here.
//!
//! A launch is handed over only once the finishing Skwir answered. One
//! that does not answer within two seconds (stopped, or gone and its
//! marker left after a crash) is given up on, and the launch goes on as a
//! process of its own; a marker that is not this Skwir's is never
//! removed.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(any(target_os = "linux", windows))]
use std::time::Duration;

/// What a later launch asks of the Skwir it hands itself to.
#[derive(Clone, Debug, PartialEq, Eq)]
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

/// Hand `opening` to a Skwir finishing without its window, if one is
/// there: whether it took it (this launch then ends). Called before
/// anything else of the program, so where none is it costs almost
/// nothing: a stat on Linux, a pipe not found on Windows; no bus
/// connection, no thread. A finishing Skwir that does not answer within
/// two seconds is given up on (false). Always false on macOS and other
/// systems.
pub fn hand_over(opening: &Opening) -> bool {
    #[cfg(target_os = "linux")]
    {
        linux::marker()
            .is_some_and(|marker| linux::hand_over_at(&marker, linux::patient_session, opening))
    }
    #[cfg(windows)]
    {
        pipe::name().is_some_and(|name| pipe::hand_over_to(&name, opening))
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = opening;
        false
    }
}

/// While the returned value is held, later launches are handed here:
/// `taken` is called with each, on a thread of the module's (the session
/// bus's on Linux, the pipe's on Windows), before the launch is answered;
/// it must return at once, make no call on the session bus, and never
/// drop the [`TakingOver`] (which waits for it). `None` where this cannot
/// be: another Skwir takes them already, no session bus or runtime folder
/// (Linux), on macOS and other systems.
///
/// Waits for the bus's answer to the name asked (Linux): call it on a
/// worker.
#[must_use]
pub fn take_over(taken: impl Fn(Opening) + Send + Sync + 'static) -> Option<TakingOver> {
    #[cfg(target_os = "linux")]
    {
        linux::take_over_with(crate::bus::session, linux::marker()?, taken)
    }
    #[cfg(windows)]
    {
        pipe::take_over_at(pipe::name()?, taken)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = taken;
        None
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
    _held: linux::Held,
    #[cfg(windows)]
    _held: pipe::Held,
}

impl Drop for TakingOver {
    fn drop(&mut self) {
        // Closed first: a launch being taken is done before this goes on,
        // and none is taken after, whatever the bus or the pipe still
        // delivers while what serves it stops (`_held`, dropped next).
        self.gate.close();
    }
}

/// What a launch handed over is given to, until closed.
#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
struct Gate(parking_lot::Mutex<Option<Taker>>);

type Taker = Box<dyn Fn(Opening) + Send + Sync>;

#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
impl Gate {
    fn new(taken: impl Fn(Opening) + Send + Sync + 'static) -> Arc<Gate> {
        Arc::new(Gate(parking_lot::Mutex::new(Some(Box::new(taken)))))
    }

    /// Give `opening` to the taker if it still takes: whether it took it.
    /// Held meanwhile, so [`Gate::close`] waits for it.
    fn pass(&self, opening: Opening) -> bool {
        let held = self.0.lock();
        let Some(taken) = held.as_ref() else {
            return false;
        };
        taken(opening);
        true
    }

    fn close(&self) {
        self.0.lock().take();
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use skwir_core::CancelToken;
    use zbus::blocking::Connection;
    use zbus::fdo::{RequestNameFlags, RequestNameReply};

    use super::{Gate, Opening, PATIENCE, TakingOver};

    pub(super) const NAME: &str = "org.skwir.skwir";
    pub(super) const PATH: &str = "/org/skwir/skwir";
    /// As [`Finishing`] is declared.
    const INTERFACE: &str = "org.skwir.skwir.Finishing";
    /// The most a marker holds: a pid and a line's end.
    const MARKER_BOUND: u64 = 32;

    /// Where a finishing Skwir says so: in the session's runtime folder.
    pub(super) fn marker() -> Option<PathBuf> {
        skwir_core::xdg::runtime_dir().map(|runtime| runtime.join("skwir").join("finishing"))
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
        connect: impl FnOnce() -> zbus::Result<Connection>,
        opening: &Opening,
    ) -> bool {
        // Where no Skwir finishes: a stat, and nothing more.
        if std::fs::symlink_metadata(marker).is_err() {
            return false;
        }
        let ask = || -> zbus::Result<bool> {
            let bus = connect()?;
            // Asked first, so a name no one holds is not started by the
            // bus (were a service file to name it), and a stale marker
            // costs no failed call.
            let dbus = zbus::blocking::fdo::DBusProxy::new(&bus)?;
            if !dbus.name_has_owner(NAME.try_into()?)? {
                return Ok(false);
            }
            let location = opening
                .location
                .as_deref()
                .map_or(&[][..], OsStr::as_encoded_bytes);
            let cwd = opening.cwd.as_os_str().as_encoded_bytes();
            let token = opening.token.as_deref().unwrap_or_default();
            bus.call_method(
                Some(NAME),
                PATH,
                Some(INTERFACE),
                "Open",
                &(location, cwd, token),
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

    /// What a later launch calls.
    struct Finishing(Arc<Gate>);

    #[zbus::interface(name = "org.skwir.skwir.Finishing")]
    impl Finishing {
        /// Open `location` (its bytes; empty for what the window had),
        /// read against `cwd`, the window brought forward with `token`
        /// (empty for none): answered once taken.
        fn open(&self, location: Vec<u8>, cwd: Vec<u8>, token: String) -> zbus::fdo::Result<()> {
            let opening = Opening {
                location: (!location.is_empty()).then(|| skwir_core::os_path::from_bytes(location)),
                cwd: PathBuf::from(skwir_core::os_path::from_bytes(cwd)),
                token: (!token.is_empty()).then_some(token),
            };
            if self.0.pass(opening) {
                Ok(())
            } else {
                Err(zbus::fdo::Error::Failed(
                    "Skwir no longer takes launches over".into(),
                ))
            }
        }
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
    pub(super) fn take_over_with<E: std::fmt::Display>(
        connect: impl FnMut() -> Result<Connection, E> + Send + 'static,
        marker: PathBuf,
        taken: impl Fn(Opening) + Send + Sync + 'static,
    ) -> Option<TakingOver> {
        let gate = Gate::new(taken);
        let stop = CancelToken::new();
        let (first, answered) = std::sync::mpsc::sync_channel(1);
        let thread = {
            let (gate, stop) = (Arc::clone(&gate), stop.clone());
            std::thread::Builder::new()
                .name("skwir-finishing".into())
                .spawn(move || {
                    let mut first = Some(first);
                    let mut served: Option<Served> = None;
                    crate::bus::keep(connect, &stop, |bus| {
                        // What the bus that ended had is let go of first
                        // (its marker, ours, written again once served).
                        drop(served.take());
                        served = Served::on(bus, &gate, &marker);
                        if let Some(first) = first.take() {
                            let _ = first.send(served.is_some());
                        }
                    });
                    drop(served);
                })
        };
        let thread = thread
            .inspect_err(|error| tracing::debug!("not taking launches over: {error}"))
            .ok()?;
        let taking = TakingOver {
            gate,
            _held: Held {
                stop,
                thread: Some(thread),
            },
        };
        // Not served (dropped: stopped and waited for), or no bus (the
        // thread ended without a word).
        answered.recv().unwrap_or(false).then_some(taking)
    }

    /// The object and the name served on one connection, which the
    /// program shares (`bus::session`), and the marker written: all taken
    /// back when dropped.
    struct Served {
        bus: Connection,
        marker: PathBuf,
    }

    impl Served {
        /// Served on `bus`, or `None` where it cannot be: another Skwir
        /// holds the name, this connection serves it already (a second
        /// [`super::take_over`] of this Skwir), no marker can be written.
        fn on(bus: &Connection, gate: &Arc<Gate>, marker: &Path) -> Option<Served> {
            let objects = bus.object_server();
            match objects.at(PATH, Finishing(Arc::clone(gate))) {
                Ok(true) => {}
                Ok(false) => {
                    tracing::debug!("this Skwir takes later launches over already");
                    return None;
                }
                Err(error) => {
                    tracing::debug!("not taking launches over: {error}");
                    return None;
                }
            }
            let named = bus.request_name_with_flags(NAME, RequestNameFlags::DoNotQueue.into());
            if !matches!(named, Ok(RequestNameReply::PrimaryOwner)) {
                tracing::debug!(?named, "another Skwir takes later launches over");
                let _ = objects.remove::<Finishing, _>(PATH);
                return None;
            }
            let served = Served {
                bus: bus.clone(),
                marker: marker.to_owned(),
            };
            if let Err(error) = write_marker(marker) {
                // Dropped: neither the name nor the object kept.
                tracing::debug!("not taking launches over: {}: {error}", marker.display());
                return None;
            }
            Some(served)
        }
    }

    impl Drop for Served {
        fn drop(&mut self) {
            // The name first: a launch that finds the marker meanwhile
            // finds no one holds it.
            let _ = self.bus.release_name(NAME);
            let _ = self.bus.object_server().remove::<Finishing, _>(PATH);
            let_go_of_marker(&self.marker);
        }
    }

    /// Our pid in `marker`, which takes the place of what was there whole.
    fn write_marker(marker: &Path) -> std::io::Result<()> {
        let folder = marker
            .parent()
            .ok_or_else(|| std::io::Error::other("a marker in no folder"))?;
        std::fs::create_dir_all(folder)?;
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

    use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;
    use windows_sys::Win32::Storage::FileSystem::SECURITY_IDENTIFICATION;
    use windows_sys::Win32::System::Pipes::{GetNamedPipeServerProcessId, PeekNamedPipe};
    use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

    use super::{Gate, Opening, PATIENCE, TakingOver, frame};

    /// How often a wait on the pipe looks again: a wait lasts at most
    /// [`PATIENCE`], and only while a launch is handed over.
    const LOOK: Duration = Duration::from_millis(5);

    /// The pipe of this user in this session: another user's, or this
    /// user's in another session (a second sign-in, a remote desktop),
    /// is not this one.
    pub(super) fn name() -> Option<OsString> {
        let sid = crate::windows::user_sid()?;
        let mut session = 0u32;
        // SAFETY: this process's id, and an out value.
        if unsafe { ProcessIdToSessionId(std::process::id(), &raw mut session) } == 0 {
            return None;
        }
        Some(format!(r"\\.\pipe\org.skwir.skwir.finishing-{session}-{sid}").into())
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
        let Some(message) = frame::encode(&frame::Fields::of(opening)) else {
            return false;
        };
        let deadline = Instant::now() + PATIENCE;
        let Some(mut connected) = connect(pipe, deadline) else {
            return false;
        };
        let mut server = 0u32;
        // SAFETY: the pipe just opened, and an out value.
        if unsafe { GetNamedPipeServerProcessId(connected.as_raw_handle(), &raw mut server) } != 0 {
            // SAFETY: a process id, nothing else. Allowed or not, the
            // window still opens.
            unsafe { AllowSetForegroundWindow(server) };
        }
        // Within the pipe's buffer: written at once, read or not.
        if connected.write_all(&message).is_err() {
            return false;
        }
        let mut answer = [0u8];
        matches!(wait(&connected, 1, deadline, None), Waited::Ready)
            && connected.read_exact(&mut answer).is_ok()
            && answer[0] == frame::TAKEN
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
    pub(super) fn take_over_at(
        pipe: OsString,
        taken: impl Fn(Opening) + Send + Sync + 'static,
    ) -> Option<TakingOver> {
        // Its first instance: refused where any is there, made by
        // another Skwir (or anyone).
        let first = crate::windows::pipe::instance(&pipe, true)
            .inspect_err(|error| tracing::debug!("not taking launches over: {error}"))
            .ok()?;
        let gate = Gate::new(taken);
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (pipe, gate, stop) = (pipe.clone(), Arc::clone(&gate), Arc::clone(&stop));
            std::thread::Builder::new()
                .name("skwir-finishing".into())
                .spawn(move || serve(&pipe, first, &gate, &stop))
                .inspect_err(|error| tracing::debug!("not taking launches over: {error}"))
                .ok()?
        };
        Some(TakingOver {
            gate,
            _held: Held {
                pipe,
                stop,
                thread: Some(thread),
            },
        })
    }

    /// Answer each launch that connects, one at a time, until `stop`.
    fn serve(pipe: &OsStr, mut waiting: OwnedHandle, gate: &Gate, stop: &AtomicBool) {
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
            answer(&connected, gate, stop);
            drop(connected);
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
    /// answer it taken; given up on after [`PATIENCE`], or at `stop`.
    fn answer(mut connected: &File, gate: &Gate, stop: &AtomicBool) {
        let deadline = Instant::now() + PATIENCE;
        let mut length = [0u8; 4];
        if !matches!(wait(connected, 4, deadline, Some(stop)), Waited::Ready)
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
        if !matches!(wait(connected, length, deadline, Some(stop)), Waited::Ready)
            || connected.read_exact(&mut body).is_err()
        {
            return;
        }
        let Some(fields) = frame::decode(&body) else {
            return;
        };
        if !gate.pass(fields.into_opening()) || connected.write_all(&[frame::TAKEN]).is_err() {
            return;
        }
        // The answer read before the connection goes, which would take
        // it along: until the launch lets go (never more to read).
        let _ = wait(connected, usize::MAX, deadline, Some(stop));
    }
}

/// A launch's message on the pipe (Windows), and its answer. All numbers
/// little-endian:
///
/// - `[u32 length][body]`, the body at most `LARGEST` bytes;
/// - the body `[u8 VERSION][location][cwd][token]`, nothing after;
/// - each field `[u32 length in bytes][bytes]`, `u32::MAX` alone for none
///   (the folder is never none): the location's and the folder's UTF-16
///   code units (so a lone surrogate, which a Windows name may hold, goes
///   across whole), the token's UTF-8.
///
/// The answer is one byte, `TAKEN`, once the launch is taken; anything
/// else (the pipe closed first) is a launch not taken. A message of
/// another version is not read, and the launch goes on as a process of
/// its own: a later Skwir launched while an earlier one finishes.
#[cfg(any(windows, test))]
#[cfg_attr(not(windows), allow(dead_code))]
mod frame {
    /// What the body starts with.
    pub(super) const VERSION: u8 = 1;
    /// The answer: taken.
    pub(super) const TAKEN: u8 = 1;
    /// The most a body holds: within a pipe's buffer, so it is written at
    /// once, and above any path Windows opens.
    pub(super) const LARGEST: usize = 60 * 1024;
    /// A field's length saying it is none.
    const NONE: u32 = u32::MAX;

    /// What a message carries.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(super) struct Fields {
        pub(super) location: Option<Vec<u16>>,
        pub(super) cwd: Vec<u16>,
        pub(super) token: Option<String>,
    }

    #[cfg(windows)]
    impl Fields {
        pub(super) fn of(opening: &super::Opening) -> Fields {
            use std::os::windows::ffi::OsStrExt;
            Fields {
                location: opening.location.as_ref().map(|l| l.encode_wide().collect()),
                cwd: opening.cwd.as_os_str().encode_wide().collect(),
                token: opening.token.clone(),
            }
        }

        pub(super) fn into_opening(self) -> super::Opening {
            use std::os::windows::ffi::OsStringExt;
            super::Opening {
                location: self.location.map(|l| std::ffi::OsString::from_wide(&l)),
                cwd: std::ffi::OsString::from_wide(&self.cwd).into(),
                token: self.token,
            }
        }
    }

    /// The message saying `fields`, its length first; `None` when its
    /// body would be larger than [`LARGEST`].
    pub(super) fn encode(fields: &Fields) -> Option<Vec<u8>> {
        let mut body = vec![VERSION];
        units(&mut body, fields.location.as_deref());
        units(&mut body, Some(&fields.cwd));
        field(&mut body, fields.token.as_deref().map(str::as_bytes));
        if body.len() > LARGEST {
            return None;
        }
        let mut message = u32::try_from(body.len()).ok()?.to_le_bytes().to_vec();
        message.extend(body);
        Some(message)
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
    /// is not one of [`VERSION`], cut short, or more.
    pub(super) fn decode(body: &[u8]) -> Option<Fields> {
        let (&version, mut rest) = body.split_first()?;
        if version != VERSION {
            return None;
        }
        let location = match take(&mut rest)? {
            Field::Bytes(bytes) => Some(units_of(bytes)?),
            Field::None => None,
        };
        let Field::Bytes(cwd) = take(&mut rest)? else {
            return None;
        };
        let cwd = units_of(cwd)?;
        let token = match take(&mut rest)? {
            Field::Bytes(bytes) => Some(String::from_utf8(bytes.to_vec()).ok()?),
            Field::None => None,
        };
        rest.is_empty().then_some(Fields {
            location,
            cwd,
            token,
        })
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
    use super::frame::{Fields, LARGEST, TAKEN, VERSION, decode, encode};

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
            },
        ] {
            let message = encode(&fields).unwrap();
            assert_eq!(decode(body(&message)), Some(fields));
        }
        assert_ne!(TAKEN, 0, "an answer is told apart from nothing");
    }

    #[test]
    fn what_is_not_a_message_is_refused() {
        let message = encode(&every()).unwrap();
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
        let made = |fields: [Option<&[u8]>; 3]| {
            let mut body = vec![VERSION];
            for f in fields {
                body.extend(field(f));
            }
            body
        };
        assert!(decode(&made([None, Some(b"a\0"), None])).is_some());
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
        };
        // The version, three lengths, the folder.
        let around = (LARGEST - 1 - 12) / 2;
        assert!(encode(&at(around)).is_some());
        assert_eq!(encode(&at(around + 1)), None);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::linux::{NAME, hand_over_at, take_over_with};
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
            .name_has_owner(NAME.try_into().unwrap())
            .unwrap()
    }

    fn opening() -> Opening {
        Opening {
            // Not UTF-8, with an apostrophe and a space.
            location: Some(OsString::from_vec(b"/srv/Dossier d'\xe9t\xe9".to_vec())),
            cwd: PathBuf::from(OsString::from_vec(b"/srv/caf\xe9".to_vec())),
            token: Some("activation-1".into()),
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

    /// Where no Skwir finishes, a launch makes no connection.
    #[test]
    fn without_a_marker_a_launch_connects_to_nothing() {
        let folder = folder();
        let connected = AtomicBool::new(false);
        let handed = hand_over_at(
            &folder.path().join("skwir").join("finishing"),
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
        let taking = take_over_with(connector(&bus.address), marker.clone(), taken).unwrap();
        assert!(held(&bus.address));
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap().trim(),
            std::process::id().to_string()
        );
        let first = opening();
        assert!(hand_over_at(&marker, patient(&bus.address), &first));
        assert_eq!(
            *told.lock(),
            std::slice::from_ref(&first),
            "taken before answered"
        );
        let bare = Opening {
            location: None,
            cwd: PathBuf::from("/"),
            token: None,
        };
        assert!(hand_over_at(&marker, patient(&bus.address), &bare));
        assert_eq!(*told.lock(), [first, bare]);
        drop(taking);
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
        assert!(!hand_over_at(&marker, patient(&bus.address), &opening()));
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
        let taking = take_over_with(connector(&bus.address), marker.clone(), taken).unwrap();
        assert!(held(&bus.address));
        drop(taking);
        assert!(!held(&bus.address), "the name let go of");
        assert!(!marker.exists(), "the marker removed");
        assert!(!hand_over_at(&marker, patient(&bus.address), &opening()));
        assert!(told.lock().is_empty());

        let (_, taken) = taker();
        let taking = take_over_with(connector(&bus.address), marker.clone(), taken).unwrap();
        std::fs::write(&marker, "4194304\n").unwrap();
        drop(taking);
        assert!(!held(&bus.address));
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "4194304\n");
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
                taken,
            )
            .unwrap()
        };
        let (_, other) = taker();
        assert!(take_over_with(connector(&bus.address), marker.clone(), other).is_none());
        let (_, again) = taker();
        let same = {
            let shared = shared.clone();
            move || Ok::<_, zbus::Error>(shared.clone())
        };
        assert!(take_over_with(same, marker.clone(), again).is_none());
        assert!(hand_over_at(&marker, patient(&bus.address), &opening()));
        assert_eq!(told.lock().len(), 1, "the first still takes them");
        drop(first);

        let (told, taken) = taker();
        let second = take_over_with(connector(&bus.address), marker.clone(), taken).unwrap();
        assert!(hand_over_at(&marker, patient(&bus.address), &opening()));
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
        let taking = take_over_with(connector(&address), marker.clone(), taken).unwrap();
        assert!(hand_over_at(&marker, patient(&address), &opening()));

        drop(first);
        let _second = daemon(folder.path(), &address).unwrap();
        until("taken over on the new bus", &|| {
            hand_over_at(&marker, patient(&address), &opening())
        });
        assert_eq!(told.lock().len(), 2);
        drop(taking);
        assert!(!held(&address));
        assert!(!marker.exists());
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use std::os::windows::ffi::OsStringExt;
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
        }
    }

    #[test]
    fn the_pipe_is_the_users_in_this_session() {
        let name = pipe::name().unwrap().into_string().unwrap();
        let sid = crate::windows::user_sid().unwrap();
        assert!(name.starts_with(r"\\.\pipe\org.skwir.skwir.finishing-"));
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
        };
        assert!(pipe::hand_over_to(&pipe, &bare));
        assert_eq!(*told.lock(), [first, bare]);
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
        assert!(pipe::take_over_at(pipe.clone(), other).is_none());
        assert!(pipe::hand_over_to(&pipe, &opening()));
        assert_eq!(told.lock().len(), 1);
        drop(first);
    }
}
