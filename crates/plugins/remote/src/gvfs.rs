//! GVfs, the desktop's own remotes (ADR-077): `gio mount` connects, its
//! questions answered by the window; each mount is a folder of GVfs's FUSE
//! bridge, shared with every other program of the session. Linux only:
//! GVfs runs nowhere else.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use skwir_core::cancel::CancelToken;
use skwir_core::fs::reach::Remote;
use skwir_core::words;
use skwir_core::words::Words;

use crate::address::{Address, Scheme};
use crate::connector::{Ask, Connected, Connector, Offered, Recognised};
use crate::gio_copy::GioCopy;
use crate::gvfs_catalog::{Asks, Questions};
use crate::gvfs_spec::Spec;

/// The FUSE bridge's type in the mount table.
pub const BRIDGE: &str = "fuse.gvfsd-fuse";

/// How long connecting may take once nothing is asked: a server that
/// does not answer.
const PATIENCE: Duration = Duration::from_secs(60);

/// GVfs's daemon on the session bus, which starts the bridge.
#[cfg(target_os = "linux")]
pub(crate) const DAEMON: &str = "org.gtk.vfs.Daemon";

/// How long a mount just made may be waited for while the bridge answers
/// an I/O error for it: made again at once under the name of one just let
/// go of (a server ejected and connected to again), it does so for what
/// was reached in the one gone, the mount's folder included, for up to
/// about a second and a half, though the bridge lists the mount and a
/// stat may still be answered from what the system kept (seen with NFS,
/// 2026-11-13: waiting for the bridge to stop listing the mount gone
/// does not shorten it).
const STALE_PATIENCE: Duration = Duration::from_secs(3);

/// How long letting go of a mount GVfs still says busy is asked again: it
/// closes what its bridge held (a folder just listed through it) only
/// after the bridge's release returned.
const BUSY_PATIENCE: Duration = Duration::from_secs(5);

/// GVfs, as the plugin asks it.
#[derive(Default)]
pub struct Gvfs {
    /// Variables `gio` runs with beside skwir's own: a test's private
    /// session bus and runtime folder, never the user's session.
    pub env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    /// The runtime folder whose `gvfs` folder is the bridge, when not the
    /// user's (a test's).
    pub runtime: Option<PathBuf>,
    /// Its copies in batches (ADR-078).
    pub gio_copy: OnceLock<Arc<GioCopy>>,
}

impl Gvfs {
    /// `gio`, its words in English, run as asked.
    fn gio(&self, gio: &Path) -> Command {
        let mut command = skwir_platform::process::command(gio);
        english(&mut command);
        command.envs(self.env.iter().map(|(k, v)| (k, v)));
        command
    }

    /// GVfs's FUSE bridge: the session's own (`$XDG_RUNTIME_DIR/gvfs`),
    /// when mounted. Never another the mount table lists: `gio` speaks to
    /// this session's GVfs alone, and a bridge of another runtime folder
    /// is another user's, or the user's own while a window runs with a
    /// runtime folder of its own (a test's), whose GVfs it must not reach.
    pub fn bridge(&self) -> Option<PathBuf> {
        bridge_in(self.runtime.clone())
    }

    /// GVfs's daemon on the session's bus (the test's), as the bus says
    /// it: on a connection of its own, which waits 5 s at most.
    fn daemon(&self) -> Daemon {
        #[cfg(target_os = "linux")]
        {
            let asked = || -> anyhow::Result<Daemon> {
                let bus = crate::bus::connect(&self.env, Duration::from_secs(5))?;
                let proxy = zbus::blocking::fdo::DBusProxy::new(&bus)?;
                let name = zbus::names::BusName::try_from(DAEMON)?;
                if proxy.name_has_owner(name.clone())? {
                    // Its start not known (another user's, another pid
                    // namespace's): as one just started, its bridge may come.
                    let settled = proxy
                        .get_connection_unix_process_id(name)
                        .ok()
                        .and_then(running_for)
                        .is_some_and(|ran| ran >= crate::connector::APPEARS_WITHIN);
                    return Ok(if settled {
                        Daemon::Settled
                    } else {
                        Daemon::Starting
                    });
                }
                let activatable = proxy
                    .list_activatable_names()?
                    .iter()
                    .any(|n| n.as_str() == DAEMON);
                Ok(if activatable {
                    Daemon::Startable
                } else {
                    Daemon::Absent
                })
            };
            asked().unwrap_or(Daemon::Absent)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = &self.env;
            Daemon::Absent
        }
    }

    /// What copies in batches to and from its SFTP mounts.
    pub fn gio_copy(&self) -> Arc<GioCopy> {
        Arc::clone(
            self.gio_copy
                .get_or_init(|| Arc::new(GioCopy::new(self.env.clone(), self.runtime.clone()))),
        )
    }

