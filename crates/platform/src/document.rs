//! On-demand native PDF sessions. A manager thread owns every child and temporary
//! file, so closing a preview never waits or removes files on the UI thread.
//!
//! The manager sleeps in one `select!` over jobs, worker replies and
//! cancellation, like the media session's. Every request carries an id and
//! the worker answers each one once, in order, so a request that was given
//! up on is answered too: its reply is drained, not a reason to restart the
//! worker. A cancelled request tells the worker to stop (`Cancel`, which a
//! search notices at its next page); only a worker that stays busy past a
//! short grace, times out or breaks the protocol is killed and reopened.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use anyhow::Context;
use bibli_core::CancelToken;
use bibli_core::document::{
    DocumentInfo, DocumentPage, DocumentSession, MAX_REPLY, MAX_SIDE, PIXELS_FILE, SearchHit,
    Tagged, WorkerReply, WorkerRequest,
};
use crossbeam_channel::{Receiver, Sender};

use crate::line_channel::LineChannel;

/// Worker spawn, library load and the document's first parse.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);
/// One page render or one whole-document search.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// How long an abandoned request may keep the worker busy once it has been
/// told to stop: a page that is being rendered finishes, a search stops at
/// its next page. Longer than that, the worker is restarted.
const ABANDONED_GRACE: Duration = Duration::from_secs(2);

struct Job {
    request: WorkerRequest,
    cancel: CancelToken,
    reply: Sender<anyhow::Result<Response>>,
}
enum Response {
    Page(DocumentPage),
    Matches(Vec<SearchHit>),
}

pub struct WorkerSession {
    info: DocumentInfo,
    jobs: Sender<Job>,
}

#[derive(Clone, Copy, Debug)]
pub enum Engine {
    Pdfium,
    Poppler,
}

/// The engine's worker executable: its environment override, beside our
/// binary, or on `PATH`. Plugins that require it name this, not the literals.
pub fn worker_path(engine: Engine) -> Option<PathBuf> {
    let (variable, executable) = match engine {
        Engine::Pdfium => ("BIBLI_PDF_WORKER", "bibli-pdf-worker"),
        Engine::Poppler => ("BIBLI_POPPLER_WORKER", "bibli-poppler-worker"),
    };
    crate::process::find_worker(variable, executable)
}

/// What the manager starts a worker with.
struct Launch {
    executable: PathBuf,
    pdf: PathBuf,
    scratch: PathBuf,
    library: Option<PathBuf>,
}

impl WorkerSession {
    /// Called on a scheduler worker. None means the optional worker/library is
    /// absent; malformed or encrypted documents produce an ordinary error.
    pub(crate) fn open(
        engine: Engine,
        pdf: PathBuf,
        source: PathBuf,
        scratch: tempfile::TempDir,
        library: Option<PathBuf>,
        lifetime: CancelToken,
    ) -> anyhow::Result<Option<Arc<dyn DocumentSession>>> {
        let Some(executable) = worker_path(engine) else {
            return Ok(None);
        };
        Self::open_with_worker(executable, pdf, source, scratch, library, lifetime)
    }

