//! The tree-sitter helper's manager (ADR-110): starts
//! `skwir-tree-sitter-worker` isolated, hands it the grammars found and
//! the texts to parse, and answers colors, folds and outlines.
//!
//! One helper serves every caller ([`shared`]), started at the first call
//! and let go of once no text is open in it and nothing was asked of it
//! for [`KEPT`], as bat's grammars are (ADR-060); the next call starts it
//! again. It runs isolated where the system can (`sandbox::fed_reading`),
//! reading the grammars' folders and its own working folder, where each
//! text is written for it to parse: the copy a preview holds (ADR-101),
//! never the user's file read again. A thread owns the helper's process
//! and its folder, so nothing here waits on the UI thread nor removes
//! files there: dropping a [`Doc`] only sends. Calls block, and run on
//! workers; none waits past its own bound.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, OnceLock, Weak};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use skwir_core::CancelToken;
use skwir_core::document::Tagged;
use skwir_core::syntax::{Grammar, MAX_REPLY, MAX_REQUEST, MAX_TEXT, Reply, Request, Span, Symbol};

use crate::process::Failure;
use crate::sandbox::{self, Otherwise};

/// The helper. Plugins that require it name this, not the literals.
pub const WORKER: crate::process::Worker = crate::process::Worker {
    variable: "SKWIR_TREE_SITTER_WORKER",
    executable: "skwir-tree-sitter-worker",
};

/// Where the helper is, if it is.
pub fn worker_path() -> Option<PathBuf> {
    WORKER.find()
}

/// How long the helper is kept after its last answer, no text open in it:
/// the next file looked at finds its grammar loaded.
pub const KEPT: Duration = Duration::from_secs(60);
/// The helper's start, its greeting and the grammars told.
const STARTUP: Duration = Duration::from_secs(15);
/// A text's colors, folds or outline, or forgetting it: kept, not made.
const ANSWER: Duration = Duration::from_secs(15);
/// A grammar loaded and its queries compiled: a WebAssembly grammar is
/// compiled then, a large one in seconds.
const CHECK: Duration = Duration::from_secs(60);
/// A text parsed and colored whole: this, and `OPEN_PER_MIB` for each MiB
/// of it, which an unoptimized build of the helper (the tests') takes a
/// WebAssembly grammar to parse.
const OPEN: Duration = Duration::from_secs(15);
const OPEN_PER_MIB: Duration = Duration::from_secs(6);
/// How long a helper told to stop has to stop: one still busy past it is
/// ended (`docs/invariants/reader.md`).
const CANCEL_GRACE: Duration = Duration::from_secs(5);

/// The helper every caller shares, told no grammar until
/// [`Helper::set_grammars`] is.
pub fn shared() -> &'static Helper {
    static SHARED: LazyLock<Helper> = LazyLock::new(|| Helper::new(None, true, KEPT));
    &SHARED
}

/// A helper of its own running `executable`, isolated as the shared one,
/// told no grammar until `set_grammars` is: tests, and a tool that ships
/// the program elsewhere.
pub fn with_program(executable: PathBuf) -> Helper {
    Helper::new(Some(executable), true, KEPT)
}

/// A text the helper parsed: the handle that names it, how many lines it
/// holds, and the capture names its spans index (`Span::capture`).
pub struct Opened {
    pub doc: Doc,
    pub lines: u32,
    pub captures: Vec<String>,
}

/// A grammar checked: the ABI it was generated for, and what is wrong with
/// it or a query (the query left out, the grammar used without it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checked {
    pub abi: u32,
    pub problems: Vec<String>,
}

/// A text open in the helper. Dropping it forgets it there without
/// waiting; the helper it was opened in lives as long as it does.
pub struct Doc {
    id: u32,
    process: Arc<Process>,
    closed: bool,
}

impl Drop for Doc {
    fn drop(&mut self) {
        if !self.closed {
            self.process.forget(self.id);
        }
    }
}

/// A tree-sitter helper, started when first needed, shared by every
/// caller, let go of when unused.
pub struct Helper {
    inner: Arc<Inner>,
}

struct Inner {
    /// The helper's program; `None` finds it (`WORKER`) when it starts.
    executable: Option<PathBuf>,
    /// Whether it runs isolated where the system can: always, but for
    /// tests of a stand-in that watch its process.
    isolate: bool,
    /// How long it is kept unused (`KEPT`).
    kept: Duration,
    state: Mutex<State>,
    /// Held while a helper starts, so that two calls start one.
    starting: Mutex<()>,
}

