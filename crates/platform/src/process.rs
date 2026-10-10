//! Running other programs without holding anyone up.
//!
//! [`run`] and [`run_to_exit`]: bounded, cancellable tool execution. Call
//! only from scheduler workers. Each Unix child owns a process group so
//! cancellation also stops descendants.
//!
//! [`spawn_detached`] and [`spawn_watched`]: programs that outlive the call
//! (a terminal, an editor). Nobody waits on them: one reaper thread collects
//! every exit, so no child lingers as a zombie and no scheduler worker is
//! held for as long as a window stays open.

use std::io::{self, Read, Seek, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use anyhow::Context;
use bibli_core::CancelToken;
use bibli_core::os_path::find_program;

/// One of our optional worker executables: its name, and the variable
/// that says where it is instead.
#[derive(Clone, Copy)]
pub struct Worker {
    pub variable: &'static str,
    pub executable: &'static str,
}

impl Worker {
    /// Where it is, if it can run there: the variable's choice, else
    /// where bibli's own helpers are (`helper_folders`), else on `PATH`. A
    /// choice naming nothing that runs is no worker: a probe says it is
    /// missing rather than installed, and an engine chosen automatically
    /// goes on to the next.
    pub fn find(&self) -> Option<PathBuf> {
        if let Some(chosen) = std::env::var_os(self.variable) {
            return find_program(chosen);
        }
        std::env::current_exe()
            .ok()
            .and_then(|bin| {
                helper_folders(bin.parent()?)
                    .into_iter()
                    .find_map(|dir| find_program(dir.join(self.executable)))
            })
            .or_else(|| find_program(self.executable))
    }

    /// [`Worker::find`], or what to do without it, in one wording for
    /// every worker: "Install bibli-archive-worker to open archives".
    pub fn require(&self, purpose: &str) -> anyhow::Result<PathBuf> {
        self.find()
            .with_context(|| format!("Install {} to {purpose}", self.executable))
    }

    /// Whether it can work here: found, and answering `--probe` (`probe`).
    /// Starts it: call from a worker.
    pub fn works(&self) -> bool {
        self.works_with(&[])
    }

    /// [`Worker::works`], told what it would be launched with (`envs`:
    /// `PDFium`'s library as the settings name it).
    pub fn works_with(&self, envs: &[(&str, &std::ffi::OsStr)]) -> bool {
        self.find().is_some_and(|path| probe(&path, envs))
    }

    /// What a plugin needing it lists (`Plugin::requirements`), probed as
    /// the settings show it: installed only where it works (`works`), not
    /// where its program is and its libraries are not.
    pub fn requirement(self, purpose: &str) -> bibli_core::plugin::Requirement {
        bibli_core::plugin::Requirement::new(self.executable, purpose, move || self.works())
    }
}

/// How long a helper may take to answer `--probe`: `GStreamer`'s first
/// start reads every plugin it has.
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

/// Whether the helper at `path` can work here: run with `--probe`, the
/// one argument every helper of bibli's answers before any other, it
/// checks what it needs beside the libraries it links (`PDFium`'s, which
/// it opens; `GStreamer`'s playback plugins) and exits 0 when it has it,
/// else non-zero, saying why. One whose library is missing is refused by
/// the system before it starts (the loader's 127 on Linux, an abort on
/// macOS, a missing DLL on Windows), which fails the probe too. A helper
/// that hangs is no helper.
pub fn probe(path: &Path, envs: &[(&str, &std::ffi::OsStr)]) -> bool {
    let mut command = Command::new(path);
    command.arg("--probe").envs(envs.iter().copied());
    match run(&mut command, &CancelToken::new(), PROBE_TIMEOUT) {
        Ok(_) => true,
        Err(error) => {
            tracing::debug!("{} cannot work here: {error:#}", path.display());
            false
        }
    }
}

/// Which build of bibli this program is (`BIBLI_BUILD`, which its build
/// script sets, `tools/build/identity.rs`), told by the binary as it
/// starts (`set_build`) so that this crate is not built again for each
/// commit. Untold (a test), no failure is explained (`explain`).
static BUILD: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();

/// Say which build of bibli this program is: once, as it starts.
pub fn set_build(build: &'static str) {
    let _ = BUILD.set(build);
}

/// How long a helper may take to answer `--build`, the first thing it
/// answers: only one older than the question, taking it for its input,
/// does anything else.
const BUILD_TIMEOUT: Duration = Duration::from_secs(5);

/// `error`, from a call of the helper at `helper`, and after it what may
/// explain it: the helper is of another build of bibli than this one, as
/// it says when asked with `--build`, or older than the question, saying
/// nothing. A helper is never refused for it, since most calls work all
/// the same: only a failure says so. A call `cancel`led, a helper of this
/// build, one that could not answer (its libraries missing) and a program
/// that does not say its build (`set_build`) leave `error` as it is.
/// Asks once per helper file (`build_of`): call from a worker.
pub fn explain(helper: &Path, cancel: &CancelToken, error: anyhow::Error) -> anyhow::Error {
    explain_as(BUILD.get().copied(), helper, cancel, error)
}

/// [`explain`], this program being of the build `ours`.
fn explain_as(
    ours: Option<&str>,
    helper: &Path,
    cancel: &CancelToken,
    error: anyhow::Error,
) -> anyhow::Error {
    let Some(ours) = ours else {
        return error;
    };
    if cancel.is_cancelled() {
        return error;
    }
    let build = match build_of(helper) {
        Said::Build(theirs) if theirs == ours => return error,
        Said::Unknown => return error,
        Said::Build(theirs) => format!("another build of bibli ({theirs}; bibli is {ours})"),
        Said::Nothing => "an older build of bibli".to_owned(),
    };
    let name = helper
        .file_name()
        .unwrap_or(helper.as_os_str())
        .to_string_lossy();
    // The failure first, whole, as it would read without this: one line
    // where it is shown.
    let failure = format!("{error:#}");
    anyhow::anyhow!(
        "{}. {name} is from {build}, which may be why: rebuild or reinstall it",
        failure.trim_end_matches('.')
    )
}

/// What a helper said when asked `--build`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Said {
    /// The build it is of.
    Build(String),
    /// Nothing that is one: it ran and took the question for its input,
    /// a helper older than the question.
    Nothing,
    /// It could not answer: its libraries missing (the loader's 127, a
    /// missing DLL's status), killed, or too slow.
    Unknown,
}

