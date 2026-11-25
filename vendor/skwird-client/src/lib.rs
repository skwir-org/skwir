//! skwird's client (`docs/DESIGN.md`, decision 1): find an engine, or
//! start one, and ask it.
//!
//! An engine is looked for in this order: the system service's socket,
//! the user's, and where neither answers, the engine started, if it is
//! installed. A client must work without skwird: every failure here is an
//! error to do without, never a reason to stop. An engine that speaks no
//! version the client speaks is done without too.
//!
//! On Windows, an engine is reached by a named pipe ([`pipe`]): the system
//! service's, taken only if its owner is the system's, then the user's
//! own, its server checked to run as the user.
//!
//! Every call can be given up on: after a silence of the engine's
//! ([`Find::silence`]), or from another thread ([`Client::stopper`]). So
//! can a connect, its hello and a started engine's wait included, with a
//! stopper made before it ([`Client::connect_stopped`]).
//!
//! [`fake`] is an engine of a test's making, for clients' tests.
//!
//! Like `skwird-protocol`, this crate needs `serde` and `serde_json`, and
//! on Windows `windows-sys` (its pipes), so that a client may keep a copy
//! of both in its own tree.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

pub use skwird_protocol as protocol;
#[cfg(any(unix, windows))]
use skwird_protocol::Request;
use skwird_protocol::{Answer, Ask, Changes, Coverage, Done, Entry, Query, Welcome, WirePath};

pub mod fake;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
pub mod pipe;

