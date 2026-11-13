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
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::Context;
use skwir_core::CancelToken;
use skwir_core::os_path::find_program;

/// One of our optional worker executables: its name, and the variable
/// that says where it is instead.
#[derive(Clone, Copy)]
pub struct Worker {
    pub variable: &'static str,
    pub executable: &'static str,
}

impl Worker {
    /// Where it is, if it can run there: the variable's choice, else
    /// where skwir's own helpers are (`helper_folders`), else on `PATH`. A
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
    /// every worker: "Install skwir-archive-worker to open archives".
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
    pub fn requirement(self, purpose: &str) -> skwir_core::plugin::Requirement {
        skwir_core::plugin::Requirement::new(self.executable, purpose, move || self.works())
    }
}

/// How long a helper may take to answer `--probe`: `GStreamer`'s first
/// start reads every plugin it has.
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

/// Whether the helper at `path` can work here: run with `--probe`, the
/// one argument every helper of skwir's answers before any other, it
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

/// Which build of skwir this program is (`SKWIR_BUILD`, which its build
/// script sets, `tools/build/identity.rs`), told by the binary as it
/// starts (`set_build`) so that this crate is not built again for each
/// commit. Untold (a test), no failure is explained (`explain`).
static BUILD: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();

/// Say which build of skwir this program is: once, as it starts.
pub fn set_build(build: &'static str) {
    let _ = BUILD.set(build);
}

/// Which build of skwir this program is, as it said (`set_build`): its
/// version, and the commit unless it is a release. `None` untold.
pub fn build() -> Option<&'static str> {
    BUILD.get().copied()
}

/// How long a helper may take to answer `--build`, the first thing it
/// answers: only one older than the question, taking it for its input,
/// does anything else.
const BUILD_TIMEOUT: Duration = Duration::from_secs(5);

/// `error`, from a call of the helper at `helper`, and after it what may
/// explain it: the helper is of another build of skwir than this one, as
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
        Said::Build(theirs) => format!("another build of skwir ({theirs}; skwir is {ours})"),
        Said::Nothing => "an older build of skwir".to_owned(),
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

/// Where skwir's helpers are, from the folder its program is in: beside
/// it (a build, `just dist`, a Mac's bundle, Windows' installer), else
/// where a Linux package keeps a program's private helpers,
/// `/usr/bin/skwir` beside `/usr/lib/skwir/` (Debian, Arch) or
/// `/usr/libexec/skwir/` (Fedora, and the Nix package's `libexec/skwir`
/// beside its `bin`) (ADR-040, ADR-124). On Windows, a helper that
/// loads the libraries the package carries for it sits among them, in
/// their folder (the media helper in `gstreamer\bin`, ADR-076): Windows
/// finds a program's libraries beside it.
fn helper_folders(bin: &Path) -> Vec<PathBuf> {
    let mut folders = vec![bin.to_owned()];
    if cfg!(windows) {
        folders.push(bin.join("gstreamer").join("bin"));
    }
    if let Some(prefix) = bin.parent() {
        folders.push(prefix.join("lib").join("skwir"));
        folders.push(prefix.join("libexec").join("skwir"));
    }
    folders
}

/// The installed `program` something needs, as its real path: a sandbox
/// binds the store it lives in, not the profile's symlink farm.
pub fn required(program: &str) -> anyhow::Result<PathBuf> {
    find_program(program)
        .or_else(|| application(program))
        .and_then(|path| skwir_core::os_path::canonical(path).ok())
        .with_context(|| format!("{program} is not installed"))
}