/// What the helper at `path` says to `--build`, asked once per file as it
/// is: one built again is asked again.
fn build_of(path: &Path) -> Said {
    type Asked = Vec<(PathBuf, Option<std::time::SystemTime>, Said)>;
    static ASKED: parking_lot::Mutex<Asked> = parking_lot::const_mutex(Vec::new());
    let modified = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok();
    if let Some((.., said)) = ASKED
        .lock()
        .iter()
        .find(|(asked, at, _)| asked == path && *at == modified)
    {
        return said.clone();
    }
    let said = ask_build(path);
    let mut asked = ASKED.lock();
    asked.retain(|(asked, ..)| asked != path);
    asked.push((path.to_owned(), modified, said.clone()));
    said
}

/// [`build_of`], asked.
fn ask_build(path: &Path) -> Said {
    // Windows' STATUS_DLL_NOT_FOUND, 0xC0000135, as the exit code reads it.
    const DLL_NOT_FOUND: i32 = -1_073_741_515;
    let mut command = Command::new(path);
    command.arg("--build");
    let Ok(finished) = run_to_exit(&mut command, None, &CancelToken::new(), BUILD_TIMEOUT) else {
        return Said::Unknown;
    };
    if matches!(finished.status.code(), None | Some(127 | DLL_NOT_FOUND)) {
        return Said::Unknown;
    }
    let said = String::from_utf8_lossy(&finished.stdout);
    let build = said.trim_end_matches(['\r', '\n']);
    let is_build = (1..=64).contains(&build.len())
        && build
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"+.-".contains(&byte));
    if finished.status.success() && is_build {
        Said::Build(build.to_owned())
    } else {
        Said::Nothing
    }
}

/// Where bibli's helpers are, from the folder its program is in: beside
/// it (a build, `just dist`, a Mac's bundle, Windows' installer, the Nix
/// package), else where a Linux package keeps a program's private
/// helpers, `/usr/bin/bibli` beside `/usr/lib/bibli/` (Debian, Arch) or
/// `/usr/libexec/bibli/` (Fedora) (ADR-040).
fn helper_folders(bin: &Path) -> Vec<PathBuf> {
    let mut folders = vec![bin.to_owned()];
    if let Some(prefix) = bin.parent() {
        folders.push(prefix.join("lib").join("bibli"));
        folders.push(prefix.join("libexec").join("bibli"));
    }
    folders
}

/// The installed `program` something needs, as its real path: a sandbox
/// binds the store it lives in, not the profile's symlink farm.
pub fn required(program: &str) -> anyhow::Result<PathBuf> {
    find_program(program)
        .or_else(|| application(program))
        .and_then(|path| bibli_core::os_path::canonical(path).ok())
        .with_context(|| format!("{program} is not installed"))
}

/// Where an application that puts no program on any path keeps the one
/// bibli runs as `program`: `LibreOffice`'s `soffice`, in its bundle on
/// macOS (in `/Applications` or the user's own) and in its folder under
/// Program Files on Windows. Linux's packages put it on `PATH`.
fn application(program: &str) -> Option<PathBuf> {
    if program != "libreoffice" {
        return None;
    }
    let places: Vec<PathBuf> = if cfg!(target_os = "macos") {
        [
            Some(PathBuf::from("/Applications")),
            bibli_core::os_path::home().map(|home| home.join("Applications")),
        ]
        .into_iter()
        .flatten()
        .map(|dir| dir.join("LibreOffice.app/Contents/MacOS/soffice"))
        .collect()
    } else if cfg!(windows) {
        ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"]
            .into_iter()
            .filter_map(std::env::var_os)
            .map(|dir| {
                Path::new(&dir)
                    .join("LibreOffice")
                    .join("program")
                    .join("soffice.exe")
            })
            .collect()
    } else {
        Vec::new()
    };
    places.into_iter().find_map(find_program)
}

