//! The tree-sitter helper's manager (ADR-110): starts
//! `skwir-tree-sitter-worker` isolated, hands it the grammars found and
//! the texts to parse, and answers colors, folds and outlines.
//!
//! One helper serves every caller ([`shared`]), started at the first call
//! and let go of once nothing was asked of it for [`KEPT`], as bat's
//! grammars are (ADR-060); the next call starts it again. A text open in
//! it ([`Doc`]) keeps its text, not the helper: asked again once the
//! helper was let go of, it is opened again in the next from that text.
//! It runs isolated where the system can (`sandbox::fed_reading`),
//! reading the grammars' folders and its own working folder, where each
//! text is written for it to parse: the copy a preview holds (ADR-101),
//! never the user's file read again. A thread owns the helper's process
//! and its folder, so nothing here waits on the UI thread nor removes
//! files there: dropping a [`Doc`] only sends.
//!
//! Calls block, and run on workers: each waits its own bound from when it
//! is asked, and its caller's token gives up on it at once. One parse runs
//! at a time, the others waiting their turn here; what reads what the
//! helper keeps is sent at once and answered beside a parse that runs.

use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, OnceLock, Weak};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use skwir_core::CancelToken;
use skwir_core::document::Tagged;
use skwir_core::syntax::{Grammar, MAX_REPLY, MAX_REQUEST, MAX_TEXT, Reply, Request, Span, Symbol};

use crate::line_channel::LineChannel;
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

/// How long the helper is kept after its last answer: the next file
/// looked at finds its grammar loaded.
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

/// Where every helper keeps the WebAssembly grammars it compiled, once
/// set (`keep_compiled_in`).
static COMPILED: OnceLock<PathBuf> = OnceLock::new();

/// Keep the WebAssembly grammars helpers compile in `folder` (a folder of
/// skwir's cache: `app.cache.namespace("wasmtime")`), so each compiles once
/// rather than at each start of a helper. Wasmtime bounds it; deleting it
/// costs a compile. Without it, nothing is kept. Set once, at start.
pub fn keep_compiled_in(folder: PathBuf) {
    let _ = COMPILED.set(folder);
}

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

/// A text to parse: bytes from `start` of what a preview holds, shared
/// with it (`Text::shared`), or a copy (`From<&[u8]>`, for a short text).
/// Its `Doc` keeps it, to open it again in a helper started later.
#[derive(Clone)]
pub struct Text {
    bytes: Arc<[u8]>,
    start: usize,
}

impl Text {
    /// The bytes of `bytes` from `start` (its end past it), shared.
    pub fn shared(bytes: Arc<[u8]>, start: usize) -> Text {
        let start = start.min(bytes.len());
        Text { bytes, start }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[self.start..]
    }
}

impl From<&[u8]> for Text {
    fn from(bytes: &[u8]) -> Self {
        Text::shared(bytes.into(), 0)
    }
}

impl<const N: usize> From<&[u8; N]> for Text {
    fn from(bytes: &[u8; N]) -> Self {
        Text::from(&bytes[..])
    }
}

impl From<&str> for Text {
    fn from(text: &str) -> Self {
        Text::from(text.as_bytes())
    }
}