    /// The folder of the bridge that holds `top`'s mount, with the
    /// address of that mount's top as its spec says it (an NFS export
    /// GVfs found holding the path asked for): the same server, at or
    /// above `top`. Reads the bridge's folder, which GVfs answers from its
    /// list of mounts, never from a server.
    fn find_mount(&self, top: &Address) -> Option<(PathBuf, Address)> {
        let bridge = self.bridge()?;
        std::fs::read_dir(&bridge)
            .ok()?
            .filter_map(Result::ok)
            .find_map(|entry| {
                let mounted = Spec::parse(entry.file_name().to_str()?)?.address()?;
                mounted
                    .below(top)
                    .is_some()
                    .then(|| (bridge.join(entry.file_name()), mounted))
            })
    }
}

impl Connector for Gvfs {
    fn id(&self) -> &'static str {
        "gvfs"
    }

    fn title(&self) -> Words {
        Words::as_written("GVfs")
    }

    fn schemes(&self) -> &'static [Scheme] {
        &[
            Scheme::Sftp,
            Scheme::Smb,
            Scheme::Dav,
            Scheme::Davs,
            Scheme::Ftp,
            Scheme::Ftps,
            Scheme::Nfs,
            Scheme::Afp,
            Scheme::Mtp,
            Scheme::Gphoto2,
            Scheme::Afc,
        ]
    }

    fn unavailable(&self) -> Option<Words> {
        if !cfg!(target_os = "linux") {
            return Some(words!(remote_gvfs_linux_only = "GVfs runs on Linux only"));
        }
        if skwir_core::os_path::find_program("gio").is_none() {
            return Some(crate::connector::not_installed("gio (GLib)"));
        }
        // Its daemon need not run yet (a desktop other than GNOME's): `gio
        // mount` starts it, and it starts its bridge. One that has run a
        // while without its bridge (gvfsd-fuse not installed, as Debian
        // packages it apart; `GVFS_DISABLE_FUSE`; no `/dev/fuse`) will
        // never show a folder: unusable, so that the next tool is tried and
        // no password is asked for a mount Skwir cannot open.
        if self.bridge().is_some() {
            return None;
        }
        match self.daemon() {
            Daemon::Starting | Daemon::Startable => None,
            Daemon::Settled => Some(words!(
                remote_gvfs_without_bridge = "GVfs runs without its FUSE bridge (gvfsd-fuse)",
                note = "Why GVfs cannot open a server here: its daemon runs, but not the \
                        bridge its folders are read through (not installed, or turned off). \
                        Said in the list of why each tool cannot open an address."
            )),
            Daemon::Absent => Some(crate::connector::not_installed("GVfs")),
        }
    }

    fn connect(
        &self,
        address: &Address,
        ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Connected> {
        let top = address.top();
        let gio = skwir_core::os_path::find_program("gio")
            .ok_or_else(|| anyhow::anyhow!(crate::connector::not_installed("gio (GLib)")))?;
        mount(self.gio(&gio), &top, &self.env, ask, cancel)?;
        // A daemon `gio` just started mounts its bridge as it starts, and
        // the bridge lists the mount once told of it, on its own loop,
        // after `gio` returned: each waited for (`connector::appears`).
        if crate::connector::appears(cancel, || self.bridge()).is_none() {
            anyhow::ensure!(!cancel.is_cancelled(), crate::connector::cancelled());
            anyhow::bail!(words!(
                remote_gvfs_no_bridge = "GVfs mounted { $address } but its FUSE bridge is not running: is gvfsd-fuse installed?",
                address = top.to_string(),
                note = "address: the server's address (sftp://example.com/)."
            ));
        }
        let found = crate::connector::appears(cancel, || self.find_mount(&top));
        anyhow::ensure!(
            found.is_some() || !cancel.is_cancelled(),
            crate::connector::cancelled()
        );
        let (point, mounted) = found.ok_or_else(|| {
            anyhow::anyhow!(words!(
                remote_gvfs_no_folder = "GVfs mounted { $address } but its folder is not there",
                address = top.to_string(),
                note = "address: the server's address (sftp://example.com/)."
            ))
        })?;
        // The folder asked, which is opened next.
        let asked = mounted.below(address).map_or_else(
            || point.clone(),
            |inside| point.join(skwir_core::os_path::from_bytes(inside.to_vec())),
        );
        answering(&asked, cancel)?;
        Ok(Connected {
            point,
            address: mounted,
        })
    }

    fn recognises(&self, remote: &Remote) -> Option<Recognised> {
        // A mount of this session's GVfs alone: another's is not used,
        // nor let go of through this session's `gio`.
        if remote.fstype != BRIDGE || remote.point.parent() != self.bridge().as_deref() {
            return None;
        }
        let name = remote.point.file_name()?.to_str()?;
        let spec = Spec::parse(name)?;
        Some(Recognised {
            address: spec.address(),
            label: spec.label(),
            ours: false,
            device: spec.device(),
        })
    }

    fn disconnect(
        &self,
        remote: &Remote,
        recognised: &Recognised,
        cancel: &CancelToken,
    ) -> anyhow::Result<()> {
        let address = recognised.address.as_ref().ok_or_else(|| {
            anyhow::anyhow!(words!(
                remote_gvfs_no_address = "GVfs says no address for this mount"
            ))
        })?;
        let gio = skwir_core::os_path::find_program("gio")
            .ok_or_else(|| anyhow::anyhow!(crate::connector::not_installed("gio (GLib)")))?;
        // Refused while GVfs is at work on the mount (busy: a read through
        // its bridge, a folder just listed; a file merely open is not),
        // asked again; gone, it is let go of.
        let until = Instant::now() + BUSY_PATIENCE;
        loop {
            let mut command = self.gio(&gio);
            command.arg("mount").arg("-u").arg(address.uri());
            let finished =
                skwir_platform::process::run_to_exit(&mut command, None, cancel, PATIENCE)?;
            skwir_core::fs::reach::changed();
            if finished.status.success() || !still_bridged(&remote.point) {
                return Ok(());
            }
            if Instant::now() >= until || cancel.is_cancelled() {
                anyhow::bail!("{}", said(&finished.stderr));
            }
            tracing::debug!(
                refused = %said(&finished.stderr),
                "GVfs kept its mount: asked again"
            );
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    fn transfers(&self) -> Vec<Arc<dyn skwir_core::op::transfer::Transfer>> {
        vec![self.gio_copy()]
    }

    /// The phones, cameras and iPhones GVfs's volume monitors offer on the
    /// session's bus (the test's), mounted or not (ADR-096).
    fn device_signals(&self) -> Vec<String> {
        #[cfg(target_os = "linux")]
        {
            crate::gvfs_devices::signals()
        }
        #[cfg(not(target_os = "linux"))]
        {
            Vec::new()
        }
    }

    fn devices(&self) -> Vec<Offered> {
        #[cfg(target_os = "linux")]
        {
            // Offered only where GVfs would open them.
            if skwir_core::os_path::find_program("gio").is_none() {
                return Vec::new();
            }
            crate::gvfs_devices::offered(&self.env)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Vec::new()
        }
    }

    fn bus_env(&self) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        self.env.clone()
    }
}

/// GVfs's daemon on the session's bus, as [`Gvfs::daemon`] finds it.
// Elsewhere than on Linux, GVfs is never found: `Absent` alone.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Daemon {
    /// It runs, and has run as long as a connection waits for its bridge
    /// ([`crate::connector::APPEARS_WITHIN`]), which it mounts as it starts.
    Settled,
    /// It runs, just started or since a time not known: its bridge may
    /// still come.
    Starting,
    /// It does not run, and the bus would start it when asked (`gio
    /// mount` does), and it its bridge.
    Startable,
    /// Neither: GVfs is not installed, or the bus did not answer.
    Absent,
}