/// A child that is killed, with its process group, and reaped when dropped.
/// On Windows its group is a job object holding it and what it starts.
pub(crate) struct ChildGuard {
    pub(crate) child: Child,
    #[cfg(windows)]
    job: Option<crate::windows::Job>,
}

impl ChildGuard {
    pub(crate) fn new(child: Child) -> Self {
        Self {
            #[cfg(windows)]
            job: crate::windows::Job::holding(&child),
            child,
        }
    }

    #[cfg(unix)]
    fn pid(&self) -> Option<rustix::process::Pid> {
        rustix::process::Pid::from_raw(i32::try_from(self.child.id()).ok()?)
    }

    /// Whether the child has exited, without reaping it. Until it is reaped
    /// its pid cannot be reused, so the group kill in `finish` and `Drop`
    /// reaches only its own descendants, never a stranger's group.
    pub(crate) fn exited(&mut self) -> io::Result<bool> {
        #[cfg(unix)]
        {
            use rustix::process::{WaitId, WaitIdOptions, waitid};
            let pid = self
                .pid()
                .ok_or_else(|| io::Error::other("invalid child pid"))?;
            Ok(waitid(
                WaitId::Pid(pid),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
            )?
            .is_some())
        }
        #[cfg(not(unix))]
        {
            Ok(self.child.try_wait()?.is_some())
        }
    }

    /// Kill what the child left running in its group, while the child's
    /// unreaped pid still names that group; on Windows, everything in its
    /// job.
    fn stop_group(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.pid()
            && self.exited().is_ok()
        {
            // `exited` fails with ECHILD once someone reaped the leader
            // (`Child::try_wait`): its pid may be anyone's by now.
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.stop();
        }
    }

    /// Stop the group and reap the child, which has exited.
    pub(crate) fn finish(mut self) -> io::Result<ExitStatus> {
        self.stop_group();
        self.child.wait()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.stop_group();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// How a program run by [`run_to_exit`] ended.
#[derive(Debug)]
pub struct Finished {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    /// The start of what it wrote to standard error, enough for a diagnostic.
    pub stderr: String,
}

/// Bytes of standard error kept in [`Finished::stderr`].
const DIAGNOSTIC: u64 = 4096;

/// The most [`run_to_exit`] gives a child as its standard input. It is a
/// private temporary file written before the child starts, not a pipe: a
/// child that exits without reading it cannot make the write fail or
/// block, whatever its length (a pipe's buffer holds a page at least, and
/// a Mermaid diagram's source was refused past it).
pub const MAX_INPUT: usize = 1024 * 1024;

/// Turns to run a program heavy in memory or time (`ImageMagick`, a 3D
/// model's helper): at most so many at once, the others waiting their
/// turn, a wait given up once cancelled. A grid of large models started
/// one helper per worker, each holding most of a gigabyte.
pub struct Turns {
    most: usize,
    held: parking_lot::Mutex<Held>,
    freed: parking_lot::Condvar,
}

/// The turns running, and what is to be told when one is free.
struct Held {
    running: usize,
    waiting: Vec<Box<dyn FnOnce() + Send>>,
}

/// A turn taken, given back when dropped.
pub struct Turn<'a>(&'a Turns);

impl Turns {
    pub const fn new(most: usize) -> Self {
        Turns {
            most,
            held: parking_lot::const_mutex(Held {
                running: 0,
                waiting: Vec::new(),
            }),
            freed: parking_lot::Condvar::new(),
        }
    }

    /// Wait for a turn; `None` once `cancel` is cancelled.
    pub fn take(&self, cancel: &CancelToken) -> Option<Turn<'_>> {
        let mut held = self.held.lock();
        while held.running >= self.most {
            if cancel.is_cancelled() {
                return None;
            }
            self.freed.wait_for(&mut held, Duration::from_millis(50));
        }
        if cancel.is_cancelled() {
            return None;
        }
        held.running += 1;
        Some(Turn(self))
    }

    /// A turn if one is free now, without waiting.
    pub fn try_take(&self) -> Option<Turn<'_>> {
        let mut held = self.held.lock();
        (held.running < self.most).then(|| {
            held.running += 1;
            Turn(self)
        })
    }

    /// Call `then` once a turn is free: now if one is, else when the next
    /// is given back (on the thread giving it back). What waits for a turn
    /// this way holds no thread meanwhile.
    pub fn when_free(&self, then: Box<dyn FnOnce() + Send>) {
        let mut held = self.held.lock();
        if held.running < self.most {
            drop(held);
            then();
        } else {
            held.waiting.push(then);
        }
    }
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        let waiting = {
            let mut held = self.0.held.lock();
            held.running -= 1;
            std::mem::take(&mut held.waiting)
        };
        self.0.freed.notify_one();
        for then in waiting {
            then();
        }
    }
}