/// What a helper is told: the grammars, and the folders their files lie
/// in, which it reads where it runs isolated.
#[derive(Debug, Default, PartialEq, Eq)]
struct Told {
    grammars: Vec<Grammar>,
    folders: Vec<PathBuf>,
}

struct State {
    grammars: Arc<Told>,
    /// The helper calls go to, told `grammars`; `None` once let go of.
    current: Option<Arc<Process>>,
    /// The last started, which texts still open in it keep running after
    /// it was let go of: calls go to it again rather than to another.
    last: Weak<Process>,
    last_use: Instant,
    /// Whether a thread waits to let go of `current`.
    waiting: bool,
}

impl Helper {
    fn new(executable: Option<PathBuf>, isolate: bool, kept: Duration) -> Helper {
        Helper {
            inner: Arc::new(Inner {
                executable,
                isolate,
                kept,
                state: Mutex::new(State {
                    grammars: Arc::new(Told::default()),
                    current: None,
                    last: Weak::new(),
                    last_use: Instant::now(),
                    waiting: false,
                }),
                starting: Mutex::new(()),
            }),
        }
    }

    /// The grammars found, in place of those before, read where their
    /// files are (`set_grammars_reading`): tests, and a tool that names
    /// grammars itself.
    pub fn set_grammars(&self, grammars: Vec<Grammar>) {
        let folders = files_folders(&grammars);
        self.set_grammars_reading(grammars, folders);
    }

    /// The grammars found, in place of those before, and the folders their
    /// files lie in (`grammars::find`), which a helper told them reads
    /// where it runs isolated, and nothing else of the user's. A helper
    /// told others is let go of: texts open in it keep it until they are
    /// dropped, and the next text opened starts one told these.
    pub fn set_grammars_reading(&self, grammars: Vec<Grammar>, folders: Vec<PathBuf>) {
        let told = Told { grammars, folders };
        let mut state = self.inner.state.lock();
        if *state.grammars == told {
            return;
        }
        state.grammars = Arc::new(told);
        let before = state.current.take();
        drop(state);
        drop(before);
    }

    /// Parse `text` with the grammar named `name` (one of those set) and
    /// keep its colors in the helper. `cancel` stops it: the helper is
    /// told, and the call returns at once.
    pub fn open(&self, name: &str, text: &[u8], cancel: &CancelToken) -> anyhow::Result<Opened> {
        anyhow::ensure!(
            text.len() as u64 <= MAX_TEXT,
            "the text exceeds {} MiB",
            MAX_TEXT >> 20
        );
        let process = self.process(cancel)?;
        let id = process.next.fetch_add(1, Ordering::Relaxed);
        let written = Written::new(&process, id, text)?;
        let mib = u32::try_from(text.len().div_ceil(1 << 20)).unwrap_or(u32::MAX);
        let asked = Request::Open {
            doc: id,
            name: name.to_owned(),
            text: written.inside.clone(),
        };
        let answer = process.call(asked, OPEN + OPEN_PER_MIB.saturating_mul(mib), cancel);
        drop(written);
        self.used(&process);
        match answer {
            Ok(Reply::Opened { lines, captures }) => Ok(Opened {
                doc: Doc {
                    id,
                    process,
                    closed: false,
                },
                lines,
                captures,
            }),
            // A parse cancelled may have ended before it heard so: what it
            // kept is forgotten.
            Ok(reply) => {
                process.forget(id);
                Err(unexpected(&reply))
            }
            Err(error) => {
                process.forget(id);
                Err(error)
            }
        }
    }

    /// The spans of `doc`'s `lines` (at most `syntax::MAX_LINES` of
    /// them; those past its last left out), each line's in order.
    pub fn colors(&self, doc: &Doc, lines: Range<u32>) -> anyhow::Result<Vec<Vec<Span>>> {
        let asked = Request::Colors {
            doc: doc.id,
            from: lines.start,
            to: lines.end,
        };
        match self.ask(doc, asked)? {
            Reply::Colors { lines } => Ok(lines),
            reply => Err(unexpected(&reply)),
        }
    }

    /// Where `doc` folds: lines `start..end` a fold hides after `start`,
    /// nested or apart, in order of `start`.
    pub fn folds(&self, doc: &Doc) -> anyhow::Result<Vec<(u32, u32)>> {
        match self.ask(doc, Request::Folds { doc: doc.id })? {
            Reply::Folds(folds) => Ok(folds),
            reply => Err(unexpected(&reply)),
        }
    }