/// How long process `pid` has run: its start (`/proc/<pid>/stat`'s 22nd
/// field, in clock ticks after boot) before the time since boot
/// (`/proc/uptime`, the same clock). `None` where either is not read, or
/// the process is no GVfs daemon (a number another process took since).
#[cfg(target_os = "linux")]
fn running_for(pid: u32) -> Option<Duration> {
    let program = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    if !program.file_name()?.to_string_lossy().contains("gvfsd") {
        return None;
    }
    running_since_boot(pid)
}

/// How long process `pid` has run ([`running_for`]), whatever it is.
#[cfg(target_os = "linux")]
fn running_since_boot(pid: u32) -> Option<Duration> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Read after its name, which may hold spaces and parentheses: the
    // third field (its state) first.
    let ticks: u64 = stat[stat.rfind(')')? + 1..]
        .split_whitespace()
        .nth(22 - 3)?
        .parse()
        .ok()?;
    let hz = rustix::param::clock_ticks_per_second().max(1);
    let started =
        Duration::from_secs(ticks / hz) + Duration::from_nanos((ticks % hz) * 1_000_000_000 / hz);
    let uptime: f64 = std::fs::read_to_string("/proc/uptime")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    Duration::try_from_secs_f64(uptime)
        .ok()?
        .checked_sub(started)
}

/// The program GVfs runs for a mount of `scheme` (`gvfsd-mtp`): its
/// backends are named by the type of their mounts.
pub(crate) fn backend_of(scheme: Scheme) -> String {
    format!("gvfsd-{}", scheme.name())
}

