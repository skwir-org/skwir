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

use crate::address::{Address, Scheme};
use crate::connector::{Ask, Connected, Connector, Recognised};
use crate::gio_copy::GioCopy;
use crate::gvfs_spec::Spec;

/// The FUSE bridge's type in the mount table.
pub const BRIDGE: &str = "fuse.gvfsd-fuse";

/// How long connecting may take once nothing is asked: a server that
/// does not answer.
const PATIENCE: Duration = Duration::from_secs(60);

/// GVfs's daemon on the session bus, which starts the bridge.
#[cfg(target_os = "linux")]
const DAEMON: &str = "org.gtk.vfs.Daemon";

/// How long a daemon just started may take to mount its bridge.
const BRIDGE_PATIENCE: Duration = Duration::from_secs(3);

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
        let mut command = Command::new(gio);
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

    /// Whether GVfs's daemon runs on the session's bus (the test's), or
    /// would be started there when asked.
    fn can_start(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            crate::bus::can_start(DAEMON, &self.env)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = &self.env;
            false
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

    fn title(&self) -> &'static str {
        "GVfs"
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
        ]
    }

    fn unavailable(&self) -> Option<String> {
        if !cfg!(target_os = "linux") {
            return Some("GVfs runs on Linux only".to_owned());
        }
        if skwir_core::os_path::find_program("gio").is_none() {
            return Some("gio (GLib) is not installed".to_owned());
        }
        // Its daemon need not run yet (a desktop other than GNOME's): `gio
        // mount` starts it, and it starts its bridge.
        if self.bridge().is_none() && !self.can_start() {
            return Some("GVfs is not installed".to_owned());
        }
        None
    }

    fn connect(
        &self,
        address: &Address,
        ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Connected> {
        let top = address.top();
        let gio = skwir_core::os_path::find_program("gio")
            .ok_or_else(|| anyhow::anyhow!("gio (GLib) is not installed"))?;
        mount(self.gio(&gio), &top, ask, cancel)?;
        // A daemon `gio` just started mounts its bridge as it starts: a
        // moment, looked for again.
        let since = Instant::now();
        while self.bridge().is_none() && since.elapsed() < BRIDGE_PATIENCE {
            std::thread::sleep(Duration::from_millis(50));
            skwir_core::fs::reach::changed();
        }
        if self.bridge().is_none() {
            anyhow::bail!(
                "GVfs mounted {top} but its FUSE bridge is not running: is gvfsd-fuse installed?"
            );
        }
        skwir_core::fs::reach::changed();
        let (point, address) = self
            .find_mount(&top)
            .ok_or_else(|| anyhow::anyhow!("GVfs mounted {top} but its folder is not there"))?;
        Ok(Connected { point, address })
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
        let address = recognised
            .address
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("GVfs says no address for this mount"))?;
        let gio = skwir_core::os_path::find_program("gio")
            .ok_or_else(|| anyhow::anyhow!("gio (GLib) is not installed"))?;
        // Refused while GVfs still holds the mount (busy: what its bridge
        // held, closed a moment after), asked again; gone, it is let go of.
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
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    fn transfers(&self) -> Vec<Arc<dyn skwir_core::op::transfer::Transfer>> {
        vec![self.gio_copy()]
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
/// when mounted ([`Gvfs::bridge`]).
pub(crate) fn bridge_in(runtime: Option<PathBuf>) -> Option<PathBuf> {
    let own = runtime.or_else(skwir_core::xdg::runtime_dir)?.join("gvfs");
    skwir_core::fs::reach::bridges(BRIDGE)
        .contains(&own)
        .then_some(own)
}

/// `gio`'s own words in English, which its questions are read in; what
/// GVfs's daemon says stays in the user's language.
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
    /// `User [default]: `, after what the server said.
    User {
        message: String,
        default: Option<String>,
    },
    Domain {
        message: String,
        default: Option<String>,
    },
    Password {
        message: String,
    },
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
        Some(match word.as_str() {
            "User" => Asked::User { message, default },
            "Domain" => Asked::Domain { message, default },
            "Password" => Asked::Password { message },
            _ => Asked::Choice { message, choices },
        })
    }
}

/// Mount `top` with `gio mount`, answering what it asks: the address's
/// user and domain, the window's password, the window's choice to go on
/// where GVfs cannot vouch for the server (the first choice; Cancel
/// otherwise).
fn mount(
    mut command: Command,
    top: &Address,
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
    let mut since = Instant::now();
    let mut ended = false;
    let outcome = loop {
        if cancel.is_cancelled() {
            break Err(anyhow::anyhow!("cancelled"));
        }
        if since.elapsed() > PATIENCE {
            break Err(anyhow::anyhow!(
                "{} did not answer within {} seconds",
                top.host,
                PATIENCE.as_secs()
            ));
        }
        if !ended {
            match said_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Some(text)) => {
                    if let Some(asked) = prompts.feed(&text) {
                        let Some(answer) = answer(top, &asked, ask, cancel) else {
                            break Err(anyhow::anyhow!("you cancelled"));
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

/// What to answer `asked`: `None` to cancel.
fn answer(
    top: &Address,
    asked: &Asked,
    ask: &Arc<dyn Ask>,
    cancel: &CancelToken,
) -> Option<String> {
    match asked {
        Asked::User { default, .. } => Some(
            top.user
                .clone()
                .or_else(|| default.clone())
                .unwrap_or_default(),
        ),
        Asked::Domain { default, .. } => Some(
            top.domain
                .clone()
                .or_else(|| default.clone())
                .unwrap_or_default(),
        ),
        Asked::Password { message } => {
            let title = if message.is_empty() {
                format!("Password for {}", top.label())
            } else {
                message.clone()
            };
            // A key's passphrase is asked each time; a server's password
            // may be kept (ADR-082).
            if message.to_lowercase().contains("passphrase") {
                ask.secret(&title, "Connect", cancel)
            } else {
                ask.password(&title, "Connect", cancel).map(|p| p.text)
            }
        }
        Asked::Choice { message, choices } => {
            // GVfs asks to go on where it cannot vouch for the server: its
            // first choice goes on, the last cancels.
            let (go_on, _) = choices.first()?;
            ask.confirm(&format!("Connect to {}?", top.host), message, cancel)
                .then(|| go_on.to_string())
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
                message: String::new()
            })
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

    #[test]
    fn what_gio_says_on_failing_loses_its_prefix() {
        assert_eq!(
            said("gio: sftp://example.com/: Connection refused\n"),
            "Connection refused"
        );
        assert_eq!(said("gio: Permission denied"), "Permission denied");
    }
}