    /// The definitions `doc` holds, in the text's order.
    pub fn outline(&self, doc: &Doc) -> anyhow::Result<Vec<Symbol>> {
        match self.ask(doc, Request::Outline { doc: doc.id })? {
            Reply::Outline(symbols) => Ok(symbols),
            reply => Err(unexpected(&reply)),
        }
    }

    /// Forget `doc` in the helper, waiting for it to have: dropping it
    /// does the same without waiting.
    pub fn close(&self, mut doc: Doc) -> anyhow::Result<()> {
        doc.closed = true;
        match self.ask(&doc, Request::Close { doc: doc.id })? {
            Reply::Done => Ok(()),
            reply => Err(unexpected(&reply)),
        }
    }

    /// Load the grammar named `name` and compile its queries, parsing
    /// nothing: the Reading page's question.
    pub fn check(&self, name: &str, cancel: &CancelToken) -> anyhow::Result<Checked> {
        let process = self.process(cancel)?;
        let answer = process.call(
            Request::Check {
                name: name.to_owned(),
            },
            CHECK,
            cancel,
        );
        self.used(&process);
        match answer? {
            Reply::Checked { abi, problems } => Ok(Checked { abi, problems }),
            reply => Err(unexpected(&reply)),
        }
    }

    /// `asked` of the helper `doc` was opened in.
    fn ask(&self, doc: &Doc, asked: Request) -> anyhow::Result<Reply> {
        let answer = doc.process.call(asked, ANSWER, &CancelToken::new());
        self.used(&doc.process);
        answer
    }

    /// The helper calls go to: the one running, else one started now (a
    /// helper that ended, once per call).
    fn process(&self, cancel: &CancelToken) -> anyhow::Result<Arc<Process>> {
        if let Some(process) = self.current() {
            return Ok(process);
        }
        let _starting = self.inner.starting.lock();
        if let Some(process) = self.current() {
            return Ok(process);
        }
        anyhow::ensure!(!cancel.is_cancelled(), "cancelled");
        let grammars = Arc::clone(&self.inner.state.lock().grammars);
        let process = Process::start(&self.inner, grammars, cancel)?;
        let mut state = self.inner.state.lock();
        // Unless the grammars changed meanwhile: then it serves this call
        // alone.
        let before = if Arc::ptr_eq(&process.grammars, &state.grammars) {
            state.last = Arc::downgrade(&process);
            state.current.replace(Arc::clone(&process))
        } else {
            None
        };
        drop(state);
        drop(before);
        self.used(&process);
        Ok(process)
    }

    /// The helper calls go to, if it has not ended and was told the
    /// grammars set: the one kept, else the last started, if a text open
    /// in it keeps it running.
    fn current(&self) -> Option<Arc<Process>> {
        let state = self.inner.state.lock();
        let process = state.current.clone().or_else(|| state.last.upgrade())?;
        let usable =
            process.ended.get().is_none() && Arc::ptr_eq(&process.grammars, &state.grammars);
        drop(state);
        usable.then_some(process)
    }

    /// `process` answered: kept for `kept` from now, by a thread that lets
    /// go of it then. One let go of while a text kept it running is kept
    /// again.
    fn used(&self, process: &Arc<Process>) {
        let mut state = self.inner.state.lock();
        state.last_use = Instant::now();
        if state.current.is_none()
            && process.ended.get().is_none()
            && Arc::ptr_eq(&process.grammars, &state.grammars)
        {
            state.current = Some(Arc::clone(process));
        }
        if !state.waiting && state.current.is_some() {
            let inner = Arc::clone(&self.inner);
            // A thread lives only while a helper is kept. Without one, it
            // stays until the next call starts it.
            state.waiting = std::thread::Builder::new()
                .name("skwir-tree-sitter-kept".into())
                .spawn(move || let_go_when_unused(&inner))
                .is_ok();
        }
    }
}

/// Wait until the helper has been unused for its `kept`, then let go of
/// it: it ends once no text holds it.
fn let_go_when_unused(inner: &Inner) {
    loop {
        let mut state = inner.state.lock();
        let due = state.last_use + inner.kept;
        let now = Instant::now();
        if due > now {
            drop(state);
            std::thread::sleep(due - now);
            continue;
        }
        let current = state.current.take();
        state.waiting = false;
        drop(state);
        // Its thread ends it, and removes its folder, once nothing holds it.
        drop(current);
        return;
    }
}