/// Where an application that puts no program on any path keeps the one
/// skwir runs as `program`: `LibreOffice`'s `soffice`, in its bundle on
/// macOS (in `/Applications` or the user's own) and in its folder under
/// Program Files on Windows. Linux's packages put it on `PATH`.
fn application(program: &str) -> Option<PathBuf> {
    if program != "libreoffice" {
        return None;
    }
    let places: Vec<PathBuf> = if cfg!(target_os = "macos") {
        [
            Some(PathBuf::from("/Applications")),
            skwir_core::os_path::home().map(|home| home.join("Applications")),
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
    /// What the child left running was left be (`finish_leaving`): it
    /// ended well, and its daemon carries a mount. Set only then: a child
    /// stopped any other way (cancelled, too long) is stopped with its
    /// group, as any.
    left: bool,
    #[cfg(windows)]
    job: Option<crate::windows::Job>,
}

impl ChildGuard {
    pub(crate) fn new(child: Child) -> Self {
        Self {
            #[cfg(windows)]
            job: crate::windows::Job::holding(&child),
            child,
            left: false,
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

    /// Stop the child and what it started where they are (`SIGSTOP` to
    /// its group), or let them go on (`SIGCONT`): a paused copy's tool. On
    /// Windows, which has no such signal, each thread of the processes in
    /// its job is suspended, or resumed.
    fn hold(&mut self, held: bool) {
        #[cfg(unix)]
        if let Some(pid) = self.pid()
            && self.exited().is_ok_and(|exited| !exited)
        {
            let signal = if held {
                rustix::process::Signal::STOP
            } else {
                rustix::process::Signal::CONT
            };
            // What it started in a session of its own (bubblewrap's
            // `--new-session`, a helper isolated) is outside its group:
            // each process under it is held too, the group first, and let
            // go of last.
            if held {
                let _ = rustix::process::kill_process_group(pid, signal);
            }
            for under in descendants(pid) {
                let _ = rustix::process::kill_process(under, signal);
            }
            if !held {
                let _ = rustix::process::kill_process_group(pid, signal);
            }
        }
        #[cfg(windows)]
        if self.child.try_wait().is_ok_and(|ended| ended.is_none()) {
            let pids = match &self.job {
                Some(job) => job.processes(),
                None => vec![self.child.id()],
            };
            crate::windows::hold(&pids, held);
        }
    }

    /// Stop the child and what it left running, at once: what writes to
    /// it, or waits for it, hears so. It is reaped by `finish` or `Drop`.
    pub(crate) fn kill(&mut self) {
        self.stop_group();
        let _ = self.child.kill();
    }

    /// Stop the group and reap the child, which has exited.
    pub(crate) fn finish(self) -> io::Result<ExitStatus> {
        self.finish_leaving(false)
    }

    /// [`ChildGuard::finish`]; with `leave`, what the child started is left
    /// running when it ended well.
    fn finish_leaving(mut self, leave: bool) -> io::Result<ExitStatus> {
        // Told before it is reaped: once reaped, its pid no longer names
        // its group.
        if leave && self.ended_well() {
            #[cfg(windows)]
            if let Some(job) = self.job.take() {
                job.release();
            }
            self.left = true;
        } else {
            self.stop_group();
        }
        self.child.wait()
    }

    /// Whether the child exited, and with success, without reaping it.
    fn ended_well(&mut self) -> bool {
        #[cfg(unix)]
        {
            use rustix::process::{WaitId, WaitIdOptions, waitid};
            self.pid()
                .and_then(|pid| {
                    waitid(
                        WaitId::Pid(pid),
                        WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
                    )
                    .ok()
                    .flatten()
                })
                .is_some_and(|status| status.exit_status() == Some(0))
        }
        #[cfg(not(unix))]
        {
            self.child
                .try_wait()
                .ok()
                .flatten()
                .is_some_and(|status| status.success())
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.left {
            // Ended well, its daemon left running (`finish_leaving`):
            // reaped there.
            return;
        }
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
    run_until_exit(command, input, cancel, timeout, output, false, in_group)
}

/// [`run_capped`] for a program that must not meet the terminal Skwir was
/// started from, in a session of its own: an interactive shell opens
/// `/dev/tty`, finds its group is not the terminal's foreground and stops
/// itself until the time limit. With no terminal it goes on without job
/// control. Stopped whole as [`run_capped`] stops it: a session leader's
/// group is its own.
///
/// # Errors
/// As [`run_capped`].
#[cfg(unix)]
pub fn run_capped_without_terminal(
    command: &mut Command,
    input: Option<&[u8]>,
    cancel: &CancelToken,
    timeout: Duration,
    output: u64,
) -> anyhow::Result<Finished> {
    run_until_exit(command, input, cancel, timeout, output, false, in_session)
}

/// A program in a process group of its own: stopped with what it started.
#[cfg(unix)]
fn in_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn in_group(_: &mut Command) {}

/// A program in a session of its own, no terminal its own either. Not with
/// [`in_group`]: `setsid` refuses a process that leads a group already.
#[cfg(unix)]
fn in_session(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: what runs between fork and exec is one system call, which
    // allocates nothing (an `io::Error` of a code is no allocation), as
    // `pre_exec` asks.
    #[allow(unsafe_code)]
    unsafe {
        command.pre_exec(|| {
            rustix::process::setsid()?;
            Ok(())
        });
    }
}

/// [`run_to_exit`] for a program that leaves another running when it
/// ends well, which outlives skwir: a tool that mounts a remote and
/// daemonizes, its ssh carrying the mount (sshfs, `rclone mount --daemon`,
/// ADR-077). Cancelled, too long, or ended badly, it and what it started
/// are stopped as `run_to_exit` stops them; ended well, nothing it left
/// running is touched.
///
/// # Errors
/// As [`run_to_exit`].
pub fn run_leaving(
    command: &mut Command,
    cancel: &CancelToken,
    timeout: Duration,
) -> anyhow::Result<Finished> {
    run_until_exit(
        command,
        None,
        cancel,
        timeout,
        crate::inspection::limits::HELPER_OUTPUT,
        true,
        in_group,
    )
}

fn run_until_exit(
    command: &mut Command,
    input: Option<&[u8]>,
    cancel: &CancelToken,
    timeout: Duration,
    output: u64,
    leave: bool,
    apart: fn(&mut Command),
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
    apart(command);
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
    let status = child.finish_leaving(leave)?;
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

/// Run `command` to its end, as [`run_to_exit`] does but without a time
/// limit, for a program that says how far it went as it goes (rsync's
/// progress): `watch` is given what it writes to standard output as it
/// writes it, on a thread of the call's, through a pipe that thread
/// drains. Cancelled, it is stopped with what it started; while `held`
/// says so (a paused plan's), it and what it started are held where they
/// are (on Unix: a program has no pause of its own to ask). A failed exit
/// status is not an error.
///
/// # Errors
/// When it cannot start, or is cancelled.
pub fn run_watching(
    command: &mut Command,
    input: Option<&[u8]>,
    cancel: &CancelToken,
    held: &dyn Fn() -> bool,
    watch: &mut (dyn FnMut(&[u8]) + Send),
) -> anyhow::Result<Finished> {
    watched(command, input, cancel, Limit::None, held, watch, &|_| {})
}

/// [`run_watching`], `look` given its process's id as it runs, every
/// `CHECK_EVERY`: what follows the files it has open
/// ([`open_files`]) where it says nothing of each (ADR-135).
///
/// # Errors
/// When it cannot start, or is cancelled.
pub fn run_looking(
    command: &mut Command,
    input: Option<&[u8]>,
    cancel: &CancelToken,
    held: &dyn Fn() -> bool,
    watch: &mut (dyn FnMut(&[u8]) + Send),
    look: &dyn Fn(u32),
) -> anyhow::Result<Finished> {
    watched(command, input, cancel, Limit::None, held, watch, look)
}

/// A regular file a process has open, and how far into it it has read or
/// written (ADR-135).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenFile {
    pub path: PathBuf,
    pub at: u64,
}

/// The files process `pid` and those it started have open, each with its
/// offset, as the system says: on Linux `/proc`'s `fd` and `fdinfo`, on
/// macOS `proc_pidinfo` (the process alone); none elsewhere, which says
/// no other process's offsets. What a helper copying files says of none
/// of them as it goes (sftp, gio) is followed so: it reads a file it sends
/// from its start, and writes one it brings from its start.
pub fn open_files(pid: u32) -> Vec<OpenFile> {
    #[cfg(target_os = "linux")]
    {
        let mut pids = vec![pid];
        if let Some(own) = i32::try_from(pid)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
        {
            pids.extend(
                descendants(own)
                    .into_iter()
                    .filter_map(|child| u32::try_from(child.as_raw_nonzero().get()).ok()),
            );
        }
        pids.into_iter().flat_map(open_files_of).collect()
    }
    #[cfg(target_os = "macos")]
    {
        mac_open::open_files_of(pid)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        Vec::new()
    }
}

/// `pid`'s open regular files, from `/proc`: each descriptor's link, and
/// its `pos` in `fdinfo`. A link to no path (a pipe, a socket) or to a
/// file removed since is left out.
#[cfg(target_os = "linux")]
fn open_files_of(pid: u32) -> Vec<OpenFile> {
    let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
        return Vec::new();
    };
    fds.flatten()
        .filter_map(|fd| {
            let path = std::fs::read_link(fd.path()).ok()?;
            if !path.is_absolute() || path.as_os_str().as_encoded_bytes().ends_with(b" (deleted)") {
                return None;
            }
            let info =
                std::fs::read_to_string(format!("/proc/{pid}/fdinfo/{}", fd.file_name().display()))
                    .ok()?;
            let at = info
                .lines()
                .find_map(|line| line.strip_prefix("pos:"))?
                .trim()
                .parse()
                .ok()?;
            Some(OpenFile { path, at })
        })
        .collect()
}

/// A process's open files on macOS (`libproc`): its descriptors
/// (`PROC_PIDLISTFDS`), then each vnode's path and offset
/// (`PROC_PIDFDVNODEPATHINFO`).
#[cfg(target_os = "macos")]
mod mac_open {
    use super::OpenFile;
    use std::os::unix::ffi::OsStrExt;

    /// `struct proc_fileinfo` of `<sys/proc_info.h>`, which libc leaves out.
    // Its fields keep the C names, as the header has them.
    #[allow(clippy::struct_field_names)]
    #[repr(C)]
    struct ProcFileinfo {
        fi_openflags: u32,
        fi_status: u32,
        fi_offset: libc::off_t,
        fi_type: i32,
        fi_guardflags: u32,
    }

    /// `struct vnode_fdinfowithpath`.
    #[repr(C)]
    struct VnodeFdinfoWithPath {
        pfi: ProcFileinfo,
        pvip: libc::vnode_info_path,
    }

    const PROC_PIDFDVNODEPATHINFO: libc::c_int = 2;

    // libproc's calls write at most the size given into the buffers this
    // makes, each as large as it says: no other way to read another
    // process's descriptors on macOS.
    #[allow(unsafe_code)]
    pub(super) fn open_files_of(pid: u32) -> Vec<OpenFile> {
        let Ok(pid) = libc::c_int::try_from(pid) else {
            return Vec::new();
        };
        let unit = std::mem::size_of::<libc::proc_fdinfo>();
        // SAFETY: a null buffer asks for the size needed.
        let needed =
            unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
        let Ok(needed) = usize::try_from(needed) else {
            return Vec::new();
        };
        // Room for descriptors opened since.
        let mut fds: Vec<libc::proc_fdinfo> = Vec::with_capacity(needed / unit + 8);
        let room = libc::c_int::try_from(fds.capacity() * unit).unwrap_or(libc::c_int::MAX);
        // SAFETY: `fds` holds `room` bytes; the call writes no more.
        let got = unsafe {
            libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, fds.as_mut_ptr().cast(), room)
        };
        let Ok(got) = usize::try_from(got) else {
            return Vec::new();
        };
        // SAFETY: the call wrote `got` bytes of whole entries.
        unsafe { fds.set_len((got / unit).min(fds.capacity())) };
        fds.iter()
            .filter(|fd| fd.proc_fdtype == libc::PROX_FDTYPE_VNODE as u32)
            .filter_map(|fd| {
                let mut info = std::mem::MaybeUninit::<VnodeFdinfoWithPath>::zeroed();
                let size =
                    libc::c_int::try_from(std::mem::size_of::<VnodeFdinfoWithPath>()).ok()?;
                // SAFETY: `info` is `size` bytes, zeroed.
                let wrote = unsafe {
                    libc::proc_pidfdinfo(
                        pid,
                        fd.proc_fd,
                        PROC_PIDFDVNODEPATHINFO,
                        info.as_mut_ptr().cast(),
                        size,
                    )
                };
                if wrote != size {
                    return None;
                }
                // SAFETY: the call filled it whole.
                let info = unsafe { info.assume_init() };
                // Only a regular file.
                if (info.pvip.vip_vi.vi_stat.vst_mode & libc::S_IFMT) != libc::S_IFREG {
                    return None;
                }
                let bytes: Vec<u8> = info
                    .pvip
                    .vip_path
                    .iter()
                    .flatten()
                    .take_while(|c| **c != 0)
                    .map(|c| *c as u8)
                    .collect();
                let path = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(&bytes));
                let at = u64::try_from(info.pfi.fi_offset).ok()?;
                path.is_absolute().then_some(OpenFile { path, at })
            })
            .collect()
    }
}

/// How a helper's work is followed: told how far it went, from 0 to 1,
/// and asked whether to hold it (a paused plan's). A part of a larger
/// whole is followed as that part ([`Follow::part`]): a file of several,
/// the helper's share of a step.
#[derive(Clone, Copy)]
pub struct Follow<'a> {
    went: &'a (dyn Fn(f64) + Sync),
    held: &'a (dyn Fn() -> bool + Sync),
    from: f64,
    share: f64,
}

impl<'a> Follow<'a> {
    pub fn new(went: &'a (dyn Fn(f64) + Sync), held: &'a (dyn Fn() -> bool + Sync)) -> Self {
        Follow {
            went,
            held,
            from: 0.0,
            share: 1.0,
        }
    }

    /// Nothing follows it.
    pub fn none() -> Follow<'static> {
        fn nothing(_: f64) {}
        fn never() -> bool {
            false
        }
        Follow::new(&nothing, &never)
    }

    /// It went `fraction` (0 to 1) of its way.
    pub fn went(&self, fraction: f64) {
        (self.went)(self.from + self.share * fraction.clamp(0.0, 1.0));
    }

    /// Whether to hold it.
    pub fn held(&self) -> bool {
        (self.held)()
    }

    /// Its way from `start` to `end` (each 0 to 1), followed as a whole.
    #[must_use]
    pub fn part(&self, start: f64, end: f64) -> Follow<'a> {
        Follow {
            from: self.from + self.share * start,
            share: self.share * (end - start).max(0.0),
            ..*self
        }
    }
}