/// Wait, at most [`STALE_PATIENCE`], while the bridge answers an I/O error
/// for `folder`, the folder asked in a mount just made: opened, it would
/// fail. Asked of the bridge itself (`stat::answers`), which asks the
/// server once, as the folder's listing does next. Any other answer is
/// the listing's to say.
fn answering(folder: &Path, cancel: &CancelToken) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let io = rustix::io::Errno::IO.raw_os_error();
        let until = Instant::now() + STALE_PATIENCE;
        while skwir_core::fs::stat::answers(folder)
            .is_err_and(|error| error.raw_os_error() == Some(io))
            && Instant::now() < until
        {
            if cancel.is_cancelled() {
                anyhow::bail!(crate::connector::cancelled());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(())
    }
    // Never reached: GVfs is unavailable here.
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (folder, cancel, STALE_PATIENCE);
        anyhow::bail!(words!(remote_gvfs_linux_only = "GVfs runs on Linux only"))
    }
}

/// Whether a GVfs bridge still holds a mount at `point`.
fn still_bridged(point: &Path) -> bool {
    skwir_core::fs::reach::all()
        .iter()
        .filter(|remote| remote.fstype == BRIDGE)
        .flat_map(skwir_core::fs::reach::bridged)
        .any(|remote| remote.point == point)
}

/// The bridge of the runtime folder `runtime` (the user's when `None`),
/// when mounted ([`Gvfs::bridge`]). gvfsd mounts it in
/// `$XDG_RUNTIME_DIR` as written, which the mount table lists with its
/// links resolved: the user's is named as the table names it
/// (`folders::session_runtime`, from memory after its first reading).
pub(crate) fn bridge_in(runtime: Option<PathBuf>) -> Option<PathBuf> {
    let own = runtime
        .or_else(skwir_core::folders::session_runtime)?
        .join("gvfs");
    skwir_core::fs::reach::bridges(BRIDGE)
        .contains(&own)
        .then_some(own)
}

/// `gio`'s own words in English, which its prompts are read in; what
/// GVfs's daemon says stays in its language (`gvfs_catalog` reads it).
pub(crate) fn english(command: &mut Command) {
    command
        .env_remove("LC_ALL")
        .env_remove("LANGUAGE")
        .env("LC_MESSAGES", "C");
}

/// What `gio` said on failing, without its prefix (`gio: sftp://h/: `).
fn said(stderr: &str) -> String {
    let line = stderr
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(stderr);
    let line = line.strip_prefix("gio: ").unwrap_or(line);
    // The address it names is the one the user typed.
    match line.split_once(": ") {
        Some((first, rest)) if first.contains("://") => rest.to_owned(),
        _ => line.to_owned(),
    }
}

/// What `gio mount` asks on its standard output, a question at a time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    /// `User [default]: `: `message` what GVfs said for its question,
    /// `default` the user GVfs offers.
    User {
        message: String,
        default: Option<String>,
    },
    /// `Domain [default]: `, as `User`: `message` its question's words,
    /// said before it or before the `User` prompt of the same question.
    Domain {
        message: String,
        default: Option<String>,
    },
    /// `Password: `: `message` what GVfs said for it, before it or before
    /// the `User` or `Domain` prompts of the same question; `user` whether
    /// a user was asked there.
    Password { message: String, user: bool },
    /// A question and its choices, numbered as `gio` reads them back.
    Choice {
        message: String,
        choices: Vec<(u32, String)>,
    },
}

/// Reads `gio mount`'s output as it comes; a question is whole once its
/// prompt (`Password: `) ends what was said.
#[derive(Default)]
pub struct Prompts {
    said: String,
    /// The question of GVfs's that `User` or `Domain` prompts began, which
    /// a `Password` prompt ends: what GVfs said, and whether a user was
    /// asked. `gio mount` says what GVfs asks once, then prompts for a
    /// user, a domain and a password in turn, as GVfs wants them.
    asking: Option<(String, bool)>,
}