    fn open_with_worker(
        executable: PathBuf,
        pdf: PathBuf,
        source: PathBuf,
        scratch: tempfile::TempDir,
        library: Option<PathBuf>,
        lifetime: CancelToken,
    ) -> anyhow::Result<Option<Arc<dyn DocumentSession>>> {
        let (jobs, rx) = crossbeam_channel::unbounded::<Job>();
        let (ready, startup) = crossbeam_channel::bounded(1);
        let cancelled = lifetime.subscribe();
        std::thread::Builder::new()
            .name("bibli-pdf-session".into())
            .spawn(move || {
                let launch = Launch {
                    executable,
                    pdf,
                    scratch: scratch.path().to_owned(),
                    library,
                };
                let setup = || -> anyhow::Result<(Worker, DocumentInfo, FileVersion)> {
                    let version = file_version(&source)?;
                    let (worker, greeting) = Worker::start(&launch, &lifetime)?;
                    match greeting {
                        WorkerReply::Ready(info) if info.pages > 0 => Ok((worker, info, version)),
                        WorkerReply::Unavailable(message) => Err(Unavailable(message).into()),
                        WorkerReply::Error(message) => anyhow::bail!("{message}"),
                        _ => anyhow::bail!("invalid document worker greeting"),
                    }
                };
                match setup() {
                    Ok((worker, info, version)) => {
                        if ready.send(Ok(info)).is_ok() {
                            Manager {
                                launch,
                                source,
                                version,
                                lifetime,
                                worker: Some(worker),
                            }
                            .run(&rx);
                        }
                    }
                    Err(error) => {
                        let _ = ready.send(Err(error));
                    }
                }
                // The manager dropped its worker (kill and reap) on this
                // thread; only now is the scratch directory removed.
                drop(scratch);
            })?;
        crossbeam_channel::select! {
            recv(startup) -> verdict => match verdict {
                Ok(Ok(info)) => Ok(Some(Arc::new(Self { info, jobs }))),
                Ok(Err(error)) if error.is::<Unavailable>() => Ok(None),
                Ok(Err(error)) => Err(error),
                Err(_) => anyhow::bail!("document worker stopped during startup"),
            },
            recv(cancelled.receiver()) -> _ => anyhow::bail!("document open cancelled"),
        }
    }

    fn request(&self, request: WorkerRequest, cancel: &CancelToken) -> anyhow::Result<Response> {
        anyhow::ensure!(!cancel.is_cancelled(), "document request cancelled");
        let (reply, rx) = crossbeam_channel::bounded(1);
        self.jobs
            .send(Job {
                request,
                cancel: cancel.clone(),
                reply,
            })
            .context("document session closed")?;
        let cancelled = cancel.subscribe();
        crossbeam_channel::select! {
            recv(rx) -> result => result.unwrap_or_else(|_| anyhow::bail!("document session closed")),
            recv(cancelled.receiver()) -> _ => anyhow::bail!("document request cancelled"),
        }
    }
}

/// The worker says its library is missing: `auto` tries the next engine.
#[derive(Debug)]
struct Unavailable(String);
impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unavailable: {}", self.0)
    }
}
impl std::error::Error for Unavailable {}

impl DocumentSession for WorkerSession {
    fn info(&self) -> &DocumentInfo {
        &self.info
    }
    fn render(&self, page: u32, side: u32, cancel: &CancelToken) -> anyhow::Result<DocumentPage> {
        match self.request(WorkerRequest::Render { page, side }, cancel)? {
            Response::Page(page) => Ok(page),
            Response::Matches(_) => anyhow::bail!("unexpected document search result"),
        }
    }
    fn search(&self, query: &str, cancel: &CancelToken) -> anyhow::Result<Vec<SearchHit>> {
        anyhow::ensure!(query.len() <= 4096, "search query exceeds limit");
        match self.request(
            WorkerRequest::Search {
                query: query.into(),
            },
            cancel,
        )? {
            Response::Matches(hits) => Ok(hits),
            Response::Page(_) => anyhow::bail!("unexpected document page"),
        }
    }
}

pub(crate) type FileVersion = (u64, SystemTime, (i64, i64));
/// Size, modification and change time: what tells a file was rewritten,
/// for sessions and for preview cache keys alike.
pub(crate) fn file_version(path: &Path) -> anyhow::Result<FileVersion> {
    let m = path.metadata()?;
    #[cfg(unix)]
    let changed = {
        use std::os::unix::fs::MetadataExt;
        (m.ctime(), m.ctime_nsec())
    };
    #[cfg(not(unix))]
    let changed = (0, 0);
    Ok((m.len(), m.modified()?, changed))
}

/// The session's manager thread: the one owner of the worker process.
struct Manager {
    launch: Launch,
    source: PathBuf,
    version: FileVersion,
    lifetime: CancelToken,
    worker: Option<Worker>,
}

