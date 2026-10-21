//! What the connectors that mount through FUSE themselves share (sshfs,
//! rclone): a mount point of Skwir's per address, the tool run until it
//! has mounted and left running after, a mount let go of, and the socket
//! through which ssh asks the window (ADR-077).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::cancel::CancelToken;

use crate::address::Address;
use crate::connector::{self, Ask, Recognised};

/// The mount point of `top` for `connector`, made if missing: an empty
/// folder of Skwir's, or one where the tool's mount is gone (a server
/// that went away while it was mounted, a FUSE program killed), which is
/// let go of first.
///
/// # Errors
/// When the user has no runtime folder, or the folder cannot be made.
pub fn point_for(connector: &str, top: &Address) -> anyhow::Result<PathBuf> {
    let folder = connector::mounts_folder()
        .ok_or_else(|| anyhow::anyhow!("there is no runtime folder to mount in"))?;
    make_private(&folder)?;
    let point = folder.join(connector::point_name(connector, top));
    if skwir_core::fs::reach::of(&point).is_some_and(|remote| remote.point == point) {
        // Mounted already. A mount whose program died answers nothing and
        // is let go of; a live one is never taken from under what uses it
        // (connections to one address wait their turn, `Remotes::reach`).
        match std::fs::symlink_metadata(&point) {
            Err(error) if error.kind() == std::io::ErrorKind::NotConnected => {
                let _ = force_unmount(&point);
                skwir_core::fs::reach::changed();
            }
            _ => anyhow::bail!("it is connected already"),
        }
    }
    match std::fs::create_dir(&point) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    Ok(point)
}

/// `folder` and the folders above it up to the runtime folder, made with
/// only the user allowed in.
pub(crate) fn make_private(folder: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(folder)
}

/// Whether `point` is a mount of Skwir's made by `connector`, and of what.
pub fn ours(connector: &str, point: &Path) -> Option<Recognised> {
    let folder = connector::mounts_folder()?;
    if point.parent() != Some(folder.as_path()) {
        return None;
    }
    let (by, address) = connector::of_point_name(point.file_name()?.to_str()?)?;
    (by == connector).then(|| Recognised {
        label: address.label(),
        address: Some(address),
        ours: true,
        device: None,
    })
}

/// The token of one attempt to connect, below the connection's: the user
/// refusing a question ends it (`Askpass`), and what it ended says so.
pub fn attempt(cancel: &CancelToken) -> CancelToken {
    cancel.child()
}

/// Why an attempt ended: the user's refusal, said as such, else `error`.
pub fn ended(
    attempt: &CancelToken,
    connection: &CancelToken,
    error: anyhow::Error,
) -> anyhow::Error {
    if attempt.is_cancelled() && !connection.is_cancelled() {
        anyhow::anyhow!("you cancelled")
    } else {
        error
    }
}

