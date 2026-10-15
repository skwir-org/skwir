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
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use anyhow::Context;
use crossbeam_channel::{Receiver, Sender};
use skwir_core::CancelToken;
use skwir_core::document::{
    DocumentInfo, DocumentPage, DocumentSession, MAX_PAGE_SIZE, MAX_REPLY, MAX_SIDE, PIXELS_FILE,
    SearchHit, Tagged, WorkerReply, WorkerRequest,
};

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
    Located(Option<u32>),
}

pub struct WorkerSession {
    info: DocumentInfo,
    jobs: Sender<Job>,
}

#[derive(Clone, Copy, Debug)]
pub enum Engine {
    Pdfium,
    Poppler,
    /// Chosen explicitly only: `auto` does not try it (ADR-014).
    Mupdf,
    /// `DjVu` by `DjVuLibre`, the reference library: its worker links it, and
    /// is a package of its own (ADR-061).
    Djvulibre,
    /// `DjVu` by djvu-rs, in Rust: every package carries its worker.
    DjvuRs,
}

impl Engine {
    /// Its name, as the Documents page says it.
    pub fn label(self) -> &'static str {
        match self {
            Engine::Pdfium => "PDFium",
            Engine::Poppler => "Poppler",
            Engine::Mupdf => "MuPDF",
            Engine::Djvulibre => "DjVuLibre",
            Engine::DjvuRs => "djvu-rs",
        }
    }
}

/// The engine's worker executable: its environment override, beside our
/// binary, or on `PATH`. Plugins that require it name this, not the literals.
pub fn worker_path(engine: Engine) -> Option<PathBuf> {
    worker(engine).find()
}

/// The engine's worker.
pub fn worker(engine: Engine) -> crate::process::Worker {
    let (variable, executable) = match engine {
        Engine::Pdfium => ("SKWIR_PDF_WORKER", "skwir-pdf-worker"),
        Engine::Poppler => ("SKWIR_POPPLER_WORKER", "skwir-poppler-worker"),
        Engine::Mupdf => ("SKWIR_MUPDF_WORKER", "skwir-mupdf-worker"),
        Engine::Djvulibre => ("SKWIR_DJVULIBRE_WORKER", "skwir-djvulibre-worker"),
        Engine::DjvuRs => ("SKWIR_DJVU_WORKER", "skwir-djvu-worker"),
    };
    crate::process::Worker {
        variable,
        executable,
    }
}

/// What the manager starts a worker with.
struct Launch {
    executable: PathBuf,
    pdf: PathBuf,
    scratch: PathBuf,
    setup: Setup,
    /// Whether it runs isolated where the system can (`sandbox`): always,
    /// but for tests that watch a stand-in worker's process.
    isolate: bool,
}

/// What an engine is told beside the document: `PDFium`'s library, and the
/// pages a reflowable document is laid out into.
#[derive(Clone, Debug, Default)]
pub(crate) struct Setup {
    pub library: Option<PathBuf>,
    pub layout: Option<Layout>,
    /// Only its first page is drawn (a thumbnail): the engine reads no
    /// outline nor labels (`document::FIRST_PAGE`).
    pub first_page: bool,
}

/// The pages a reflowable document (a book) is laid out into: their size
/// and the size of body text, in points. Only `MuPDF` lays documents out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    pub em: f32,
}
impl Layout {
    /// Book pages (A5), body text at `text_size` pixels, the reader's own
    /// measure of text: 16 px is 12 pt.
    pub fn book(text_size: Option<f32>) -> Self {
        Self {
            width: 420.,
            height: 595.,
            em: text_size.unwrap_or(16.).clamp(8., 48.) * 0.75,
        }
    }
}

/// A book laid out into pages by `MuPDF`, its text at `text_size` pixels, for
/// the reader: the same session as a PDF's, over pages the engine made.
/// `None` when the `MuPDF` worker is absent. Called on a scheduler worker.
pub fn open_book(
    source: PathBuf,
    text_size: Option<f32>,
    lifetime: CancelToken,
) -> anyhow::Result<Option<Arc<dyn DocumentSession>>> {
    let setup = Setup {
        library: None,
        layout: Some(Layout::book(text_size)),
        first_page: false,
    };
    WorkerSession::open(
        Engine::Mupdf,
        source.clone(),
        source,
        crate::file::memory_dir("skwir-pages-")?,
        setup,
        lifetime,
    )
}

/// `PDFium`'s library as the settings name it, shared by what opens
/// documents (the reader, thumbnails, the requirement's probe), so one
/// change reaches them all. `None`: the one a package carries beside the
/// helper, else the system's (ADR-041).
pub type PdfiumChoice = Arc<parking_lot::RwLock<Option<PathBuf>>>;