/// Why a client got no answer.
#[derive(Debug)]
pub enum Error {
    /// No engine answered, and none could be started.
    NoEngine(String),
    /// The engine speaks no version this client speaks.
    Version,
    /// The engine answered with an error.
    Engine(protocol::Failure),
    /// The engine said something this client did not ask for.
    Unexpected,
    /// The call was stopped from another thread ([`Stopper::stop`]): the
    /// connection is closed.
    Stopped,
    /// The engine said nothing for this long ([`Find::silence`]): the
    /// connection is closed.
    Silent(Duration),
    Io(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoEngine(why) => write!(f, "no skwird engine: {why}"),
            Error::Version => write!(f, "the skwird engine speaks another protocol"),
            Error::Engine(failure) => write!(f, "skwird: {} ({})", failure.message, failure.what),
            Error::Unexpected => write!(f, "skwird answered what was not asked"),
            Error::Stopped => write!(f, "skwird: the call was stopped"),
            Error::Silent(d) => write!(f, "skwird said nothing for {d:?}: given up on"),
            Error::Io(e) => write!(f, "skwird: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// How to find an engine.
#[derive(Clone, Debug)]
pub struct Find {
    /// The client's name: the engine keeps its registrations by it.
    pub client: String,
    /// The program to start where no engine answers: none, to start none.
    pub start: Option<PathBuf>,
    /// A socket to ask, in place of the system's and the user's: on
    /// Windows, a pipe's name, which an engine started is given to serve
    /// on (`SKWIRD_PIPE`). On Unix, an engine is started for it only where
    /// it is the one an engine serves on in the runtime folder (`runtime`),
    /// its `skwird/socket`.
    pub socket: Option<PathBuf>,
    /// How long a started engine is waited for.
    pub wait: Duration,
    /// The longest the engine may say nothing while a call waits on it,
    /// its hello included: past it, the call gives up ([`Error::Silent`])
    /// and the connection is closed. None waits as long as the engine
    /// takes: a first build may take a minute on a loaded machine. A
    /// call can also be stopped from another thread ([`Client::stopper`]).
    pub silence: Option<Duration>,
    /// The user's runtime folder, in place of `XDG_RUNTIME_DIR`: where the
    /// user's engine's socket is looked for, and what an engine started is
    /// given, so that it serves where it is looked for. Unix's alone.
    pub runtime: Option<PathBuf>,
    /// The user's state folder, in place of `XDG_STATE_HOME`: where an
    /// engine started logs, and keeps what clients registered. On
    /// Windows, where an engine started logs alone.
    pub state: Option<PathBuf>,
}

impl Find {
    /// Ask the system's engine or the user's, and start `skwird` (found on
    /// the `PATH`) where neither answers.
    pub fn new(client: &str) -> Find {
        Find {
            client: client.to_owned(),
            start: Some(PathBuf::from("skwird")),
            socket: None,
            wait: Duration::from_secs(5),
            silence: None,
            runtime: None,
            state: None,
        }
    }

    /// The user's runtime folder: as given, or [`runtime_folder`].
    #[cfg(unix)]
    fn runtime(&self) -> Option<PathBuf> {
        self.runtime.clone().or_else(runtime_folder)
    }

    /// Where to look for an engine, in turn, where no socket is given.
    #[cfg(unix)]
    fn sockets(&self) -> Vec<PathBuf> {
        std::iter::once(system_socket())
            .chain(self.runtime().map(|r| socket_in(&r)))
            .collect()
    }

    /// Where to look for an engine where no pipe is given, in turn: the
    /// system service's, then the user's. A pipe of the system's name may
    /// be anyone's: it is taken only if its owner is the system's
    /// ([`pipe::Stream::connect`]).
    #[cfg(windows)]
    // The same call as on Unix, which reads the runtime folder given.
    #[allow(clippy::unused_self)]
    fn sockets(&self) -> Vec<PathBuf> {
        std::iter::once(system_socket())
            .chain(pipe::user_pipe())
            .collect()
    }

    /// Where an engine started serves: in the runtime folder it is given.
    /// A socket given elsewhere is one it would never serve on, waited for
    /// in vain while it serves another: none is started for it.
    #[cfg(unix)]
    fn started(&self) -> Result<PathBuf, Error> {
        let socket = self
            .runtime()
            .map(|r| socket_in(&r))
            .ok_or_else(|| Error::NoEngine(NO_RUNTIME.into()))?;
        match &self.socket {
            Some(s) if *s != socket => Err(Error::NoEngine(
                "an engine started serves in its runtime folder, not at the socket given".into(),
            )),
            _ => Ok(socket),
        }
    }

    /// Where an engine started serves.
    #[cfg(windows)]
    fn started(&self) -> Result<PathBuf, Error> {
        self.socket
            .clone()
            .or_else(pipe::user_pipe)
            .ok_or_else(|| Error::NoEngine("this user's SID cannot be read".into()))
    }
}

/// A folder an environment variable names, read as the XDG base folders
/// are: none where it is empty or relative. The engine reads them so
/// (`skwird::dirs`), and a client and its engine must agree.
fn absolute(value: Option<OsString>) -> Option<PathBuf> {
    value
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// The system service's socket (phase 3); on Windows, its pipe (phase 4,
/// step 7); on macOS, which has no `/run`, in `/var/run`, where its
/// system's runtime files are.
pub fn system_socket() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"\\.\pipe\skwird")
    } else if cfg!(target_os = "macos") {
        PathBuf::from("/var/run/skwird/socket")
    } else {
        PathBuf::from("/run/skwird/socket")
    }
}

/// The user's engine's socket, where the user has a runtime folder
/// ([`runtime_folder`]); on Windows, the user's pipe.
pub fn user_socket() -> Option<PathBuf> {
    #[cfg(windows)]
    return pipe::user_pipe();
    #[cfg(unix)]
    return runtime_folder().map(|r| socket_in(&r));
    // No engine serves on this system (rule 7).
    #[cfg(not(any(unix, windows)))]
    None
}

/// What a client says where it finds no runtime folder.
#[cfg(unix)]
const NO_RUNTIME: &str = if cfg!(target_os = "macos") {
    "no runtime folder: XDG_RUNTIME_DIR is not absolute, and the system gives no temporary folder of yours"
} else {
    "XDG_RUNTIME_DIR is not set, or not absolute"
};

/// The user's runtime folder, where their engine's socket is: the one
/// `XDG_RUNTIME_DIR` names (absolute), on every Unix. macOS sets none:
/// there, the user's own temporary folder (`/var/folders/…/T/`, as the
/// system gives it for their user, whatever `TMPDIR` says), theirs alone
/// and on the machine's own disk. The engine reads it so (`skwird::dirs`):
/// a client and its engine must agree, or a user would have two.
#[cfg(unix)]
pub fn runtime_folder() -> Option<PathBuf> {
    let named = absolute(std::env::var_os("XDG_RUNTIME_DIR"));
    #[cfg(target_os = "macos")]
    let named = named.or_else(macos::user_temp);
    named
}

#[cfg(unix)]
fn socket_in(runtime: &Path) -> PathBuf {
    runtime.join("skwird").join("socket")
}

/// What a connection runs over: a Unix socket, or a named pipe on Windows.
#[cfg(unix)]
pub(crate) type Conn = std::os::unix::net::UnixStream;
#[cfg(windows)]
pub(crate) type Conn = pipe::Stream;

/// No connection is made on this system (rule 7): a [`Stopper`] holds
/// none.
#[cfg(not(any(unix, windows)))]
pub(crate) type Conn = std::convert::Infallible;

/// A connection to `at`, each read and write waited for at most `silence`.
/// Linux holds a connect while the engine's queue of connections is full:
/// one that accepts none would hold it past the silence, and past a stop.
/// So the connect does not wait: while the queue is full, it is made again
/// after a pause, until the silence has passed or a stop comes.
#[cfg(target_os = "linux")]
fn connect_to(at: &Path, silence: Option<Duration>, stop: &Stop) -> std::io::Result<Conn> {
    use rustix::net::{AddressFamily, SocketAddrUnix, SocketFlags, SocketType};
    let fd = rustix::net::socket_with(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
        None,
    )?;
    let addr = SocketAddrUnix::new(at)?;
    let deadline = silence.map(|d| std::time::Instant::now() + d);
    let mut pause = Duration::from_millis(1);
    loop {
        match rustix::net::connect(&fd, &addr) {
            Ok(()) => break,
            Err(rustix::io::Errno::AGAIN) => {
                let left = deadline.map(|d| d.saturating_duration_since(std::time::Instant::now()));
                if left.is_some_and(|l| l.is_zero()) {
                    return Err(std::io::ErrorKind::TimedOut.into());
                }
                if stop.pause(left.map_or(pause, |l| l.min(pause))) {
                    return Err(std::io::ErrorKind::ConnectionAborted.into());
                }
                pause = (pause * 2).min(Duration::from_millis(100));
            }
            Err(e) => return Err(e.into()),
        }
    }
    let stream = std::os::unix::net::UnixStream::from(fd);
    stream.set_nonblocking(false)?;
    // The socket's own, so its clone's too.
    stream.set_read_timeout(silence)?;
    stream.set_write_timeout(silence)?;
    Ok(stream)
}

/// A connection to `at`, each read and write waited for at most `silence`.
/// Elsewhere than Linux (macOS), a connect to a full queue is refused at
/// once: it never waits.
#[cfg(all(unix, not(target_os = "linux")))]
fn connect_to(at: &Path, silence: Option<Duration>, _: &Stop) -> std::io::Result<Conn> {
    let stream = std::os::unix::net::UnixStream::connect(at)?;
    // The socket's own, so its clone's too.
    stream.set_read_timeout(silence)?;
    stream.set_write_timeout(silence)?;
    Ok(stream)
}

/// Whether a connect failed for nothing listening there: no socket, or a
/// dead engine's left; on Windows, no pipe of that name. An engine may be
/// started there then, and nowhere else.
#[cfg(any(unix, windows))]
fn nothing_listens(e: &Error) -> bool {
    matches!(e, Error::Io(e) if matches!(
        e.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
    ))
}

/// A connection to the pipe `at`, each read and write waited for at most
/// `silence`; a free instance of it, as long, or 5 s, unless stopped
/// meanwhile: waited for a little at a time, as a stop cannot end the
/// system's wait for one.
#[cfg(windows)]
fn connect_to(at: &Path, silence: Option<Duration>, stop: &Stop) -> std::io::Result<Conn> {
    let busy = i32::try_from(windows_sys::Win32::Foundation::ERROR_PIPE_BUSY).ok();
    let deadline = std::time::Instant::now() + silence.unwrap_or(Duration::from_secs(5));
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match pipe::Stream::connect(at.as_os_str(), Some(left.min(Duration::from_millis(100)))) {
            Ok(mut stream) => {
                stream.set_timeout(silence);
                return Ok(stream);
            }
            Err(e) if e.raw_os_error() == busy && !left.is_zero() && !stop.stopped() => {}
            Err(e) => return Err(e),
        }
    }
}