fn unexpected(reply: &Reply) -> anyhow::Error {
    anyhow::anyhow!("the tree-sitter helper answered out of turn: {reply:?}")
}

/// A helper running, as calls reach it: its thread, which owns its
/// process and its folder, takes one job at a time. The thread ends the
/// helper, and removes the folder, once nothing holds this.
struct Process {
    jobs: crossbeam_channel::Sender<Job>,
    /// The grammars it was told.
    grammars: Arc<Told>,
    executable: PathBuf,
    /// Its working folder, where skwir writes and where the helper reads.
    outside: PathBuf,
    inside: PathBuf,
    /// The id of the next text opened.
    next: AtomicU32,
    /// Why it ended, once it did: what is asked of it is refused so.
    ended: Arc<OnceLock<String>>,
}

struct Job {
    request: Request,
    timeout: Duration,
    cancel: CancelToken,
    /// Where its answer goes: nowhere for what is not waited for (a text
    /// dropped).
    reply: Option<crossbeam_channel::Sender<Result<Reply, Failure>>>,
}

impl Job {
    fn answer(self, answer: Result<Reply, Failure>) {
        if let Some(reply) = self.reply {
            let _ = reply.send(answer);
        }
    }
}

impl Process {
    /// A helper started, greeted and told `grammars`, and its thread.
    fn start(
        inner: &Inner,
        grammars: Arc<Told>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Arc<Process>> {
        let executable = match &inner.executable {
            Some(executable) => executable.clone(),
            None => WORKER.require("color code with tree-sitter grammars")?,
        };
        let told = Request::Grammars(grammars.grammars.clone());
        anyhow::ensure!(
            skwir_core::jsonl::encode(&Tagged {
                id: u64::MAX,
                body: &told,
            })?
            .len()
                <= MAX_REQUEST,
            "the grammars found are more than the tree-sitter helper is told at once"
        );
        let work = crate::file::memory_dir("skwir-tree-sitter-")?;
        let folders = readable(&grammars.folders);
        let folders: Vec<&Path> = folders.iter().map(PathBuf::as_path).collect();
        // A grammar is code, and a text nobody vouched for may target its
        // scanner: the helper reads the grammars and its own folder.
        let mut run = if inner.isolate {
            sandbox::fed_reading(&executable, work.path(), Otherwise::Run, &folders)?
        } else {
            sandbox::plain(&executable, Path::new(""), work.path())
        };
        let explain = |error| crate::process::explain(&executable, cancel, error);
        let mut helper = crate::process::Helper::new(
            crate::line_channel::LineChannel::spawn(
                &mut run.command,
                MAX_REPLY,
                "skwir-tree-sitter",
            )?,
            "tree-sitter helper",
        );
        let cancelled = cancel.subscribe();
        let deadline = crossbeam_channel::after(STARTUP);
        match helper.wait(0, &cancelled, &deadline) {
            Ok(Reply::Ready { .. }) => {}
            Ok(Reply::Error(error) | Reply::Unavailable(error)) => anyhow::bail!("{error}"),
            Ok(reply) => return Err(unexpected(&reply)),
            Err(Failure::Cancelled) => anyhow::bail!("cancelled"),
            Err(Failure::Broken(error)) => return Err(explain(error)),
        }
        let id = helper.send(&told).map_err(explain)?;
        match helper.wait(id, &cancelled, &deadline) {
            Ok(Reply::Done) => {}
            Ok(Reply::Error(error)) => anyhow::bail!("{error}"),
            Ok(reply) => return Err(unexpected(&reply)),
            Err(Failure::Cancelled) => anyhow::bail!("cancelled"),
            Err(Failure::Broken(error)) => return Err(explain(error)),
        }
        let ended = Arc::new(OnceLock::new());
        let (jobs, asked) = crossbeam_channel::unbounded();
        let outside = work.path().to_owned();
        let said = Arc::clone(&ended);
        std::thread::Builder::new()
            .name("skwir-tree-sitter".into())
            .spawn(move || {
                Manager {
                    helper: Some(helper),
                    ended: said,
                }
                .run(&asked);
                // The helper was ended (killed and reaped) on this thread;
                // only now is its folder removed.
                drop(work);
            })?;
        Ok(Arc::new(Process {
            jobs,
            grammars,
            executable,
            outside,
            inside: run.work,
            next: AtomicU32::new(1),
            ended,
        }))
    }

    /// `request` answered, within `timeout`; `cancel` gives up on it at
    /// once and tells the helper to stop. A request's own failure is its
    /// error; a helper that failed is ended, and says so to what follows.
    fn call(
        &self,
        request: Request,
        timeout: Duration,
        cancel: &CancelToken,
    ) -> anyhow::Result<Reply> {
        if let Some(why) = self.ended.get() {
            anyhow::bail!("the tree-sitter helper ended: {why}");
        }
        anyhow::ensure!(!cancel.is_cancelled(), "cancelled");
        let (reply, answered) = crossbeam_channel::bounded(1);
        let job = Job {
            request,
            timeout,
            cancel: cancel.clone(),
            reply: Some(reply),
        };
        if self.jobs.send(job).is_err() {
            anyhow::bail!("the tree-sitter helper ended");
        }
        let cancelled = cancel.subscribe();
        let answer = crossbeam_channel::select! {
            recv(answered) -> answer => answer.unwrap_or_else(|_| {
                Err(Failure::Broken(anyhow::anyhow!("the tree-sitter helper's thread ended")))
            }),
            recv(cancelled.receiver()) -> _ => Err(Failure::Cancelled),
        };
        match answer {
            Ok(Reply::Error(error) | Reply::Unavailable(error)) => Err(anyhow::anyhow!(error)),
            Ok(reply) => Ok(reply),
            Err(Failure::Cancelled) => anyhow::bail!("cancelled"),
            Err(Failure::Broken(error)) => {
                Err(crate::process::explain(&self.executable, cancel, error))
            }
        }
    }

    /// Forget text `id` in the helper, without waiting for it to.
    fn forget(&self, id: u32) {
        let _ = self.jobs.send(Job {
            request: Request::Close { doc: id },
            timeout: ANSWER,
            cancel: CancelToken::new(),
            reply: None,
        });
    }
}

/// The folders a helper reads of `folders`: those inside another left
/// out, as each is bound with what it holds.
fn readable(folders: &[PathBuf]) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = folders
        .iter()
        .filter(|folder| folder.is_absolute())
        .cloned()
        .collect();
    // A folder sorts before what is inside it.
    folders.sort();
    folders.dedup();
    let mut kept: Vec<PathBuf> = Vec::with_capacity(folders.len());
    for folder in folders {
        if !kept.last().is_some_and(|last| folder.starts_with(last)) {
            kept.push(folder);
        }
    }
    kept
}