/// Run `command` to its end, however it ends: cancelled or timed out is an
/// error, a failed exit status is not. `input`, if any, is its standard
/// input (at most [`MAX_INPUT`] bytes); otherwise it reads nothing.
///
/// Output goes to private temporary files, not pipes that could deadlock,
/// capped while it runs. The cap is derived from the largest helper listing
/// (an archive's member names, JSON-escaped), so a helper bound can never
/// exceed what is captured. Rendered artifacts must separately be checked by
/// the caller before decoding.
pub fn run_to_exit(
    command: &mut Command,
    input: Option<&[u8]>,
    cancel: &CancelToken,
    timeout: Duration,
) -> anyhow::Result<Finished> {
    run_capped(
        command,
        input,
        cancel,
        timeout,
        crate::inspection::limits::HELPER_OUTPUT,
    )
}

/// How often a program running is looked at (cancelled, too long, too
/// much said) while it has not ended.
const CHECK_EVERY: Duration = Duration::from_millis(50);

/// Wakes a wait for a child as it exits: its pidfd on Linux, asleep until
/// it ends (a wait polled every 10 ms, a hundred wakes a second for each
/// helper running); elsewhere a short sleep.
struct ExitSignal {
    #[cfg(target_os = "linux")]
    pidfd: Option<rustix::fd::OwnedFd>,
}

impl ExitSignal {
    fn of(child: &ChildGuard) -> Self {
        #[cfg(target_os = "linux")]
        {
            ExitSignal {
                pidfd: child.pid().and_then(|pid| {
                    rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty()).ok()
                }),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = child;
            ExitSignal {}
        }
    }

    /// Wait until the child exits, or `at_most` passes.
    // Elsewhere than on Linux the signal holds nothing to wait on.
    #[cfg_attr(not(target_os = "linux"), allow(clippy::unused_self))]
    fn wait(&self, at_most: Duration) {
        #[cfg(target_os = "linux")]
        if let Some(pidfd) = &self.pidfd {
            let mut fds = [rustix::event::PollFd::new(
                pidfd,
                rustix::event::PollFlags::IN,
            )];
            let timeout = rustix::time::Timespec {
                tv_sec: i64::try_from(at_most.as_secs()).unwrap_or(i64::MAX),
                tv_nsec: i64::from(at_most.subsec_nanos()),
            };
            let _ = rustix::event::poll(&mut fds, Some(&timeout));
            return;
        }
        std::thread::sleep(at_most.min(Duration::from_millis(10)));
    }
}

/// [`run_to_exit`] for a program whose output is bounded otherwise than a
/// helper's (a database's answer): more than `output` bytes on either
/// stream is an error, and stops it.
pub fn run_capped(
    command: &mut Command,
    input: Option<&[u8]>,
    cancel: &CancelToken,
    timeout: Duration,
    output: u64,
) -> anyhow::Result<Finished> {
    let program = Path::new(command.get_program())
        .file_name()
        .unwrap_or(command.get_program())
        .to_string_lossy()
        .into_owned();
    anyhow::ensure!(!cancel.is_cancelled(), "{program} was cancelled");
    let stdin = match input {
        Some(bytes) => {
            anyhow::ensure!(bytes.len() <= MAX_INPUT, "{program}: input too large");
            let mut file = tempfile::tempfile()?;
            file.write_all(bytes)?;
            file.seek(io::SeekFrom::Start(0))?;
            Stdio::from(file)
        }
        None => Stdio::null(),
    };
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    command
        .stdin(stdin)
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = ChildGuard::new(
        command
            .spawn()
            .with_context(|| format!("start {program}"))?,
    );
    // The pipe's write end was dropped above and the child holds the read
    // end: it sees the input, then end of file.
    let start = Instant::now();
    let exit = ExitSignal::of(&child);
    let too_much = |stdout: &std::fs::File, stderr: &std::fs::File| -> io::Result<bool> {
        Ok(stdout.metadata()?.len() > output || stderr.metadata()?.len() > output)
    };
    loop {
        anyhow::ensure!(!cancel.is_cancelled(), "{program} was cancelled");
        anyhow::ensure!(start.elapsed() < timeout, "{program} timed out");
        anyhow::ensure!(!too_much(&stdout, &stderr)?, "{program}: excessive output");
        if child.exited()? {
            break;
        }
        exit.wait(CHECK_EVERY.min(timeout.saturating_sub(start.elapsed())));
    }
    // What it wrote since the last look: cut to the limit, a program that
    // wrote too much and ended in between said less than it did.
    anyhow::ensure!(!too_much(&stdout, &stderr)?, "{program}: excessive output");
    let status = child.finish()?;
    stderr.rewind()?;
    let mut diagnostic = Vec::new();
    stderr.take(DIAGNOSTIC).read_to_end(&mut diagnostic)?;
    stdout.rewind()?;
    let mut bytes = Vec::new();
    stdout.take(output).read_to_end(&mut bytes)?;
    Ok(Finished {
        status,
        stdout: bytes,
        stderr: String::from_utf8_lossy(&diagnostic).trim().to_owned(),
    })
}