/// Stop every read and write on the connection, from any thread.
#[cfg(unix)]
fn shut(conn: &Conn) {
    let _ = conn.shutdown(std::net::Shutdown::Both);
}

#[cfg(windows)]
fn shut(conn: &Conn) {
    conn.shutdown();
}

#[cfg(not(any(unix, windows)))]
fn shut(conn: &Conn) {
    match *conn {}
}

/// A connection to an engine.
#[cfg(any(unix, windows))]
pub struct Client {
    reader: std::io::BufReader<Conn>,
    writer: Conn,
    next: u64,
    welcome: Welcome,
    silence: Option<Duration>,
    /// What its [`Stopper`] stops.
    stop: Arc<Stop>,
    /// Set once a read or a write failed: what is read after would be read
    /// from the middle of a message, so the connection is closed.
    closed: Option<Closed>,
}

/// Why a connection was closed.
#[cfg(any(unix, windows))]
#[derive(Clone, Copy, Debug)]
enum Closed {
    /// The engine said nothing for its silence.
    Silent,
    /// A read or a write failed otherwise: it may have stopped in the
    /// middle of a message (a line cut short, or longer than a line may
    /// be).
    Broken,
}

/// A connection to an engine, which this system does not make (rule 7):
/// [`Client::connect`] says so.
#[cfg(not(any(unix, windows)))]
pub struct Client {
    never: std::convert::Infallible,
}

/// What stops a connection's calls from another thread: taken from it
/// with [`Client::stopper`], or made before it ([`Stopper::new`]) and
/// given to [`Client::connect_stopped`], whose connect it stops too. A
/// call waiting on the engine returns at once with [`Error::Stopped`],
/// and so does every call after, and every connect given it after. A
/// search given up on stops so, whatever the engine is doing. One stopper
/// is for one connection: clones of it stop the same.
#[derive(Clone, Default)]
pub struct Stopper {
    stop: Arc<Stop>,
}