/// The engines `auto` tries, in its order: `PDFium` (the one skwir's
/// packages carry), then Poppler. `MuPDF` only when chosen (ADR-014).
pub const AUTO: [Engine; 2] = [Engine::Pdfium, Engine::Poppler];

/// The engines that read `DjVu`, in `auto`'s order: `DjVuLibre` where it is
/// installed, then djvu-rs, which skwir's packages carry (ADR-061).
pub const DJVU_AUTO: [Engine; 2] = [Engine::Djvulibre, Engine::DjvuRs];

/// The document at `source` read by the first of `engines` whose worker
/// there is, for the reader. `None` when there is none: an engine gives
/// way to the next only when it is absent, never when it fails on the
/// document (that is the document's error). Called on a scheduler worker.
pub fn open(
    source: &Path,
    engines: &[Engine],
    lifetime: &CancelToken,
) -> anyhow::Result<Option<Arc<dyn DocumentSession>>> {
    for &engine in engines {
        if let Some(session) = WorkerSession::open(
            engine,
            source.to_owned(),
            source.to_owned(),
            crate::file::memory_dir("skwir-pages-")?,
            Setup::default(),
            lifetime.clone(),
        )? {
            return Ok(Some(session));
        }
    }
    Ok(None)
}

/// The first page of the PDF at `pdf`, at most `side` pixels on its
/// longer side, drawn by the first of `engines` there is, `PDFium` told
/// `library` when the settings name one: what a thumbnail draws, as the
/// reader would. `None` when no engine is. A session opened for one page
/// and closed. Called on a scheduler worker.
pub fn first_page(
    pdf: &Path,
    side: u32,
    engines: &[Engine],
    library: Option<&Path>,
    cancel: &CancelToken,
) -> anyhow::Result<Option<skwir_core::thumbs::Image>> {
    for &engine in engines {
        let setup = Setup {
            library: library
                .filter(|_| matches!(engine, Engine::Pdfium))
                .map(Path::to_owned),
            layout: None,
            first_page: true,
        };
        let Some(session) = WorkerSession::open(
            engine,
            pdf.to_owned(),
            pdf.to_owned(),
            crate::file::memory_dir("skwir-pages-")?,
            setup,
            cancel.child(),
        )?
        else {
            continue;
        };
        let page = session.render(1, side, cancel)?;
        return Ok(Some(skwir_core::thumbs::Image {
            width: page.width,
            height: page.height,
            rgba: page.into_rgba().into(),
        }));
    }
    Ok(None)
}

impl WorkerSession {
    /// Called on a scheduler worker. None means the optional worker/library is
    /// absent; malformed or encrypted documents produce an ordinary error.
    pub(crate) fn open(
        engine: Engine,
        pdf: PathBuf,
        source: PathBuf,
        scratch: tempfile::TempDir,
        setup: Setup,
        lifetime: CancelToken,
    ) -> anyhow::Result<Option<Arc<dyn DocumentSession>>> {
        let Some(executable) = worker_path(engine) else {
            return Ok(None);
        };
        Self::open_with_worker(executable, pdf, source, scratch, setup, lifetime, true)
    }