impl Manager {
    fn run(mut self, jobs: &Receiver<Job>) {
        let cancelled = self.lifetime.subscribe();
        loop {
            // Replies to requests given up on arrive while idle too: take
            // them as they come, so the worker is never stuck writing one.
            let idle = self
                .worker
                .as_ref()
                .map_or_else(crossbeam_channel::never, |w| w.channel.replies().clone());
            crossbeam_channel::select! {
                recv(jobs) -> job => {
                    let Ok(job) = job else { break };
                    let result = self.handle(&job);
                    let _ = job.reply.send(result);
                }
                recv(idle) -> reply => {
                    if let Some(worker) = &mut self.worker && worker.accept(reply).is_err() {
                        self.worker = None;
                    }
                }
                recv(cancelled.receiver()) -> _ => break,
            }
        }
    }

    fn handle(&mut self, job: &Job) -> anyhow::Result<Response> {
        let cancel = job.cancel.linked(&self.lifetime);
        anyhow::ensure!(!cancel.is_cancelled(), "document request cancelled");
        self.unchanged()?;
        let reply = match self.exchange(&job.request, &cancel) {
            Ok(reply) => reply,
            Err(Failure::Cancelled) => anyhow::bail!("document request cancelled"),
            Err(Failure::Broken(error)) => {
                // Timed out, exited or broke the protocol: a new one serves
                // the next request.
                self.worker = None;
                return Err(error);
            }
        };
        self.unchanged()?;
        match reply {
            WorkerReply::Page {
                content,
                width,
                height,
            } => {
                anyhow::ensure!(
                    width > 0 && height > 0 && width <= MAX_SIDE && height <= MAX_SIDE,
                    "invalid document bitmap size"
                );
                let bytes = u64::from(width) * u64::from(height) * 4;
                let path = self.launch.scratch.join(PIXELS_FILE);
                anyhow::ensure!(
                    path.metadata()?.len() == bytes,
                    "invalid document bitmap length"
                );
                let rgba = std::fs::read(path)?;
                Ok(Response::Page(DocumentPage {
                    width,
                    height,
                    rgba,
                    content: Arc::new(content),
                }))
            }
            WorkerReply::Matches(hits) => Ok(Response::Matches(hits)),
            // A page the worker could not render is that page's error; the
            // worker itself is fine and keeps the document open.
            WorkerReply::Error(message) | WorkerReply::Unavailable(message) => {
                anyhow::bail!("{message}")
            }
            WorkerReply::Ready(_) => anyhow::bail!("unexpected document greeting"),
        }
    }

    fn unchanged(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            file_version(&self.source)? == self.version,
            "document changed; reopen the preview"
        );
        Ok(())
    }

    /// Send `request` and wait for its reply, starting a worker if there is
    /// none and replacing one that stays busy with an abandoned request.
    fn exchange(
        &mut self,
        request: &WorkerRequest,
        cancel: &CancelToken,
    ) -> Result<WorkerReply, Failure> {
        let cancelled = cancel.subscribe();
        if let Some(worker) = &mut self.worker
            && worker.busy()
        {
            let grace = crossbeam_channel::after(ABANDONED_GRACE);
            match worker.settle(&cancelled, &grace) {
                Ok(()) => {}
                Err(Failure::Cancelled) => return Err(Failure::Cancelled),
                Err(Failure::Broken(_)) => self.worker = None,
            }
        }
        if self.worker.is_none() {
            let (worker, greeting) = Worker::start(&self.launch, cancel).map_err(|error| {
                if cancel.is_cancelled() {
                    Failure::Cancelled
                } else {
                    Failure::Broken(error)
                }
            })?;
            if !matches!(greeting, WorkerReply::Ready(_)) {
                return Err(Failure::Broken(anyhow::anyhow!(
                    "could not reopen document worker"
                )));
            }
            self.worker = Some(worker);
        }
        let worker = self.worker.as_mut().expect("started above");
        let id = worker.send(request).map_err(Failure::Broken)?;
        let deadline = crossbeam_channel::after(REQUEST_TIMEOUT);
        match worker.wait(id, &cancelled, &deadline) {
            Err(Failure::Cancelled) => {
                // Tell the worker to stop; its answers are drained later.
                if worker.send(&WorkerRequest::Cancel).is_err() {
                    self.worker = None;
                }
                Err(Failure::Cancelled)
            }
            other => other,
        }
    }
}