/// What a [`Stopper`] stops, shared with the connection it stops.
#[derive(Default)]
struct Stop {
    stopped: AtomicBool,
    /// The connection being made, or made, while its client keeps it: the
    /// one a stop closes.
    conn: Mutex<Option<Conn>>,
    /// Woken by a stop, so that a started engine's wait ends at once.
    woken: Condvar,
}

impl Stop {
    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    fn held(&self) -> MutexGuard<'_, Option<Conn>> {
        // What is held is set or taken whole: a thread that panicked
        // holding it left nothing half done.
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Hold `conn` as the connection a stop closes, refused (`conn`
    /// closed) if one came already. Checked under the lock a stop takes to
    /// close it: either the stop is seen here, or `conn` is held before it
    /// is closed.
    #[cfg(any(unix, windows))]
    fn hold(&self, conn: Conn) -> Result<(), Error> {
        let mut held = self.held();
        if self.stopped() {
            return Err(Error::Stopped);
        }
        *held = Some(conn);
        Ok(())
    }

    /// Wait `pause`, or less if a stop comes: whether one came.
    #[cfg(any(unix, windows))]
    fn pause(&self, pause: Duration) -> bool {
        let held = self.held();
        if self.stopped() {
            return true;
        }
        drop(
            self.woken
                .wait_timeout(held, pause)
                .unwrap_or_else(PoisonError::into_inner),
        );
        self.stopped()
    }
}

impl std::fmt::Debug for Stopper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Stopper")
    }
}

impl Stopper {
    /// A stopper for a connection still to be made: given to
    /// [`Client::connect_stopped`], it stops the connect, then the
    /// connection's calls.
    pub fn new() -> Stopper {
        Stopper::default()
    }

    /// Stop the connection's call, and every call after, and the connect
    /// still making it: the connection is closed. An engine still to be
    /// started is not.
    pub fn stop(&self) {
        self.stop.stopped.store(true, Ordering::Release);
        if let Some(conn) = self.stop.held().as_ref() {
            shut(conn);
        }
        self.stop.woken.notify_all();
    }
}

/// A connection let go of is closed, its stopper kept or not: the clone
/// a stop closes is let go of with it.
#[cfg(any(unix, windows))]
impl Drop for Client {
    fn drop(&mut self) {
        self.stop.held().take();
    }
}

impl Client {
    /// The first engine that answers as `find` says, started if none does
    /// and `find` says so.
    pub fn connect(find: &Find) -> Result<Client, Error> {
        Client::connect_stopped(find, &Stopper::new())
    }

    /// The first engine that answers as `find` says, started if none does
    /// and `find` says so, unless `stopper` stops it first, from another
    /// thread: [`Error::Stopped`], at once, wherever the connect waits (a
    /// hello, a started engine's wait; on Windows, a busy pipe's within
    /// 100 ms). The client made is stopped by `stopper` too
    /// ([`Client::stopper`] gives it again).
    #[cfg(any(unix, windows))]
    pub fn connect_stopped(find: &Find, stopper: &Stopper) -> Result<Client, Error> {
        let stop = &stopper.stop;
        let sockets: Vec<PathBuf> = match &find.socket {
            Some(s) => vec![s.clone()],
            None => find.sockets(),
        };
        // How the first engine that listens and failed failed.
        let mut failed: Option<Error> = None;
        for socket in &sockets {
            match Client::open(socket, &find.client, find.silence, stop) {
                Ok(client) => return Ok(client),
                Err(e @ (Error::Version | Error::Stopped)) => return Err(e),
                Err(e) if nothing_listens(&e) => {}
                Err(e) => {
                    failed.get_or_insert(e);
                }
            }
        }
        let Some(program) = &find.start else {
            return Err(Error::NoEngine("none answers".into()));
        };
        let socket = find.started()?;
        // One engine per scope (rule 4): where an engine listens and failed
        // (silent, gone in the middle of its hello, refusing), the system
        // service's or where one started would, none is started beside it,
        // nor waited for. An engine going away listens no more.
        if let Some(e) = failed {
            return Err(Error::NoEngine(format!("one runs, and failed: {e}")));
        }
        // No engine started for a search given up on.
        if stop.stopped() {
            return Err(Error::Stopped);
        }
        start(program, find)?;
        // Until the engine binds its socket: a start's wait, not a loop
        // that idles.
        let deadline = std::time::Instant::now() + find.wait;
        let mut pause = Duration::from_millis(10);
        loop {
            match Client::open(&socket, &find.client, find.silence, stop) {
                Ok(client) => return Ok(client),
                Err(e @ (Error::Version | Error::Stopped)) => return Err(e),
                Err(_) if std::time::Instant::now() < deadline => {
                    if stop.pause(pause) {
                        return Err(Error::Stopped);
                    }
                    pause = (pause * 2).min(Duration::from_millis(200));
                }
                Err(e) => return Err(Error::NoEngine(format!("started, it did not answer: {e}"))),
            }
        }
    }