/// Run `command`, a tool that mounts at `point` and then runs on its own
/// (sshfs daemonizes, `rclone mount --daemon`), until it has mounted: its
/// exit, then the mount in the table. Cancelled or too long, the tool and
/// what it started are stopped; mounted, nothing it left running is
/// touched (the ssh it started carries the mount).
///
/// # Errors
/// What the tool said when it failed.
pub fn mount(
    command: &mut Command,
    point: &Path,
    cancel: &CancelToken,
    patience: Duration,
) -> anyhow::Result<()> {
    let finished =
        skwir_platform::process::run_leaving(command, cancel, patience).inspect_err(|_| {
            let _ = remove_point(point);
        })?;
    if !finished.status.success() {
        let _ = remove_point(point);
        let said = finished.stderr.trim();
        anyhow::bail!(
            "{}",
            if said.is_empty() {
                format!("it ended ({})", finished.status)
            } else {
                said.to_owned()
            }
        );
    }
    // A daemon may say it is ready a moment before the kernel lists it.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        skwir_core::fs::reach::changed();
        if skwir_core::fs::reach::of(point).is_some_and(|remote| remote.point == point) {
            return Ok(());
        }
        if Instant::now() > deadline || cancel.is_cancelled() {
            let _ = remove_point(point);
            anyhow::bail!("it said it mounted, but nothing is mounted there");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Let go of the FUSE mount at `point`; with `ours`, its folder too.
///
/// # Errors
/// When the system refused: a program still uses it.
pub fn unmount(point: &Path, ours: bool, cancel: &CancelToken) -> anyhow::Result<()> {
    let mut command = unmount_command(point, false);
    let finished =
        skwir_platform::process::run_to_exit(&mut command, None, cancel, Duration::from_secs(30))?;
    skwir_core::fs::reach::changed();
    if !finished.status.success() {
        let said = finished.stderr.trim();
        if !said.contains("busy") {
            anyhow::bail!("{said}");
        }
        // Refused as busy for a watch too: GVfs's trash watches the top
        // of every mount. Where nothing reads, writes or works there, it
        // is let go of at once (lazily), which ends the watches.
        match skwir_platform::process::using(point) {
            Some(users) if users.is_empty() => force_unmount(point)?,
            Some(users) => anyhow::bail!("{}", in_use(&users)),
            None => anyhow::bail!("another program is using it"),
        }
    }
    if ours {
        let _ = remove_point(point);
    }
    Ok(())
}

/// What Eject says when `users` (programs' names) use a mount.
fn in_use(users: &[String]) -> String {
    let quoted: Vec<String> = users.iter().map(|u| format!("“{u}”")).collect();
    match quoted.as_slice() {
        [one] => format!("{one} is using it"),
        [rest @ .., last] => format!("{} and {last} are using it", rest.join(", ")),
        [] => "another program is using it".to_owned(),
    }
}

/// Let go of the mount at `point` whatever holds it (a lazy unmount: the
/// kernel detaches it now, and what still uses it sees it gone). For a
/// mount whose program died, or a server that no longer answers.
///
/// # Errors
/// When the system refused.
pub fn force_unmount(point: &Path) -> anyhow::Result<()> {
    let mut command = unmount_command(point, true);
    let finished = skwir_platform::process::run_to_exit(
        &mut command,
        None,
        &CancelToken::new(),
        Duration::from_secs(30),
    )?;
    skwir_core::fs::reach::changed();
    anyhow::ensure!(finished.status.success(), "{}", finished.stderr.trim());
    Ok(())
}

fn unmount_command(point: &Path, lazy: bool) -> Command {
    #[cfg(target_os = "linux")]
    {
        let fusermount = ["fusermount3", "fusermount"]
            .into_iter()
            .find(|p| skwir_core::os_path::find_program(p).is_some())
            .unwrap_or("fusermount3");
        let mut command = Command::new(fusermount);
        command.arg(if lazy { "-uz" } else { "-u" }).arg(point);
        command
    }
    #[cfg(not(target_os = "linux"))]
    {
        let mut command = Command::new("umount");
        if lazy {
            command.arg("-f");
        }
        command.arg(point);
        command
    }
}

/// Remove an empty mount point of Skwir's.
fn remove_point(point: &Path) -> std::io::Result<()> {
    std::fs::remove_dir(point)
}

/// A socket ssh's questions reach the window through while a connection
/// is made: the `skwir` executable is ssh's `SSH_ASKPASS`, and asks here
/// (`skwir_platform::askpass`). Removed when dropped.
pub struct Askpass {
    server: skwir_platform::askpass::Server,
    program: PathBuf,
    /// The attempt it answers for: ended as it is dropped, so a question
    /// the window still shows is withdrawn once the tool is done.
    attempt: CancelToken,
}

impl Drop for Askpass {
    fn drop(&mut self) {
        self.attempt.cancel();
    }
}

impl Askpass {
    /// Answer through `ask` until dropped.
    ///
    /// # Errors
    /// When the socket cannot be made.
    pub fn serve(ask: &Arc<dyn Ask>, cancel: &CancelToken) -> anyhow::Result<Askpass> {
        Askpass::serve_with(std::env::current_exe()?, ask, cancel)
    }

    /// [`Askpass::serve`], ssh running `program` to ask: a test's.
    ///
    /// # Errors
    /// When the socket cannot be made.
    pub fn serve_with(
        program: PathBuf,
        ask: &Arc<dyn Ask>,
        cancel: &CancelToken,
    ) -> anyhow::Result<Askpass> {
        use skwir_platform::askpass::Kind;
        let folder = connector::mounts_folder()
            .and_then(|f| f.parent().map(Path::to_path_buf))
            .ok_or_else(|| anyhow::anyhow!("there is no runtime folder"))?;
        make_private(&folder)?;
        let attempt = cancel.clone();
        let (ask, cancel) = (Arc::clone(ask), cancel.clone());
        let server = skwir_platform::askpass::Server::start(&folder, move |question| {
            let prompt = question.prompt.trim().trim_end_matches(':').trim();
            let answer = match question.kind {
                Kind::Secret => ask.secret(prompt, "Connect", &cancel),
                Kind::Confirm => ask
                    .confirm("Connect to this server?", prompt, &cancel)
                    .then(|| "yes".to_owned()),
                Kind::Notice => return None,
            };
            // Cancelled: the attempt ends, rather than ssh asking again (a
            // refused password counts as a wrong one).
            if answer.is_none() {
                cancel.cancel();
            }
            answer
        })?;
        Ok(Askpass {
            server,
            program,
            attempt,
        })
    }

    /// Have `command`'s ssh ask here.
    pub fn give(&self, command: &mut Command) {
        command
            .env("SSH_ASKPASS", &self.program)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env(skwir_platform::askpass::SOCKET_VAR, self.server.path());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eject_names_what_uses_a_mount() {
        let names = |n: &[&str]| n.iter().map(|&n| n.to_owned()).collect::<Vec<_>>();
        assert_eq!(in_use(&names(&["vim"])), "“vim” is using it");
        assert_eq!(
            in_use(&names(&["fish", "less", "vim"])),
            "“fish”, “less” and “vim” are using it"
        );
    }
}