    fn open_with_worker(
        executable: PathBuf,
        pdf: PathBuf,
        source: PathBuf,
        scratch: tempfile::TempDir,
        setup: Setup,
        lifetime: CancelToken,
        isolate: bool,
    ) -> anyhow::Result<Option<Arc<dyn DocumentSession>>> {
        let (jobs, rx) = crossbeam_channel::unbounded::<Job>();
        let (ready, startup) = crossbeam_channel::bounded(1);
        let cancelled = lifetime.subscribe();
        std::thread::Builder::new()
            .name("skwir-pdf-session".into())
            .spawn(move || {
                let launch = Launch {
                    executable,
                    pdf,
                    scratch: scratch.path().to_owned(),
                    setup,
                    isolate,
                };
                let started = || -> anyhow::Result<(Worker, DocumentInfo)> {
                    let (worker, greeting) = Worker::start(&launch, &lifetime)?;
                    match greeting {
                        WorkerReply::Ready(info) if info.pages > 0 => Ok((worker, info)),
                        WorkerReply::Unavailable(message) => Err(Unavailable(message).into()),
                        WorkerReply::Error(message) => anyhow::bail!("{message}"),
                        _ => anyhow::bail!("invalid document worker greeting"),
                    }
                };
                let setup = || -> anyhow::Result<(Worker, DocumentInfo, FileVersion)> {
                    let version = file_version(&source)?;
                    // An engine without its library is no failure of
                    // its helper: the next engine is tried.
                    let (worker, info) = started().map_err(|error| {
                        if error.is::<Unavailable>() {
                            error
                        } else {
                            crate::process::explain(&launch.executable, &lifetime, error)
                        }
                    })?;
                    Ok((worker, info, version))
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
            Response::Matches(_) | Response::Located(_) => {
                anyhow::bail!("unexpected document search result")
            }
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
            Response::Page(_) | Response::Located(_) => {
                anyhow::bail!("unexpected document reply")
            }
        }
    }
    fn locate(&self, text: &str, near: u32, cancel: &CancelToken) -> anyhow::Result<Option<u32>> {
        anyhow::ensure!(text.len() <= 4096, "text to locate exceeds limit");
        match self.request(
            WorkerRequest::Locate {
                text: text.into(),
                near,
            },
            cancel,
        )? {
            Response::Located(page) => Ok(page.filter(|page| (1..=self.info.pages).contains(page))),
            Response::Page(_) | Response::Matches(_) => {
                anyhow::bail!("unexpected document reply")
            }
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
                return Err(crate::process::explain(
                    &self.launch.executable,
                    &cancel,
                    error,
                ));
            }
        };
        self.unchanged()?;
        self.read(reply)
            .map_err(|error| crate::process::explain(&self.launch.executable, &cancel, error))
    }

    /// What the helper answered a request, as a response.
    fn read(&self, reply: WorkerReply) -> anyhow::Result<Response> {
        match reply {
            WorkerReply::Page {
                content,
                width,
                height,
                order,
            } => {
                anyhow::ensure!(
                    width > 0 && height > 0 && width <= MAX_SIDE && height <= MAX_SIDE,
                    "invalid document bitmap size"
                );
                anyhow::ensure!(
                    [content.width, content.height]
                        .iter()
                        .all(|side| side.is_finite() && *side > 0. && *side <= MAX_PAGE_SIZE),
                    "invalid document page size"
                );
                let bytes = u64::from(width) * u64::from(height) * 4;
                // Read as a file, no more than the page: the worker reads
                // an untrusted document, and what it leaves could be a pipe.
                let pixels = self.launch.scratch.join(PIXELS_FILE);
                let read = crate::file::read_regular(&pixels, bytes)?;
                // Read: emptied, so a page kept in memory (`memory_dir`) is
                // not held twice until the next.
                if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&pixels) {
                    let _ = file.set_len(0);
                }
                anyhow::ensure!(read.len() as u64 == bytes, "invalid document bitmap length");
                Ok(Response::Page(DocumentPage {
                    width,
                    height,
                    pixels: read,
                    order,
                    content: Arc::new(content),
                }))
            }
            WorkerReply::Matches(hits) => Ok(Response::Matches(hits)),
            WorkerReply::Located(page) => Ok(Response::Located(page)),
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
        // A document is a file nobody vouched for: its engine runs isolated
        // where the system can (ADR-041), as skwir's other helpers on such
        // files do, reading its own folder (where a package carries
        // PDFium) and the library the settings name.
        let run = if launch.isolate {
            let mut readable: Vec<&Path> = launch.executable.parent().into_iter().collect();
            readable.extend(launch.setup.library.as_deref());
            crate::sandbox::untrusted_reading(
                &launch.executable,
                &launch.pdf,
                &launch.scratch,
                crate::sandbox::Otherwise::Run,
                &readable,
            )?
        } else {
            crate::sandbox::plain(&launch.executable, &launch.pdf, &launch.scratch)
        };
        let mut command = run.command;
        // Run as it is (no sandbox here), a document whose path is no
        // UTF-8 is named by a link in the session's own folder: an engine
        // (MuPDF) takes its path as text, and refused it.
        #[cfg(unix)]
        let input = if run.isolated || run.input.to_str().is_some() {
            run.input.clone()
        } else {
            let inside = crate::sandbox::inside_name(&run.input);
            let name = Path::new(&inside).file_name().unwrap_or_default();
            let link = run.work.join(name);
            if std::fs::symlink_metadata(&link).is_err() {
                std::os::unix::fs::symlink(&run.input, &link)?;
            }
            link
        };
        #[cfg(not(unix))]
        let input = run.input.clone();
        command.arg(&input).arg(&run.work);
        if let Some(Layout { width, height, em }) = launch.setup.layout {
            command.arg(format!("--layout={width},{height},{em}"));
        }
        if launch.setup.first_page {
            command.arg(skwir_core::document::FIRST_PAGE);
        }
        if let Some(library) = &launch.setup.library {
            command.env("SKWIR_PDFIUM_LIBRARY", library);
        }
        let mut worker = Self {
            channel: LineChannel::spawn(&mut command, MAX_REPLY, "skwir-pdf")?,
            sent: 0,
            due: 0,
        };
        let cancelled = cancel.subscribe();
        let deadline = crossbeam_channel::after(STARTUP_TIMEOUT);
        match worker.wait(0, &cancelled, &deadline) {
            Ok(greeting) => Ok((worker, greeting)),
            Err(Failure::Cancelled) => anyhow::bail!("document open cancelled"),
            // Its library is not there: the engine is not installed, and
            // one chosen automatically gives way to the next.
            Err(Failure::Broken(_)) if worker.channel.missing_runtime() => {
                Err(Unavailable("the engine's library is missing".into()).into())
            }
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
        cancelled: &skwir_core::cancel::CancelSignal,
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
        cancelled: &skwir_core::cancel::CancelSignal,
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
                Setup::default(),
                lifetime.clone(),
                // It reports its process id, which isolation renumbers.
                false,
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

    /// Run as it is, a worker is handed a document whose path is no UTF-8
    /// by a link of a UTF-8 name: `MuPDF` takes its path as text.
    #[test]
    fn a_document_named_in_no_utf8_is_handed_by_a_link() {
        use std::os::unix::ffi::OsStrExt;
        let helpers = tempfile::tempdir().unwrap();
        let documents = tempfile::tempdir().unwrap();
        let source = documents
            .path()
            .join(std::ffi::OsStr::from_bytes(b"caf\xe9 report.pdf"));
        std::fs::write(&source, b"%PDF synthetic").unwrap();
        let executable = helpers.path().join("worker");
        let shell = crate::process::required("sh").unwrap();
        let script = format!(
            concat!(
                "#!{shell}\n",
                "printf '%s' \"$1\" > \"$2/args\"\n",
                "head -c 4 \"$1\" > \"$2/read\"\n",
                "printf '%s\\n' '{{\"id\":0,\"body\":{{\"Ready\":{{\"pages\":1,\"outlines\":[]}}}}}}'\n",
                "while IFS= read -r line; do :; done\n"
            ),
            shell = shell.display(),
        );
        std::fs::write(&executable, script).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let scratch_path = scratch.path().to_owned();
        let session = WorkerSession::open_with_worker(
            executable,
            source.clone(),
            source,
            scratch,
            Setup::default(),
            CancelToken::new(),
            false,
        )
        .unwrap()
        .unwrap();
        let read = |name: &str| std::fs::read_to_string(scratch_path.join(name)).unwrap();
        assert_eq!(
            read("args"),
            scratch_path.join("document.pdf").to_str().unwrap()
        );
        assert_eq!(read("read"), "%PDF");
        drop(session);
    }

    #[test]
    fn a_worker_reads_its_document_isolated_and_nothing_else() {
        if !crate::sandbox::available() {
            eprintln!("bubblewrap cannot isolate here");
            return;
        }
        let helpers = tempfile::tempdir().unwrap();
        let documents = tempfile::tempdir().unwrap();
        let source = documents.path().join("A report’s [draft].pdf");
        std::fs::write(&source, b"%PDF synthetic").unwrap();
        // Beside the document, as anything of the user's is.
        let private = documents.path().join("private.txt");
        std::fs::write(&private, "not for the worker").unwrap();
        let executable = helpers.path().join("worker");
        let shell = crate::process::required("sh").unwrap();
        let script = format!(
            concat!(
                "#!{shell}\n",
                "printf '%s %s' \"$1\" \"$2\" > \"$2/args\"\n",
                "head -c 4 \"$1\" > \"$2/read\"\n",
                "cat '{private}' > \"$2/private\" 2>/dev/null\n",
                "printf '%s\\n' '{{\"id\":0,\"body\":{{\"Ready\":{{\"pages\":1,\"outlines\":[]}}}}}}'\n",
                "while IFS= read -r line; do :; done\n"
            ),
            shell = shell.display(),
            private = private.display(),
        );
        std::fs::write(&executable, script).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let scratch_path = scratch.path().to_owned();
        let session = WorkerSession::open_with_worker(
            executable,
            source.clone(),
            source,
            scratch,
            Setup::default(),
            CancelToken::new(),
            true,
        )
        .unwrap()
        .unwrap();
        assert_eq!(session.info().pages, 1);
        let read = |name: &str| std::fs::read_to_string(scratch_path.join(name)).unwrap();
        assert_eq!(read("args"), "/input/document.pdf /work");
        assert_eq!(read("read"), "%PDF");
        assert_eq!(
            read("private"),
            "",
            "a file beside the document stays out of reach"
        );
        drop(session);
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