    /// No engine runs on this system (rule 7).
    #[cfg(not(any(unix, windows)))]
    pub fn connect_stopped(_: &Find, _: &Stopper) -> Result<Client, Error> {
        Err(Error::NoEngine(
            "skwird runs on Linux, macOS and Windows alone".into(),
        ))
    }

    /// The engine at `socket`, said hello to, waited on as long as it
    /// takes.
    #[cfg(any(unix, windows))]
    pub fn at(socket: &Path, client: &str) -> Result<Client, Error> {
        Client::open(socket, client, None, &Arc::default())
    }

    /// The engine at `socket`, said hello to, each call given up on after
    /// `silence` without a word from it, and all of it, the hello
    /// included, when `stop` stops it.
    #[cfg(any(unix, windows))]
    fn open(
        socket: &Path,
        client: &str,
        silence: Option<Duration>,
        stop: &Arc<Stop>,
    ) -> Result<Client, Error> {
        if stop.stopped() {
            return Err(Error::Stopped);
        }
        // A timeout of zero is refused: the shortest there is, instead.
        let silence = silence.map(|d| d.max(Duration::from_millis(1)));
        let stream = connect_to(socket, silence, stop).map_err(|e| {
            if stop.stopped() {
                Error::Stopped
            } else {
                Error::Io(e)
            }
        })?;
        let writer = stream.try_clone()?;
        stop.hold(writer.try_clone()?)?;
        let mut c = Client {
            reader: std::io::BufReader::new(stream),
            writer,
            next: 1,
            welcome: Welcome {
                version: 0,
                scope: protocol::Scope::User,
                engine: String::new(),
                roots: Vec::new(),
                features: Vec::new(),
            },
            silence,
            stop: Arc::clone(stop),
            closed: None,
        };
        let id = c.ask(Ask::Hello(protocol::Hello {
            versions: vec![protocol::VERSION],
            client: client.to_owned(),
        }))?;
        // An engine refuses a hello of no version it speaks with an error
        // (`what` is `version`), as [`Client::answer`] gives it.
        match c.answer(id) {
            Ok(Answer::Hello(welcome)) if welcome.version == protocol::VERSION => {
                c.welcome = welcome;
                Ok(c)
            }
            Ok(Answer::Hello(_)) => Err(Error::Version),
            Err(Error::Engine(f)) if f.what == "version" => Err(Error::Version),
            Ok(_) => Err(Error::Unexpected),
            Err(e) => Err(e),
        }
    }

    /// What the engine said of itself.
    #[cfg(any(unix, windows))]
    pub fn welcome(&self) -> &Welcome {
        &self.welcome
    }

    /// What the engine said of itself.
    #[cfg(not(any(unix, windows)))]
    pub fn welcome(&self) -> &Welcome {
        match self.never {}
    }

    /// What stops this connection's calls from another thread: the
    /// stopper it was connected with, if it was given one. It fails no
    /// more, the connection's clone made as it is connected.
    #[cfg(any(unix, windows))]
    // Its result as before, for the clients that keep a copy of this.
    #[allow(clippy::unnecessary_wraps)]
    pub fn stopper(&self) -> Result<Stopper, Error> {
        Ok(Stopper {
            stop: Arc::clone(&self.stop),
        })
    }

    /// What stops this connection's calls from another thread.
    #[cfg(not(any(unix, windows)))]
    pub fn stopper(&self) -> Result<Stopper, Error> {
        match self.never {}
    }