impl Prompts {
    pub fn feed(&mut self, text: &str) -> Option<Asked> {
        self.said.push_str(text);
        let at = self.said.rfind('\n').map_or(0, |at| at + 1);
        let last = &self.said[at..];
        let prompt = last.strip_suffix(": ")?;
        let (word, default) = match prompt.split_once(" [") {
            Some((word, rest)) => (word, rest.strip_suffix(']').map(ToOwned::to_owned)),
            None => (prompt, None),
        };
        if !matches!(word, "User" | "Domain" | "Password" | "Choice") {
            return None;
        }
        let word = word.to_owned();
        let before = self.said[..at].to_owned();
        self.said.clear();
        let mut message = Vec::new();
        let mut choices = Vec::new();
        for line in before.lines() {
            if let Some((number, choice)) = line
                .strip_prefix('[')
                .and_then(|l| l.split_once("] "))
                .and_then(|(n, c)| Some((n.parse::<u32>().ok()?, c)))
            {
                choices.push((number, choice.to_owned()));
            } else if !line.trim().is_empty() {
                message.push(line);
            }
        }
        let message = message.join("\n");
        // A prompt after words of its own begins a question; one with none
        // goes on with the question before.
        let asking = self.asking.take();
        let (question, user) = match asking {
            Some((said, user)) if message.is_empty() => (said, user),
            _ => (message.clone(), false),
        };
        Some(match word.as_str() {
            "User" | "Domain" => {
                self.asking = Some((question.clone(), user || word == "User"));
                if word == "User" {
                    Asked::User {
                        message: question,
                        default,
                    }
                } else {
                    Asked::Domain {
                        message: question,
                        default,
                    }
                }
            }
            "Password" => Asked::Password {
                message: question,
                user,
            },
            _ => Asked::Choice { message, choices },
        })
    }
}

/// Mount `top` with `gio mount`, answering what it asks: the address's
/// user and domain, the window's password, the window's choice to go on
/// where GVfs cannot vouch for the server (the first choice; Cancel
/// otherwise). `env` names the bus of the daemon that asks (a test's).
fn mount(
    mut command: Command,
    top: &Address,
    env: &[(std::ffi::OsString, std::ffi::OsString)],
    ask: &Arc<dyn Ask>,
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    command
        .arg("mount")
        .arg(top.uri())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let (said_tx, said_rx) = crossbeam_channel::unbounded::<Option<String>>();
    let mut stdout = child.stdout.take().expect("piped");
    std::thread::Builder::new()
        .name("skwir-gio".into())
        .spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match stdout.read(&mut buf) {
                    Ok(0) | Err(_) => {
                        let _ = said_tx.send(None);
                        return;
                    }
                    Ok(n) => {
                        let _ = said_tx.send(Some(String::from_utf8_lossy(&buf[..n]).into_owned()));
                    }
                }
            }
        })?;
    let mut stderr = child.stderr.take().expect("piped");
    let errors = std::thread::Builder::new()
        .name("skwir-gio".into())
        .spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        })?;
    let mut stdin = child.stdin.take();
    let mut prompts = Prompts::default();
    let wording = Wording {
        env,
        read: std::cell::OnceCell::new(),
    };
    let mut since = Instant::now();
    let mut ended = false;
    let outcome = loop {
        if cancel.is_cancelled() {
            break Err(anyhow::anyhow!(crate::connector::cancelled()));
        }
        if since.elapsed() > PATIENCE {
            let seconds = PATIENCE.as_secs();
            // A device's name is its serial: it, as the message's start
            // names it.
            let said = match top.scheme.device() {
                Some(_) => words!(
                    remote_gvfs_device_silent = "it did not answer within { $seconds ->
                        [one] { $seconds } second
                       *[other] { $seconds } seconds
                    }",
                    seconds = seconds,
                    note = "it: a phone or a camera, which GVfs was asked to open."
                ),
                None => words!(
                    remote_gvfs_server_silent = "{ $host } did not answer within { $seconds ->
                        [one] { $seconds } second
                       *[other] { $seconds } seconds
                    }",
                    host = top.host.as_str(),
                    seconds = seconds,
                    note = "host: the server's name or address, which GVfs was asked to open."
                ),
            };
            break Err(anyhow::anyhow!(said));
        }
        if !ended {
            match said_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Some(text)) => {
                    if let Some(asked) = prompts.feed(&text) {
                        let answer = match answer(top, &asked, &wording, ask, cancel) {
                            Ok(answer) => answer,
                            Err(why) => break Err(anyhow::anyhow!(why)),
                        };
                        if let Some(stdin) = stdin.as_mut() {
                            // Written without a trace: it may be a password.
                            let _ = stdin.write_all(answer.as_bytes());
                            let _ = stdin.write_all(b"\n");
                            let _ = stdin.flush();
                        }
                        since = Instant::now();
                    }
                    continue;
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Ok(None) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => ended = true,
            }
        }
        match child.try_wait()? {
            Some(status) => break Ok(status),
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    drop(stdin);
    let status = match outcome {
        Ok(status) => status,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let errors = errors.join().unwrap_or_default();
    if status.success() || errors.contains("already mounted") {
        Ok(())
    } else {
        Err(anyhow::anyhow!("{}", said(&errors)))
    }
}

/// GVfs's questions as its daemon words them (`gvfs_catalog`), read the
/// first time a mount asks for a password, kept for its others.
struct Wording<'a> {
    /// The variables naming the daemon's bus (a test's).
    env: &'a [(std::ffi::OsString, std::ffi::OsString)],
    read: std::cell::OnceCell<Questions>,
}

