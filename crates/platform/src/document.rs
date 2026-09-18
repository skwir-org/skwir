//! On-demand `PDFium` sessions. A manager thread owns every child and temporary
//! file, so closing a preview never waits or removes files on the UI thread.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant, SystemTime};

use anyhow::Context;
use bibli_core::CancelToken;
use bibli_core::document::{
    DocumentInfo, DocumentPage, DocumentSession, MAX_REPLY, MAX_SIDE, PIXELS_FILE, SearchHit,
    WorkerReply, WorkerRequest,
};

use crate::process::ChildGuard;

struct Job {
    request: WorkerRequest,
    cancel: CancelToken,
    reply: mpsc::Sender<anyhow::Result<Response>>,
}
enum Response {
    Page(DocumentPage),
    Matches(Vec<SearchHit>),
}

pub struct PdfiumSession {
    info: DocumentInfo,
    jobs: mpsc::Sender<Job>,
}

pub(crate) fn worker_path() -> Option<PathBuf> {
    std::env::var_os("BIBLI_PDF_WORKER")
        .map(PathBuf::from)
        .or_else(|| {
            let sibling = std::env::current_exe()
                .ok()?
                .parent()?
                .join("bibli-pdf-worker");
            sibling.is_file().then_some(sibling)
        })
        .or_else(|| crate::process::find("bibli-pdf-worker"))
}