    /// Why no call can be made on this connection any more, if none can.
    #[cfg(any(unix, windows))]
    fn closed(&self) -> Option<Error> {
        if self.stop.stopped() {
            return Some(Error::Stopped);
        }
        self.closed.map(|why| match why {
            Closed::Silent => Error::Silent(self.silence.unwrap_or_default()),
            Closed::Broken => Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "the connection was closed: a read or a write on it failed",
            )),
        })
    }

    /// What a read or a write that failed means: a stop, the engine's
    /// silence, or the error itself. Whichever it is, the connection is
    /// closed: a read may have stopped in the middle of a line, which no
    /// read after could find the end of.
    #[cfg(any(unix, windows))]
    fn failed(&mut self, e: std::io::Error) -> Error {
        if self.stop.stopped() {
            return Error::Stopped;
        }
        shut(&self.writer);
        if let (std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut, Some(d)) =
            (e.kind(), self.silence)
        {
            self.closed = Some(Closed::Silent);
            return Error::Silent(d);
        }
        self.closed = Some(Closed::Broken);
        Error::Io(e)
    }

    #[cfg(any(unix, windows))]
    fn ask(&mut self, ask: Ask) -> Result<u64, Error> {
        if let Some(e) = self.closed() {
            return Err(e);
        }
        let id = self.next;
        self.next += 1;
        protocol::write(&mut self.writer, &Request { id, ask }).map_err(|e| self.failed(e))?;
        Ok(id)
    }

    #[cfg(any(unix, windows))]
    fn answer(&mut self, id: u64) -> Result<Answer, Error> {
        loop {
            let response: protocol::Response = match protocol::read(&mut self.reader) {
                Ok(Some(response)) => response,
                Ok(None) => return Err(self.failed(std::io::ErrorKind::UnexpectedEof.into())),
                Err(e) => return Err(self.failed(e)),
            };
            if response.id == id {
                return match response.answer {
                    Answer::Error(f) => Err(Error::Engine(f)),
                    a => Ok(a),
                };
            }
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn ask(&mut self, _: Ask) -> Result<u64, Error> {
        match self.never {}
    }

    #[cfg(not(any(unix, windows)))]
    fn answer(&mut self, _: u64) -> Result<Answer, Error> {
        match self.never {}
    }

    /// How far answers below `root` can be trusted.
    pub fn coverage(&mut self, root: &Path) -> Result<Coverage, Error> {
        let id = self.ask(Ask::Coverage {
            root: WirePath::of(root),
        })?;
        match self.answer(id)? {
            Answer::Coverage(c) => Ok(c),
            _ => Err(Error::Unexpected),
        }
    }

    /// Prove `root` current now, and say how far it can be trusted.
    pub fn sync(&mut self, root: &Path) -> Result<Coverage, Error> {
        let id = self.ask(Ask::Sync {
            root: WirePath::of(root),
        })?;
        match self.answer(id)? {
            Answer::Coverage(c) => Ok(c),
            _ => Err(Error::Unexpected),
        }
    }

    /// Ask `query`, each batch of results handed to `each` as it comes;
    /// `each` giving false cancels the rest.
    pub fn query(
        &mut self,
        query: &Query,
        mut each: impl FnMut(&[Entry]) -> bool,
    ) -> Result<Done, Error> {
        let id = self.ask(Ask::Query(query.clone()))?;
        let mut cancelled = false;
        loop {
            match self.answer(id)? {
                Answer::Results(batch) => {
                    if !cancelled && !each(&batch) {
                        self.ask(Ask::Cancel { id })?;
                        cancelled = true;
                    }
                }
                Answer::Done(done) => return Ok(done),
                _ => return Err(Error::Unexpected),
            }
        }
    }

    /// What the folder at `root` holds (`docs/DESIGN.md`, Protocol,
    /// `totals`): each batch of its children, with their totals, handed to
    /// `each` as it comes, `each` giving false cancelling the rest; then
    /// the folder's own totals and how far they hold. With `largest`, only
    /// so many children come, the rest summed in the answer.
    pub fn totals(
        &mut self,
        root: &Path,
        largest: Option<protocol::Largest>,
        mut each: impl FnMut(&[protocol::Child]) -> bool,
    ) -> Result<protocol::TotalsDone, Error> {
        let id = self.ask(Ask::Totals {
            root: WirePath::of(root),
            largest,
        })?;
        let mut cancelled = false;
        loop {
            match self.answer(id)? {
                Answer::Children(batch) => {
                    if !cancelled && !each(&batch) {
                        self.ask(Ask::Cancel { id })?;
                        cancelled = true;
                    }
                }
                Answer::TotalsDone(done) => return Ok(*done),
                _ => return Err(Error::Unexpected),
            }
        }
    }

    /// What changed below `root` since `since`, or none if the token is
    /// too old: walk again.
    pub fn changes(&mut self, root: &Path, since: &str) -> Result<Option<Changes>, Error> {
        let id = self.ask(Ask::Changes {
            root: WirePath::of(root),
            since: since.to_owned(),
        })?;
        match self.answer(id)? {
            Answer::Changes(c) => Ok(Some(c)),
            Answer::TooOld {} => Ok(None),
            _ => Err(Error::Unexpected),
        }
    }

    /// Keep `add` out of the index, take back `remove`: what this client
    /// keeps out, after.
    pub fn exclude(&mut self, add: &[&Path], remove: &[&Path]) -> Result<Vec<PathBuf>, Error> {
        let id = self.ask(Ask::Exclude {
            add: add.iter().map(|p| WirePath::of(p)).collect(),
            remove: remove.iter().map(|p| WirePath::of(p)).collect(),
        })?;
        match self.answer(id)? {
            Answer::Excluded { folders } => Ok(folders.iter().map(WirePath::to_path).collect()),
            _ => Err(Error::Unexpected),
        }
    }

    /// Take back `remove`, then want `add` live: each folder this client
    /// keeps live after, in the order the engine's share fills in, live or
    /// why not, and the share. A folder past the share is checked, not
    /// live.
    pub fn watch(&mut self, add: &[&Path], remove: &[&Path]) -> Result<protocol::Watching, Error> {
        let id = self.ask(Ask::Watch {
            add: add.iter().map(|p| WirePath::of(p)).collect(),
            remove: remove.iter().map(|p| WirePath::of(p)).collect(),
        })?;
        match self.answer(id)? {
            Answer::Watching(watching) => Ok(watching),
            _ => Err(Error::Unexpected),
        }
    }
}

impl Client {
    /// Forget `forget`, keep `keep`, then release `release`, each a
    /// removable volume by the identity the engine spelled
    /// ([`protocol::SeenVolume::id`]): the volumes this client keeps after,
    /// and those mounted. Released before the client asks for its unmount,
    /// a volume holds nothing the engine opened. An engine whose hello says
    /// no `volumes` ([`protocol::VOLUMES`]) refuses it.
    pub fn volumes(
        &mut self,
        keep: &[&str],
        forget: &[&str],
        release: &[&str],
    ) -> Result<protocol::Volumes, Error> {
        let owned = |ids: &[&str]| ids.iter().map(|&id| id.to_owned()).collect();
        self.ask_volumes(Ask::Volumes {
            keep: owned(keep),
            forget: owned(forget),
            release: owned(release),
            release_at: Vec::new(),
        })
    }

    /// Release every kept volume mounted at or below each of `points`,
    /// before the client asks for their unmount, as an eject of any volume
    /// knows it, by where it is mounted: answered once nothing of them is
    /// held, or after a few seconds. The volumes this client keeps after,
    /// and those mounted.
    pub fn release_at(&mut self, points: &[&Path]) -> Result<protocol::Volumes, Error> {
        self.ask_volumes(Ask::Volumes {
            keep: Vec::new(),
            forget: Vec::new(),
            release: Vec::new(),
            release_at: points.iter().map(|p| WirePath::of(p)).collect(),
        })
    }

    /// `ask`, a [`Ask::Volumes`], asked and its answer awaited.
    fn ask_volumes(&mut self, ask: Ask) -> Result<protocol::Volumes, Error> {
        let id = self.ask(ask)?;
        match self.answer(id)? {
            Answer::Volumes(volumes) => Ok(volumes),
            _ => Err(Error::Unexpected),
        }
    }
}

/// The program to start, a path relative to the client's folder made
/// absolute: from the folder the engine starts in, it would name another
/// program. A bare name is looked for on the `PATH`.
#[cfg(any(unix, windows))]
fn program_path(program: &Path) -> Result<PathBuf, Error> {
    Ok(
        if program.is_relative() && program.components().count() > 1 {
            std::env::current_dir()?.join(program)
        } else {
            program.to_owned()
        },
    )
}

/// The engine's log, in `state`'s `skwird` folder, appended to.
#[cfg(unix)]
fn log_in(state: Option<PathBuf>) -> Option<std::fs::File> {
    let dir = state?.join("skwird");
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("engine.log"))
        .ok()
}