impl Wording<'_> {
    fn questions(&self) -> &Questions {
        self.read.get_or_init(|| Questions::of_daemon(self.env))
    }
}

/// Why a connection was not made: the user cancelled what it asked.
fn you_cancelled() -> Words {
    words!(
        remote_gvfs_you_cancelled = "you cancelled",
        note = "Why a connection was not made: the user cancelled a question it asked."
    )
}

/// What to answer `asked`, GVfs's words read as `wording` has them; else
/// why the mount ends (the user cancelled, a question Skwir cannot ask).
fn answer(
    top: &Address,
    asked: &Asked,
    wording: &Wording,
    ask: &Arc<dyn Ask>,
    cancel: &CancelToken,
) -> Result<String, Words> {
    // GVfs's words alone tell what it asks, in English or its daemon's
    // language: never what else it asks with it (a proxy's password comes
    // with a user too).
    let asks = |message: &str| wording.questions().of(message);
    let server = |message: &str| asks(message) == Some(Asks::Password);
    match asked {
        // The server's login for the server's question alone; for any
        // other, the user GVfs offers. A proxy's that GVfs offers none
        // for ends the mount unasked, so that the next tool is tried:
        // Skwir asks no user name, and an empty one fails.
        Asked::User { message, default } => {
            if server(message) {
                return Ok(top
                    .user
                    .clone()
                    .or_else(|| default.clone())
                    .unwrap_or_default());
            }
            match default {
                Some(offered) => Ok(offered.clone()),
                None if asks(message) == Some(Asks::Other) => Err(words!(
                    remote_gvfs_proxy_user =
                        "GVfs asks for the proxy's user name, which Skwir cannot ask for",
                    note = "Why GVfs did not connect: a proxy between Skwir and the server wants \
                            a user name of its own. Another tool is tried after."
                )),
                None => Ok(String::new()),
            }
        }
        Asked::Domain { message, default } => Ok(server(message)
            .then(|| top.domain.clone())
            .flatten()
            .or_else(|| default.clone())
            .unwrap_or_default()),
        Asked::Password { message, user } => {
            let connect = crate::connector::connect_button();
            // A server's password may be kept (ADR-082); a key's
            // passphrase, a proxy's password, a code are asked each time,
            // and so is what none of GVfs's words name.
            if server(message) {
                // What GVfs asked, as it said it; Skwir's own question where
                // its words ask for a user too, answered from the address.
                let title = if *user {
                    crate::connector::password_for(top.label())
                } else {
                    Words::as_written(message.clone())
                };
                ask.password(&title, &connect, cancel)
                    .map(|p| p.text)
                    .ok_or_else(you_cancelled)
            } else {
                // Its words, which say what is asked; Skwir's own where it
                // said none.
                let title = if message.is_empty() {
                    crate::connector::password_for(top.label())
                } else {
                    Words::as_written(message.clone())
                };
                ask.secret(&title, &connect, cancel)
                    .ok_or_else(you_cancelled)
            }
        }
        Asked::Choice { message, choices } => {
            // GVfs asks to go on where it cannot vouch for the server: its
            // first choice goes on, the last cancels.
            let (go_on, _) = choices.first().ok_or_else(you_cancelled)?;
            let title = words!(
                remote_gvfs_connect_to = "Connect to { $host }?",
                host = top.host.as_str(),
                note = "A question asked where the server cannot be vouched for; host: its name \
                        or address. What GVfs said is shown under it."
            );
            ask.confirm(&title, &Words::as_written(message.clone()), cancel)
                .then(|| go_on.to_string())
                .ok_or_else(you_cancelled)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GVfs mount of another runtime folder than the session's (another
    /// user's, or the user's while a test window runs) is not this
    /// session's: neither used, nor let go of, nor connected through.
    #[test]
    fn another_sessions_bridge_is_not_this_ones() {
        let gvfs = Gvfs {
            runtime: Some(PathBuf::from("/nonexistent/skwir-test-runtime")),
            ..Gvfs::default()
        };
        assert_eq!(gvfs.bridge(), None);
        let theirs = Remote {
            point: PathBuf::from("/run/user/1000/gvfs/sftp:host=example.com"),
            fstype: BRIDGE.to_owned(),
            source: "gvfsd-fuse".into(),
            device: None,
        };
        assert!(gvfs.recognises(&theirs).is_none());
    }

    #[test]
    fn gio_mounts_questions_are_read_whole() {
        let mut prompts = Prompts::default();
        assert_eq!(
            prompts.feed("Enter password for ssh as me on example.com\nUs"),
            None
        );
        assert_eq!(
            prompts.feed("er [me]: "),
            Some(Asked::User {
                message: "Enter password for ssh as me on example.com".into(),
                default: Some("me".into()),
            })
        );
        assert_eq!(
            prompts.feed("Password: "),
            Some(Asked::Password {
                message: "Enter password for ssh as me on example.com".into(),
                user: true,
            }),
            "the question the user's prompt began"
        );
        let question = "The identity of the remote computer (example.com) is unknown.\n\
            This happens when you log in to a computer the first time.\n\n\
            The identity sent by the remote computer is SHA256:abc. If you want to be \
            absolutely sure it is safe to continue, contact the system administrator.\n\
            [1] Log In Anyway\n[2] Cancel Login\nChoice: ";
        match prompts.feed(question) {
            Some(Asked::Choice { message, choices }) => {
                assert!(message.contains("SHA256:abc"));
                assert_eq!(
                    choices,
                    [(1, "Log In Anyway".into()), (2, "Cancel Login".into())]
                );
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            prompts.feed("Mounted sftp://h/ at x: "),
            None,
            "not a question"
        );
    }

    /// A password's question goes on through the prompts before it, which
    /// GVfs's words began; a prompt after words of its own begins another.
    #[test]
    fn a_password_is_asked_with_the_words_its_question_began_with() {
        let mut prompts = Prompts::default();
        // SMB's, its user in the address: a domain asked, then a password.
        let smb = "Authentication Required\nEnter password for share “photos” on “nas”:";
        assert_eq!(
            prompts.feed(&format!("{smb}\nDomain [WORKGROUP]: ")),
            Some(Asked::Domain {
                message: smb.into(),
                default: Some("WORKGROUP".into()),
            })
        );
        assert_eq!(
            prompts.feed("Password: "),
            Some(Asked::Password {
                message: smb.into(),
                user: false,
            })
        );
        // A user, then a domain, then the password: one question.
        let both = "Authentication Required\nEnter user and password for share “photos” on “nas”:";
        prompts.feed(&format!("{both}\nUser: "));
        assert_eq!(
            prompts.feed("Domain: "),
            Some(Asked::Domain {
                message: both.into(),
                default: None,
            }),
            "the words of the question it goes on with"
        );
        assert_eq!(
            prompts.feed("Password: "),
            Some(Asked::Password {
                message: both.into(),
                user: true,
            })
        );
        let passphrase = "Authentication Required\nEnter passphrase for secure key for “nas”:";
        assert_eq!(
            prompts.feed(&format!("{passphrase}\nPassword: ")),
            Some(Asked::Password {
                message: passphrase.into(),
                user: false,
            })
        );
        // No words, none before: nothing said of what it is.
        assert_eq!(
            prompts.feed("Password: "),
            Some(Asked::Password {
                message: String::new(),
                user: false,
            })
        );
    }

    /// What a test's window is asked, and how: `secret` asked each time,
    /// `password` a server's, which may be kept.
    #[cfg(target_os = "linux")]
    #[derive(Default)]
    struct Window(std::sync::Mutex<Vec<String>>);

    #[cfg(target_os = "linux")]
    impl Ask for Window {
        fn secret(&self, title: &Words, _: &Words, _: &CancelToken) -> Option<String> {
            let asked = format!("secret: {}", title.english());
            self.0.lock().unwrap().push(asked);
            Some("typed".into())
        }

        fn confirm(&self, _: &Words, _: &Words, _: &CancelToken) -> bool {
            false
        }

        fn password(
            &self,
            title: &Words,
            _: &Words,
            _: &CancelToken,
        ) -> Option<crate::connector::Password> {
            let asked = format!("password: {}", title.english());
            self.0.lock().unwrap().push(asked);
            Some(crate::connector::Password {
                text: "typed".into(),
                remember: true,
            })
        }
    }

    /// A key's passphrase is asked as one, never kept (ADR-082), in GVfs's
    /// English and in its daemon's language alike; a server's password may
    /// be kept; a proxy's password, a code and what GVfs's words name not
    /// are asked each time, a user asked with them or not, and never
    /// answered with the server's login. Linux's alone: it makes a catalog
    /// with msgfmt, where GVfs runs.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_key_s_passphrase_is_never_asked_as_a_password_in_any_language() {
        use crate::gvfs_catalog::Catalog;
        use crate::gvfs_catalog::tests::{FRENCH, msgfmt, said};
        let folder = tempfile::tempdir().unwrap();
        let mo = folder.path().join("fr.mo");
        msgfmt(&FRENCH, "little", &mo);
        let wording = Wording {
            env: &[],
            read: std::cell::OnceCell::from(Questions::in_catalogs(&[Catalog::read(&mo).unwrap()])),
        };
        let top = Address::parse("sftp://me@example.com/").unwrap();
        let window = Arc::new(Window::default());
        let ask: Arc<dyn Ask> = window.clone();
        let asked = |message: &str, user: bool| {
            let asked = Asked::Password {
                message: message.to_owned(),
                user,
            };
            let answered = answer(&top, &asked, &wording, &ask, &CancelToken::new());
            assert_eq!(answered.ok().as_deref(), Some("typed"));
            window.0.lock().unwrap().pop().expect("asked")
        };
        let english_passphrase =
            "Authentication Required\nEnter passphrase for secure key for “me” on “example.com”:";
        let english_password = "Authentication Required\nEnter password for “me” on “example.com”:";
        for passphrase in [
            english_passphrase.to_owned(),
            said(FRENCH[0].1, &["me", "example.com"]),
            said(FRENCH[1].1, &["example.com"]),
        ] {
            assert_eq!(
                asked(&passphrase, false),
                format!("secret: {passphrase}"),
                "asked each time, in GVfs's words"
            );
        }
        for password in [
            english_password.to_owned(),
            said(FRENCH[2].1, &["me", "example.com"]),
            said(FRENCH[5].1, &["photos", "example.com"]),
        ] {
            assert_eq!(asked(&password, false), format!("password: {password}"));
        }
        // Not the server's (a code, a proxy's password, which GVfs asks
        // with a user of the proxy's), and words never seen: asked each
        // time in GVfs's words, a user asked with them or not.
        let english_proxy = "Authentication Required\nEnter proxy password:";
        let never_seen = "Some question of a newer GVfs";
        for neither in [
            said(FRENCH[8].1, &["me", "example.com"]),
            said(FRENCH[9].1, &["example.com"]),
            "Enter verification code for me on example.com".to_owned(),
            said(FRENCH[7].1, &[]),
            english_proxy.to_owned(),
            never_seen.to_owned(),
        ] {
            for user in [false, true] {
                assert_eq!(
                    asked(&neither, user),
                    format!("secret: {neither}"),
                    "user asked: {user}"
                );
            }
        }
        assert!(asked("", false).starts_with("secret: "), "no words");
        // A server's password GVfs asks with a user: Skwir's own question,
        // the user answered from the address.
        let with_user = said(FRENCH[3].1, &["example.com"]);
        let asked_with_user = asked(&with_user, true);
        assert!(
            asked_with_user.starts_with("password: "),
            "{asked_with_user}"
        );
        assert!(!asked_with_user.contains("Saisissez"), "{asked_with_user}");
        // The user it asks with them: the server's login for the server's
        // question alone, else GVfs's own offer; a proxy's GVfs offers none
        // for ends the mount, unasked, so that the next tool is tried.
        let user = |message: &str, default: Option<&str>| {
            let asked = Asked::User {
                message: message.to_owned(),
                default: default.map(ToOwned::to_owned),
            };
            answer(&top, &asked, &wording, &ask, &CancelToken::new()).map_err(|why| why.english())
        };
        let english_user = "Authentication Required\nEnter user and password for “example.com”:";
        assert_eq!(user(english_user, None).as_deref(), Ok("me"));
        assert_eq!(user(&with_user, Some("someone")).as_deref(), Ok("me"));
        for proxy in [english_proxy.to_owned(), said(FRENCH[7].1, &[])] {
            assert_eq!(
                user(&proxy, None),
                Err("GVfs asks for the proxy's user name, which Skwir cannot ask for".to_owned()),
                "never the server's login, nor none"
            );
            assert_eq!(user(&proxy, Some("proxied")).as_deref(), Ok("proxied"));
        }
        assert_eq!(user(never_seen, None).as_deref(), Ok(""));
        assert_eq!(user(never_seen, Some("offered")).as_deref(), Ok("offered"));
        assert!(window.0.lock().unwrap().is_empty(), "nothing else asked");
    }

    /// How long a process has run, read in `/proc`: this test's, just
    /// started; never taken for a GVfs daemon's.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_s_time_running_is_read_and_its_program_checked() {
        let ran = running_since_boot(std::process::id()).expect("read in /proc");
        assert!(ran < Duration::from_secs(600), "{ran:?}");
        assert!(running_since_boot(1).expect("pid 1's") >= ran);
        assert_eq!(running_for(std::process::id()), None, "no GVfs daemon");
    }

    #[test]
    fn what_gio_says_on_failing_loses_its_prefix() {
        assert_eq!(
            said("gio: sftp://example.com/: Connection refused\n"),
            "Connection refused"
        );
        assert_eq!(said("gio: Permission denied"), "Permission denied");
    }
}