/// The folders of `grammars`' files: what a helper told grammars named by
/// hand reads.
fn files_folders(grammars: &[Grammar]) -> Vec<PathBuf> {
    grammars
        .iter()
        .flat_map(Grammar::files)
        .filter_map(|file| file.parent().map(Path::to_owned))
        .collect()
}

/// A text written in a helper's folder for it to read, removed once read.
struct Written {
    outside: PathBuf,
    inside: PathBuf,
}

impl Written {
    fn new(process: &Process, id: u32, text: &[u8]) -> anyhow::Result<Written> {
        let name = format!("text-{id}");
        let outside = process.outside.join(&name);
        std::fs::write(&outside, text)?;
        Ok(Written {
            outside,
            inside: process.inside.join(name),
        })
    }
}

impl Drop for Written {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.outside);
    }
}

/// A helper's thread: the one owner of its process.
struct Manager {
    /// `None` once it ended.
    helper: Option<crate::process::Helper<Reply>>,
    ended: Arc<OnceLock<String>>,
}

impl Manager {
    /// Serve jobs until nothing holds the process. While none runs, the
    /// helper says nothing: what it says then (its exit) ends it.
    fn run(mut self, asked: &crossbeam_channel::Receiver<Job>) {
        loop {
            let Some(replies) = self
                .helper
                .as_ref()
                .map(|helper| helper.channel.replies().clone())
            else {
                let Ok(job) = asked.recv() else { return };
                let why = self.ended.get().cloned().unwrap_or_default();
                job.answer(Err(Failure::Broken(anyhow::anyhow!("{why}"))));
                continue;
            };
            crossbeam_channel::select! {
                recv(asked) -> job => {
                    let Ok(job) = job else { return };
                    self.handle(job);
                }
                recv(replies) -> reply => {
                    let helper = self.helper.as_mut().expect("its replies were read");
                    let why = match helper.accept(reply) {
                        Err(error) => format!("{error:#}"),
                        Ok(_) => "the tree-sitter helper answered what it was not asked".into(),
                    };
                    self.end(why);
                }
            }
        }
    }