/// Start the engine, apart from this process: in a process group of its
/// own, so that a signal to the client's terminal does not reach it; its
/// output to its log, never the client's; at `/`, so that it holds no
/// folder of the client's busy (a mount, a removable volume); with the
/// folders `find` names. A thread of the client's waits on it, so that
/// it is no zombie of the client's once it ends.
#[cfg(unix)]
fn start(program: &Path, find: &Find) -> Result<(), Error> {
    use std::os::unix::process::CommandExt;
    let log = log_in(
        find.state
            .clone()
            .or_else(|| absolute(std::env::var_os("XDG_STATE_HOME")))
            .or_else(|| absolute(std::env::var_os("HOME")).map(|h| h.join(".local/state"))),
    );
    let mut command = std::process::Command::new(program_path(program)?);
    command
        .arg("serve")
        .current_dir("/")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .process_group(0);
    if let Some(runtime) = &find.runtime {
        command.env("XDG_RUNTIME_DIR", runtime);
    }
    if let Some(state) = &find.state {
        command.env("XDG_STATE_HOME", state);
    }
    match log {
        Some(file) => command.stderr(file),
        None => command.stderr(std::process::Stdio::null()),
    };
    let mut engine = command
        .spawn()
        .map_err(|e| Error::NoEngine(format!("cannot start it: {e}")))?;
    // Should no thread start, the engine is a zombie of the client's once
    // it ends, until the client exits.
    let _ = std::thread::Builder::new()
        .name("skwird reaper".into())
        .spawn(move || {
            let _ = engine.wait();
        });
    Ok(())
}

