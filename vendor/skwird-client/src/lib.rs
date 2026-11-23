//! skwird's client (`docs/DESIGN.md`, decision 1): find an engine, or
//! start one, and ask it.
//!
//! An engine is looked for in this order: the system service's socket,
//! the user's, and where neither answers, the engine started, if it is
//! installed. A client must work without skwird: every failure here is an
//! error to do without, never a reason to stop. An engine that speaks no
//! version the client speaks is done without too.
//!
//! [`fake`] is an engine of a test's making, for clients' tests.
//!
//! Like `skwird-protocol`, this crate needs `serde` and `serde_json` alone,
//! so that a client may keep a copy of both in its own tree.

use std::path::{Path, PathBuf};
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
    Io(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoEngine(why) => write!(f, "no skwird engine: {why}"),
            Error::Version => write!(f, "the skwird engine speaks another protocol"),
            Error::Engine(failure) => write!(f, "skwird: {} ({})", failure.message, failure.what),
            Error::Unexpected => write!(f, "skwird answered what was not asked"),
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
        }
    }
}

/// The system service's socket (phase 3).
pub fn system_socket() -> PathBuf {
    PathBuf::from("/run/skwird/socket")
}

/// The user's engine's socket, where the user has a runtime folder.
pub fn user_socket() -> Option<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty())?;
    Some(PathBuf::from(runtime).join("skwird").join("socket"))
}

/// A connection to an engine.
#[cfg(unix)]
pub struct Client {
    reader: std::io::BufReader<std::os::unix::net::UnixStream>,
    writer: std::os::unix::net::UnixStream,
    next: u64,
    welcome: Welcome,
}

/// A connection to an engine, which no system but Unix makes yet (rule
/// 7): [`Client::connect`] says so.
#[cfg(not(unix))]
pub struct Client {
    never: std::convert::Infallible,
}

impl Client {
    /// The first engine that answers as `find` says, started if none does
    /// and `find` says so.
    #[cfg(unix)]
    pub fn connect(find: &Find) -> Result<Client, Error> {
        let sockets: Vec<PathBuf> = match &find.socket {
            Some(s) => vec![s.clone()],
            None => std::iter::once(system_socket())
                .chain(user_socket())
                .collect(),
        };
        for socket in &sockets {
            match Client::at(socket, &find.client) {
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
            None => {
                user_socket().ok_or_else(|| Error::NoEngine("XDG_RUNTIME_DIR is not set".into()))?
            }
        };
        start(program)?;
        // Until the engine binds its socket: a start's wait, not a loop
        // that idles.
        let deadline = std::time::Instant::now() + find.wait;
        let mut pause = Duration::from_millis(10);
        loop {
            match Client::at(&socket, &find.client) {
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

    /// The engine at `socket`, said hello to.
    #[cfg(unix)]
    pub fn at(socket: &Path, client: &str) -> Result<Client, Error> {
        let stream = std::os::unix::net::UnixStream::connect(socket)?;
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

    #[cfg(unix)]
    fn ask(&mut self, ask: Ask) -> Result<u64, Error> {
        let id = self.next;
        self.next += 1;
        protocol::write(&mut self.writer, &Request { id, ask })?;
        Ok(id)
    }

    #[cfg(unix)]
    fn answer(&mut self, id: u64) -> Result<Answer, Error> {
        loop {
            let response: protocol::Response = protocol::read(&mut self.reader)?
                .ok_or_else(|| Error::Io(std::io::ErrorKind::UnexpectedEof.into()))?;
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

    /// Want `add` live, take back `remove`: what this client wants live,
    /// after. A folder past the engine's share of watches is checked,
    /// not live: its coverage says which.
    pub fn watch(&mut self, add: &[&Path], remove: &[&Path]) -> Result<Vec<PathBuf>, Error> {
        let id = self.ask(Ask::Watch {
            add: add.iter().map(|p| WirePath::of(p)).collect(),
            remove: remove.iter().map(|p| WirePath::of(p)).collect(),
        })?;
        match self.answer(id)? {
            Answer::Watching { folders } => Ok(folders.iter().map(WirePath::to_path).collect()),
            _ => Err(Error::Unexpected),
        }
    }
}

/// Start the engine, apart from this process: in a process group of its
/// own, so that a signal to the client's terminal does not reach it, and
/// its output to its log, never the client's.
#[cfg(unix)]
fn start(program: &Path) -> Result<(), Error> {
    use std::os::unix::process::CommandExt;
    let log = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .map(|d| d.join("skwird"))
        .and_then(|d| {
            std::fs::create_dir_all(&d).ok()?;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(d.join("engine.log"))
                .ok()
        });
    let mut command = std::process::Command::new(program);
    command
        .arg("serve")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .process_group(0);
    match log {
        Some(file) => command.stderr(file),
        None => command.stderr(std::process::Stdio::null()),
    };
    command
        .spawn()
        .map(drop)
        .map_err(|e| Error::NoEngine(format!("cannot start it: {e}")))
}