/// [`run_to_exit`] without input, for a program whose failed exit is an
/// error: its standard output, or its status and standard error.
pub fn run(
    command: &mut Command,
    cancel: &CancelToken,
    timeout: Duration,
) -> anyhow::Result<Vec<u8>> {
    let finished = run_to_exit(command, None, cancel, timeout)?;
    if !finished.status.success() {
        let program = Path::new(command.get_program())
            .file_name()
            .unwrap_or(command.get_program())
            .to_string_lossy()
            .into_owned();
        anyhow::bail!(
            "{program} failed ({}): {}",
            finished.status,
            finished.stderr
        );
    }
    Ok(finished.stdout)
}

/// What to do when a watched child exits. It runs on the reaper thread,
/// which every watched child shares: do something brief (send on a channel,
/// schedule a job, wake the app), never the work itself.
pub type OnExit = Box<dyn FnOnce(io::Result<ExitStatus>) + Send>;

/// The folder the process `pid` works in, as the system says: Linux's
/// `/proc/<pid>/cwd`, macOS's `proc_pidinfo` (compiled, never run);
/// `None` on Windows, which keeps it in the process's own memory.
pub fn working_dir(pid: u32) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let mut info: libc::proc_vnodepathinfo = unsafe_zeroed();
        let size = libc::c_int::try_from(size_of::<libc::proc_vnodepathinfo>()).ok()?;
        // SAFETY: a buffer of the record PROC_PIDVNODEPATHINFO fills, of
        // the size given.
        #[allow(unsafe_code)]
        let filled = unsafe {
            libc::proc_pidinfo(
                libc::c_int::try_from(pid).ok()?,
                libc::PROC_PIDVNODEPATHINFO,
                0,
                (&raw mut info).cast(),
                size,
            )
        };
        if filled != size {
            return None;
        }
        // The path, NUL-terminated within its fixed field.
        let bytes: Vec<u8> = info
            .pvi_cdir
            .vip_path
            .iter()
            .flatten()
            .map(|c| c.cast_unsigned())
            .take_while(|b| *b != 0)
            .collect();
        (!bytes.is_empty()).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(&bytes)))
    }
    #[cfg(windows)]
    {
        let _ = pid;
        None
    }
}

/// An all-zero record, as the system fills one.
#[cfg(target_os = "macos")]
fn unsafe_zeroed<T>() -> T {
    // SAFETY: only called for the C records above, for which all zeros
    // is a valid value.
    #[allow(unsafe_code)]
    unsafe {
        std::mem::zeroed()
    }
}

/// Whether the process `pid` runs, as the system says: `None` when it
/// will not say. What a process left behind (a folder named by its pid)
/// is taken away only once this says it has ended.
pub fn runs(pid: u32) -> Option<bool> {
    #[cfg(target_os = "linux")]
    {
        Some(std::path::Path::new("/proc").join(pid.to_string()).exists())
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        let pid = rustix::process::Pid::from_raw(i32::try_from(pid).ok()?)?;
        match rustix::process::test_kill_process(pid) {
            Ok(()) | Err(rustix::io::Errno::PERM) => Some(true),
            Err(rustix::io::Errno::SRCH) => Some(false),
            Err(_) => None,
        }
    }
    #[cfg(windows)]
    {
        crate::windows::process_runs(pid)
    }
}

/// Start `command` and let it run on its own: in a process group of its own
/// (a Ctrl+C in the terminal bibli was started from does not reach it), its
/// exit collected by the reaper. Standard streams are the caller's to set.
///
/// # Errors
/// When the program cannot be started.
pub fn spawn_detached(command: &mut Command) -> io::Result<()> {
    spawn_watched(command, |_| {})
}

/// [`spawn_detached`], and call `on_exit` with the program's exit status
/// when it ends (see [`OnExit`] for where that runs). For a terminal or an
/// editor whose end means the user is done; no thread waits in between.
///
/// # Errors
/// When the program cannot be started; `on_exit` is then never called.
pub fn spawn_watched(
    command: &mut Command,
    on_exit: impl FnOnce(io::Result<ExitStatus>) + Send + 'static,
) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let child = command.spawn()?;
    reaper::watch(child, Box::new(on_exit));
    Ok(())
}

fn report(on_exit: OnExit, status: io::Result<ExitStatus>) {
    // A panicking callback must not take the reaper, and every other
    // child's exit, down with it.
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| on_exit(status))).is_err() {
        tracing::warn!("a child exit handler panicked");
    }
}