enum Failure {
    Cancelled,
    /// The worker cannot be trusted with another request.
    Broken(anyhow::Error),
}

struct Worker {
    channel: LineChannel<Tagged<WorkerReply>>,
    /// Id of the last request sent, and of the next reply due: replies come
    /// once per request, in order, starting with the greeting as 0.
    sent: u64,
    due: u64,
}
impl Worker {
    fn start(launch: &Launch, cancel: &CancelToken) -> anyhow::Result<(Self, WorkerReply)> {
        anyhow::ensure!(!cancel.is_cancelled(), "document open cancelled");
        let mut command = Command::new(&launch.executable);
        command.arg(&launch.pdf).arg(&launch.scratch);
        if let Some(library) = &launch.library {
            command.env("BIBLI_PDFIUM_LIBRARY", library);
        }
        let mut worker = Self {
            channel: LineChannel::spawn(&mut command, MAX_REPLY, "bibli-pdf")?,
            sent: 0,
            due: 0,
        };
        let cancelled = cancel.subscribe();
        let deadline = crossbeam_channel::after(STARTUP_TIMEOUT);
        match worker.wait(0, &cancelled, &deadline) {
            Ok(greeting) => Ok((worker, greeting)),
            Err(Failure::Cancelled) => anyhow::bail!("document open cancelled"),
            Err(Failure::Broken(error)) => Err(error),
        }
    }

    fn busy(&self) -> bool {
        self.due <= self.sent
    }

    fn send(&mut self, request: &WorkerRequest) -> anyhow::Result<u64> {
        let id = self.sent + 1;
        self.channel.send(&Tagged { id, body: request })?;
        self.sent = id;
        Ok(id)
    }

    /// Take one reply off the channel, in order.
    fn accept(
        &mut self,
        reply: Result<anyhow::Result<Tagged<WorkerReply>>, crossbeam_channel::RecvError>,
    ) -> anyhow::Result<Tagged<WorkerReply>> {
        let reply = reply.map_err(|_| anyhow::anyhow!("document worker exited unexpectedly"))??;
        anyhow::ensure!(
            reply.id == self.due && reply.id <= self.sent,
            "document worker answered out of order"
        );
        self.due += 1;
        Ok(reply)
    }

    /// Wait for the reply to `id`, dropping the answers to earlier requests.
    fn wait(
        &mut self,
        id: u64,
        cancelled: &bibli_core::cancel::CancelSignal,
        deadline: &Receiver<Instant>,
    ) -> Result<WorkerReply, Failure> {
        loop {
            crossbeam_channel::select! {
                recv(self.channel.replies()) -> reply => {
                    let reply = self.accept(reply).map_err(Failure::Broken)?;
                    if reply.id == id {
                        return Ok(reply.body);
                    }
                }
                recv(cancelled.receiver()) -> _ => return Err(Failure::Cancelled),
                recv(deadline) -> _ => {
                    return Err(Failure::Broken(anyhow::anyhow!("document worker timed out")));
                }
            }
        }
    }