/// A step of a plan followed as it goes (`op::Gauge`): its work, `total`
/// bytes announced, counted as a helper says how far it went; held while
/// the plan is paused.
pub struct Metered {
    gauge: skwir_core::op::Gauge,
    total: u64,
    counted: std::sync::atomic::AtomicU64,
}

impl Metered {
    pub fn new(gauge: skwir_core::op::Gauge, total: u64) -> Self {
        Metered {
            gauge,
            total,
            counted: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// It went `fraction` of its way: what was not counted yet, counted.
    pub fn went(&self, fraction: f64) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let reached = (fraction.clamp(0.0, 1.0) * self.total as f64) as u64;
        let before = self
            .counted
            .fetch_max(reached, std::sync::atomic::Ordering::Relaxed);
        if reached > before {
            self.gauge.advance(reached - before);
        }
    }

    /// Whether its plan is paused.
    pub fn held(&self) -> bool {
        self.gauge.is_paused()
    }
}

/// What a helper of Skwir's says of how far it went, a line of its
/// standard output: `progress <done> <total>`.
pub const PROGRESS: &str = "progress ";

/// A helper of Skwir's run as [`run_to_exit`] runs a program, followed as
/// it goes: each line it writes `progress <done> <total>` ([`PROGRESS`])
/// tells `follow` how far it went, the other lines are its output; it is
/// held while `follow` says (on Unix), stopped when `cancel` fires or past
/// `timeout` of the time it ran (held, it does not run).
///
/// # Errors
/// When it cannot start, is cancelled, takes too long or says too much.
pub fn run_helper(
    command: &mut Command,
    cancel: &CancelToken,
    timeout: Duration,
    follow: &Follow<'_>,
) -> anyhow::Result<Finished> {
    helper(command, cancel, Bound::Total(timeout), follow)
}

/// [`run_helper`], stopped once it said nothing of how far it went (no
/// `progress` line, its start counting as one) for `stall` of the time it
/// ran, or past `ceiling` in all: a helper whose work cannot be told
/// ahead (a document's pages, a picture's) is given what it takes while
/// it goes, and stopped once it stops going.
///
/// # Errors
/// As [`run_helper`]'s; stalled, its words say for how long.
pub fn run_helper_stalling(
    command: &mut Command,
    cancel: &CancelToken,
    stall: Duration,
    ceiling: Duration,
    follow: &Follow<'_>,
) -> anyhow::Result<Finished> {
    helper(command, cancel, Bound::Stalled { stall, ceiling }, follow)
}

/// How long a helper may run, its held time left out.
#[derive(Clone, Copy)]
enum Bound {
    Total(Duration),
    Stalled { stall: Duration, ceiling: Duration },
}

/// [`run_helper`] within `bound`.
fn helper(
    command: &mut Command,
    cancel: &CancelToken,
    bound: Bound,
    follow: &Follow<'_>,
) -> anyhow::Result<Finished> {
    let limit = usize::try_from(crate::inspection::limits::HELPER_OUTPUT).unwrap_or(usize::MAX);
    let mut kept: Vec<u8> = Vec::new();
    let mut line: Vec<u8> = Vec::new();
    let mut too_much = false;
    let follow = *follow;
    // The progress lines it wrote: a stall is told by their count.
    let said = AtomicU64::new(0);
    let finished = {
        let take = |line: &[u8], kept: &mut Vec<u8>, too_much: &mut bool| {
            if let Some(progress) = line.strip_prefix(PROGRESS.as_bytes()) {
                let progress = String::from_utf8_lossy(progress);
                let mut numbers = progress.split_whitespace().map(str::parse::<u64>);
                if let (Some(Ok(done)), Some(Ok(total))) = (numbers.next(), numbers.next()) {
                    said.fetch_add(1, Ordering::Relaxed);
                    if total > 0 {
                        #[allow(clippy::cast_precision_loss)]
                        follow.went(done as f64 / total as f64);
                    }
                }
            } else if kept.len() + line.len() < limit {
                kept.extend_from_slice(line);
                kept.push(b'\n');
            } else {
                *too_much = true;
            }
        };
        let mut watch = |bytes: &[u8]| {
            for &byte in bytes {
                if byte == b'\n' {
                    take(&line, &mut kept, &mut too_much);
                    line.clear();
                } else if line.len() < limit {
                    line.push(byte);
                }
            }
        };
        let limit = match bound {
            Bound::Total(timeout) => Limit::Total(timeout),
            Bound::Stalled { stall, ceiling } => Limit::Stalled {
                stall,
                ceiling,
                said: &said,
            },
        };
        let finished = watched(
            command,
            None,
            cancel,
            limit,
            &|| follow.held(),
            &mut watch,
            &|_| {},
        )?;
        if !line.is_empty() {
            take(&line, &mut kept, &mut too_much);
        }
        finished
    };
    let program = Path::new(command.get_program())
        .file_name()
        .unwrap_or(command.get_program())
        .to_string_lossy()
        .into_owned();
    anyhow::ensure!(!too_much, "{program}: excessive output");
    Ok(Finished {
        stdout: kept,
        ..finished
    })
}

/// How long a program [`watched`] may run, its held time left out.
#[derive(Clone, Copy)]
enum Limit<'a> {
    None,
    /// In all.
    Total(Duration),
    /// Since `said` last grew (its start counting), and in all.
    Stalled {
        stall: Duration,
        ceiling: Duration,
        said: &'a AtomicU64,
    },
}