/// Start the engine, apart from this process: no console window (as
/// skwir's ADR-074 starts every program), in a process group of its own,
/// so that a console's interrupt does not reach it; none of the client's
/// handles ([`pipe::start_detached`]); its output to its log
/// (`SKWIRD_LOG`), never the client's; in the system's folder, so that it
/// holds no folder of the client's busy; serving the pipe `find` names,
/// if it names one. Windows keeps no zombie: the engine's handle is let
/// go of.
#[cfg(windows)]
fn start(program: &Path, find: &Find) -> Result<(), Error> {
    let log = find
        .state
        .clone()
        .or_else(|| absolute(std::env::var_os("LOCALAPPDATA")))
        .map(|d| d.join("skwird"))
        .filter(|d| std::fs::create_dir_all(d).is_ok())
        .map(|d| d.join("engine.log"));
    let folder = absolute(std::env::var_os("SystemRoot")).unwrap_or_else(|| PathBuf::from(r"C:\"));
    let mut set: Vec<(&str, &std::ffi::OsStr)> = Vec::new();
    if let Some(socket) = &find.socket {
        set.push(("SKWIRD_PIPE", socket.as_os_str()));
    }
    if let Some(log) = &log {
        set.push(("SKWIRD_LOG", log.as_os_str()));
    }
    let program = program_on(&program_path(program)?, std::env::var_os("PATH"))
        .ok_or_else(|| Error::NoEngine("cannot start it: not found on the PATH".into()))?;
    pipe::start_detached(&program, &["serve"], &folder, &set)
        .map_err(|e| Error::NoEngine(format!("cannot start it: {e}")))
}

/// The file `program` names, as Windows would start it, without an
/// extension as an `.exe`: a path as it is; a bare name, the first found
/// in a folder of `path` that is absolute. Never in the current folder,
/// where Windows looks first: a program left in the folder a client was
/// started from would be started in place of the engine.
#[cfg(any(windows, test))]
fn program_on(program: &Path, path: Option<OsString>) -> Option<PathBuf> {
    let file = if program.extension().is_none() {
        program.with_extension("exe")
    } else {
        program.to_owned()
    };
    if file.is_absolute() || file.components().count() > 1 {
        return Some(file);
    }
    std::env::split_paths(&path?)
        .filter(|folder| folder.is_absolute())
        .map(|folder| folder.join(&file))
        .find(|candidate| candidate.is_file())
}

/// A folder of a unit test's own, removed when dropped, by a test that
/// failed too: under the user's cache, never `/tmp` (AGENTS.md,
/// "Testing"); on Windows, under their local application data. The
/// integration tests' is `tests/common`'s.
#[cfg(test)]
pub(crate) struct Scratch(pub(crate) PathBuf);

#[cfg(test)]
impl Scratch {
    pub(crate) fn new(label: &str) -> Scratch {
        let base = if cfg!(windows) {
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
        } else {
            std::env::var_os("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        };
        let dir = base
            .unwrap()
            .join("skwird-tests")
            .join(format!("client-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    pub(crate) fn join(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

#[cfg(test)]
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::{Component, Path, PathBuf};

    use super::{Scratch, absolute, program_on};

    /// The program a client starts on Windows is found as Windows would
    /// start it, an `.exe` where it has no extension, in the absolute
    /// folders of the `PATH` alone: never in one named relatively, the
    /// current folder among them. A path is taken as it is.
    #[test]
    fn a_program_is_found_on_the_path_alone() {
        let scratch = Scratch::new("program-on");
        let (here, bin) = (scratch.join("here"), scratch.join("bin"));
        for folder in [&here, &bin] {
            std::fs::create_dir_all(folder).unwrap();
            std::fs::write(folder.join("skwird.exe"), b"").unwrap();
        }
        let path = |folders: &[&Path]| Some(std::env::join_paths(folders).unwrap());
        let bare = Path::new("skwird");
        let found = Some(bin.join("skwird.exe"));
        assert_eq!(program_on(bare, path(&[Path::new("."), &bin])), found);
        assert_eq!(
            program_on(bare, path(&[&here, &bin])),
            Some(here.join("skwird.exe"))
        );
        assert_eq!(program_on(bare, path(&[Path::new(".")])), None);
        assert_eq!(program_on(bare, None), None);
        assert_eq!(program_on(&bin.join("skwird"), None), found);
        assert_eq!(program_on(&bin.join("skwird.exe"), None), found);
        // `here`, named from the current folder: passed over, though the
        // program is there.
        let cwd = std::env::current_dir().unwrap();
        let common = cwd
            .components()
            .zip(here.components())
            .take_while(|(a, b)| a == b)
            .count();
        let relative: PathBuf =
            std::iter::repeat_n(Component::ParentDir, cwd.components().count() - common)
                .chain(here.components().skip(common))
                .collect();
        if relative.is_relative() {
            assert!(relative.join("skwird.exe").is_file(), "{relative:?}");
            assert_eq!(program_on(bare, path(&[&relative, &bin])), found);
        } else {
            eprintln!("not proven: the current folder is on another drive");
        }
    }

    /// An XDG folder named empty or relative is none, as the engine reads
    /// it.
    #[test]
    fn a_folder_named_empty_or_relative_is_none() {
        assert_eq!(absolute(None), None);
        assert_eq!(absolute(Some(OsString::new())), None);
        assert_eq!(absolute(Some("state".into())), None);
        assert_eq!(absolute(Some("./state".into())), None);
        // Absolute as each system has it.
        let state = if cfg!(windows) {
            r"C:\Users\someone\state"
        } else {
            "/home/someone/.local/state"
        };
        assert_eq!(absolute(Some(state.into())), Some(PathBuf::from(state)));
    }
}