impl From<Arc<[u8]>> for Text {
    fn from(bytes: Arc<[u8]>) -> Self {
        Text::shared(bytes, 0)
    }
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

/// A text open in a helper, and what opens it again: its grammar's name,
/// its text and the captures its spans index. Dropping it forgets it in
/// the helper without waiting. It keeps no helper running: one let go of
/// (unused for `KEPT`) or ended, the next question opens the text again
/// in the helper then current.
pub struct Doc {
    name: String,
    text: Text,
    captures: Vec<String>,
    place: Mutex<Place>,
}

/// Where a text is open: the helper, and its id there.
struct Place {
    process: Weak<Process>,
    id: u32,
    closed: bool,
}

impl Drop for Doc {
    fn drop(&mut self) {
        let place = self.place.get_mut();
        if !place.closed
            && let Some(process) = place.process.upgrade()
        {
            process.forget(place.id);
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
    /// The last started, which a call that still runs in it keeps running
    /// after it was let go of: calls go to it again rather than to another.
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
    /// told others is let go of, and the next call starts one told these.
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
    /// told, and the call returns at once. A grammar that colors nothing
    /// (none of its highlights queries compiles) fails, for bat's grammar
    /// to color the text.
    pub fn open(
        &self,
        name: &str,
        text: impl Into<Text>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Opened> {
        let text = text.into();
        let (process, id, lines, captures) = self.open_in(name, &text, cancel)?;
        Ok(Opened {
            doc: Doc {
                name: name.to_owned(),
                text,
                captures: captures.clone(),
                place: Mutex::new(Place {
                    process: Arc::downgrade(&process),
                    id,
                    closed: false,
                }),
            },
            lines,
            captures,
        })
    }

    /// `text` opened in the helper calls go to: that helper, the text's id
    /// there, its lines and its captures.
    fn open_in(
        &self,
        name: &str,
        text: &Text,
        cancel: &CancelToken,
    ) -> anyhow::Result<(Arc<Process>, u32, u32, Vec<String>)> {
        let bytes = text.as_bytes();
        anyhow::ensure!(
            bytes.len() as u64 <= MAX_TEXT,
            "the text exceeds {} MiB",
            MAX_TEXT >> 20
        );
        let process = self.process(cancel)?;
        let id = process.next.fetch_add(1, Ordering::Relaxed);
        let written = Written::new(&process, id, bytes)?;
        let mib = u32::try_from(bytes.len().div_ceil(1 << 20)).unwrap_or(u32::MAX);
        let asked = Request::Open {
            doc: id,
            name: name.to_owned(),
            text: written.inside.clone(),
        };
        let answer = process.call(asked, OPEN + OPEN_PER_MIB.saturating_mul(mib), cancel);
        drop(written);
        self.used(&process);
        match answer {
            Ok(Reply::Opened { lines, captures }) => Ok((process, id, lines, captures)),
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

    /// The spans of `doc`'s `lines`, each line's in order; those past its
    /// last left out.
    pub fn colors(
        &self,
        doc: &Doc,
        lines: Range<u32>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Vec<Vec<Span>>> {
        let mut spans = Vec::with_capacity(lines.len().min(1 << 16));
        let mut from = lines.start;
        while from < lines.end {
            let to = from
                .saturating_add(skwir_core::syntax::MAX_LINES)
                .min(lines.end);
            let reply = self.ask(doc, |id| Request::Colors { doc: id, from, to }, cancel)?;
            let Reply::Colors { lines } = reply else {
                return Err(unexpected(&reply));
            };
            // A reply holds `SPANS` spans at most: the rest is asked again.
            if lines.is_empty() {
                break;
            }
            from = from.saturating_add(u32::try_from(lines.len()).unwrap_or(u32::MAX));
            spans.extend(lines);
        }
        Ok(spans)
    }

    /// Where `doc` folds: lines `start..end` a fold hides after `start`,
    /// nested or apart, in order of `start`.
    pub fn folds(&self, doc: &Doc, cancel: &CancelToken) -> anyhow::Result<Vec<(u32, u32)>> {
        match self.ask(doc, |id| Request::Folds { doc: id }, cancel)? {
            Reply::Folds(folds) => Ok(folds),
            reply => Err(unexpected(&reply)),
        }
    }

    /// The definitions `doc` holds, in the text's order.
    pub fn outline(&self, doc: &Doc, cancel: &CancelToken) -> anyhow::Result<Vec<Symbol>> {
        match self.ask(doc, |id| Request::Outline { doc: id }, cancel)? {
            Reply::Outline(symbols) => Ok(symbols),
            reply => Err(unexpected(&reply)),
        }
    }

    /// Forget `doc` in the helper, waiting for it to have: dropping it
    /// does the same without waiting.
    pub fn close(&self, doc: Doc) -> anyhow::Result<()> {
        let (process, id) = {
            let mut place = doc.place.lock();
            place.closed = true;
            (place.process.upgrade(), place.id)
        };
        drop(doc);
        let Some(process) = process else {
            return Ok(());
        };
        match process.call(Request::Close { doc: id }, ANSWER, &CancelToken::new())? {
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

    /// `asked` (made with `doc`'s id) of the helper `doc` is open in,
    /// opened again first in the helper calls go to when that one was let
    /// go of or ended, and once more when it ends meanwhile.
    fn ask(
        &self,
        doc: &Doc,
        asked: impl Fn(u32) -> Request,
        cancel: &CancelToken,
    ) -> anyhow::Result<Reply> {
        let mut tries = 0;
        loop {
            tries += 1;
            let (process, id) = self.placed(doc, cancel)?;
            let answer = process.call(asked(id), ANSWER, cancel);
            self.used(&process);
            match answer {
                Err(_) if tries < 2 && process.ended.get().is_some() && !cancel.is_cancelled() => {}
                answer => return answer,
            }
        }
    }

    /// The helper `doc` is open in and its id there: where it is, if that
    /// helper runs; else opened again from its text in the helper calls
    /// go to, which must color it with the same captures (the grammars it
    /// was colored with).
    fn placed(&self, doc: &Doc, cancel: &CancelToken) -> anyhow::Result<(Arc<Process>, u32)> {
        let mut place = doc.place.lock();
        anyhow::ensure!(!place.closed, "the text was closed");
        if let Some(process) = place.process.upgrade()
            && process.ended.get().is_none()
        {
            return Ok((process, place.id));
        }
        let (process, id, _, captures) = self.open_in(&doc.name, &doc.text, cancel)?;
        if captures != doc.captures {
            process.forget(id);
            anyhow::bail!("the grammars changed since the text was colored");
        }
        place.process = Arc::downgrade(&process);
        place.id = id;
        Ok((process, id))
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
    /// grammars set: the one kept, else the last started, if a call that
    /// still runs keeps it.
    fn current(&self) -> Option<Arc<Process>> {
        let state = self.inner.state.lock();
        let process = state.current.clone().or_else(|| state.last.upgrade())?;
        let usable =
            process.ended.get().is_none() && Arc::ptr_eq(&process.grammars, &state.grammars);
        drop(state);
        usable.then_some(process)
    }

    /// `process` answered: kept for `kept` from now, by a thread that lets
    /// go of it then. One let go of while a call kept it running is kept
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
/// it: it ends once no call holds it.
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
/// process and its folder, sends what is asked and hands each reply to
/// its caller. The thread ends the helper, and removes the folder, once
/// nothing holds this.
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
    /// The key of the next call, which a caller giving up names.
    keys: AtomicU64,
    /// Why it ended, once it did: what is asked of it is refused so.
    ended: Arc<OnceLock<String>>,
}

/// Why a call's answer did not come.
enum Failure {
    Cancelled,
    /// The helper cannot be trusted with another request: it timed out,
    /// exited or broke the protocol.
    Broken(anyhow::Error),
}

/// What a helper's thread is handed.
enum Job {
    Ask(Asked),
    /// The caller of call `key` gave up on it (cancelled, or past its
    /// bound): one waiting is dropped, a parse running is told to stop.
    GiveUp(u64),
}

struct Asked {
    key: u64,
    request: Request,
    /// How long the helper may take once it was sent: past it, it is
    /// wedged and ended.
    timeout: Duration,
    cancel: CancelToken,
    /// Where its answer goes: nowhere for what is not waited for (a text
    /// dropped).
    reply: Option<crossbeam_channel::Sender<Result<Reply, Failure>>>,
}

impl Asked {
    fn answer(self, answer: Result<Reply, Failure>) {
        if let Some(reply) = self.reply {
            let _ = reply.send(answer);
        }
    }

    /// Whether it parses or changes what is kept: one at a time, in the
    /// order asked. What reads what is kept goes at once.
    fn parses(&self) -> bool {
        matches!(
            self.request,
            Request::Grammars(_)
                | Request::Check { .. }
                | Request::Open { .. }
                | Request::Close { .. }
        )
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
        // Where compiled WebAssembly grammars are kept, the one folder of
        // the user's it writes in; none where none was given (tests).
        let compiled = COMPILED
            .get()
            .filter(|folder| folder.is_absolute() && std::fs::create_dir_all(folder).is_ok());
        let writable: Vec<&Path> = compiled.iter().map(|folder| folder.as_path()).collect();
        let mut run = if inner.isolate {
            sandbox::fed_reading_writing(
                &executable,
                work.path(),
                Otherwise::Run,
                &folders,
                &writable,
            )?
        } else {
            sandbox::plain(&executable, Path::new(""), work.path())
        };
        if let Some(folder) = compiled {
            run.command.arg("--cache").arg(folder);
        }
        let explain = |error| crate::process::explain(&executable, cancel, error);
        let channel: LineChannel<Tagged<Reply>> =
            LineChannel::spawn(&mut run.command, MAX_REPLY, "skwir-tree-sitter")?;
        let cancelled = cancel.subscribe();
        let deadline = crossbeam_channel::after(STARTUP);
        let said = |id: u64| -> anyhow::Result<Reply> {
            crossbeam_channel::select! {
                recv(channel.replies()) -> reply => {
                    let reply = reply
                        .map_err(|_| anyhow::anyhow!("the tree-sitter helper exited unexpectedly"))?
                        .map_err(explain)?;
                    anyhow::ensure!(reply.id == id, "the tree-sitter helper answered out of order");
                    Ok(reply.body)
                }
                recv(cancelled.receiver()) -> _ => anyhow::bail!("cancelled"),
                recv(deadline) -> _ => Err(explain(anyhow::anyhow!("the tree-sitter helper did not start in {STARTUP:?}"))),
            }
        };
        match said(0)? {
            Reply::Ready { .. } => {}
            Reply::Error(error) | Reply::Unavailable(error) => anyhow::bail!("{error}"),
            reply => return Err(unexpected(&reply)),
        }
        let mut channel = channel;
        channel
            .send(&Tagged { id: 1, body: &told })
            .map_err(|error| crate::process::explain(&executable, cancel, error))?;
        let said = |id: u64| -> anyhow::Result<Reply> {
            crossbeam_channel::select! {
                recv(channel.replies()) -> reply => {
                    let reply = reply
                        .map_err(|_| anyhow::anyhow!("the tree-sitter helper exited unexpectedly"))?
                        .map_err(|error| crate::process::explain(&executable, cancel, error))?;
                    anyhow::ensure!(reply.id == id, "the tree-sitter helper answered out of order");
                    Ok(reply.body)
                }
                recv(cancelled.receiver()) -> _ => anyhow::bail!("cancelled"),
                recv(deadline) -> _ => Err(crate::process::explain(&executable, cancel, anyhow::anyhow!("the tree-sitter helper did not start in {STARTUP:?}"))),
            }
        };
        match said(1)? {
            Reply::Done => {}
            Reply::Error(error) => anyhow::bail!("{error}"),
            reply => return Err(unexpected(&reply)),
        }
        let ended = Arc::new(OnceLock::new());
        let (jobs, asked) = crossbeam_channel::unbounded();
        let outside = work.path().to_owned();
        let said = Arc::clone(&ended);
        std::thread::Builder::new()
            .name("skwir-tree-sitter".into())
            .spawn(move || {
                Manager {
                    channel: Some(channel),
                    ended: said,
                    sent: 1,
                    flying: HashMap::new(),
                    parsing: None,
                    waiting: VecDeque::new(),
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
            keys: AtomicU64::new(1),
            ended,
        }))
    }

    /// `request` answered, within `timeout` from now; `cancel` gives up on
    /// it at once, as does its bound, and a parse given up on is told to
    /// stop. A request's own failure is its error; a helper that failed
    /// is ended, and says so to what follows.
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
        let key = self.keys.fetch_add(1, Ordering::Relaxed);
        let (reply, answered) = crossbeam_channel::bounded(1);
        let asked = Asked {
            key,
            request,
            timeout,
            cancel: cancel.clone(),
            reply: Some(reply),
        };
        if self.jobs.send(Job::Ask(asked)).is_err() {
            anyhow::bail!("the tree-sitter helper ended");
        }
        let cancelled = cancel.subscribe();
        let answer = crossbeam_channel::select! {
            recv(answered) -> answer => answer.unwrap_or_else(|_| {
                Err(Failure::Broken(anyhow::anyhow!("the tree-sitter helper's thread ended")))
            }),
            recv(cancelled.receiver()) -> _ => Err(Failure::Cancelled),
            recv(crossbeam_channel::after(timeout)) -> _ => {
                let _ = self.jobs.send(Job::GiveUp(key));
                anyhow::bail!("the tree-sitter helper did not answer in {timeout:?}");
            }
        };
        match answer {
            Ok(Reply::Error(error) | Reply::Unavailable(error)) => Err(anyhow::anyhow!(error)),
            Ok(reply) => Ok(reply),
            Err(Failure::Cancelled) => {
                let _ = self.jobs.send(Job::GiveUp(key));
                anyhow::bail!("cancelled")
            }
            Err(Failure::Broken(error)) => {
                Err(crate::process::explain(&self.executable, cancel, error))
            }
        }
    }

    /// Forget text `id` in the helper, without waiting for it to.
    fn forget(&self, id: u32) {
        let _ = self.jobs.send(Job::Ask(Asked {
            key: self.keys.fetch_add(1, Ordering::Relaxed),
            request: Request::Close { doc: id },
            timeout: ANSWER,
            cancel: CancelToken::new(),
            reply: None,
        }));
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
    channel: Option<LineChannel<Tagged<Reply>>>,
    ended: Arc<OnceLock<String>>,
    /// The id of the last request sent.
    sent: u64,
    /// What was sent and not answered, by id.
    flying: HashMap<u64, Flying>,
    /// The id of the parse sent and not answered: one at a time.
    parsing: Option<u64>,
    /// The parses asked while one runs, in order.
    waiting: VecDeque<Asked>,
}

/// A request sent: who asked, and when the helper is wedged.
struct Flying {
    asked: Asked,
    due: Instant,
    /// Whether its caller gave up: its answer goes nowhere.
    given_up: bool,
}

impl Manager {
    /// Serve jobs until nothing holds the process. What the helper says
    /// that nothing asked (its exit) ends it.
    fn run(mut self, jobs: &crossbeam_channel::Receiver<Job>) {
        loop {
            let Some(replies) = self.channel.as_ref().map(|c| c.replies().clone()) else {
                let Ok(job) = jobs.recv() else { return };
                if let Job::Ask(asked) = job {
                    let why = self.ended.get().cloned().unwrap_or_default();
                    asked.answer(Err(Failure::Broken(anyhow::anyhow!("{why}"))));
                }
                continue;
            };
            let due = self
                .flying
                .values()
                .map(|flying| flying.due)
                .min()
                .map_or_else(crossbeam_channel::never, crossbeam_channel::at);
            crossbeam_channel::select! {
                recv(jobs) -> job => match job {
                    Ok(Job::Ask(asked)) => self.ask(asked),
                    Ok(Job::GiveUp(key)) => self.give_up(key),
                    Err(_) => return,
                },
                recv(replies) -> reply => self.accept(reply),
                recv(due) -> _ => self.end("the tree-sitter helper timed out"),
            }
        }
    }

    fn ask(&mut self, asked: Asked) {
        if asked.cancel.is_cancelled() {
            asked.answer(Err(Failure::Cancelled));
        } else if asked.parses() && self.parsing.is_some() {
            self.waiting.push_back(asked);
        } else {
            self.send(asked);
        }
    }

    /// Send `asked`, a parse once none runs.
    fn send(&mut self, asked: Asked) {
        let Some(channel) = self.channel.as_mut() else {
            let why = self.ended.get().cloned().unwrap_or_default();
            asked.answer(Err(Failure::Broken(anyhow::anyhow!("{why}"))));
            return;
        };
        let id = self.sent + 1;
        if let Err(error) = channel.send(&Tagged {
            id,
            body: &asked.request,
        }) {
            let why = format!("{error:#}");
            asked.answer(Err(Failure::Broken(anyhow::anyhow!("{why}"))));
            self.end(&why);
            return;
        }
        self.sent = id;
        if asked.parses() {
            self.parsing = Some(id);
        }
        let due = Instant::now() + asked.timeout;
        self.flying.insert(
            id,
            Flying {
                asked,
                due,
                given_up: false,
            },
        );
    }

    /// The next parse waiting, its caller still there, sent.
    fn next_parse(&mut self) {
        while self.parsing.is_none()
            && let Some(asked) = self.waiting.pop_front()
        {
            if asked.cancel.is_cancelled() {
                asked.answer(Err(Failure::Cancelled));
            } else {
                self.send(asked);
            }
        }
    }

    /// The caller of `key` gave up: dropped if it waits; told to stop if it
    /// parses, given a moment to; its answer dropped if it reads.
    fn give_up(&mut self, key: u64) {
        if let Some(at) = self.waiting.iter().position(|asked| asked.key == key) {
            self.waiting.remove(at);
            return;
        }
        let Some(id) = self
            .flying
            .iter()
            .find(|(_, flying)| flying.asked.key == key && !flying.given_up)
            .map(|(&id, _)| id)
        else {
            return;
        };
        let parse = self.parsing == Some(id);
        if let Some(flying) = self.flying.get_mut(&id) {
            flying.given_up = true;
            if parse {
                flying.due = flying.due.min(Instant::now() + CANCEL_GRACE);
            }
        }
        if parse {
            self.send(Asked {
                key: 0,
                request: Request::Cancel { request: id },
                timeout: CANCEL_GRACE,
                cancel: CancelToken::new(),
                reply: None,
            });
        }
    }

    fn accept(
        &mut self,
        reply: Result<anyhow::Result<Tagged<Reply>>, crossbeam_channel::RecvError>,
    ) {
        let reply = match reply {
            Ok(Ok(reply)) => reply,
            Ok(Err(error)) => return self.end(&format!("{error:#}")),
            Err(_) => return self.end("the tree-sitter helper exited unexpectedly"),
        };
        let Some(flying) = self.flying.remove(&reply.id) else {
            return self.end("the tree-sitter helper answered what it was not asked");
        };
        if self.parsing == Some(reply.id) {
            self.parsing = None;
        }
        if !flying.given_up {
            flying.asked.answer(Ok(reply.body));
        }
        self.next_parse();
    }

    /// End the helper (killed and reaped here), saying why to what asks,
    /// what was asked and not answered too.
    fn end(&mut self, why: &str) {
        tracing::debug!("tree-sitter helper ended: {why}");
        let _ = self.ended.set(why.to_owned());
        self.channel = None;
        self.parsing = None;
        let broken = || Failure::Broken(anyhow::anyhow!("{why}"));
        for (_, flying) in self.flying.drain() {
            flying.asked.answer(Err(broken()));
        }
        for asked in self.waiting.drain(..) {
            asked.answer(Err(broken()));
        }
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

    /// The helper `doc` is open in, if it runs.
    #[cfg(target_os = "linux")]
    fn process_of(doc: &Doc) -> Option<Arc<Process>> {
        doc.place.lock().process.upgrade()
    }

    /// JSON of about `bytes` bytes, long to parse.
    #[cfg(target_os = "linux")]
    fn large(bytes: usize) -> String {
        let mut large = String::from("[\n");
        while large.len() < bytes {
            large.push_str("  {\"key\": \"value\", \"n\": [1, 2.5, true, null]},\n");
        }
        large.push_str("  {}\n]\n");
        large
    }

    /// A text opened in the helper, its grammar's folder read where it runs
    /// isolated, colored line by line, checked, closed.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_text_is_opened_colored_and_closed() {
        let Some(helper) = helper(KEPT) else { return };
        let cancel = CancelToken::new();
        let opened = helper.open("json", TEXT.as_bytes(), &cancel).unwrap();
        let process = process_of(&opened.doc).unwrap();
        if sandbox::available() {
            assert_eq!(process.inside, Path::new("/work"), "isolated");
        }
        assert_eq!(opened.lines, 4);
        let colors = helper
            .colors(&opened.doc, 0..opened.lines, &cancel)
            .unwrap();
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
        assert_eq!(helper.folds(&opened.doc, &cancel).unwrap(), []);
        assert_eq!(helper.outline(&opened.doc, &cancel).unwrap(), []);
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
        let folder = process.outside.clone();
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
        helper.close(opened.doc).unwrap();
        cancel.cancel();
        assert!(helper.open("json", TEXT.as_bytes(), &cancel).is_err());
    }

    /// WebAssembly grammars compiled are kept in the folder given, which
    /// the helper writes in where it runs isolated: the next helper finds
    /// them there.
    #[cfg(target_os = "linux")]
    #[test]
    fn compiled_grammars_are_kept_in_the_cache() {
        let Some(executable) = built_helper() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let compiled = dir.path().join("wasmtime/tree-sitter");
        keep_compiled_in(compiled.clone());
        let helper = Helper::new(Some(executable), true, Duration::from_millis(200));
        helper.set_grammars(vec![json()]);
        let cancel = CancelToken::new();
        // The compiled modules, not Wasmtime's notes of their use.
        let files = || {
            walk(&compiled)
                .into_iter()
                .filter(|path| {
                    path.is_file()
                        && path
                            .extension()
                            .is_none_or(|extension| extension != "stats")
                })
                .count()
        };
        let opened = helper.open("json", TEXT.as_bytes(), &cancel).unwrap();
        until(|| files() > 0);
        let kept = files();
        let folder = process_of(&opened.doc).unwrap().outside.clone();
        drop(opened);
        until(|| !folder.exists());
        // Another helper compiles nothing again.
        let again = helper.open("json", TEXT.as_bytes(), &cancel).unwrap();
        assert_eq!(helper.colors(&again.doc, 0..4, &cancel).unwrap().len(), 4);
        assert_eq!(files(), kept);
    }

    /// Every path under `dir`.
    #[cfg(target_os = "linux")]
    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(walk(&path));
            }
            found.push(path);
        }
        found
    }

    /// Unused for its `kept`, the helper is let go of, a text open in it
    /// or not: it ends, and its folder goes. The text is opened again from
    /// what its `Doc` keeps in the next helper, which the next call starts.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_helper_is_let_go_of_when_unused_and_texts_are_opened_again() {
        let kept = Duration::from_millis(300);
        let Some(helper) = helper(kept) else { return };
        let cancel = CancelToken::new();
        let opened = helper.open("json", TEXT.as_bytes(), &cancel).unwrap();
        let colored = helper.colors(&opened.doc, 0..4, &cancel).unwrap();
        let folder = process_of(&opened.doc).unwrap().outside.clone();
        until(|| !folder.exists());
        assert!(helper.inner.state.lock().current.is_none(), "let go of");
        assert!(process_of(&opened.doc).is_none(), "a text keeps no helper");
        // Asked again: opened again, colored the same.
        assert_eq!(helper.colors(&opened.doc, 0..4, &cancel).unwrap(), colored);
        let other = process_of(&opened.doc).unwrap().outside.clone();
        assert_ne!(other, folder, "another helper");
        // A text opened meanwhile goes to the same.
        let again = helper.open("json", TEXT.as_bytes(), &cancel).unwrap();
        assert!(Arc::ptr_eq(
            &process_of(&again.doc).unwrap(),
            &process_of(&opened.doc).unwrap()
        ));
        drop(again);
        until(|| !other.exists());
        // Grammars that color it otherwise: refused, for bat to color it.
        helper.set_grammars(vec![Grammar {
            highlights: vec![
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../tree-sitter-worker/tests/fixtures/json/queries/folds.scm"),
            ],
            ..json()
        }]);
        let changed = helper.colors(&opened.doc, 0..4, &cancel);
        assert!(changed.is_err(), "{changed:?}");
    }

    /// A parse cancelled returns at once; the helper stops it and serves
    /// on, not started again.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_cancelled_parse_returns_at_once_and_the_helper_serves_on() {
        let Some(helper) = helper(KEPT) else { return };
        let cancel = CancelToken::new();
        let first = helper.open("json", TEXT.as_bytes(), &cancel).unwrap();
        let large = large(4 << 20);
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
            Arc::ptr_eq(
                &process_of(&next.doc).unwrap(),
                &process_of(&first.doc).unwrap()
            ),
            "the same helper"
        );
        assert_eq!(helper.colors(&first.doc, 0..4, &cancel).unwrap().len(), 4);
    }

    /// What reads what is kept is answered while a parse runs, and a call
    /// waits its own bound from when it is asked (GE6).
    #[cfg(target_os = "linux")]
    #[test]
    fn reads_are_answered_beside_a_parse() {
        let Some(helper) = helper(KEPT) else { return };
        let helper = Arc::new(helper);
        let cancel = CancelToken::new();
        let first = helper.open("json", TEXT.as_bytes(), &cancel).unwrap();
        let large = large(8 << 20);
        let parsing = CancelToken::new();
        let parse = {
            let (helper, parsing) = (Arc::clone(&helper), parsing.clone());
            std::thread::spawn(move || helper.open("json", large.as_bytes(), &parsing).is_ok())
        };
        // Once it parses (its text written), read the first text.
        let process = process_of(&first.doc).unwrap();
        until(|| std::fs::read_dir(&process.outside).is_ok_and(|entries| entries.count() > 0));
        let started = Instant::now();
        assert_eq!(helper.colors(&first.doc, 0..4, &cancel).unwrap().len(), 4);
        let read = started.elapsed();
        parsing.cancel();
        parse.join().unwrap();
        assert!(read < Duration::from_millis(500), "read in {read:?}");
        // A call bounded from when it is asked: one waiting behind a parse
        // gives up in its own time.
        let parsing = CancelToken::new();
        let parse = {
            let (helper, parsing) = (Arc::clone(&helper), parsing.clone());
            let large = self::large(8 << 20);
            std::thread::spawn(move || helper.open("json", large.as_bytes(), &parsing).is_ok())
        };
        until(|| std::fs::read_dir(&process.outside).is_ok_and(|entries| entries.count() > 0));
        // Its text written is not yet its parse asked: a check the helper
        // takes first is answered, and is asked again until one waits
        // behind the parse.
        let (waited, took) = loop {
            let started = Instant::now();
            let waited = process.call(
                Request::Check {
                    name: "json".into(),
                },
                Duration::from_millis(200),
                &cancel,
            );
            if waited.is_err() || parse.is_finished() {
                break (waited, started.elapsed());
            }
        };
        assert!(waited.is_err(), "{waited:?}");
        assert!(took < Duration::from_secs(2), "{took:?}");
        parsing.cancel();
        parse.join().unwrap();
        assert!(
            process.ended.get().is_none(),
            "giving up on a call that waits ends nothing"
        );
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
    /// next call, once; what was open in it is opened again there.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_helper_that_died_is_started_again() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let stand_in = dir.path().join("helper");
        let shell = crate::process::required("sh").unwrap();
        // Greets, takes its grammars, opens anything, colors one line
        // plain; dies on a second Open, and after answering a second
        // Check; tells each start in `started`.
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
                " *Colors*) printf '{{\"id\":%s,\"body\":{{\"Colors\":{{\"lines\":[[]]}}}}}}\\n' \"$id\";;\n",
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
        assert_eq!(
            helper.colors(&opened.doc, 0..1, &cancel).unwrap(),
            [Vec::<Span>::new()]
        );
        assert_eq!(starts(), 2, "started again, once");
        assert_eq!(helper.check("x", &cancel).unwrap().abi, 14);
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