/// `time` as a stall is said: whole minutes past one, else seconds.
fn said_for(time: Duration) -> String {
    let (n, unit) = match time.as_secs() {
        secs @ 60.. => (secs.saturating_add(30) / 60, "minute"),
        secs => (secs.max(1), "second"),
    };
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

/// [`run_watching`], stopped past its `limit`.
fn watched(
    command: &mut Command,
    input: Option<&[u8]>,
    cancel: &CancelToken,
    limit: Limit<'_>,
    held: &dyn Fn() -> bool,
    watch: &mut (dyn FnMut(&[u8]) + Send),
    look: &dyn Fn(u32),
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
    let mut stderr = tempfile::tempfile()?;
    command
        .stdin(stdin)
        .stdout(Stdio::piped())
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
    let mut out = child
        .child
        .stdout
        .take()
        .context("its output was not piped")?;
    let exit = ExitSignal::of(&child);
    let pid = child.child.id();
    let status = std::thread::scope(|s| {
        s.spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match out.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => watch(&buf[..n]),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        });
        let mut holding = false;
        // Held, it is not taking long: its time counts while it runs.
        let mut ran = Duration::ZERO;
        let mut since = Instant::now();
        // The progress lines heard, and how long it had run then.
        let mut heard = (0, Duration::ZERO);
        loop {
            if cancel.is_cancelled() {
                // Stopped with its group: the pipe's writers gone, the
                // reader ends. A process held is killed as it is.
                drop(child);
                anyhow::bail!("{program} was cancelled");
            }
            if !holding {
                ran += since.elapsed();
            }
            since = Instant::now();
            let stalled = match limit {
                Limit::None => None,
                Limit::Total(timeout) => (ran >= timeout).then(|| format!("{program} timed out")),
                Limit::Stalled {
                    stall,
                    ceiling,
                    said,
                } => {
                    let now = said.load(Ordering::Relaxed);
                    if now != heard.0 {
                        heard = (now, ran);
                    }
                    if ran >= ceiling {
                        Some(format!("{program} timed out"))
                    } else {
                        (ran.saturating_sub(heard.1) >= stall).then(|| {
                            format!(
                                "{program} stopped: it said nothing of its progress for {}",
                                said_for(stall)
                            )
                        })
                    }
                }
            };
            if let Some(stalled) = stalled {
                drop(child);
                anyhow::bail!(stalled);
            }
            if child.exited()? {
                break;
            }
            if held() != holding {
                holding = !holding;
                child.hold(holding);
            }
            if !holding {
                look(pid);
            }
            exit.wait(CHECK_EVERY);
        }
        Ok(child.finish()?)
    })?;
    stderr.rewind()?;
    let mut diagnostic = Vec::new();
    stderr.take(DIAGNOSTIC).read_to_end(&mut diagnostic)?;
    Ok(Finished {
        status,
        stdout: Vec::new(),
        stderr: String::from_utf8_lossy(&diagnostic).trim().to_owned(),
    })
}

/// The processes under `pid`, its children and theirs, as the system says
/// (on Linux, `/proc`'s `children`); none where it does not, a group
/// signal reaching what stays in its session there.
#[cfg(unix)]
fn descendants(pid: rustix::process::Pid) -> Vec<rustix::process::Pid> {
    #[cfg(target_os = "linux")]
    {
        let mut found = Vec::new();
        let mut next = vec![pid.as_raw_nonzero().get()];
        while let Some(parent) = next.pop() {
            let Ok(tasks) = std::fs::read_dir(format!("/proc/{parent}/task")) else {
                continue;
            };
            for task in tasks.flatten() {
                let Ok(children) = std::fs::read_to_string(task.path().join("children")) else {
                    continue;
                };
                for child in children
                    .split_whitespace()
                    .filter_map(|c| c.parse::<i32>().ok())
                {
                    if let Some(child_pid) = rustix::process::Pid::from_raw(child) {
                        found.push(child_pid);
                        next.push(child);
                    }
                }
            }
        }
        found
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        Vec::new()
    }
}