    fn handle(&mut self, job: Job) {
        if job.cancel.is_cancelled() {
            job.answer(Err(Failure::Cancelled));
            return;
        }
        let helper = self.helper.as_mut().expect("served while it runs");
        let id = match helper.send(&job.request) {
            Ok(id) => id,
            Err(error) => {
                let why = format!("{error:#}");
                self.end(why.clone());
                job.answer(Err(Failure::Broken(anyhow::anyhow!(why))));
                return;
            }
        };
        let cancelled = job.cancel.subscribe();
        let deadline = crossbeam_channel::after(job.timeout);
        match helper.wait(id, &cancelled, &deadline) {
            Ok(reply) => job.answer(Ok(reply)),
            Err(Failure::Cancelled) => {
                job.answer(Err(Failure::Cancelled));
                // Told to stop, it answers both; one busy past a moment is
                // ended, its texts with it.
                let never = CancelToken::new().subscribe();
                let settled = helper
                    .send(&Request::Cancel)
                    .map_err(Failure::Broken)
                    .and_then(|_| helper.settle(&never, &crossbeam_channel::after(CANCEL_GRACE)));
                if let Err(Failure::Broken(error)) = settled {
                    self.end(format!("{error:#}"));
                }
            }
            Err(Failure::Broken(error)) => {
                let why = format!("{error:#}");
                self.end(why.clone());
                job.answer(Err(Failure::Broken(anyhow::anyhow!(why))));
            }
        }
    }

