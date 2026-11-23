//! skwird's client (`docs/DESIGN.md`, decision 1): find an engine, or
//! start one, and ask it.
//!
//! An engine is looked for in this order: the system service's socket,
//! the user's, and where neither answers, the engine started, if it is
//! installed. A client must work without skwird: every failure here is an
//! error to do without, never a reason to stop. An engine that speaks no
//! version the client speaks is done without too.
//!
//! Every call can be given up on: after a silence of the engine's
//! ([`Find::silence`]), or from another thread ([`Client::stopper`]).
//!
//! [`fake`] is an engine of a test's making, for clients' tests.
//!
//! Like `skwird-protocol`, this crate needs `serde` and `serde_json` alone,
//! so that a client may keep a copy of both in its own tree.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::Arc;
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub use skwird_protocol as protocol;
#[cfg(unix)]
use skwird_protocol::Request;
use skwird_protocol::{Answer, Ask, Changes, Coverage, Done, Entry, Query, Welcome, WirePath};

pub mod fake;

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
    /// A socket to ask, in place of the system's and the user's.
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
    /// given, so that it serves where it is looked for.
    pub runtime: Option<PathBuf>,
    /// The user's state folder, in place of `XDG_STATE_HOME`: where an
    /// engine started logs, and keeps what clients registered.
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

    /// The user's runtime folder: as given, or `XDG_RUNTIME_DIR`.
    #[cfg(unix)]
    fn runtime(&self) -> Option<PathBuf> {
        self.runtime
            .clone()
            .or_else(|| absolute(std::env::var_os("XDG_RUNTIME_DIR")))
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

/// The system service's socket (phase 3).
pub fn system_socket() -> PathBuf {
    PathBuf::from("/run/skwird/socket")
}

/// The user's engine's socket, where the user has a runtime folder
/// (`XDG_RUNTIME_DIR`, absolute).
pub fn user_socket() -> Option<PathBuf> {
    absolute(std::env::var_os("XDG_RUNTIME_DIR")).map(|r| socket_in(&r))
}

fn socket_in(runtime: &Path) -> PathBuf {
    runtime.join("skwird").join("socket")
}

/// A connection to an engine.
#[cfg(unix)]
pub struct Client {
    reader: std::io::BufReader<std::os::unix::net::UnixStream>,
    writer: std::os::unix::net::UnixStream,
    next: u64,
    welcome: Welcome,
    silence: Option<Duration>,
    /// Set by a [`Stopper`].
    stopped: Arc<AtomicBool>,
    /// Set once the engine said nothing for `silence`: what it says after
    /// would be read from the middle of a message, so the connection is
    /// closed.
    silent: bool,
}

/// A connection to an engine, which no system but Unix makes yet (rule
/// 7): [`Client::connect`] says so.
#[cfg(not(unix))]
pub struct Client {
    never: std::convert::Infallible,
}

/// What stops a connection's calls from another thread, taken from it
/// with [`Client::stopper`]: a call waiting on the engine returns at once
/// with [`Error::Stopped`], and so does every call after. A search given
/// up on stops so, whatever the engine is doing.
#[derive(Clone, Debug)]
pub struct Stopper {
    #[cfg(unix)]
    stream: Arc<std::os::unix::net::UnixStream>,
    #[cfg(unix)]
    stopped: Arc<AtomicBool>,
    #[cfg(not(unix))]
    never: std::convert::Infallible,
}

impl Stopper {
    /// Stop the connection's call, and every call after: the connection is
    /// closed.
    #[cfg(unix)]
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }

    /// Stop the connection's call, and every call after.
    #[cfg(not(unix))]
    pub fn stop(&self) {
        match self.never {}
    }
}

impl Client {
    /// The first engine that answers as `find` says, started if none does
    /// and `find` says so.
    #[cfg(unix)]
    pub fn connect(find: &Find) -> Result<Client, Error> {
        let sockets: Vec<PathBuf> = match &find.socket {
            Some(s) => vec![s.clone()],
            None => std::iter::once(system_socket())
                .chain(find.runtime().map(|r| socket_in(&r)))
                .collect(),
        };
        for socket in &sockets {
            match Client::open(socket, &find.client, find.silence) {
                Ok(client) => return Ok(client),
                Err(Error::Version) => return Err(Error::Version),
                Err(_) => {}
            }
        }
        let Some(program) = &find.start else {
            return Err(Error::NoEngine("none answers".into()));
        };
        let socket = match &find.socket {
            Some(s) => s.clone(),
            None => find.runtime().map(|r| socket_in(&r)).ok_or_else(|| {
                Error::NoEngine("XDG_RUNTIME_DIR is not set, or not absolute".into())
            })?,
        };
        start(program, find)?;
        // Until the engine binds its socket: a start's wait, not a loop
        // that idles.
        let deadline = std::time::Instant::now() + find.wait;
        let mut pause = Duration::from_millis(10);
        loop {
            match Client::open(&socket, &find.client, find.silence) {
                Ok(client) => return Ok(client),
                Err(Error::Version) => return Err(Error::Version),
                Err(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(pause);
                    pause = (pause * 2).min(Duration::from_millis(200));
                }
                Err(e) => return Err(Error::NoEngine(format!("started, it did not answer: {e}"))),
            }
        }
    }