/// What to do when a watched child exits. It runs on the reaper thread,
/// which every watched child shares: do something brief (send on a channel,
/// schedule a job, wake the app), never the work itself.
pub type OnExit = Box<dyn FnOnce(io::Result<ExitStatus>) + Send>;

/// The other programs that use something under `folder`, by name, each
/// once: a file open there, a file of it mapped, their working folder or
/// root in it. A watch (inotify, fanotify) is no use: the system refuses
/// to unmount a folder watched as one read, but what watches it lets go
/// when it goes. This process is left out: what it holds there, its
/// caller knows (Eject asks the actions under way there; a read given up
/// on, waiting for a server, is no use). A process of another user is not
/// seen. `None` where the system does not say: on Linux, `/proc` says; on
/// macOS, `lsof` (a part of the system there; compiled only); Windows has
/// no mount of Skwir's to let go of yet.
pub fn using(folder: &Path) -> Option<Vec<String>> {
    #[cfg(target_os = "linux")]
    {
        let me = std::process::id().to_string();
        let mut names = std::collections::BTreeSet::new();
        for process in std::fs::read_dir("/proc").ok()?.flatten() {
            let pid = process.file_name();
            if pid.as_encoded_bytes().iter().all(u8::is_ascii_digit)
                && pid != me.as_str()
                && uses(&process.path(), folder)
            {
                let name = std::fs::read_to_string(process.path().join("comm")).unwrap_or_default();
                names.insert(name.trim().to_owned());
            }
        }
        Some(names.into_iter().collect())
    }
    #[cfg(target_os = "macos")]
    {
        // `+f`: the files open on the file system mounted there. `lsof`
        // stats the mount point: one whose server is gone holds it, so it
        // is given a time, then nothing is said.
        let mut command = Command::new("/usr/sbin/lsof");
        command.args(["-w", "-F", "pc", "+f", "--"]).arg(folder);
        let finished = run_to_exit(
            &mut command,
            None,
            &CancelToken::new(),
            Duration::from_secs(10),
        )
        .ok()?;
        // Nothing open there is an exit status of 1, said nothing of.
        if !finished.status.success() && !finished.stderr.is_empty() {
            return None;
        }
        // A process's `p<pid>` line, then its `c<name>`.
        let me = std::process::id().to_string();
        let mut names = std::collections::BTreeSet::new();
        let mut mine = false;
        for line in String::from_utf8_lossy(&finished.stdout).lines() {
            if let Some(pid) = line.strip_prefix('p') {
                mine = pid == me;
            } else if let Some(name) = line.strip_prefix('c')
                && !mine
            {
                names.insert(name.to_owned());
            }
        }
        Some(names.into_iter().collect())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = folder;
        None
    }
}

/// Whether the process whose `/proc` folder is `process` uses something
/// under `folder` ([`using`]). Read from what the system keeps of it,
/// never by looking under `folder`: a server gone would hold the look.
#[cfg(target_os = "linux")]
fn uses(process: &Path, folder: &Path) -> bool {
    let under = |link: &Path| std::fs::read_link(link).is_ok_and(|to| to.starts_with(folder));
    let links = |dir: &Path, name: Option<&str>| -> bool {
        std::fs::read_dir(dir).is_ok_and(|entries| {
            entries.flatten().any(|entry| match name {
                Some(name) => under(&entry.path().join(name)),
                None => under(&entry.path()),
            })
        })
    };
    if under(&process.join("cwd")) || under(&process.join("root")) {
        return true;
    }
    // A thread may work in a folder of its own.
    if links(&process.join("task"), Some("cwd")) || links(&process.join("fd"), None) {
        return true;
    }
    // A line of `maps` ends with the mapped file's path, if any.
    std::fs::read(process.join("maps")).is_ok_and(|maps| {
        maps.split(|b| *b == b'\n').any(|line| {
            line.iter().position(|b| *b == b'/').is_some_and(|at| {
                Path::new(&*skwir_core::os_path::from_bytes_ref(&line[at..])).starts_with(folder)
            })
        })
    })
}

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

/// Have `command`'s program killed by the system once the thread that
/// starts it ends, so too when Skwir ends without stopping it (a crash, a
/// kill): Linux's parent-death signal, set before it runs and kept across
/// its `exec`. Should Skwir have ended already, it does not run at all.
/// The signal comes when that *thread* ends, not the process: start it
/// from a thread that outlives it (one that stops it before ending).
#[cfg(target_os = "linux")]
pub fn ends_with_starter(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    let starter = rustix::process::getpid();
    // SAFETY: what runs between fork and exec makes two system calls and
    // allocates nothing (an `io::Error` of a code or a kind is no
    // allocation), as `pre_exec` asks.
    #[allow(unsafe_code)]
    unsafe {
        command.pre_exec(move || {
            rustix::process::set_parent_process_death_signal(Some(rustix::process::Signal::KILL))?;
            // Ended before the signal was set: it would never come.
            if rustix::process::getppid() != Some(starter) {
                return Err(io::ErrorKind::Other.into());
            }
            Ok(())
        });
    }
}

/// The processes of this user one of whose arguments is `arg`, by pid
/// (`/proc`): what a Skwir that ended left running, found by the socket
/// it was told to listen on.
#[cfg(target_os = "linux")]
pub fn naming(arg: &std::ffi::OsStr) -> Vec<u32> {
    use std::os::unix::fs::MetadataExt;
    let (me, user) = (std::process::id(), rustix::process::geteuid().as_raw());
    let Ok(processes) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    processes
        .flatten()
        .filter_map(|process| {
            let pid = process.file_name().to_str()?.parse::<u32>().ok()?;
            let ours = process.metadata().is_ok_and(|m| m.uid() == user);
            let line = std::fs::read(process.path().join("cmdline")).ok()?;
            (pid != me && ours && line.split(|b| *b == 0).any(|a| a == arg.as_encoded_bytes()))
                .then_some(pid)
        })
        .collect()
}

/// Kill the process `pid` (`SIGKILL`: what was left running is owed no
/// goodbye); whether it was signalled.
#[cfg(target_os = "linux")]
pub fn kill(pid: u32) -> bool {
    i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
        .is_some_and(|pid| {
            rustix::process::kill_process(pid, rustix::process::Signal::KILL).is_ok()
        })
}

/// Start `command` and let it run on its own: in a process group of its own
/// (a Ctrl+C in the terminal skwir was started from does not reach it), its
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
        .name("skwir-wait".into())
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
                .name("skwir-reaper".into())
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