impl PdfiumSession {
    /// Called on a scheduler worker. None means the optional worker/library is
    /// absent; malformed or encrypted documents produce an ordinary error.
    pub fn open(
        pdf: PathBuf,
        source: PathBuf,
        scratch: tempfile::TempDir,
        library: Option<PathBuf>,
        lifetime: CancelToken,
    ) -> anyhow::Result<Option<Arc<dyn DocumentSession>>> {
        let Some(executable) = worker_path() else {
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
        let (jobs, rx) = mpsc::channel::<Job>();
        let (ready, startup) = mpsc::channel();
        let cancel = lifetime.clone();
        std::thread::Builder::new()
            .name("bibli-pdf-session".into())
            .spawn(move || {
                let setup = || -> anyhow::Result<(Worker, DocumentInfo, FileVersion)> {
                    let version = file_version(&source)?;
                    let (worker, reply) = Worker::start(
                        &executable,
                        &pdf,
                        scratch.path(),
                        library.as_deref(),
                        &lifetime,
                    )?;
                    match reply {
                        WorkerReply::Ready(info) if info.pages > 0 => Ok((worker, info, version)),
                        WorkerReply::Unavailable(message) => {
                            anyhow::bail!("unavailable: {message}");
                        }
                        WorkerReply::Error(message) => anyhow::bail!("{message}"),
                        _ => anyhow::bail!("invalid document worker greeting"),
                    }
                };
                let (worker, info, version) = match setup() {
                    Ok(value) => value,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                if ready.send(Ok(info)).is_err() {
                    return;
                }
                let mut worker = Some(worker);
                while !lifetime.is_cancelled() {
                    let job = match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(job) => job,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let cancel = job.cancel.linked(&lifetime);
                    if cancel.is_cancelled() {
                        let _ = job
                            .reply
                            .send(Err(anyhow::anyhow!("document request cancelled")));
                        continue;
                    }
                    let result = (|| {
                        anyhow::ensure!(
                            file_version(&source)? == version,
                            "document changed; reopen the preview"
                        );
                        if worker.is_none() {
                            let (replacement, greeting) = Worker::start(
                                &executable,
                                &pdf,
                                scratch.path(),
                                library.as_deref(),
                                &cancel,
                            )?;
                            anyhow::ensure!(
                                matches!(greeting, WorkerReply::Ready(_)),
                                "could not reopen document worker"
                            );
                            worker = Some(replacement);
                        }
                        let reply = worker
                            .as_mut()
                            .context("document worker unavailable")?
                            .request(&job.request, &cancel)?;
                        anyhow::ensure!(
                            file_version(&source)? == version,
                            "document changed; reopen the preview"
                        );
                        match reply {
                            WorkerReply::Page {
                                content,
                                width,
                                height,
                            } => {
                                anyhow::ensure!(
                                    width > 0
                                        && height > 0
                                        && width <= MAX_SIDE
                                        && height <= MAX_SIDE,
                                    "invalid document bitmap size"
                                );
                                let bytes = u64::from(width) * u64::from(height) * 4;
                                let path = scratch.path().join(PIXELS_FILE);
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
                            WorkerReply::Error(message) | WorkerReply::Unavailable(message) => {
                                anyhow::bail!("{message}")
                            }
                            WorkerReply::Ready(_) => anyhow::bail!("unexpected document greeting"),
                        }
                    })();
                    if result.is_err() {
                        worker = None;
                    }
                    let _ = job.reply.send(result);
                }
                // Kill/reap happens on this thread before scratch is removed.
                drop(worker);
                drop(scratch);
            })?;
        let info = loop {
            anyhow::ensure!(!cancel.is_cancelled(), "document open cancelled");
            match startup.recv_timeout(Duration::from_millis(10)) {
                Ok(Ok(info)) => break info,
                Ok(Err(error)) if error.to_string().starts_with("unavailable:") => return Ok(None),
                Ok(Err(error)) => return Err(error),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("document worker stopped during startup")
                }
            }
        };
        Ok(Some(Arc::new(Self { info, jobs })))
    }

    fn request(&self, request: WorkerRequest, cancel: &CancelToken) -> anyhow::Result<Response> {
        anyhow::ensure!(!cancel.is_cancelled(), "document request cancelled");
        let (reply, rx) = mpsc::channel();
        self.jobs
            .send(Job {
                request,
                cancel: cancel.clone(),
                reply,
            })
            .context("document session closed")?;
        loop {
            anyhow::ensure!(!cancel.is_cancelled(), "document request cancelled");
            match rx.recv_timeout(Duration::from_millis(10)) {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("document session closed")
                }
            }
        }
    }
}

impl DocumentSession for PdfiumSession {
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

type FileVersion = (u64, SystemTime, (i64, i64));
fn file_version(path: &Path) -> anyhow::Result<FileVersion> {
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

struct Worker {
    _child: ChildGuard,
    input: ChildStdin,
    replies: mpsc::Receiver<anyhow::Result<WorkerReply>>,
    errors: std::fs::File,
}
impl Worker {
    fn start(
        executable: &Path,
        pdf: &Path,
        scratch: &Path,
        library: Option<&Path>,
        cancel: &CancelToken,
    ) -> anyhow::Result<(Self, WorkerReply)> {
        anyhow::ensure!(!cancel.is_cancelled(), "document open cancelled");
        let errors = tempfile::tempfile()?;
        let mut command = Command::new(executable);
        command
            .arg(pdf)
            .arg(scratch)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(errors.try_clone()?);
        if let Some(library) = library {
            command.env("BIBLI_PDFIUM_LIBRARY", library);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = ChildGuard(command.spawn().context("start PDFium worker")?);
        let input = child.0.stdin.take().context("worker stdin")?;
        let output = child.0.stdout.take().context("worker stdout")?;
        let (tx, replies) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("bibli-pdf-replies".into())
            .spawn(move || {
                let mut reader = BufReader::new(output);
                loop {
                    let mut bytes = Vec::new();
                    let read = Read::by_ref(&mut reader)
                        .take((MAX_REPLY + 1) as u64)
                        .read_until(b'\n', &mut bytes);
                    if matches!(read, Ok(0)) {
                        break;
                    }
                    let result = (|| {
                        read?;
                        anyhow::ensure!(
                            bytes.len() <= MAX_REPLY && bytes.last() == Some(&b'\n'),
                            "document reply exceeds limit"
                        );
                        Ok(serde_json::from_slice(&bytes)?)
                    })();
                    let failed = result.is_err();
                    if tx.send(result).is_err() || failed {
                        break;
                    }
                }
            })?;
        let worker = Self {
            _child: child,
            input,
            replies,
            errors,
        };
        let reply = worker.receive(cancel, Duration::from_secs(15))?;
        Ok((worker, reply))
    }
    fn request(
        &mut self,
        request: &WorkerRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<WorkerReply> {
        serde_json::to_writer(&mut self.input, request)?;
        self.input.write_all(b"\n")?;
        self.input.flush()?;
        self.receive(cancel, Duration::from_secs(30))
    }
    fn receive(&self, cancel: &CancelToken, timeout: Duration) -> anyhow::Result<WorkerReply> {
        let start = Instant::now();
        loop {
            anyhow::ensure!(!cancel.is_cancelled(), "document request cancelled");
            anyhow::ensure!(start.elapsed() < timeout, "document worker timed out");
            anyhow::ensure!(
                self.errors.metadata()?.len() <= 1024 * 1024,
                "document worker excessive diagnostics"
            );
            match self.replies.recv_timeout(Duration::from_millis(10)) {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("PDFium worker exited unexpectedly")
                }
            }
        }
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
            let script = format!(
                "#!{}\nprintf '%s' \"$$\" > \"$2/pid\"\nprintf '%s\\n' '{{\"Ready\":{{\"pages\":3,\"outlines\":[]}}}}'\nwhile IFS= read -r line; do\n case \"$line\" in\n *hang*) touch \"$2/requested\"; sleep 30;;\n *) printf '%s\\n' '{{\"Matches\":[]}}';;\n esac\ndone\n",
                shell.display()
            );
            std::fs::write(&executable, script).unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            let scratch = tempfile::tempdir().unwrap();
            let scratch_path = scratch.path().to_owned();
            let lifetime = CancelToken::new();
            let session = PdfiumSession::open_with_worker(
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

    #[test]
    fn cancelled_search_reaps_worker_and_next_request_reopens_document() {
        let fixture = Fixture::new();
        let original_pid = fixture.pid();
        let session = fixture.session.as_ref().unwrap().clone();
        let token = CancelToken::new();
        let cancelled = token.clone();
        let request = std::thread::spawn(move || session.search("hang", &cancelled));
        wait_until(|| fixture.scratch.join("requested").exists());
        token.cancel();
        assert!(
            request
                .join()
                .unwrap()
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
        wait_until(|| !Path::new(&format!("/proc/{original_pid}")).exists());
        assert!(
            fixture
                .session
                .as_ref()
                .unwrap()
                .search("needle", &CancelToken::new())
                .unwrap()
                .is_empty()
        );
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