/// A thread that waits on one child: the fallback where pidfds are not
/// available.
fn wait_on_thread(mut child: Child, on_exit: OnExit) {
    let spawned = std::thread::Builder::new()
        .name("bibli-wait".into())
        .spawn(move || {
            let status = child.wait();
            report(on_exit, status);
        });
    if let Err(error) = spawned {
        tracing::warn!("cannot wait for a child: {error}");
    }
}

/// One thread polls a pidfd per child and an eventfd that announces new
/// ones; it sleeps in `poll` while nothing exits.
#[cfg(target_os = "linux")]
mod reaper {
    use std::os::fd::OwnedFd;
    use std::process::Child;
    use std::sync::OnceLock;

    use parking_lot::Mutex;
    use rustix::event::{EventfdFlags, PollFd, PollFlags};

    use super::OnExit;

    struct Watched {
        child: Child,
        pidfd: OwnedFd,
        on_exit: OnExit,
    }

    struct Reaper {
        queue: Mutex<Vec<Watched>>,
        wake: OwnedFd,
    }

    fn reaper() -> Option<&'static Reaper> {
        static REAPER: OnceLock<Option<&'static Reaper>> = OnceLock::new();
        *REAPER.get_or_init(|| {
            let wake =
                rustix::event::eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK).ok()?;
            let reaper: &'static Reaper = Box::leak(Box::new(Reaper {
                queue: Mutex::new(Vec::new()),
                wake,
            }));
            std::thread::Builder::new()
                .name("bibli-reaper".into())
                .spawn(move || run(reaper))
                .ok()?;
            Some(reaper)
        })
    }

    pub(super) fn watch(child: Child, on_exit: OnExit) {
        let pidfd = i32::try_from(child.id())
            .ok()
            .and_then(rustix::process::Pid::from_raw)
            .and_then(|pid| {
                rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty()).ok()
            });
        match (reaper(), pidfd) {
            (Some(reaper), Some(pidfd)) => {
                reaper.queue.lock().push(Watched {
                    child,
                    pidfd,
                    on_exit,
                });
                let _ = rustix::io::write(&reaper.wake, &1u64.to_ne_bytes());
            }
            _ => super::wait_on_thread(child, on_exit),
        }
    }

    fn run(reaper: &'static Reaper) {
        let mut children: Vec<Watched> = Vec::new();
        loop {
            let polled = {
                let mut fds: Vec<PollFd<'_>> = std::iter::once(&reaper.wake)
                    .chain(children.iter().map(|watched| &watched.pidfd))
                    .map(|fd| PollFd::new(fd, PollFlags::IN))
                    .collect();
                rustix::event::poll(&mut fds, None)
            };
            match polled {
                Ok(_) | Err(rustix::io::Errno::INTR) => {}
                Err(error) => {
                    tracing::warn!("reaper poll failed: {error}");
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
            // The counter is reset by the read, so a child announced after
            // it wakes the next poll.
            let mut counter = [0u8; 8];
            let _ = rustix::io::read(&reaper.wake, &mut counter);
            children.append(&mut reaper.queue.lock());
            // A handful of children at most: asking each is cheaper than
            // mapping poll results back to them.
            let mut i = 0;
            while i < children.len() {
                match children[i].child.try_wait() {
                    Ok(None) => i += 1,
                    Ok(Some(status)) => {
                        let done = children.swap_remove(i);
                        super::report(done.on_exit, Ok(status));
                    }
                    Err(error) => {
                        let done = children.swap_remove(i);
                        super::report(done.on_exit, Err(error));
                    }
                }
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod reaper {
    use std::process::Child;

    use super::OnExit;

    pub(super) fn watch(child: Child, on_exit: OnExit) {
        super::wait_on_thread(child, on_exit);
    }
}

#[cfg(test)]
// Helpers of the tests that run on Unix alone (ADR-035).
#[cfg_attr(not(unix), allow(dead_code, unused_imports))]
mod tests {
    use super::*;

    /// A program that says more than it may and ends before it is looked
    /// at again is refused, not cut to the limit; one within it is read
    /// whole, as soon as it ends.
    #[cfg(unix)]
    #[test]
    fn output_past_the_limit_is_refused_even_once_the_program_ended() {
        let said = |text: &str, limit: u64| {
            run_capped(
                Command::new("/bin/sh").args(["-c", &format!("printf '{text}'")]),
                None,
                &CancelToken::new(),
                Duration::from_secs(5),
                limit,
            )
        };
        let error = said("0123456789", 4).unwrap_err();
        assert!(
            format!("{error:#}").contains("excessive output"),
            "{error:#}"
        );
        let started = Instant::now();
        let finished = said("0123", 4).unwrap();
        assert_eq!(finished.stdout, b"0123");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    // Scripts stand in for helpers.
    #[cfg(unix)]
    #[test]
    fn a_helper_works_only_where_it_answers_its_probe() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let helper = |name: &str, body: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        let works = helper("works", r#"[ "$1" = --probe ] && exit 0; exit 2"#);
        assert!(probe(&works, &[]));
        // What the loader says of a missing library, and a helper lacking
        // what it opens itself.
        let unloadable = helper(
            "unloadable",
            "echo 'error while loading shared libraries' >&2; exit 127",
        );
        assert!(!probe(&unloadable, &[]));
        let lacking = helper("lacking", "echo 'PDFium library unavailable' >&2; exit 1");
        assert!(!probe(&lacking, &[]));
        // What it would be launched with reaches it.
        let told = helper("told", r#"[ -n "$BIBLI_TEST_TOLD" ] && exit 0; exit 1"#);
        assert!(!probe(&told, &[]));
        assert!(probe(
            &told,
            &[("BIBLI_TEST_TOLD", std::ffi::OsStr::new("/x/libpdfium.so"))]
        ));
        assert!(!probe(&dir.path().join("absent"), &[]));
    }

    /// A helper's failure says after it that the helper is of another
    /// build, or of one older than the question, as the helper answers
    /// `--build`; of this build, unable to answer, or the call cancelled,
    /// the failure is as it was.
    #[cfg(unix)]
    #[test]
    fn a_failure_says_when_its_helper_is_of_another_build() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let helper = |name: &str, body: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        let says = |build: &str| format!(r#"[ "$1" = --build ] && echo {build} && exit 0; exit 1"#);
        let explained = |ours: Option<&str>, path: &Path, cancel: &CancelToken| {
            let failure = anyhow::anyhow!("invalid digit found in string").context("bwrap failed");
            format!("{:#}", explain_as(ours, path, cancel, failure))
        };
        let live = CancelToken::new();
        let same = helper("bibli-same-worker", &says("0.1.0+d264e57ab367"));
        assert_eq!(
            explained(Some("0.1.0+d264e57ab367"), &same, &live),
            "bwrap failed: invalid digit found in string"
        );
        let other = helper("bibli-other-worker", &says("0.1.0+a5d0b7a1c2d3"));
        assert_eq!(
            explained(Some("0.1.0+d264e57ab367"), &other, &live),
            "bwrap failed: invalid digit found in string. bibli-other-worker is from \
             another build of bibli (0.1.0+a5d0b7a1c2d3; bibli is 0.1.0+d264e57ab367), \
             which may be why: rebuild or reinstall it"
        );
        // An older helper takes the question for its input, and fails or
        // answers something else.
        let older = helper(
            "bibli-older-worker",
            "echo 'missing --members bounds' >&2; exit 1",
        );
        let chatty = helper(
            "bibli-chatty-worker",
            r#"echo '{"id":0,"body":{"Error":"x"}}'"#,
        );
        for path in [&older, &chatty] {
            assert!(
                explained(Some("0.1.0"), path, &live).ends_with(
                    "is from an older build of bibli, which may be why: rebuild or reinstall it"
                ),
                "{}",
                path.display()
            );
        }
        // Its libraries missing, it could not answer at all.
        let unloadable = helper("bibli-unloadable-worker", "exit 127");
        let alone = "bwrap failed: invalid digit found in string";
        assert_eq!(explained(Some("0.1.0"), &unloadable, &live), alone);
        assert_eq!(
            explained(Some("0.1.0"), &dir.path().join("absent"), &live),
            alone
        );
        // Cancelled, or this program untold of its build: nothing asked.
        let cancelled = CancelToken::new();
        cancelled.cancel();
        assert_eq!(explained(Some("0.1.0"), &other, &cancelled), alone);
        assert_eq!(explained(None, &other, &live), alone);
        // Built again, a helper is asked again.
        std::fs::write(
            &other,
            format!("#!/bin/sh\n{}\n", says("0.1.0+d264e57ab367")),
        )
        .unwrap();
        let later = std::time::SystemTime::now() + Duration::from_secs(1);
        std::fs::File::options()
            .write(true)
            .open(&other)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert_eq!(explained(Some("0.1.0+d264e57ab367"), &other, &live), alone);
    }

    #[test]
    fn helpers_are_beside_bibli_or_where_a_package_keeps_them() {
        let bin = Path::new("/usr/bin");
        assert_eq!(
            helper_folders(bin),
            [
                PathBuf::from("/usr/bin"),
                PathBuf::from("/usr/lib/bibli"),
                PathBuf::from("/usr/libexec/bibli"),
            ]
        );
        assert_eq!(helper_folders(Path::new("/")), [PathBuf::from("/")]);
    }

    // A POSIX shell runs the scripts.
    #[cfg(unix)]
    #[test]
    fn cancellation_prevents_spawn_and_timeout_stops_a_child() {
        let tmp = tempfile::tempdir().unwrap();
        let marker = tmp.path().join("started");
        let cancelled = CancelToken::new();
        cancelled.cancel();
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "touch \"$1\"", "test"]).arg(&marker);
        assert!(run(&mut cmd, &cancelled, Duration::from_secs(1)).is_err());
        assert!(!marker.exists());
        let start = Instant::now();
        let error = run(
            Command::new("sh").args(["-c", "sleep 30 & wait"]),
            &CancelToken::new(),
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn running_cancellation_kills_descendants() {
        let tmp = tempfile::tempdir().unwrap();
        let marker = tmp.path().join("child-pid");
        let token = CancelToken::new();
        let cancel = token.clone();
        let output = marker.clone();
        let worker = std::thread::spawn(move || {
            run(
                Command::new("sh")
                    .args(["-c", "sleep 30 & printf '%s' \"$!\" > \"$1\"; wait", "test"])
                    .arg(output),
                &cancel,
                Duration::from_secs(5),
            )
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let pid = loop {
            if let Ok(contents) = std::fs::read_to_string(&marker)
                && let Ok(pid) = contents.parse::<u32>()
            {
                break pid;
            }
            assert!(Instant::now() < deadline, "child never started");
            std::thread::sleep(Duration::from_millis(5));
        };
        token.cancel();
        assert!(
            worker
                .join()
                .unwrap()
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
        loop {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"));
            if stat.is_err() || stat.unwrap().split_whitespace().nth(2) == Some("Z") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "descendant survived cancellation"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    // A POSIX shell runs the scripts.
    #[cfg(unix)]
    #[test]
    fn captures_output_and_reports_failed_exit() {
        let token = CancelToken::new();
        assert_eq!(
            run(
                Command::new("sh").args(["-c", "printf hello"]),
                &token,
                Duration::from_secs(2)
            )
            .unwrap(),
            b"hello"
        );
        let error = run(
            Command::new("sh").args(["-c", "echo broken >&2; exit 3"]),
            &token,
            Duration::from_secs(2),
        )
        .unwrap_err();
        assert!(error.to_string().contains("broken"));
    }

    // A POSIX shell runs the scripts.
    #[cfg(unix)]
    #[test]
    fn watched_children_report_their_exit_in_any_order() {
        let (tx, rx) = crossbeam_channel::unbounded();
        for (code, delay) in [(3, "0.3"), (4, "0")] {
            let tx = tx.clone();
            spawn_watched(
                Command::new("sh").args(["-c", &format!("sleep {delay}; exit {code}")]),
                move |status| {
                    let _ = tx.send(status.unwrap().code());
                },
            )
            .unwrap();
        }
        let timeout = Duration::from_secs(5);
        assert_eq!(rx.recv_timeout(timeout).unwrap(), Some(4));
        assert_eq!(rx.recv_timeout(timeout).unwrap(), Some(3));
        assert!(spawn_detached(&mut Command::new("/nonexistent/program")).is_err());
    }

    // A POSIX shell runs the scripts.
    #[cfg(unix)]
    #[test]
    fn input_reaches_the_child_and_a_failed_exit_is_reported() {
        let token = CancelToken::new();
        let finished = run_to_exit(
            Command::new("sh").args(["-c", "cat; echo oops >&2; exit 2"]),
            Some(b"secret"),
            &token,
            Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(finished.stdout, b"secret");
        assert_eq!(finished.stderr, "oops");
        assert_eq!(finished.status.code(), Some(2));
        // Input longer than a pipe's buffer reaches it whole (a diagram's
        // source of 64 KB).
        let long = vec![b'y'; 100_000];
        let finished = run_to_exit(
            Command::new("sh").args(["-c", "wc -c"]),
            Some(&long),
            &token,
            Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(String::from_utf8_lossy(&finished.stdout).trim(), "100000");
        // A child that never reads its input still ends normally.
        let finished = run_to_exit(
            Command::new("sh").args(["-c", "exit 0"]),
            Some(&vec![b'x'; MAX_INPUT]),
            &token,
            Duration::from_secs(2),
        )
        .unwrap();
        assert!(finished.status.success());
    }

    /// A program run on Windows says how it ended, and a timeout ends it
    /// with what it started (its job).
    #[cfg(windows)]
    #[test]
    fn a_windows_program_says_its_end_and_a_timeout_ends_its_tree() {
        use std::os::windows::process::CommandExt;
        let finished = run_to_exit(
            Command::new("cmd").args(["/c", "exit 3"]),
            None,
            &CancelToken::new(),
            Duration::from_secs(20),
        )
        .unwrap();
        assert_eq!(finished.status.code(), Some(3));
        let started = Instant::now();
        // A shell that starts another, which loops for ever: as cmd reads
        // its line, not as a program's arguments are quoted.
        let error = run_to_exit(
            Command::new("cmd").raw_arg(r#"/c cmd /c "for /l %i in (0,0,1) do @rem""#),
            None,
            &CancelToken::new(),
            Duration::from_millis(500),
        )
        .unwrap_err();
        assert!(error.to_string().contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