/// A helper kept for a session (`document`'s, `archive`'s), as its manager
/// thread talks to it: requests and replies are JSON lines
/// ([`LineChannel`](crate::line_channel::LineChannel)), each request tagged
/// with an id and answered once, in order, starting with the greeting as 0
/// (`document::Tagged`), so the answer to a request given up on is drained,
/// not a reason to start another helper.
pub(crate) struct Helper<Reply> {
    pub(crate) channel: crate::line_channel::LineChannel<skwir_core::document::Tagged<Reply>>,
    /// What the helper is called in what goes wrong: "document worker".
    what: &'static str,
    /// Id of the last request sent, and of the next reply due.
    sent: u64,
    due: u64,
}

/// Why a helper's answer did not come.
pub(crate) enum Failure {
    Cancelled,
    /// The helper cannot be trusted with another request: it timed out,
    /// exited or broke the protocol.
    Broken(anyhow::Error),
}

impl<Reply: serde::de::DeserializeOwned + Send + 'static> Helper<Reply> {
    /// A helper started on `channel`, its greeting not read yet.
    pub(crate) fn new(
        channel: crate::line_channel::LineChannel<skwir_core::document::Tagged<Reply>>,
        what: &'static str,
    ) -> Self {
        Helper {
            channel,
            what,
            sent: 0,
            due: 0,
        }
    }

    /// Whether a request sent is still unanswered.
    pub(crate) fn busy(&self) -> bool {
        self.due <= self.sent
    }

    /// Send `request`: its id.
    pub(crate) fn send(&mut self, request: &impl serde::Serialize) -> anyhow::Result<u64> {
        let id = self.sent + 1;
        self.channel
            .send(&skwir_core::document::Tagged { id, body: request })?;
        self.sent = id;
        Ok(id)
    }

    /// Take one reply off the channel, in order.
    pub(crate) fn accept(
        &mut self,
        reply: Result<
            anyhow::Result<skwir_core::document::Tagged<Reply>>,
            crossbeam_channel::RecvError,
        >,
    ) -> anyhow::Result<skwir_core::document::Tagged<Reply>> {
        let what = self.what;
        let reply = reply.map_err(|_| anyhow::anyhow!("{what} exited unexpectedly"))??;
        anyhow::ensure!(
            reply.id == self.due && reply.id <= self.sent,
            "{what} answered out of order"
        );
        self.due += 1;
        Ok(reply)
    }

    /// Wait for the reply to `id`, dropping the answers to earlier requests.
    pub(crate) fn wait(
        &mut self,
        id: u64,
        cancelled: &skwir_core::cancel::CancelSignal,
        deadline: &crossbeam_channel::Receiver<Instant>,
    ) -> Result<Reply, Failure> {
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
                    let what = self.what;
                    return Err(Failure::Broken(anyhow::anyhow!("{what} timed out")));
                }
            }
        }
    }

    /// Wait until every request sent has been answered.
    pub(crate) fn settle(
        &mut self,
        cancelled: &skwir_core::cancel::CancelSignal,
        deadline: &crossbeam_channel::Receiver<Instant>,
    ) -> Result<(), Failure> {
        if self.busy() {
            self.wait(self.sent, cancelled, deadline)?;
        }
        Ok(())
    }
}