    /// No engine runs elsewhere than Linux yet (rule 7).
    #[cfg(not(unix))]
    pub fn connect(_: &Find) -> Result<Client, Error> {
        Err(Error::NoEngine(
            "skwird runs on Linux so far: Windows comes with phase 4".into(),
        ))
    }

    /// The engine at `socket`, said hello to, waited on as long as it
    /// takes.
    #[cfg(unix)]
    pub fn at(socket: &Path, client: &str) -> Result<Client, Error> {
        Client::open(socket, client, None)
    }

    /// The engine at `socket`, said hello to, each call given up on after
    /// `silence` without a word from it.
    #[cfg(unix)]
    fn open(socket: &Path, client: &str, silence: Option<Duration>) -> Result<Client, Error> {
        let stream = std::os::unix::net::UnixStream::connect(socket)?;
        // A timeout of zero is refused: the shortest there is, instead.
        let silence = silence.map(|d| d.max(Duration::from_millis(1)));
        // The socket's own, so the writer's too.
        stream.set_read_timeout(silence)?;
        stream.set_write_timeout(silence)?;
        let writer = stream.try_clone()?;
        let mut c = Client {
            reader: std::io::BufReader::new(stream),
            writer,
            next: 1,
            welcome: Welcome {
                version: 0,
                scope: protocol::Scope::User,
                engine: String::new(),
                roots: Vec::new(),
            },
            silence,
            stopped: Arc::new(AtomicBool::new(false)),
            silent: false,
        };
        let id = c.ask(Ask::Hello(protocol::Hello {
            versions: vec![protocol::VERSION],
            client: client.to_owned(),
        }))?;
        match c.answer(id)? {
            Answer::Hello(welcome) if welcome.version == protocol::VERSION => {
                c.welcome = welcome;
                Ok(c)
            }
            Answer::Error(f) if f.what == "version" => Err(Error::Version),
            _ => Err(Error::Version),
        }
    }

    /// What the engine said of itself.
    #[cfg(unix)]
    pub fn welcome(&self) -> &Welcome {
        &self.welcome
    }

    /// What the engine said of itself.
    #[cfg(not(unix))]
    pub fn welcome(&self) -> &Welcome {
        match self.never {}
    }

    /// What stops this connection's calls from another thread.
    #[cfg(unix)]
    pub fn stopper(&self) -> Result<Stopper, Error> {
        Ok(Stopper {
            stream: Arc::new(self.writer.try_clone()?),
            stopped: Arc::clone(&self.stopped),
        })
    }

    /// What stops this connection's calls from another thread.
    #[cfg(not(unix))]
    pub fn stopper(&self) -> Result<Stopper, Error> {
        match self.never {}
    }

    /// Why no call can be made on this connection any more, if none can.
    #[cfg(unix)]
    fn closed(&self) -> Option<Error> {
        if self.stopped.load(Ordering::Acquire) {
            Some(Error::Stopped)
        } else if self.silent {
            Some(Error::Silent(self.silence.unwrap_or_default()))
        } else {
            None
        }
    }

    /// What a read or a write that failed means: a stop, the engine's
    /// silence (the connection closed), or the error itself.
    #[cfg(unix)]
    fn failed(&mut self, e: std::io::Error) -> Error {
        if self.stopped.load(Ordering::Acquire) {
            return Error::Stopped;
        }
        match (e.kind(), self.silence) {
            (std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut, Some(d)) => {
                self.silent = true;
                let _ = self.writer.shutdown(std::net::Shutdown::Both);
                Error::Silent(d)
            }
            _ => Error::Io(e),
        }
    }

    #[cfg(unix)]
    fn ask(&mut self, ask: Ask) -> Result<u64, Error> {
        if let Some(e) = self.closed() {
            return Err(e);
        }
        let id = self.next;
        self.next += 1;
        protocol::write(&mut self.writer, &Request { id, ask }).map_err(|e| self.failed(e))?;
        Ok(id)
    }

    #[cfg(unix)]
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

    #[cfg(not(unix))]
    fn ask(&mut self, _: Ask) -> Result<u64, Error> {
        match self.never {}
    }

    #[cfg(not(unix))]
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

/// Start the engine, apart from this process: in a process group of its
/// own, so that a signal to the client's terminal does not reach it; its
/// output to its log, never the client's; at `/`, so that it holds no
/// folder of the client's busy (a mount, a removable volume); with the
/// folders `find` names. A thread of the client's waits on it, so that
/// it is no zombie of the client's once it ends.
#[cfg(unix)]
fn start(program: &Path, find: &Find) -> Result<(), Error> {
    use std::os::unix::process::CommandExt;
    let log = find
        .state
        .clone()
        .or_else(|| absolute(std::env::var_os("XDG_STATE_HOME")))
        .or_else(|| absolute(std::env::var_os("HOME")).map(|h| h.join(".local/state")))
        .map(|d| d.join("skwird"))
        .and_then(|d| {
            std::fs::create_dir_all(&d).ok()?;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(d.join("engine.log"))
                .ok()
        });
    // A path relative to the client's folder, made absolute: from `/` it
    // would name another program. A bare name is looked for on the PATH.
    let program = if program.is_relative() && program.components().count() > 1 {
        std::env::current_dir()?.join(program)
    } else {
        program.to_owned()
    };
    let mut command = std::process::Command::new(program);
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

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::absolute;

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