    /// Wait until every request sent has been answered.
    fn settle(
        &mut self,
        cancelled: &bibli_core::cancel::CancelSignal,
        deadline: &Receiver<Instant>,
    ) -> Result<(), Failure> {
        if self.busy() {
            self.wait(self.sent, cancelled, deadline)?;
        }
        Ok(())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !condition() {
            assert!(Instant::now() < deadline, "worker condition timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    struct Fixture {
        _root: tempfile::TempDir,
        scratch: PathBuf,
        source: PathBuf,
        lifetime: CancelToken,
        session: Option<Arc<dyn DocumentSession>>,
    }
    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("Synthetic document.pdf");
            std::fs::write(&source, b"synthetic").unwrap();
            let executable = root.path().join("worker");
            let shell = crate::process::required("sh").unwrap();
            // Answers every request once, in order, with the request's id;
            // "slow" takes a moment, "hang" never finishes.
            let script = format!(
                concat!(
                    "#!{}\n",
                    "printf '%s' \"$$\" > \"$2/pid\"\n",
                    "printf '%s\\n' '{{\"id\":0,\"body\":{{\"Ready\":{{\"pages\":3,\"outlines\":[]}}}}}}'\n",
                    "while IFS= read -r line; do\n",
                    " id=${{line#*\\\"id\\\":}}; id=${{id%%,*}}\n",
                    " case \"$line\" in\n",
                    " *hang*) touch \"$2/requested\"; sleep 30;;\n",
                    " *slow*) touch \"$2/requested\"; sleep 0.3; printf '{{\"id\":%s,\"body\":{{\"Matches\":[]}}}}\\n' \"$id\";;\n",
                    " *Cancel*) printf '{{\"id\":%s,\"body\":{{\"Error\":\"cancelled\"}}}}\\n' \"$id\";;\n",
                    " *) printf '{{\"id\":%s,\"body\":{{\"Matches\":[]}}}}\\n' \"$id\";;\n",
                    " esac\n",
                    "done\n"
                ),
                shell.display()
            );
            std::fs::write(&executable, script).unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            let scratch = tempfile::tempdir().unwrap();
            let scratch_path = scratch.path().to_owned();
            let lifetime = CancelToken::new();
            let session = WorkerSession::open_with_worker(
                executable,
                source.clone(),
                source.clone(),
                scratch,
                None,
                lifetime.clone(),
            )
            .unwrap()
            .unwrap();
            Self {
                _root: root,
                source,
                scratch: scratch_path,
                lifetime,
                session: Some(session),
            }
        }
        fn pid(&self) -> u32 {
            std::fs::read_to_string(self.scratch.join("pid"))
                .unwrap()
                .parse()
                .unwrap()
        }
    }

    #[test]
    fn closing_or_disabling_reaps_idle_worker_and_removes_scratch() {
        for disable in [false, true] {
            let mut fixture = Fixture::new();
            let pid = fixture.pid();
            if disable {
                fixture.lifetime.cancel();
            } else {
                drop(fixture.session.take());
            }
            wait_until(|| {
                !Path::new(&format!("/proc/{pid}")).exists() && !fixture.scratch.exists()
            });
            if let Some(session) = &fixture.session {
                assert!(session.search("needle", &CancelToken::new()).is_err());
            }
        }
    }

    /// Cancel `query` once the worker has it, and return the error.
    fn cancel_search(fixture: &Fixture, query: &'static str) -> String {
        let session = fixture.session.as_ref().unwrap().clone();
        let token = CancelToken::new();
        let cancelled = token.clone();
        let request = std::thread::spawn(move || session.search(query, &cancelled));
        wait_until(|| fixture.scratch.join("requested").exists());
        token.cancel();
        request.join().unwrap().unwrap_err().to_string()
    }

    #[test]
    fn a_cancelled_request_is_drained_and_the_worker_kept() {
        let fixture = Fixture::new();
        let pid = fixture.pid();
        assert!(cancel_search(&fixture, "slow").contains("cancelled"));
        let session = fixture.session.as_ref().unwrap();
        for _ in 0..3 {
            assert!(
                session
                    .search("needle", &CancelToken::new())
                    .unwrap()
                    .is_empty()
            );
        }
        assert_eq!(fixture.pid(), pid, "the document stayed open");
    }

    #[test]
    fn a_worker_stuck_on_an_abandoned_request_is_replaced() {
        let fixture = Fixture::new();
        let original_pid = fixture.pid();
        assert!(cancel_search(&fixture, "hang").contains("cancelled"));
        assert!(
            fixture
                .session
                .as_ref()
                .unwrap()
                .search("needle", &CancelToken::new())
                .unwrap()
                .is_empty()
        );
        wait_until(|| !Path::new(&format!("/proc/{original_pid}")).exists());
        assert_ne!(fixture.pid(), original_pid);
    }

    #[test]
    fn source_change_invalidates_session() {
        let fixture = Fixture::new();
        std::fs::write(&fixture.source, b"changed synthetic document").unwrap();
        let error = fixture
            .session
            .as_ref()
            .unwrap()
            .search("needle", &CancelToken::new())
            .unwrap_err();
        assert!(error.to_string().contains("document changed"));
    }
}