#[cfg(test)]
// Helpers of the tests that run on Unix alone (ADR-035).
#[cfg_attr(not(unix), allow(dead_code, unused_imports))]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_program_watched_is_heard_as_it_writes_and_stopped_when_cancelled() {
        let mut said = Vec::new();
        let finished = run_watching(
            Command::new("sh").args([
                "-c",
                "printf one; sleep 0.2; printf two; echo oops >&2; exit 3",
            ]),
            None,
            &CancelToken::new(),
            &|| false,
            &mut |bytes: &[u8]| said.extend_from_slice(bytes),
        )
        .unwrap();
        assert_eq!(said, b"onetwo");
        assert_eq!(finished.status.code(), Some(3), "a failed exit is no error");
        assert_eq!(finished.stderr, "oops");
        let cancel = CancelToken::new();
        let later = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            later.cancel();
        });
        let started = Instant::now();
        // What it started holds the pipe too: stopped with it.
        let ran = run_watching(
            Command::new("sh").args(["-c", "sleep 30 & sleep 30"]),
            None,
            &cancel,
            &|| false,
            &mut |_: &[u8]| {},
        );
        assert!(ran.is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    /// Held, a program holds what it started in a session of its own (as
    /// bubblewrap's `--new-session` starts a helper), out of its group.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_program_held_holds_what_it_started_in_a_session_of_its_own() {
        if Command::new("setsid").arg("--version").output().is_err() {
            eprintln!("skipped: no setsid here");
            return;
        }
        let said = std::sync::Mutex::new(String::new());
        let hold = std::sync::atomic::AtomicBool::new(false);
        let cancel = CancelToken::new();
        let state = std::thread::scope(|s| {
            let looker = s.spawn(|| {
                let deadline = Instant::now() + Duration::from_secs(10);
                let pid = loop {
                    if let Some(pid) = said.lock().unwrap().lines().next() {
                        break pid.trim().to_owned();
                    }
                    assert!(Instant::now() < deadline, "no pid said");
                    std::thread::sleep(Duration::from_millis(20));
                };
                hold.store(true, std::sync::atomic::Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(400));
                let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
                let state = stat
                    .rsplit(')')
                    .next()
                    .and_then(|rest| rest.split_whitespace().next())
                    .map(str::to_owned);
                cancel.cancel();
                // Let go of, so it ends with its group.
                if let Ok(pid) = pid.parse::<i32>()
                    && let Some(pid) = rustix::process::Pid::from_raw(pid)
                {
                    let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
                }
                state
            });
            let _ = run_watching(
                Command::new("sh").args(["-c", "setsid sleep 30 & echo $!; wait"]),
                None,
                &cancel,
                &|| hold.load(std::sync::atomic::Ordering::Relaxed),
                &mut |bytes: &[u8]| {
                    said.lock()
                        .unwrap()
                        .push_str(&String::from_utf8_lossy(bytes));
                },
            );
            looker.join().unwrap()
        });
        assert_eq!(state.as_deref(), Some("T"), "held, out of its group");
    }

    /// A helper's progress lines tell how far it went, as a part of the
    /// whole; its other lines are its output; past its time it is stopped.
    #[cfg(unix)]
    #[test]
    fn a_helper_s_progress_is_followed_and_its_answer_kept() {
        let heard = std::sync::Mutex::new(Vec::new());
        let went = |fraction: f64| heard.lock().unwrap().push(fraction);
        let never = || false;
        let whole = Follow::new(&went, &never);
        let finished = run_helper(
            Command::new("sh").args([
                "-c",
                "echo progress 1 4; echo progress 2 4; echo progress x; echo '{\"done\":true}'",
            ]),
            &CancelToken::new(),
            Duration::from_secs(10),
            &whole.part(0.5, 1.0),
        )
        .unwrap();
        assert_eq!(*heard.lock().unwrap(), [0.625, 0.75]);
        assert_eq!(finished.stdout, b"{\"done\":true}\n");
        let started = Instant::now();
        let ran = run_helper(
            Command::new("sh").args(["-c", "sleep 30"]),
            &CancelToken::new(),
            Duration::from_millis(300),
            &Follow::none(),
        );
        assert!(ran.unwrap_err().to_string().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    /// A helper stopped once it stalls: given what it takes while it says
    /// how far it went, stopped once it says nothing for the stall, or past
    /// the ceiling however it goes; the stall said in its words.
    #[cfg(unix)]
    #[test]
    fn a_helper_is_stopped_once_it_says_nothing_of_its_progress() {
        let run = |script: &str, stall: u64, ceiling: u64| {
            let started = Instant::now();
            let ran = run_helper_stalling(
                Command::new("sh").args(["-c", script]),
                &CancelToken::new(),
                Duration::from_millis(stall),
                Duration::from_millis(ceiling),
                &Follow::none(),
            );
            (ran, started.elapsed())
        };
        // Going on longer than the stall, saying how far it went.
        let (ran, _) = run(
            "for i in 1 2 3 4 5 6 7 8; do echo progress $i 8; sleep 0.1; done; echo done",
            500,
            20_000,
        );
        assert_eq!(ran.unwrap().stdout, b"done\n");
        // Silent after its first line.
        let (ran, took) = run("echo progress 1 4; sleep 30", 400, 20_000);
        let said = ran.unwrap_err().to_string();
        assert!(
            said.contains("said nothing of its progress for 1 second"),
            "{said}"
        );
        assert!(took < Duration::from_secs(10), "{took:?}");
        // Never a line: its start counts as one.
        let (ran, took) = run("sleep 30", 300, 20_000);
        assert!(ran.unwrap_err().to_string().contains("said nothing"));
        assert!(took < Duration::from_secs(10), "{took:?}");
        // Past its ceiling, however it goes.
        let (ran, took) = run(
            "while true; do echo progress 1 2; sleep 0.05; done",
            5_000,
            400,
        );
        assert!(ran.unwrap_err().to_string().contains("timed out"));
        assert!(took < Duration::from_secs(10), "{took:?}");
        assert_eq!(said_for(Duration::from_secs(180)), "3 minutes");
        assert_eq!(said_for(Duration::from_secs(60)), "1 minute");
        assert_eq!(said_for(Duration::from_millis(200)), "1 second");
    }

    /// What `a_program_that_ends_with_its_starter_goes_when_skwir_is_killed`
    /// kills: a `sleep` started to end with it, its pid said, then waiting
    /// to be killed.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "run by a_program_that_ends_with_its_starter_goes_when_skwir_is_killed"]
    fn starts_until_killed() {
        if std::env::var_os("SKWIR_TEST_STARTER").is_none() {
            return;
        }
        let mut command = Command::new("sleep");
        command
            .arg("1000")
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        ends_with_starter(&mut command);
        let mut child = command.spawn().unwrap();
        println!("started {}", child.id());
        let _ = io::stdin().read(&mut [0]);
        // Let go of, not killed: stopped all the same.
        let _ = child.kill();
        let _ = child.wait();
    }

    /// A program started to end with its starter is killed by the system
    /// once the Skwir that started it is (a crash, a kill), though nothing
    /// stopped it.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_program_that_ends_with_its_starter_goes_when_skwir_is_killed() {
        use std::io::BufRead;
        let mut starter = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "process::tests::starts_until_killed",
                "--ignored",
                "--nocapture",
            ])
            .env("SKWIR_TEST_STARTER", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut said = io::BufReader::new(starter.stdout.take().unwrap());
        let mut line = String::new();
        let pid = loop {
            line.clear();
            assert!(said.read_line(&mut line).unwrap() > 0, "it started nothing");
            if let Some(pid) = line.trim().strip_prefix("started ") {
                break pid.parse::<u32>().unwrap();
            }
        };
        // There, and no zombie.
        let alive = |pid: u32| {
            std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
                stat.rsplit_once(") ")
                    .is_some_and(|(_, rest)| !rest.starts_with('Z'))
            })
        };
        assert!(alive(pid), "started");
        starter.kill().unwrap();
        starter.wait().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while alive(pid) {
            if Instant::now() > deadline {
                kill(pid);
                panic!("it outlived the Skwir that started it");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// A program working in a folder is one of those using it; this
    /// process, a file held open there, is not; a folder nothing holds is
    /// used by none.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_programs_using_a_folder_are_named() {
        let tmp = tempfile::tempdir().unwrap();
        let (held, free, mine) = (
            tmp.path().join("held"),
            tmp.path().join("free"),
            tmp.path().join("mine"),
        );
        for folder in [&held, &free, &mine] {
            std::fs::create_dir(folder).unwrap();
        }
        let mut other = Command::new("sleep")
            .arg("30")
            .current_dir(&held)
            .spawn()
            .unwrap();
        let open = std::fs::File::create(mine.join("it's open")).unwrap();
        let named = using(&held).unwrap();
        let _ = other.kill();
        let _ = other.wait();
        assert_eq!(named, ["sleep"]);
        assert_eq!(using(&mine), Some(Vec::new()), "this process is left out");
        assert_eq!(using(&free), Some(Vec::new()));
        drop(open);
    }

    /// Not a test: what [`a_program_held_writes_nothing_on_any_system`]
    /// runs, a program that writes a dot every 20 ms until stopped.
    #[test]
    #[ignore = "run by another test, as a program that writes"]
    fn writes_until_stopped() {
        use std::io::Write as _;
        if std::env::var_os("SKWIR_TEST_WRITES").is_none() {
            return;
        }
        loop {
            print!(".");
            let _ = std::io::stdout().flush();
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Held, a program writes nothing until let go, on every system:
    /// stopped by its group on Unix, its threads suspended on Windows
    /// (under Wine here).
    #[test]
    fn a_program_held_writes_nothing_on_any_system() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let held = AtomicBool::new(false);
        let written = AtomicUsize::new(0);
        let cancel = CancelToken::new();
        std::thread::scope(|s| {
            s.spawn(|| {
                let ran = run_watching(
                    Command::new(std::env::current_exe().unwrap())
                        .args([
                            "--exact",
                            "process::tests::writes_until_stopped",
                            "--ignored",
                            "--nocapture",
                        ])
                        .env("SKWIR_TEST_WRITES", "1"),
                    None,
                    &cancel,
                    &|| held.load(Ordering::Relaxed),
                    &mut |bytes: &[u8]| {
                        written.fetch_add(bytes.len(), Ordering::Relaxed);
                    },
                );
                assert!(ran.is_err(), "cancelled");
            });
            let until = |at_least: usize| {
                let deadline = Instant::now() + Duration::from_secs(20);
                while written.load(Ordering::Relaxed) < at_least {
                    assert!(Instant::now() < deadline, "it writes");
                    std::thread::sleep(Duration::from_millis(10));
                }
            };
            until(3);
            held.store(true, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(400));
            let at = written.load(Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(600));
            let still = written.load(Ordering::Relaxed);
            held.store(false, Ordering::Relaxed);
            until(still + 3);
            cancel.cancel();
            assert_eq!(still, at, "nothing written while held");
        });
    }

    /// A program held, and what it started, writes nothing until let go;
    /// cancelled while held, it is stopped all the same.
    #[cfg(unix)]
    #[test]
    fn a_program_watched_is_held_while_paused() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let held = AtomicBool::new(false);
        let ticks = AtomicUsize::new(0);
        let cancel = CancelToken::new();
        std::thread::scope(|s| {
            s.spawn(|| {
                let ran = run_watching(
                    // A child of the shell's writes: held with its group.
                    Command::new("sh")
                        .args(["-c", "sh -c 'while :; do printf .; sleep 0.02; done'"]),
                    None,
                    &cancel,
                    &|| held.load(Ordering::Relaxed),
                    &mut |bytes: &[u8]| {
                        ticks.fetch_add(bytes.len(), Ordering::Relaxed);
                    },
                );
                assert!(ran.is_err(), "cancelled");
            });
            let until = |at_least: usize| {
                let deadline = Instant::now() + Duration::from_secs(10);
                while ticks.load(Ordering::Relaxed) < at_least {
                    assert!(Instant::now() < deadline, "it writes");
                    std::thread::sleep(Duration::from_millis(10));
                }
            };
            until(3);
            held.store(true, Ordering::Relaxed);
            // Held within a look (50 ms) and what was in the pipe read.
            std::thread::sleep(Duration::from_millis(300));
            let at = ticks.load(Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(500));
            assert_eq!(
                ticks.load(Ordering::Relaxed),
                at,
                "nothing written while held"
            );
            held.store(false, Ordering::Relaxed);
            until(at + 3);
            held.store(true, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(200));
            cancel.cancel();
        });
    }

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
        let told = helper("told", r#"[ -n "$SKWIR_TEST_TOLD" ] && exit 0; exit 1"#);
        assert!(!probe(&told, &[]));
        assert!(probe(
            &told,
            &[("SKWIR_TEST_TOLD", std::ffi::OsStr::new("/x/libpdfium.so"))]
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
        let same = helper("skwir-same-worker", &says("0.1.0+d264e57ab367"));
        assert_eq!(
            explained(Some("0.1.0+d264e57ab367"), &same, &live),
            "bwrap failed: invalid digit found in string"
        );
        let other = helper("skwir-other-worker", &says("0.1.0+a5d0b7a1c2d3"));
        assert_eq!(
            explained(Some("0.1.0+d264e57ab367"), &other, &live),
            "bwrap failed: invalid digit found in string. skwir-other-worker is from \
             another build of skwir (0.1.0+a5d0b7a1c2d3; skwir is 0.1.0+d264e57ab367), \
             which may be why: rebuild or reinstall it"
        );
        // An older helper takes the question for its input, and fails or
        // answers something else.
        let older = helper(
            "skwir-older-worker",
            "echo 'missing --members bounds' >&2; exit 1",
        );
        let chatty = helper(
            "skwir-chatty-worker",
            r#"echo '{"id":0,"body":{"Error":"x"}}'"#,
        );
        for path in [&older, &chatty] {
            assert!(
                explained(Some("0.1.0"), path, &live).ends_with(
                    "is from an older build of skwir, which may be why: rebuild or reinstall it"
                ),
                "{}",
                path.display()
            );
        }
        // Its libraries missing, it could not answer at all.
        let unloadable = helper("skwir-unloadable-worker", "exit 127");
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
    fn helpers_are_beside_skwir_or_where_a_package_keeps_them() {
        // On Windows, the media helper among GStreamer's libraries too.
        let among = |bin: &str| {
            let mut folders = vec![PathBuf::from(bin)];
            if cfg!(windows) {
                folders.push(Path::new(bin).join("gstreamer").join("bin"));
            }
            folders
        };
        let mut expected = among("/usr/bin");
        expected.extend([
            PathBuf::from("/usr/lib/skwir"),
            PathBuf::from("/usr/libexec/skwir"),
        ]);
        assert_eq!(helper_folders(Path::new("/usr/bin")), expected);
        assert_eq!(helper_folders(Path::new("/")), among("/"));
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

    /// A tool that mounts, cancelled while it asks (the user cancelled,
    /// or went elsewhere), is stopped with what it started, as any: only
    /// one that ended well leaves its daemon.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_program_left_running_is_stopped_when_cancelled() {
        let tmp = tempfile::tempdir().unwrap();
        let marker = tmp.path().join("pid");
        let cancel = CancelToken::new();
        let (stop, output) = (cancel.clone(), marker.clone());
        let worker = std::thread::spawn(move || {
            run_leaving(
                Command::new("sh")
                    .args(["-c", "sleep 30 & printf '%s' \"$!\" > \"$1\"; wait", "test"])
                    .arg(output),
                &stop,
                Duration::from_secs(10),
            )
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        let pid = loop {
            if let Ok(pid) = std::fs::read_to_string(&marker)
                .unwrap_or_default()
                .parse::<u32>()
            {
                break pid;
            }
            assert!(Instant::now() < deadline, "never started");
            std::thread::sleep(Duration::from_millis(5));
        };
        cancel.cancel();
        assert!(worker.join().unwrap().is_err());
        loop {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"));
            if stat.is_err() || stat.unwrap().split_whitespace().nth(2) == Some("Z") {
                break;
            }
            assert!(Instant::now() < deadline, "what it started survived");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A tool that mounts and leaves its daemon running (ADR-077): ended
    /// well, what it started runs on; ended badly, it is stopped as any.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_program_ended_well_leaves_what_it_started_running() {
        let tmp = tempfile::tempdir().unwrap();
        let started = |marker: &std::path::Path, code: u8| {
            let finished = run_leaving(
                Command::new("sh")
                    .args([
                        "-c",
                        "sleep 30 & printf '%s' \"$!\" > \"$1\"; exit \"$2\"",
                        "test",
                    ])
                    .arg(marker)
                    .arg(code.to_string()),
                &CancelToken::new(),
                Duration::from_secs(5),
            )
            .unwrap();
            let pid: i32 = std::fs::read_to_string(marker).unwrap().parse().unwrap();
            (finished.status.success(), pid)
        };
        let alive = |pid: i32| {
            std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .is_ok_and(|stat| stat.split_whitespace().nth(2) != Some("Z"))
        };
        let (ok, left) = started(&tmp.path().join("left"), 0);
        assert!(ok);
        std::thread::sleep(Duration::from_millis(100));
        assert!(alive(left), "what it started runs on");
        let _ = rustix::process::kill_process(
            rustix::process::Pid::from_raw(left).unwrap(),
            rustix::process::Signal::KILL,
        );
        let (ok, stopped) = started(&tmp.path().join("stopped"), 3);
        assert!(!ok);
        let deadline = Instant::now() + Duration::from_secs(2);
        while alive(stopped) {
            assert!(
                Instant::now() < deadline,
                "a failed program's daemon survived"
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