    /// End the helper (killed and reaped here), saying why to what asks.
    fn end(&mut self, why: String) {
        tracing::debug!("tree-sitter helper ended: {why}");
        let _ = self.ended.set(why);
        self.helper = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The helper built beside the tests (`cargo nextest run -p
    /// skwir-tree-sitter-worker -p skwir-platform` builds it); without it
    /// a test says so and passes, as the other helpers' do.
    #[cfg(target_os = "linux")]
    fn built_helper() -> Option<PathBuf> {
        let helper = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.parent()?.join("skwir-tree-sitter-worker")))
            .filter(|path| path.is_file());
        if helper.is_none() {
            eprintln!("skipped: needs skwir-tree-sitter-worker built beside the tests");
        }
        helper
    }

    /// tree-sitter-json as WebAssembly, the helper's own fixture.
    #[cfg(target_os = "linux")]
    fn json() -> Grammar {
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../tree-sitter-worker/tests/fixtures/json");
        Grammar {
            name: "json".into(),
            library: fixtures.join("tree-sitter-json.wasm"),
            symbol: "tree_sitter_json".into(),
            highlights: vec![fixtures.join("queries/highlights.scm")],
            ..Grammar::default()
        }
    }

    /// The real helper, isolated where the system can, told `json`.
    #[cfg(target_os = "linux")]
    fn helper(kept: Duration) -> Option<Helper> {
        let helper = Helper::new(Some(built_helper()?), true, kept);
        helper.set_grammars(vec![json()]);
        Some(helper)
    }

    #[cfg(target_os = "linux")]
    fn until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !condition() {
            assert!(Instant::now() < deadline, "it never came");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(target_os = "linux")]
    const TEXT: &str = "{\n  \"count\": 42,\n  \"ok\": [true, null]\n}\n";

    /// A text opened in the helper, its grammar's folder read where it runs
    /// isolated, colored line by line, checked, closed.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_text_is_opened_colored_and_closed() {
        let Some(helper) = helper(KEPT) else { return };
        let cancel = CancelToken::new();
        let opened = helper.open("json", TEXT.as_bytes(), &cancel).unwrap();
        if sandbox::available() {
            assert_eq!(opened.doc.process.inside, Path::new("/work"), "isolated");
        }
        assert_eq!(opened.lines, 4);
        let colors = helper.colors(&opened.doc, 0..opened.lines).unwrap();
        assert_eq!(colors.len(), 4);
        let at = |line: usize, text: &str| {
            let start = TEXT.lines().nth(line).unwrap().find(text).unwrap() as u32;
            let span = colors[line]
                .iter()
                .find(|span| span.start <= start && start < span.end)
                .unwrap();
            opened.captures[usize::from(span.capture)].as_str()
        };
        assert_eq!(at(1, "42"), "number");
        assert_eq!(at(2, "true"), "constant.builtin");
        assert_eq!(helper.folds(&opened.doc).unwrap(), []);
        assert_eq!(helper.outline(&opened.doc).unwrap(), []);
        assert_eq!(
            helper.check("json", &cancel).unwrap(),
            Checked {
                abi: 14,
                problems: Vec::new()
            }
        );
        let error = helper.check("yaml", &cancel).unwrap_err().to_string();
        assert!(error.contains("no grammar named yaml"), "{error}");
        // Written for it to read, and removed once read.
        let folder = opened.doc.process.outside.clone();
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
        helper.close(opened.doc).unwrap();
        cancel.cancel();
        assert!(helper.open("json", TEXT.as_bytes(), &cancel).is_err());
    }

    /// Unused for its `kept`, the helper is let go of, but not while a text
    /// is open in it; it ends, and its folder goes, once both hold. The
    /// next call starts another.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_helper_is_let_go_of_when_unused_and_started_again() {
        let kept = Duration::from_millis(300);
        let Some(helper) = helper(kept) else { return };
        let cancel = CancelToken::new();
        let opened = helper.open("json", TEXT.as_bytes(), &cancel).unwrap();
        let folder = opened.doc.process.outside.clone();
        std::thread::sleep(kept * 2);
        assert!(folder.is_dir(), "a text open keeps it");
        assert!(helper.inner.state.lock().current.is_none(), "let go of");
        assert_eq!(helper.colors(&opened.doc, 0..1).unwrap().len(), 1);
        // Asked again, it is the one calls go to.
        let again = helper.open("json", TEXT.as_bytes(), &cancel).unwrap();
        assert!(Arc::ptr_eq(&again.doc.process, &opened.doc.process));
        drop(opened);
        drop(again);
        until(|| !folder.exists());
        let next = helper.open("json", TEXT.as_bytes(), &cancel).unwrap();
        let other = next.doc.process.outside.clone();
        assert_ne!(other, folder, "another helper");
        assert_eq!(helper.colors(&next.doc, 0..4).unwrap().len(), 4);
        drop(next);
        until(|| !other.exists());
    }

    /// A parse cancelled returns at once; the helper stops it and serves
    /// on, not started again.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_cancelled_parse_returns_at_once_and_the_helper_serves_on() {
        let Some(helper) = helper(KEPT) else { return };
        let cancel = CancelToken::new();
        let first = helper.open("json", TEXT.as_bytes(), &cancel).unwrap();
        let mut large = String::from("[\n");
        while large.len() < 4 << 20 {
            large.push_str("  {\"key\": \"value\", \"n\": [1, 2.5, true, null]},\n");
        }
        large.push_str("  {}\n]\n");
        let cancelled = CancelToken::new();
        let later = cancelled.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            later.cancel();
        });
        let started = Instant::now();
        let error = helper
            .open("json", large.as_bytes(), &cancelled)
            .err()
            .unwrap();
        assert_eq!(error.to_string(), "cancelled");
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        let next = helper.open("json", TEXT.as_bytes(), &cancel).unwrap();
        assert!(
            Arc::ptr_eq(&next.doc.process, &first.doc.process),
            "the same helper"
        );
        assert_eq!(helper.colors(&first.doc, 0..4).unwrap().len(), 4);
    }

    /// Every grammar found is told at once, though the list is longer than
    /// a pipe holds (a few hundred KiB for an editor's grammars).
    #[cfg(target_os = "linux")]
    #[test]
    fn many_grammars_are_told_at_once() {
        let Some(executable) = built_helper() else {
            return;
        };
        let helper = Helper::new(Some(executable), true, KEPT);
        let mut grammars: Vec<Grammar> = (0..3000)
            .map(|n| Grammar {
                name: format!("language-{n}"),
                library: format!("/nowhere/grammars/language-{n}.so").into(),
                symbol: format!("tree_sitter_language_{n}"),
                highlights: vec![format!("/nowhere/queries/language-{n}/highlights.scm").into()],
                ..Grammar::default()
            })
            .collect();
        grammars.push(json());
        let told = skwir_core::jsonl::encode(&Request::Grammars(grammars.clone())).unwrap();
        assert!(told.len() > 256 << 10, "{} bytes", told.len());
        helper.set_grammars(grammars);
        let cancel = CancelToken::new();
        let opened = helper.open("json", TEXT.as_bytes(), &cancel).unwrap();
        assert_eq!(opened.lines, 4);
    }

    /// A helper that died, answering or idle, is started again by the
    /// next call, once; what was open in it says it ended.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_helper_that_died_is_started_again() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let stand_in = dir.path().join("helper");
        let shell = crate::process::required("sh").unwrap();
        // Greets, takes its grammars, opens anything; dies on a second
        // Open, and after answering a second Check; tells each start in
        // `started`.
        let script = format!(
            concat!(
                "#!{shell}\n",
                "echo start >> '{dir}/started'\n",
                "printf '%s\\n' '{{\"id\":0,\"body\":{{\"Ready\":{{\"version\":\"0\",\"abi\":[13,15],\"wasm\":false}}}}}}'\n",
                "opened=0\n",
                "checked=0\n",
                "while IFS= read -r line; do\n",
                " id=${{line#*\\\"id\\\":}}; id=${{id%%,*}}\n",
                " case \"$line\" in\n",
                " *Open*) [ $opened = 1 ] && exit 3; opened=1;",
                " printf '{{\"id\":%s,\"body\":{{\"Opened\":{{\"lines\":1,\"captures\":[]}}}}}}\\n' \"$id\";;\n",
                " *Check*) printf '{{\"id\":%s,\"body\":{{\"Checked\":{{\"abi\":14,\"problems\":[]}}}}}}\\n' \"$id\";",
                " [ $checked = 1 ] && exit 0; checked=1;;\n",
                " *) printf '{{\"id\":%s,\"body\":\"Done\"}}\\n' \"$id\";;\n",
                " esac\n",
                "done\n"
            ),
            shell = shell.display(),
            dir = dir.path().display(),
        );
        std::fs::write(&stand_in, script).unwrap();
        std::fs::set_permissions(&stand_in, std::fs::Permissions::from_mode(0o700)).unwrap();
        let helper = Helper::new(Some(stand_in), false, KEPT);
        helper.set_grammars(vec![Grammar {
            name: "x".into(),
            ..Grammar::default()
        }]);
        let cancel = CancelToken::new();
        let starts = || {
            std::fs::read_to_string(dir.path().join("started"))
                .unwrap_or_default()
                .lines()
                .count()
        };
        let opened = helper.open("x", b"text", &cancel).unwrap();
        let died = helper
            .open("x", b"text", &cancel)
            .err()
            .unwrap()
            .to_string();
        assert!(died.contains("exited"), "{died}");
        assert_eq!(starts(), 1);
        let ended = helper.colors(&opened.doc, 0..1).unwrap_err().to_string();
        assert!(ended.contains("ended"), "{ended}");
        assert_eq!(helper.check("x", &cancel).unwrap().abi, 14);
        assert_eq!(starts(), 2, "started again, once");
        // Answered, then gone while nothing is asked: the next call does
        // not find it there.
        assert_eq!(helper.check("x", &cancel).unwrap().abi, 14);
        until(|| helper.current().is_none());
        assert_eq!(helper.check("x", &cancel).unwrap().abi, 14);
        assert_eq!(starts(), 3);
    }

    #[test]
    fn the_folders_read_are_those_told_or_of_the_grammars_files() {
        let grammar = |library: &str, highlights: &[&str]| Grammar {
            library: library.into(),
            highlights: highlights.iter().map(PathBuf::from).collect(),
            ..Grammar::default()
        };
        assert_eq!(
            readable(&files_folders(&[
                grammar("/g/parser/rust.so", &["/g/queries/rust/highlights.scm"]),
                grammar(
                    "/g/parser/c.so",
                    &["/g/queries/c/highlights.scm", "/q/c.scm"]
                ),
                grammar(
                    "/g/parser/x.so",
                    &["/g/parser/x/highlights.scm", "relative.scm"]
                ),
                grammar("/h/grammars/json.wasm", &["/h/grammars/queries/json.scm"]),
                grammar("/g/parsers/y.so", &[]),
            ])),
            [
                "/g/parser",
                "/g/parsers",
                "/g/queries/c",
                "/g/queries/rust",
                "/h/grammars",
                "/q"
            ]
            .map(PathBuf::from)
        );
        // Folders told are read as told, those inside another left out.
        assert_eq!(
            readable(&["/g/b", "/g", "/h/x", "relative"].map(PathBuf::from)),
            ["/g", "/h/x"].map(PathBuf::from)
        );
    }
}
